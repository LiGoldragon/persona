//! Shim binary — decode a `signal-message`
//! `Frame::Reply` from stdin, assert on variant and per-variant
//! fields, optionally dump decoded NOTA for inspection by a peer
//! derivation.
//!
//! CLI shape (one --expect per invocation):
//!
//!   --expect submission-accepted --expect-slot <N>
//!   --expect inbox-listing
//!       [--expect-entry-count <N>]
//!       [--expect-entry-body <text>]
//!       [--expect-entry-sender <name>]
//!   --expect unimplemented --expect-operation <submission|stamped|inbox-query>
//!
//! Optional:
//!   --capture-datom <path>  Write the decoded reply's NOTA-text form
//!                          to this file so a downstream derivation
//!                          can inspect / consume it.
//!
//! Exit 0 if the frame decodes, the variant matches, and every
//! expectation holds. Exit non-zero with a diagnostic on stderr
//! otherwise.

use std::io::{Read, Write};

use datom_codec::Datomizable;
use protos::{Protosizable, Textualizable};
use signal_message::{MessageOperationKind, Response, Restorable, Signal};

#[derive(Debug)]
enum Expectation {
    SubmissionAccepted {
        slot: i64,
    },
    InboxListing {
        count: Option<usize>,
        body: Option<String>,
        sender: Option<String>,
    },
    Unimplemented {
        operation: MessageOperationKind,
    },
}

fn parse_operation(value: &str) -> MessageOperationKind {
    match value {
        "submission" => MessageOperationKind::Submit,
        "stamped" => MessageOperationKind::SubmitStamped,
        "inbox-query" => MessageOperationKind::QueryInbox,
        other => panic!("unknown operation: {other}"),
    }
}

fn parse() -> (Expectation, Option<String>) {
    let mut args = std::env::args().skip(1);
    let mut variant = None;
    let mut slot = None;
    let mut count = None;
    let mut body = None;
    let mut sender = None;
    let mut operation = None;
    let mut capture_datom = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--expect" => variant = args.next(),
            "--expect-slot" => slot = args.next().map(|v| v.parse::<i64>().expect("slot i64")),
            "--expect-entry-count" => {
                count = args
                    .next()
                    .map(|v| v.parse::<usize>().expect("count usize"))
            }
            "--expect-entry-body" => body = args.next(),
            "--expect-entry-sender" => sender = args.next(),
            "--expect-operation" => operation = args.next().map(|v| parse_operation(&v)),
            "--capture-datom" => capture_datom = args.next(),
            other => panic!("unknown arg: {other}"),
        }
    }
    let expectation = match variant.as_deref() {
        Some("submission-accepted") => Expectation::SubmissionAccepted {
            slot: slot.expect("submission-accepted needs --expect-slot"),
        },
        Some("inbox-listing") => Expectation::InboxListing {
            count,
            body,
            sender,
        },
        Some("unimplemented") => Expectation::Unimplemented {
            operation: operation.expect("unimplemented needs --expect-operation"),
        },
        Some(other) => panic!("unknown expect variant: {other}"),
        None => panic!("--expect is required"),
    };
    (expectation, capture_datom)
}

fn write_datom(reply: &Response, path: &str) {
    let text = reply.clone().datomize(Vec::new()).protosize().textualize();
    let mut file = std::fs::File::create(path).expect("create capture file");
    file.write_all(text.as_bytes()).expect("write capture text");
    file.write_all(b"\n").expect("write capture newline");
}

fn main() {
    let (expect, capture_datom) = parse();

    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .expect("read reply frame bytes from stdin");

    assert!(bytes.len() >= 4, "frame is shorter than its length prefix");
    let length = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    assert_eq!(
        bytes.len() - 4,
        length,
        "length prefix does not match the body it frames"
    );
    let reply_payload: Response = Signal::<Response>::from(bytes[4..].to_vec())
        .restore()
        .expect("restore reply from frame");

    if let Some(path) = capture_datom.as_deref() {
        write_datom(&reply_payload, path);
    }

    match (expect, &reply_payload) {
        (
            Expectation::SubmissionAccepted { slot: want },
            Response::SubmissionAccepted(acceptance),
        ) => {
            let got = *acceptance;
            assert_eq!(
                got, want,
                "submission-accepted slot mismatch (expected {want}, got {got})"
            );
            eprintln!("decoded SubmissionAccepted slot={got}");
        }
        (
            Expectation::InboxListing {
                count: want_count,
                body: want_body,
                sender: want_sender,
            },
            Response::InboxListing(listing),
        ) => {
            let entries = &listing.messages;
            if let Some(want) = want_count {
                let got = entries.len();
                assert_eq!(
                    got, want,
                    "inbox-listing entry count mismatch (expected {want}, got {got})"
                );
            }
            if let Some(want) = want_body.as_deref() {
                let found = entries
                    .iter()
                    .any(|entry| entry.message_body.as_str() == want);
                assert!(
                    found,
                    "inbox-listing missing entry with body={want:?}; entries={:?}",
                    entries
                );
            }
            if let Some(want) = want_sender.as_deref() {
                let found = entries
                    .iter()
                    .any(|entry| entry.message_sender.as_str() == want);
                assert!(
                    found,
                    "inbox-listing missing entry with sender={want:?}; entries={:?}",
                    entries
                );
            }
            eprintln!(
                "decoded InboxListing entries={} bodies={:?}",
                entries.len(),
                entries
                    .iter()
                    .map(|entry| entry.message_body.as_str())
                    .collect::<Vec<_>>()
            );
        }
        (
            Expectation::Unimplemented { operation: want },
            Response::MessageRequestUnimplemented(unimplemented),
        ) => {
            assert_eq!(
                unimplemented.message_operation_kind, want,
                "unimplemented operation mismatch"
            );
            eprintln!(
                "decoded MessageRequestUnimplemented operation={:?} reason={:?}",
                unimplemented.message_operation_kind, unimplemented.message_unimplemented_reason
            );
        }
        (expect, got) => {
            panic!("variant mismatch: expected {expect:?}, decoded reply was {got:?}");
        }
    }
}
