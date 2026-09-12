//! Shim binary — a one-shot "tap router" used as a midway witness.
//!
//! Binds a Unix socket. Accepts one connection. Reads one
//! length-prefixed Signal Frame. Writes the raw frame bytes to a
//! capture file so a downstream derivation can inspect what a real
//! producer sent. Writes a canned length-prefixed reply back on the
//! same connection. Exits.
//!
//! The capture file is the architectural-truth artifact: it answers
//! "what bytes did persona-message-daemon actually forward to the
//! router?" without depending on the router being correct. A later
//! derivation re-decodes the captured bytes through the typed
//! contract crate and asserts on shape (origin, body, ...).
//!
//! CLI:
//!   --socket <path>             Bind here.
//!   --capture <path>            Write received frame bytes here.
//!   --reply submission-accepted-slot=N
//!         | unimplemented-stamped
//!         | unimplemented-submission
//!         | unimplemented-inbox-query
//!                               Canned reply variant.
//!   --ready-file <path>         (Optional) `touch` this file after
//!                               binding so a peer process can
//!                               proceed without polling the socket.
//!
//! Exit 0 after one round-trip. Exit non-zero on bind/read/write
//! failure or unknown args.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use signal_message::{
    ByteViewable, MessageOperationKind, MessageRequestUnimplementedReply,
    MessageUnimplementedReason, Response, Signalizable,
};

enum CannedReply {
    SubmissionAcceptedSlot(i64),
    UnimplementedSubmission,
    UnimplementedStamped,
    UnimplementedInboxQuery,
}

fn parse_reply(spec: &str) -> CannedReply {
    if let Some(rest) = spec.strip_prefix("submission-accepted-slot=") {
        return CannedReply::SubmissionAcceptedSlot(rest.parse().expect("slot must be u64"));
    }
    match spec {
        "unimplemented-submission" => CannedReply::UnimplementedSubmission,
        "unimplemented-stamped" => CannedReply::UnimplementedStamped,
        "unimplemented-inbox-query" => CannedReply::UnimplementedInboxQuery,
        other => panic!("unknown --reply spec: {other}"),
    }
}

struct Cli {
    socket: PathBuf,
    capture: PathBuf,
    reply: CannedReply,
    ready_file: Option<PathBuf>,
}

impl Cli {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut socket = None;
        let mut capture = None;
        let mut reply = None;
        let mut ready_file = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--socket" => socket = args.next().map(PathBuf::from),
                "--capture" => capture = args.next().map(PathBuf::from),
                "--reply" => reply = args.next().map(|spec| parse_reply(&spec)),
                "--ready-file" => ready_file = args.next().map(PathBuf::from),
                other => panic!("unknown arg: {other}"),
            }
        }
        Self {
            socket: socket.expect("--socket is required"),
            capture: capture.expect("--capture is required"),
            reply: reply.expect("--reply is required"),
            ready_file,
        }
    }
}

fn build_reply(canned: CannedReply) -> Response {
    match canned {
        CannedReply::SubmissionAcceptedSlot(slot) => Response::SubmissionAccepted(slot),
        CannedReply::UnimplementedSubmission => {
            Response::MessageRequestUnimplemented(MessageRequestUnimplementedReply {
                message_operation_kind: MessageOperationKind::Submit,
                message_unimplemented_reason: MessageUnimplementedReason::NotInPrototypeScope,
            })
        }
        CannedReply::UnimplementedStamped => {
            Response::MessageRequestUnimplemented(MessageRequestUnimplementedReply {
                message_operation_kind: MessageOperationKind::SubmitStamped,
                message_unimplemented_reason: MessageUnimplementedReason::NotInPrototypeScope,
            })
        }
        CannedReply::UnimplementedInboxQuery => {
            Response::MessageRequestUnimplemented(MessageRequestUnimplementedReply {
                message_operation_kind: MessageOperationKind::QueryInbox,
                message_unimplemented_reason: MessageUnimplementedReason::NotInPrototypeScope,
            })
        }
    }
}

fn read_length_prefixed_frame(stream: &mut std::os::unix::net::UnixStream) -> Vec<u8> {
    let mut length_bytes = [0u8; 4];
    stream
        .read_exact(&mut length_bytes)
        .expect("read frame length prefix");
    // The Signal wire writes its length prefix as big-endian.
    let length = u32::from_be_bytes(length_bytes) as usize;
    let mut payload = vec![0u8; length];
    stream.read_exact(&mut payload).expect("read frame payload");
    let mut framed = Vec::with_capacity(4 + length);
    framed.extend_from_slice(&length_bytes);
    framed.extend_from_slice(&payload);
    framed
}

fn main() {
    let cli = Cli::parse();

    if let Some(parent) = cli.socket.parent() {
        std::fs::create_dir_all(parent).expect("create socket parent dir");
    }
    let _ = std::fs::remove_file(&cli.socket);
    let listener = UnixListener::bind(&cli.socket).expect("bind tap socket");

    eprintln!("wire-tap-router socket={}", cli.socket.display());

    if let Some(ready) = cli.ready_file.as_ref() {
        std::fs::write(ready, b"").expect("write ready file");
    }

    let (mut stream, _addr) = listener.accept().expect("accept one connection");
    let captured = read_length_prefixed_frame(&mut stream);

    std::fs::write(&cli.capture, &captured).expect("write capture file");
    eprintln!(
        "wire-tap-router captured {} bytes to {}",
        captured.len(),
        cli.capture.display()
    );

    // The envelope is retired, so there is no exchange identity to echo:
    // the reply is the canned contract value behind its length prefix.
    let signal = build_reply(cli.reply)
        .signalize()
        .expect("signalize canned reply");
    let reply_bytes = signal.bytes();
    let length = u32::try_from(reply_bytes.len()).expect("reply length fits in u32");
    stream
        .write_all(&length.to_be_bytes())
        .expect("write canned reply length prefix");
    stream.write_all(reply_bytes).expect("write canned reply");
    stream.flush().expect("flush canned reply");
}
