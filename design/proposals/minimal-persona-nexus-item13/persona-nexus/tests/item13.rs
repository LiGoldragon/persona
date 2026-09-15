use std::collections::BTreeMap;

use meta_signal_persona::FirstPrompt as MetaFirstPrompt;
use nexus::{Permissive, SocketAuthority};
use persona_nexus::{
    DraftLaunch, HarnessFirstPrompt, MemoryQuotaLedger, OrdinarySocket, PersonaNexus, QuotaLedger,
    QuotaState, QuotaWindowStorage, SemaEngineQuotaLedger,
};
use signal_persona::{
    FirstPrompt, ObservedAt, Query, QuotaSample, RemainingAmount, ResetInterval, Signal,
    SourceTimestamp, SubscriptionId, UnknownQuota,
};

struct RecordingQuotaWindowStorage {
    windows: BTreeMap<SubscriptionId, persona_nexus::QuotaWindow>,
    writes: Vec<StorageWrite>,
}

#[derive(Debug, Eq, PartialEq)]
enum StorageWrite {
    Asserted,
    Mutated,
}

impl QuotaWindowStorage for RecordingQuotaWindowStorage {
    fn load_window(
        &self,
        subscription: &SubscriptionId,
    ) -> Result<Option<persona_nexus::QuotaWindow>, sema_engine::Error> {
        Ok(self.windows.get(subscription).cloned())
    }

    fn store_window(
        &mut self,
        prior: Option<&persona_nexus::QuotaWindow>,
        window: persona_nexus::QuotaWindow,
    ) -> Result<(), sema_engine::Error> {
        self.writes.push(if prior.is_some() {
            StorageWrite::Mutated
        } else {
            StorageWrite::Asserted
        });
        self.windows
            .insert(window.sample.subscription.clone(), window);
        Ok(())
    }
}

struct RecordingSignalTransport {
    sent: Vec<Signal>,
}

impl RecordingSignalTransport {
    fn send<Ledger: QuotaLedger>(
        &mut self,
        nexus: &mut PersonaNexus<Ledger>,
        signal: Signal,
        observed_at: ObservedAt,
    ) -> Signal {
        self.sent.push(signal.clone());
        nexus.receive_signal(signal, observed_at)
    }
}

struct FakeHarness {
    ordinary: Vec<FirstPrompt>,
    meta: Vec<MetaFirstPrompt>,
}

impl HarnessFirstPrompt for FakeHarness {
    fn hand_first_prompt(&mut self, prompt: FirstPrompt) {
        self.ordinary.push(prompt);
    }

    fn hand_meta_first_prompt(&mut self, prompt: MetaFirstPrompt) {
        self.meta.push(prompt);
    }
}

fn sample(subscription: &str, timestamp: u64, interval: u64, remaining: u64) -> QuotaSample {
    QuotaSample {
        subscription: SubscriptionId(subscription.into()),
        source_timestamp: SourceTimestamp(timestamp),
        reset_interval: ResetInterval(interval),
        remaining: RemainingAmount(remaining),
    }
}

#[test]
fn reset_replaces_a_subscription_window_and_rejects_its_old_samples() {
    let mut ledger = MemoryQuotaLedger::default();
    assert!(matches!(
        ledger.record_quota(ObservedAt(15), sample("alpha", 10, 10, 8)),
        QuotaState::Known(_)
    ));
    assert!(matches!(
        ledger.record_quota(ObservedAt(25), sample("alpha", 20, 10, 4)),
        QuotaState::Known(_)
    ));
    assert_eq!(
        ledger.record_quota(ObservedAt(25), sample("alpha", 10, 10, 8)),
        QuotaState::Unknown(UnknownQuota::SupersededWindow)
    );
}

#[test]
fn future_timestamps_and_zero_intervals_are_explicitly_unknown() {
    let mut ledger = MemoryQuotaLedger::default();
    assert_eq!(
        ledger.record_quota(ObservedAt(9), sample("alpha", 10, 10, 4)),
        QuotaState::Unknown(UnknownQuota::FutureSourceTimestamp)
    );
    assert_eq!(
        ledger.record_quota(ObservedAt(10), sample("alpha", 10, 0, 4)),
        QuotaState::Unknown(UnknownQuota::ZeroRemainingInterval)
    );
}

#[test]
fn fixture_storage_uses_explicit_assert_then_mutate_operations() {
    let mut first = SemaEngineQuotaLedger {
        storage: RecordingQuotaWindowStorage {
            windows: BTreeMap::new(),
            writes: vec![],
        },
    };
    assert!(matches!(
        first.record_quota(ObservedAt(10), sample("subscription-a", 10, 10, 5)),
        QuotaState::Known(_)
    ));
    assert!(matches!(
        first.record_quota(ObservedAt(10), sample("subscription-a", 10, 10, 1)),
        QuotaState::Known(_)
    ));
    assert_eq!(
        first.storage.windows[&SubscriptionId("subscription-a".into())]
            .sample
            .remaining,
        RemainingAmount(1)
    );
    assert_eq!(
        first.storage.writes,
        vec![StorageWrite::Asserted, StorageWrite::Mutated]
    );
}

#[test]
fn nexus_receives_signal_while_the_cli_translation_seam_remains_outside_it() {
    let mut nexus = PersonaNexus {
        ledger: MemoryQuotaLedger::default(),
        ordinary_authority: SocketAuthority::Ordinary,
        meta_authority: SocketAuthority::Privileged,
    };
    let mut transport = RecordingSignalTransport { sent: vec![] };
    let reply = transport.send(
        &mut nexus,
        Signal::Query(Query::RecordQuota(sample("alpha", 10, 10, 4))),
        ObservedAt(10),
    );
    assert!(matches!(
        reply,
        Signal::Response(signal_persona::Response::QuotaRecorded(_))
    ));
    assert_eq!(transport.sent.len(), 1);
    assert_eq!(nexus.ordinary_authority.mode(), 0o660);
    assert_eq!(nexus.meta_authority.mode(), 0o600);
    assert!(nexus.meta_authority.admits(42, 42));
    assert!(!nexus.meta_authority.admits(7, 42));
}

#[test]
fn each_launch_hands_the_already_assembled_first_prompt_to_its_harness_once() {
    let launch = persona_nexus::FirstPromptLaunch {
        assembly_revision: 1,
    };
    let mut harness = FakeHarness {
        ordinary: vec![],
        meta: vec![],
    };
    launch.launch_ordinary(&mut harness, FirstPrompt("ordinary assembled".into()));
    launch.launch_meta(&mut harness, MetaFirstPrompt("meta assembled".into()));
    assert_eq!(
        harness.ordinary,
        vec![FirstPrompt("ordinary assembled".into())]
    );
    assert_eq!(harness.meta, vec![MetaFirstPrompt("meta assembled".into())]);
}
