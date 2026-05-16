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

use signal_core::{ExchangeIdentifier, NonEmpty, Reply, SignalVerb, SubReply};
use signal_persona_message::{
    Frame, FrameBody, MessageOperationKind, MessageReply, MessageRequestUnimplemented, MessageSlot,
    MessageUnimplementedReason, SubmissionAcceptance,
};

enum CannedReply {
    SubmissionAcceptedSlot(u64),
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

fn build_reply_frame(canned: CannedReply, request_exchange: ExchangeIdentifier) -> Frame {
    let payload = match canned {
        CannedReply::SubmissionAcceptedSlot(slot) => {
            MessageReply::SubmissionAccepted(SubmissionAcceptance {
                message_slot: MessageSlot::new(slot),
            })
        }
        CannedReply::UnimplementedSubmission => {
            MessageReply::MessageRequestUnimplemented(MessageRequestUnimplemented {
                operation: MessageOperationKind::MessageSubmission,
                reason: MessageUnimplementedReason::NotInPrototypeScope,
            })
        }
        CannedReply::UnimplementedStamped => {
            MessageReply::MessageRequestUnimplemented(MessageRequestUnimplemented {
                operation: MessageOperationKind::StampedMessageSubmission,
                reason: MessageUnimplementedReason::NotInPrototypeScope,
            })
        }
        CannedReply::UnimplementedInboxQuery => {
            MessageReply::MessageRequestUnimplemented(MessageRequestUnimplemented {
                operation: MessageOperationKind::InboxQuery,
                reason: MessageUnimplementedReason::NotInPrototypeScope,
            })
        }
    };
    // Echo the request's exchange identifier in the reply so the
    // caller (which round-trips by exchange ID) accepts the reply.
    Frame::new(FrameBody::Reply {
        exchange: request_exchange,
        reply: Reply::completed(NonEmpty::single(SubReply::Ok {
            verb: SignalVerb::Assert,
            payload,
        })),
    })
}

fn read_length_prefixed_frame(stream: &mut std::os::unix::net::UnixStream) -> Vec<u8> {
    let mut length_bytes = [0u8; 4];
    stream
        .read_exact(&mut length_bytes)
        .expect("read frame length prefix");
    // signal-core's Frame::encode_length_prefixed writes the prefix
    // as big-endian — see signal-core/src/frame.rs `length_prefix`.
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

    // Decode just enough of the captured request to extract its
    // exchange identifier so we echo it on the reply. We don't
    // re-emit the request, we just need its envelope to match.
    let request_frame =
        Frame::decode_length_prefixed(&captured).expect("decode captured request envelope");
    let request_exchange = match request_frame.into_body() {
        FrameBody::Request { exchange, .. } => exchange,
        other => panic!("wire-tap-router expected a Request frame from the caller, got {other:?}"),
    };

    let reply_frame = build_reply_frame(cli.reply, request_exchange);
    let reply_bytes = reply_frame
        .encode_length_prefixed()
        .expect("encode canned reply frame");
    stream.write_all(&reply_bytes).expect("write canned reply");
    stream.flush().expect("flush canned reply");
}
