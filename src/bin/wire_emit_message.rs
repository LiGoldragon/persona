//! Shim binary — emit a `signal-message` request
//! frame as length-prefixed bytes on stdout.
//!
//! Used by the nix-chained wire test: derivA runs this with
//! fixed args; the bytes become the file output that derivB
//! reads with `wire-decode-message`.
//!
//! CLI:
//!   --variant <submission|stamped|inbox-query>
//!                       Optional. Defaults to submission for
//!                       backwards compatibility with the original
//!                       wire-test chain.
//!   --recipient <name>  Required for all variants.
//!   --body <text>       Required for submission and stamped;
//!                       ignored for inbox-query.
//!   --origin <spec>     Required for stamped. Spec grammar:
//!                         internal:<component>
//!                         external:owner
//!                         external:non-owner-user:<uid>
//!                         external:network:<peer>
//!   --stamped-at <nanos>
//!                       Optional. Defaults to 0. Used by stamped.
//!
//! Output: length-prefixed Frame bytes on stdout.

use std::io::Write;

use signal_engine_management::TimestampNanos;
use signal_frame::{ExchangeIdentifier, ExchangeLane, LaneSequence, RequestPayload, SessionEpoch};
use signal_message::{
    Frame, FrameBody, InboxQuery, MessageBody, MessageKind, MessageRecipient, MessageRequest,
    MessageSubmission, StampedMessageSubmission,
};
use signal_persona_origin::{
    ComponentName, ConnectionClass, MessageOrigin, NetworkPeer, UnixUserIdentifier,
};

#[derive(Debug)]
enum Variant {
    Submission,
    Stamped,
    InboxQuery,
}

fn parse_variant(value: &str) -> Variant {
    match value {
        "submission" => Variant::Submission,
        "stamped" => Variant::Stamped,
        "inbox-query" => Variant::InboxQuery,
        other => panic!("unknown variant: {other}"),
    }
}

fn parse_component(value: &str) -> ComponentName {
    match value.to_ascii_lowercase().as_str() {
        "mind" => ComponentName::Mind,
        "message" => ComponentName::Message,
        "router" => ComponentName::Router,
        "terminal" => ComponentName::Terminal,
        "harness" => ComponentName::Harness,
        "system" => ComponentName::System,
        "introspect" => ComponentName::Introspect,
        other => panic!("unknown component: {other}"),
    }
}

fn parse_origin(spec: &str) -> MessageOrigin {
    if let Some(rest) = spec.strip_prefix("internal:") {
        return MessageOrigin::Internal(parse_component(rest));
    }
    if let Some(rest) = spec.strip_prefix("external:") {
        if rest == "owner" {
            return MessageOrigin::External(ConnectionClass::Owner);
        }
        if let Some(uid) = rest.strip_prefix("non-owner-user:") {
            return MessageOrigin::External(ConnectionClass::NonOwnerUser(
                UnixUserIdentifier::new(uid.parse::<u32>().expect("uid u32")),
            ));
        }
        if let Some(peer) = rest.strip_prefix("network:") {
            return MessageOrigin::External(ConnectionClass::Network(NetworkPeer::new(peer)));
        }
    }
    panic!("unknown origin spec: {spec}");
}

struct Cli {
    variant: Variant,
    recipient: String,
    body: Option<String>,
    origin: Option<MessageOrigin>,
    stamped_at: u64,
}

impl Cli {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut variant = Variant::Submission;
        let mut recipient = None;
        let mut body = None;
        let mut origin = None;
        let mut stamped_at = 0u64;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--variant" => {
                    variant = parse_variant(&args.next().expect("--variant value"));
                }
                "--recipient" => recipient = args.next(),
                "--body" => body = args.next(),
                "--origin" => origin = args.next().map(|spec| parse_origin(&spec)),
                "--stamped-at" => {
                    stamped_at = args
                        .next()
                        .expect("--stamped-at value")
                        .parse()
                        .expect("stamped-at u64");
                }
                other => panic!("unknown arg: {other}"),
            }
        }
        Self {
            variant,
            recipient: recipient.expect("--recipient is required"),
            body,
            origin,
            stamped_at,
        }
    }

    fn build_request(self) -> MessageRequest {
        let recipient = MessageRecipient::new(self.recipient);
        match self.variant {
            Variant::Submission => MessageRequest::Submit(MessageSubmission {
                recipient,
                kind: MessageKind::Send,
                body: MessageBody::new(self.body.expect("--body is required for submission")),
            }),
            Variant::Stamped => {
                let body = MessageBody::new(self.body.expect("--body is required for stamped"));
                let origin = self.origin.expect("--origin is required for stamped");
                MessageRequest::SubmitStamped(StampedMessageSubmission {
                    submission: MessageSubmission {
                        recipient,
                        kind: MessageKind::Send,
                        body,
                    },
                    origin,
                    stamped_at: TimestampNanos::new(self.stamped_at),
                })
            }
            Variant::InboxQuery => MessageRequest::QueryInbox(InboxQuery { recipient }),
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let request = cli.build_request();
    let frame = Frame::new(FrameBody::Request {
        exchange: ExchangeIdentifier::new(
            SessionEpoch::new(1),
            ExchangeLane::Connector,
            LaneSequence::first(),
        ),
        request: request.into_request(),
    });
    let bytes = frame
        .encode_length_prefixed()
        .expect("encode length-prefixed frame");
    std::io::stdout()
        .write_all(&bytes)
        .expect("write bytes to stdout");
}
