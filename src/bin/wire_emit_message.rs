//! Shim binary — emit a `signal-persona-message`
//! `Frame::Request(MessageSubmission(...))` as length-prefixed bytes
//! on stdout.
//!
//! Used by the nix-chained wire test: derivA runs this with
//! fixed args; the bytes become the file output that derivB
//! reads with `wire-decode-message`.
//!
//! Args:
//!   --recipient <name>
//!   --body <text>
//!
//! Output: length-prefixed Frame bytes on stdout.

use std::io::Write;

use signal_core::{FrameBody, Request};
use signal_persona_message::{
    Frame, MessageBody, MessageKind, MessageRecipient, MessageRequest, MessageSubmission,
};

struct Cli {
    recipient: String,
    body: String,
}

impl Cli {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut recipient = None;
        let mut body = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--recipient" => recipient = args.next(),
                "--body" => body = args.next(),
                other => panic!("unknown arg: {other}"),
            }
        }
        Self {
            recipient: recipient.expect("--recipient is required"),
            body: body.expect("--body is required"),
        }
    }

    fn message_submission(self) -> MessageSubmission {
        MessageSubmission {
            recipient: MessageRecipient::new(self.recipient),
            kind: MessageKind::Send,
            body: MessageBody::new(self.body),
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let request = MessageRequest::MessageSubmission(cli.message_submission());
    let frame = Frame::new(FrameBody::Request(Request::from_payload(request)));
    let bytes = frame
        .encode_length_prefixed()
        .expect("encode length-prefixed frame");
    std::io::stdout()
        .write_all(&bytes)
        .expect("write bytes to stdout");
}
