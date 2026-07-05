//! Shim binary — decode a `signal-message`
//! length-prefixed `Frame::Request` from stdin and assert
//! on shape.
//!
//! Used as a downstream derivation in the nix-chained wire test,
//! taking an upstream derivation's frame bytes as input.
//!
//! CLI:
//!
//!   --expect-recipient <name>   Required.
//!   --expect-body <text>        Required.
//!
//!   --expect-variant <submission|stamped|inbox-query>
//!                               Optional. If given, the decoded
//!                               operation must be exactly that
//!                               variant.
//!
//!   --expect-origin <spec>      Optional. Asserts the origin field
//!                               on a StampedMessageSubmission.
//!                               Implies --expect-variant stamped.
//!                               Spec grammar:
//!                                 internal:<component>
//!                                 internal-instance:<component>:<instance>
//!                                 external:owner
//!                                 external:non-owner-user:<uid>
//!                                 external:network:<peer>
//!
//!   --capture-nota <path>       Optional. Write the decoded request
//!                               as a NOTA text record to this file
//!                               so a peer derivation can consume it.
//!
//! Exit 0 if the frame decodes and every expectation holds.
//! Exit non-zero (with diagnostic on stderr) otherwise.

use std::io::{Read, Write};

use signal_message::{
    ComponentInstanceName, ComponentName, ConnectionClass, Frame, FrameBody, Input,
    InternalComponentInstanceOrigin, MessageOrigin, NetworkPeer, NotaEncode, UnixUserIdentifier,
};

struct Expectations {
    recipient: String,
    body: String,
    variant: Option<ExpectedVariant>,
    origin: Option<MessageOrigin>,
    capture_nota: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum ExpectedVariant {
    Submission,
    Stamped,
    InboxQuery,
}

fn parse_variant(value: &str) -> ExpectedVariant {
    match value {
        "submission" => ExpectedVariant::Submission,
        "stamped" => ExpectedVariant::Stamped,
        "inbox-query" => ExpectedVariant::InboxQuery,
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
    if let Some(rest) = spec.strip_prefix("internal-instance:") {
        let (component, instance) = rest
            .split_once(':')
            .expect("internal-instance origin expects component:instance");
        return MessageOrigin::InternalComponentInstance(InternalComponentInstanceOrigin {
            component_name: parse_component(component),
            component_instance_name: ComponentInstanceName::new(instance.to_owned()),
        });
    }
    if let Some(rest) = spec.strip_prefix("internal:") {
        return MessageOrigin::Internal(parse_component(rest));
    }
    if let Some(rest) = spec.strip_prefix("external:") {
        if rest == "owner" {
            return MessageOrigin::External(ConnectionClass::Owner);
        }
        if let Some(uid) = rest.strip_prefix("non-owner-user:") {
            return MessageOrigin::External(ConnectionClass::NonOwnerUser(
                UnixUserIdentifier::new(uid.parse::<u64>().expect("uid u64")),
            ));
        }
        if let Some(peer) = rest.strip_prefix("network:") {
            return MessageOrigin::External(ConnectionClass::Network(NetworkPeer::new(
                peer.to_owned(),
            )));
        }
    }
    panic!("unknown origin spec: {spec}");
}

impl Expectations {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut recipient = None;
        let mut body = None;
        let mut variant = None;
        let mut origin = None;
        let mut capture_nota = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--expect-recipient" => recipient = args.next(),
                "--expect-body" => body = args.next(),
                "--expect-variant" => variant = args.next().map(|v| parse_variant(&v)),
                "--expect-origin" => origin = args.next().map(|v| parse_origin(&v)),
                "--capture-nota" => capture_nota = args.next(),
                other => panic!("unknown arg: {other}"),
            }
        }
        if origin.is_some() && variant.is_none() {
            variant = Some(ExpectedVariant::Stamped);
        }
        Self {
            recipient: recipient.expect("--expect-recipient is required"),
            body: body.expect("--expect-body is required"),
            variant,
            origin,
            capture_nota,
        }
    }

    fn assert_submission(&self, submission: &signal_message::MessageSubmission) {
        assert_eq!(
            submission.message_recipient.payload().as_str(),
            self.recipient.as_str(),
            "recipient mismatch (expected {}, got {})",
            self.recipient,
            submission.message_recipient.payload().as_str()
        );
        assert_eq!(
            submission.message_body.payload().as_str(),
            self.body.as_str(),
            "body mismatch (expected {}, got {})",
            self.body,
            submission.message_body.payload().as_str()
        );
        eprintln!(
            "decoded MessageSubmission {{ recipient: {}, body: {} }}",
            submission.message_recipient.payload().as_str(),
            submission.message_body.payload().as_str()
        );
    }
}

fn write_nota(request: &Input, path: &str) {
    let text = request.to_nota();
    let mut file = std::fs::File::create(path).expect("create capture-nota file");
    file.write_all(text.as_bytes())
        .expect("write capture-nota text");
    file.write_all(b"\n").expect("write capture-nota newline");
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
            let mut operations = request.payloads.into_vec();
            assert_eq!(operations.len(), 1, "expected one message operation");
            let operation = operations.remove(0);

            if let Some(path) = expect.capture_nota.as_deref() {
                write_nota(&operation, path);
            }

            match (&expect.variant, &operation) {
                (Some(ExpectedVariant::Submission) | None, Input::Submit(submission)) => {
                    expect.assert_submission(submission);
                    if expect.origin.is_some() {
                        panic!(
                            "--expect-origin given but decoded variant is MessageSubmission (no origin field)"
                        );
                    }
                }
                (Some(ExpectedVariant::Stamped) | None, Input::SubmitStamped(stamped)) => {
                    expect.assert_submission(&stamped.message_submission);
                    if let Some(want_origin) = expect.origin.as_ref() {
                        assert_eq!(
                            &stamped.message_origin, want_origin,
                            "origin mismatch (expected {:?}, got {:?})",
                            want_origin, stamped.message_origin
                        );
                        eprintln!(
                            "decoded origin matches expectation: {:?}",
                            stamped.message_origin
                        );
                    } else {
                        eprintln!("decoded origin (unasserted): {:?}", stamped.message_origin);
                    }
                }
                (Some(ExpectedVariant::InboxQuery), Input::QueryInbox(query)) => {
                    assert_eq!(
                        query.payload().payload().as_str(),
                        expect.recipient.as_str(),
                        "inbox-query recipient mismatch"
                    );
                    eprintln!(
                        "decoded InboxQuery {{ recipient: {} }}",
                        query.payload().payload().as_str()
                    );
                }
                (expected, got) => {
                    panic!(
                        "variant mismatch: expected {:?}, decoded {:?}",
                        expected, got
                    );
                }
            }
        }
        other => panic!("expected Operation request frame, got {other:?}"),
    }
}
