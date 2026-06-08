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
//!   --capture-nota <path>  Write the decoded reply's NOTA-text form
//!                          to this file so a downstream derivation
//!                          can inspect / consume it.
//!
//! Exit 0 if the frame decodes, the variant matches, and every
//! expectation holds. Exit non-zero with a diagnostic on stderr
//! otherwise.

use std::io::{Read, Write};

use signal_frame::{Reply, SubReply};
use signal_message::{Frame, FrameBody, MessageOperationKind, Output};

#[derive(Debug)]
enum Expectation {
    SubmissionAccepted {
        slot: u64,
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
    let mut capture_nota = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--expect" => variant = args.next(),
            "--expect-slot" => slot = args.next().map(|v| v.parse::<u64>().expect("slot u64")),
            "--expect-entry-count" => {
                count = args
                    .next()
                    .map(|v| v.parse::<usize>().expect("count usize"))
            }
            "--expect-entry-body" => body = args.next(),
            "--expect-entry-sender" => sender = args.next(),
            "--expect-operation" => operation = args.next().map(|v| parse_operation(&v)),
            "--capture-nota" => capture_nota = args.next(),
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
    (expectation, capture_nota)
}

fn write_nota(reply: &Output, path: &str) {
    let text = reply.to_nota();
    let mut file = std::fs::File::create(path).expect("create capture-nota file");
    file.write_all(text.as_bytes())
        .expect("write capture-nota text");
    file.write_all(b"\n").expect("write capture-nota newline");
}

fn main() {
    let (expect, capture_nota) = parse();

    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .expect("read reply frame bytes from stdin");

    let frame = Frame::decode_length_prefixed(&bytes).expect("decode length-prefixed reply frame");

    let reply_payload = match frame.into_body() {
        FrameBody::Reply { reply, .. } => match reply {
            Reply::Accepted { per_operation, .. } => match per_operation.into_head() {
                SubReply::Ok(payload) => payload,
                other => panic!("expected SubReply::Ok payload, got {other:?}"),
            },
            other => panic!("expected accepted reply, got {other:?}"),
        },
        other => panic!("expected reply frame body, got {other:?}"),
    };

    if let Some(path) = capture_nota.as_deref() {
        write_nota(&reply_payload, path);
    }

    match (expect, &reply_payload) {
        (
            Expectation::SubmissionAccepted { slot: want },
            Output::SubmissionAccepted(acceptance),
        ) => {
            let got = *acceptance.0.payload();
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
            Output::InboxListing(listing),
        ) => {
            if let Some(want) = want_count {
                let got = listing.0.len();
                assert_eq!(
                    got, want,
                    "inbox-listing entry count mismatch (expected {want}, got {got})"
                );
            }
            if let Some(want) = want_body.as_deref() {
                let found = listing
                    .0
                    .iter()
                    .any(|entry| entry.body.payload().as_str() == want);
                assert!(
                    found,
                    "inbox-listing missing entry with body={want:?}; entries={:?}",
                    listing.0
                );
            }
            if let Some(want) = want_sender.as_deref() {
                let found = listing
                    .0
                    .iter()
                    .any(|entry| entry.sender.payload().as_str() == want);
                assert!(
                    found,
                    "inbox-listing missing entry with sender={want:?}; entries={:?}",
                    listing.0
                );
            }
            eprintln!(
                "decoded InboxListing entries={} bodies={:?}",
                listing.0.len(),
                listing
                    .0
                    .iter()
                    .map(|e| e.body.payload().as_str())
                    .collect::<Vec<_>>()
            );
        }
        (
            Expectation::Unimplemented { operation: want },
            Output::MessageRequestUnimplemented(unimplemented),
        ) => {
            assert_eq!(
                unimplemented.operation, want,
                "unimplemented operation mismatch"
            );
            eprintln!(
                "decoded MessageRequestUnimplemented operation={:?} reason={:?}",
                unimplemented.operation, unimplemented.reason
            );
        }
        (expect, got) => {
            panic!("variant mismatch: expected {expect:?}, decoded reply was {got:?}");
        }
    }
}
