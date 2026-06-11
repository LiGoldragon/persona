//! Shim binary — emit a `signal-message`
//! `Frame::Reply(...)` as length-prefixed bytes on stdout.
//!
//! Used by the nix-chained midway tests: one derivation runs this
//! to produce a reply frame; the next decodes those bytes with
//! `wire-decode-message-reply` and asserts shape.
//!
//! CLI shape (one --variant per invocation):
//!
//!   --variant submission-accepted --slot <N>
//!   --variant inbox-listing [--entry slot=N,sender=S,body=B ...]
//!   --variant unimplemented --operation <submission|stamped|inbox-query>
//!                           --reason <not-in-prototype-scope
//!                                     |dependency-router
//!                                     |resource-router-socket
//!                                     |resource-peer-credentials>
//!
//! Output: length-prefixed Frame bytes on stdout.
//! Exit non-zero on bad arguments or encode failure.

use std::io::Write;

use signal_frame::{
    ExchangeIdentifier, ExchangeLane, LaneSequence, NonEmpty, Reply, SessionEpoch, SubReply,
};
use signal_message::{
    DependencyKind, Frame, FrameBody, InboxEntry, InboxListing, MessageBody, MessageOperationKind,
    MessageRequestUnimplemented, MessageSender, MessageSlot, MessageUnimplementedReason, Output,
    ResourceKind, SubmissionAcceptance,
};

enum Variant {
    SubmissionAccepted {
        slot: u64,
    },
    InboxListing {
        entries: Vec<EntrySpec>,
    },
    Unimplemented {
        operation: MessageOperationKind,
        reason: MessageUnimplementedReason,
    },
}

struct EntrySpec {
    slot: u64,
    sender: String,
    body: String,
}

impl EntrySpec {
    fn parse(spec: &str) -> Self {
        let mut slot = None;
        let mut sender = None;
        let mut body = None;
        for field in spec.split(',') {
            let (key, value) = field
                .split_once('=')
                .expect("entry field must be key=value");
            match key {
                "slot" => slot = Some(value.parse::<u64>().expect("slot must be u64")),
                "sender" => sender = Some(value.to_string()),
                "body" => body = Some(value.to_string()),
                other => panic!("unknown entry field key: {other}"),
            }
        }
        Self {
            slot: slot.expect("entry needs slot="),
            sender: sender.expect("entry needs sender="),
            body: body.expect("entry needs body="),
        }
    }

    fn into_entry(self) -> InboxEntry {
        InboxEntry {
            message_slot: MessageSlot::new(self.slot),
            sender: MessageSender::new(self.sender),
            body: MessageBody::new(self.body),
        }
    }
}

fn parse_operation(value: &str) -> MessageOperationKind {
    match value {
        "submission" => MessageOperationKind::Submit,
        "stamped" => MessageOperationKind::SubmitStamped,
        "inbox-query" => MessageOperationKind::QueryInbox,
        other => panic!("unknown operation: {other}"),
    }
}

fn parse_reason(value: &str) -> MessageUnimplementedReason {
    match value {
        "not-in-prototype-scope" => MessageUnimplementedReason::NotInPrototypeScope,
        "dependency-router" => {
            MessageUnimplementedReason::DependencyMissing(DependencyKind::Router)
        }
        "resource-router-socket" => {
            MessageUnimplementedReason::ResourceUnavailable(ResourceKind::RouterSocket)
        }
        "resource-peer-credentials" => {
            MessageUnimplementedReason::ResourceUnavailable(ResourceKind::PeerCredentials)
        }
        other => panic!("unknown unimplemented reason: {other}"),
    }
}

fn parse() -> Variant {
    let mut args = std::env::args().skip(1);
    let mut kind = None;
    let mut slot = None;
    let mut entries = Vec::new();
    let mut operation = None;
    let mut reason = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--variant" => kind = args.next(),
            "--slot" => {
                slot = args
                    .next()
                    .map(|v| v.parse::<u64>().expect("slot must be u64"))
            }
            "--entry" => entries.push(EntrySpec::parse(&args.next().expect("--entry needs value"))),
            "--operation" => operation = args.next().map(|v| parse_operation(&v)),
            "--reason" => reason = args.next().map(|v| parse_reason(&v)),
            other => panic!("unknown arg: {other}"),
        }
    }
    match kind.as_deref() {
        Some("submission-accepted") => Variant::SubmissionAccepted {
            slot: slot.expect("submission-accepted needs --slot"),
        },
        Some("inbox-listing") => Variant::InboxListing { entries },
        Some("unimplemented") => Variant::Unimplemented {
            operation: operation.expect("unimplemented needs --operation"),
            reason: reason.unwrap_or(MessageUnimplementedReason::NotInPrototypeScope),
        },
        Some(other) => panic!("unknown variant: {other}"),
        None => panic!("--variant is required"),
    }
}

fn build_reply(variant: Variant) -> Output {
    match variant {
        Variant::SubmissionAccepted { slot } => {
            Output::SubmissionAccepted(SubmissionAcceptance::new(MessageSlot::new(slot)))
        }
        Variant::InboxListing { entries } => Output::InboxListing(InboxListing::new(
            entries.into_iter().map(EntrySpec::into_entry).collect(),
        )),
        Variant::Unimplemented { operation, reason } => {
            Output::MessageRequestUnimplemented(MessageRequestUnimplemented { operation, reason })
        }
    }
}

fn main() {
    let variant = parse();
    let reply = build_reply(variant);
    let frame = Frame::new(FrameBody::Reply {
        exchange: ExchangeIdentifier::new(
            SessionEpoch::new(1),
            ExchangeLane::Connector,
            LaneSequence::first(),
        ),
        reply: Reply::committed(NonEmpty::single(SubReply::Ok(reply))),
    });
    let bytes = frame
        .encode_length_prefixed()
        .expect("encode length-prefixed reply frame");
    std::io::stdout()
        .write_all(&bytes)
        .expect("write reply bytes to stdout");
}
