//! Shim binary — decode a `signal-persona-message`
//! length-prefixed Frame from stdin and assert content.
//!
//! Args (assertion mode):
//!   --expect-recipient <name>
//!   --expect-body <text>
//!
//! Exit 0 if the frame decodes AND matches the expectations.
//! Exit non-zero (with diagnostic on stderr) otherwise.
//!
//! Used as derivB in the nix-chained wire test, taking
//! derivA's output as input.

use std::io::Read;

use signal_core::{FrameBody, Request, SemaVerb};
use signal_persona_message::{Frame, MessageRequest};

struct Expectations {
    recipient: String,
    body: String,
}

impl Expectations {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut recipient = None;
        let mut body = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--expect-recipient" => recipient = args.next(),
                "--expect-body" => body = args.next(),
                other => panic!("unknown arg: {other}"),
            }
        }
        Self {
            recipient: recipient.expect("--expect-recipient is required"),
            body: body.expect("--expect-body is required"),
        }
    }
}

fn main() {
    let expect = Expectations::parse();
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .expect("read frame bytes from stdin");

    let frame = Frame::decode_length_prefixed(&bytes).expect("decode length-prefixed frame");

    match frame.into_body() {
        FrameBody::Request(Request::Operation { verb, payload }) => {
            assert_eq!(verb, SemaVerb::Assert, "expected Assert verb");
            match payload {
                MessageRequest::Submit(submit) => {
                    assert_eq!(
                        submit.recipient, expect.recipient,
                        "recipient mismatch (expected {}, got {})",
                        expect.recipient, submit.recipient
                    );
                    assert_eq!(
                        submit.body, expect.body,
                        "body mismatch (expected {}, got {})",
                        expect.body, submit.body
                    );
                    eprintln!(
                        "✓ decoded SubmitMessage {{ recipient: {}, body: {} }}",
                        submit.recipient, submit.body
                    );
                }
                MessageRequest::Inbox(_) => panic!("expected Submit variant, got Inbox"),
            }
        }
        other => panic!("expected Operation request frame, got {other:?}"),
    }
}
