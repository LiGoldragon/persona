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

use signal_message::{
    ByteViewable, ComponentName, ConnectionClass, MessageKind, MessageOrigin, MessageSubmission,
    Query, Signalizable, StampedMessageSubmission, ThreadSelection,
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
                uid.parse::<i64>().expect("uid i64"),
            ));
        }
        if let Some(peer) = rest.strip_prefix("network:") {
            return MessageOrigin::External(ConnectionClass::Network(peer.to_owned()));
        }
    }
    panic!("unknown origin spec: {spec}");
}

struct Cli {
    variant: Variant,
    recipient: String,
    body: Option<String>,
    origin: Option<MessageOrigin>,
    stamped_at: i64,
}

impl Cli {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut variant = Variant::Submission;
        let mut recipient = None;
        let mut body = None;
        let mut origin = None;
        let mut stamped_at = 0i64;
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
                        .expect("stamped-at i64");
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

    fn build_request(self) -> Query {
        let recipient = self.recipient;
        match self.variant {
            Variant::Submission => Query::Submit(MessageSubmission {
                message_recipient: recipient,
                message_kind: MessageKind::Send,
                message_body: self.body.expect("--body is required for submission"),
                thread_selection: ThreadSelection::None,
            }),
            Variant::Stamped => {
                let body = self.body.expect("--body is required for stamped");
                let origin = self.origin.expect("--origin is required for stamped");
                Query::SubmitStamped(StampedMessageSubmission {
                    message_submission: MessageSubmission {
                        message_recipient: recipient,
                        message_kind: MessageKind::Send,
                        message_body: body,
                        thread_selection: ThreadSelection::None,
                    },
                    message_origin: origin,
                    stamped_at: self.stamped_at,
                })
            }
            Variant::InboxQuery => Query::QueryInbox(recipient),
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let request = cli.build_request();
    let signal = request.signalize().expect("signalize request");
    let bytes = signal.bytes();
    let length = u32::try_from(bytes.len()).expect("frame length fits in u32");
    let mut out = std::io::stdout();
    out.write_all(&length.to_be_bytes())
        .expect("write length prefix to stdout");
    out.write_all(bytes).expect("write bytes to stdout");
}
