//! Shim binary — read a `signal-persona-message` Frame from
//! stdin, transform the inner SubmitMessage into a
//! `signal-persona-store` `CommitMessage`, and emit a new
//! Frame on the store channel as length-prefixed bytes on
//! stdout.
//!
//! This is the *router-shaped* step in the wire test:
//! receives a message-channel frame, derives a sender
//! (faked here from --sender for test isolation), produces
//! a store-channel frame.
//!
//! Args:
//!   --sender <name>   (the router-resolved sender; in
//!                      production this comes from process
//!                      ancestry)
//!
//! Used as the middle derivation in the
//! message → relay → store nix chain.

use std::io::{Read, Write};

use signal_core::{FrameBody, Request};
use signal_persona_message::{
    Frame as MessageFrame, MessageRequest,
};
use signal_persona_store::{CommitMessage, Frame as StoreFrame, StoreRequest};

struct RelayArgs {
    sender: String,
}

impl RelayArgs {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut sender = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--sender" => sender = args.next(),
                other => panic!("unknown arg: {other}"),
            }
        }
        Self {
            sender: sender.expect("--sender is required"),
        }
    }
}

fn main() {
    let args = RelayArgs::parse();
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .expect("read inbound frame bytes from stdin");

    let inbound = MessageFrame::decode_length_prefixed(&input)
        .expect("decode inbound message frame");

    let submit = match inbound.into_body() {
        FrameBody::Request(Request::Operation { payload, .. }) => match payload {
            MessageRequest::Submit(submit) => submit,
            MessageRequest::Inbox(_) => panic!("relay only handles Submit; got Inbox"),
        },
        other => panic!("relay expects Operation request frame; got {other:?}"),
    };

    let commit = StoreRequest::CommitMessage(CommitMessage {
        recipient: submit.recipient,
        sender: args.sender,
        body: submit.body,
    });
    let outbound = StoreFrame::new(FrameBody::Request(Request::assert(commit)));
    let bytes = outbound
        .encode_length_prefixed()
        .expect("encode store frame");
    std::io::stdout()
        .write_all(&bytes)
        .expect("write store frame to stdout");
}
