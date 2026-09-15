use std::collections::BTreeMap;

use meta_signal_persona::FirstPrompt as MetaFirstPrompt;
use persona_nexus::{
    DraftLaunch, HarnessFirstPrompt, MemoryQuotaLedger, OrdinarySocket, PersonaNexus, QuotaLedger,
    QuotaState, SemaEngineQuotaLedger, SemaEngineQuotaStore,
};
use signal_persona::{
    FirstPrompt, ObservedAt, Query, QuotaSample, RemainingAmount, ResetInterval, Signal,
    SourceTimestamp, SubscriptionId, UnknownQuota,
};

struct FakeSemaStore {
    windows: BTreeMap<SubscriptionId, persona_nexus::QuotaWindow>,
}

impl SemaEngineQuotaStore for FakeSemaStore {
    fn replace_window(&mut self, subscription: SubscriptionId, window: persona_nexus::QuotaWindow) {
        self.windows.insert(subscription, window);
    }

    fn window(&self, subscription: &SubscriptionId) -> Option<persona_nexus::QuotaWindow> {
        self.windows.get(subscription).cloned()
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
fn independent_fixture_ledgers_keep_subscription_windows_separate() {
    let mut first = SemaEngineQuotaLedger {
        store: FakeSemaStore {
            windows: BTreeMap::new(),
        },
        engine: None,
    };
    let mut second = SemaEngineQuotaLedger {
        store: FakeSemaStore {
            windows: BTreeMap::new(),
        },
        engine: None,
    };
    assert!(matches!(
        first.record_quota(ObservedAt(10), sample("subscription-a", 10, 10, 5)),
        QuotaState::Known(_)
    ));
    assert!(matches!(
        second.record_quota(ObservedAt(10), sample("subscription-a", 10, 10, 1)),
        QuotaState::Known(_)
    ));
    assert_eq!(
        first.store.windows[&SubscriptionId("subscription-a".into())]
            .sample
            .remaining,
        RemainingAmount(5)
    );
    assert_eq!(
        second.store.windows[&SubscriptionId("subscription-a".into())]
            .sample
            .remaining,
        RemainingAmount(1)
    );
}

#[test]
fn nexus_receives_signal_while_the_cli_translation_seam_remains_outside_it() {
    let mut nexus = PersonaNexus {
        ledger: MemoryQuotaLedger::default(),
    };
    let reply = nexus.receive_signal(
        Signal::Query(Query::RecordQuota(sample("alpha", 10, 10, 4))),
        ObservedAt(10),
    );
    assert!(matches!(
        reply,
        Signal::Response(signal_persona::Response::QuotaRecorded(_))
    ));
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
