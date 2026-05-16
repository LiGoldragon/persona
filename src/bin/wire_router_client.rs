//! Shim binary — connect to a Unix socket, send length-prefixed
//! request bytes from stdin, read length-prefixed reply bytes from
//! the socket, write them to stdout.
//!
//! This is the bytes-in / bytes-out connector that makes a daemon
//! talkable from a Nix derivation. Combined with `wire-emit-*` and
//! `wire-decode-*`, it lets a derivation script a real daemon
//! interaction without depending on the daemon's own CLI.
//!
//! CLI:
//!   --socket <path>             Connect to this Unix socket path.
//!   --connect-attempts <N>      Retry connect up to N times waiting
//!                               for the daemon to bind (default 40).
//!   --connect-interval-ms <ms>  Sleep between connect attempts
//!                               (default 50).
//!
//! Stdin: a single length-prefixed Signal Frame's worth of bytes.
//! Stdout: a single length-prefixed Signal Frame's worth of bytes.
//!
//! Exit 0 on a clean send + read cycle. Exit non-zero otherwise.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

struct Cli {
    socket: PathBuf,
    connect_attempts: u32,
    connect_interval_ms: u64,
}

impl Cli {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut socket = None;
        let mut connect_attempts = 40u32;
        let mut connect_interval_ms = 50u64;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--socket" => socket = args.next().map(PathBuf::from),
                "--connect-attempts" => {
                    connect_attempts = args
                        .next()
                        .expect("--connect-attempts value")
                        .parse()
                        .expect("connect-attempts u32");
                }
                "--connect-interval-ms" => {
                    connect_interval_ms = args
                        .next()
                        .expect("--connect-interval-ms value")
                        .parse()
                        .expect("connect-interval-ms u64");
                }
                other => panic!("unknown arg: {other}"),
            }
        }
        Self {
            socket: socket.expect("--socket is required"),
            connect_attempts,
            connect_interval_ms,
        }
    }
}

fn connect_with_retry(socket: &std::path::Path, attempts: u32, interval_ms: u64) -> UnixStream {
    let mut last_error = None;
    for attempt in 0..attempts {
        match UnixStream::connect(socket) {
            Ok(stream) => return stream,
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < attempts {
                    std::thread::sleep(Duration::from_millis(interval_ms));
                }
            }
        }
    }
    panic!(
        "failed to connect to {} after {} attempts: {:?}",
        socket.display(),
        attempts,
        last_error
    );
}

fn read_length_prefixed_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut length_bytes = [0u8; 4];
    stream
        .read_exact(&mut length_bytes)
        .expect("read reply length prefix");
    // signal-core's Frame::encode_length_prefixed writes the prefix
    // as big-endian — see signal-core/src/frame.rs `length_prefix`.
    let length = u32::from_be_bytes(length_bytes) as usize;
    let mut payload = vec![0u8; length];
    stream.read_exact(&mut payload).expect("read reply payload");
    let mut framed = Vec::with_capacity(4 + length);
    framed.extend_from_slice(&length_bytes);
    framed.extend_from_slice(&payload);
    framed
}

fn main() {
    let cli = Cli::parse();

    let mut request_bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut request_bytes)
        .expect("read request bytes from stdin");
    assert!(
        !request_bytes.is_empty(),
        "stdin must carry a length-prefixed frame"
    );

    let mut stream = connect_with_retry(
        cli.socket.as_path(),
        cli.connect_attempts,
        cli.connect_interval_ms,
    );
    stream
        .write_all(&request_bytes)
        .expect("write request bytes to socket");
    stream.flush().expect("flush socket");

    let reply_bytes = read_length_prefixed_frame(&mut stream);

    std::io::stdout()
        .write_all(&reply_bytes)
        .expect("write reply bytes to stdout");
}
