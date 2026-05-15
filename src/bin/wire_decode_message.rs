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

use signal_core::SignalVerb;
use signal_persona_message::{Frame, FrameBody, MessageRequest};

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
        FrameBody::Request { request, .. } => {
            let checked = request
                .into_checked()
                .map_err(|(error, _request)| error)
                .expect("request passes signal-core receive checks");
            let mut operations = checked.operations.into_vec();
            assert_eq!(operations.len(), 1, "expected one message operation");
            let operation = operations.remove(0);
            assert_eq!(operation.verb, SignalVerb::Assert, "expected Assert verb");
            match operation.payload {
                MessageRequest::MessageSubmission(submission) => {
                    expect.assert_submission(&submission);
                }
                MessageRequest::StampedMessageSubmission(stamped) => {
                    expect.assert_submission(&stamped.submission);
                }
                MessageRequest::InboxQuery(_) => {
                    panic!("expected MessageSubmission variant, got InboxQuery")
                }
            }
        }
        other => panic!("expected Operation request frame, got {other:?}"),
    }
}

impl Expectations {
    fn assert_submission(&self, submission: &signal_persona_message::MessageSubmission) {
        assert_eq!(
            submission.recipient.as_str(),
            self.recipient.as_str(),
            "recipient mismatch (expected {}, got {})",
            self.recipient,
            submission.recipient.as_str()
        );
        assert_eq!(
            submission.body.as_str(),
            self.body.as_str(),
            "body mismatch (expected {}, got {})",
            self.body,
            submission.body.as_str()
        );
        eprintln!(
            "decoded MessageSubmission {{ recipient: {}, body: {} }}",
            submission.recipient.as_str(),
            submission.body.as_str()
        );
    }
}
