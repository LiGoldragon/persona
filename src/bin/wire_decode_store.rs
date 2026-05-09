//! Shim binary — decode a `signal-persona-store` Frame from
//! stdin and assert its `CommitMessage` content.
//!
//! Args (assertion mode):
//!   --expect-recipient <name>
//!   --expect-sender <name>
//!   --expect-body <text>
//!
//! Exit 0 if the frame decodes AND matches; non-zero
//! otherwise.
//!
//! Used as the final derivation in the message → relay →
//! store nix chain.

use std::io::Read;

use signal_core::{FrameBody, Request, SemaVerb};
use signal_persona_store::{Frame, StoreRequest};

struct Expectations {
    recipient: String,
    sender: String,
    body: String,
}

impl Expectations {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut recipient = None;
        let mut sender = None;
        let mut body = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--expect-recipient" => recipient = args.next(),
                "--expect-sender" => sender = args.next(),
                "--expect-body" => body = args.next(),
                other => panic!("unknown arg: {other}"),
            }
        }
        Self {
            recipient: recipient.expect("--expect-recipient is required"),
            sender: sender.expect("--expect-sender is required"),
            body: body.expect("--expect-body is required"),
        }
    }
}

fn main() {
    let expect = Expectations::parse();
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .expect("read store frame bytes from stdin");

    let frame = Frame::decode_length_prefixed(&bytes).expect("decode store frame");

    match frame.into_body() {
        FrameBody::Request(Request::Operation { verb, payload }) => {
            assert_eq!(verb, SemaVerb::Assert);
            match payload {
                StoreRequest::CommitMessage(commit) => {
                    assert_eq!(commit.recipient, expect.recipient, "recipient mismatch");
                    assert_eq!(commit.sender, expect.sender, "sender mismatch");
                    assert_eq!(commit.body, expect.body, "body mismatch");
                    eprintln!(
                        "✓ decoded CommitMessage {{ recipient: {}, sender: {}, body: {} }}",
                        commit.recipient, commit.sender, commit.body
                    );
                }
                StoreRequest::ReadInbox(_) => panic!("expected CommitMessage; got ReadInbox"),
            }
        }
        other => panic!("expected store Operation request; got {other:?}"),
    }
}
