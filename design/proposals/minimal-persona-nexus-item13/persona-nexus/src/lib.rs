//! Item 13 draft: typed socket boundaries, a quota-ledger seam, and one-call launch seams.
//! No socket, process, provider, account, or Sema store is opened by this crate.

use std::collections::BTreeMap;

use meta_signal_persona::{FirstPrompt as MetaFirstPrompt, MetaSignal};
use signal_persona::{
    FirstPrompt, InlineDatom, ObservedAt, Query, QuotaSample, ResetInterval, Response, Signal,
    SourceTimestamp, SubscriptionId, UnknownQuota,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaWindow {
    pub started_at: SourceTimestamp,
    pub sample: QuotaSample,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuotaState {
    Known(QuotaWindow),
    Unknown(UnknownQuota),
}

#[derive(Clone, Debug, Default)]
pub struct MemoryQuotaLedger {
    windows: BTreeMap<SubscriptionId, QuotaWindow>,
}

/// A port for the sema-engine-backed adapter. Fixture implementations are pure.
pub trait SemaEngineQuotaStore {
    fn replace_window(&mut self, subscription: SubscriptionId, window: QuotaWindow);
    fn window(&self, subscription: &SubscriptionId) -> Option<QuotaWindow>;
}

pub trait QuotaLedger {
    fn record_quota(&mut self, observed_at: ObservedAt, sample: QuotaSample) -> QuotaState;
}

pub trait QuotaReader {
    fn quota_state(&self, subscription: &SubscriptionId) -> Option<QuotaState>;
}

pub trait WindowDerivation {
    fn window_start(&self, sample: &QuotaSample) -> Option<SourceTimestamp>;
}

pub struct FixedWindowDerivation {
    pub rule_revision: u8,
}

impl WindowDerivation for FixedWindowDerivation {
    fn window_start(&self, sample: &QuotaSample) -> Option<SourceTimestamp> {
        let ResetInterval(interval) = sample.reset_interval;
        if interval == 0 {
            None
        } else {
            Some(SourceTimestamp(
                sample.source_timestamp.0 - (sample.source_timestamp.0 % interval),
            ))
        }
    }
}

impl QuotaLedger for MemoryQuotaLedger {
    fn record_quota(&mut self, observed_at: ObservedAt, sample: QuotaSample) -> QuotaState {
        if sample.source_timestamp.0 > observed_at.0 {
            return QuotaState::Unknown(UnknownQuota::FutureSourceTimestamp);
        }
        let derivation = FixedWindowDerivation { rule_revision: 1 };
        let Some(started_at) = derivation.window_start(&sample) else {
            return QuotaState::Unknown(UnknownQuota::ZeroRemainingInterval);
        };
        if self
            .windows
            .get(&sample.subscription)
            .is_some_and(|stored| stored.started_at.0 > started_at.0)
        {
            return QuotaState::Unknown(UnknownQuota::SupersededWindow);
        }
        let window = QuotaWindow { started_at, sample };
        self.windows
            .insert(window.sample.subscription.clone(), window.clone());
        QuotaState::Known(window)
    }
}

impl QuotaReader for MemoryQuotaLedger {
    fn quota_state(&self, subscription: &SubscriptionId) -> Option<QuotaState> {
        self.windows
            .get(subscription)
            .cloned()
            .map(QuotaState::Known)
    }
}

/// The production adapter is the only place that may bind this port to sema-engine.
/// This item-13 draft keeps it generic so its tests require no durable resource.
pub struct SemaEngineQuotaLedger<Store> {
    pub store: Store,
    /// None in pure fixtures; a production adapter owns the opened sema-engine.
    pub engine: Option<sema_engine::Engine>,
}

impl<Store: SemaEngineQuotaStore> QuotaReader for SemaEngineQuotaLedger<Store> {
    fn quota_state(&self, subscription: &SubscriptionId) -> Option<QuotaState> {
        self.store.window(subscription).map(QuotaState::Known)
    }
}

impl<Store: SemaEngineQuotaStore> QuotaLedger for SemaEngineQuotaLedger<Store> {
    fn record_quota(&mut self, observed_at: ObservedAt, sample: QuotaSample) -> QuotaState {
        let mut decision = MemoryQuotaLedger::default();
        if let Some(window) = self.store.window(&sample.subscription) {
            decision.windows.insert(sample.subscription.clone(), window);
        }
        let state = decision.record_quota(observed_at, sample);
        if let QuotaState::Known(window) = &state {
            self.store
                .replace_window(window.sample.subscription.clone(), window.clone());
        }
        state
    }
}

pub trait InlineDatomTranslation {
    fn translate_ordinary(&self, input: InlineDatom) -> Signal;
    fn translate_meta(&self, input: InlineDatom) -> MetaSignal;
}

pub trait OrdinarySocket {
    fn receive_signal(&mut self, input: Signal, observed_at: ObservedAt) -> Signal;
}

pub trait MetaSocket {
    fn receive_meta_signal(&mut self, input: MetaSignal) -> MetaSignal;
}

pub struct PersonaNexus<Ledger> {
    pub ledger: Ledger,
}

impl<Ledger: QuotaLedger> OrdinarySocket for PersonaNexus<Ledger> {
    fn receive_signal(&mut self, input: Signal, observed_at: ObservedAt) -> Signal {
        match input {
            Signal::Query(Query::RecordQuota(sample)) => {
                match self.ledger.record_quota(observed_at, sample.clone()) {
                    QuotaState::Known(_) => Signal::Response(Response::QuotaRecorded(sample)),
                    QuotaState::Unknown(reason) => Signal::Response(Response::QuotaUnknown(reason)),
                }
            }
            Signal::Query(Query::AssembleFirstPrompt(prompt)) => {
                Signal::Response(Response::FirstPromptAssembled(prompt))
            }
            signal => signal,
        }
    }
}

impl<Ledger> MetaSocket for PersonaNexus<Ledger> {
    fn receive_meta_signal(&mut self, input: MetaSignal) -> MetaSignal {
        input
    }
}

pub trait HarnessFirstPrompt {
    fn hand_first_prompt(&mut self, prompt: FirstPrompt);
    fn hand_meta_first_prompt(&mut self, prompt: MetaFirstPrompt);
}

pub trait DraftLaunch {
    fn launch_ordinary<Harness: HarnessFirstPrompt>(
        &self,
        harness: &mut Harness,
        assembled: FirstPrompt,
    );
    fn launch_meta<Harness: HarnessFirstPrompt>(
        &self,
        harness: &mut Harness,
        assembled: MetaFirstPrompt,
    );
}

pub struct FirstPromptLaunch {
    pub assembly_revision: u8,
}

impl DraftLaunch for FirstPromptLaunch {
    fn launch_ordinary<Harness: HarnessFirstPrompt>(
        &self,
        harness: &mut Harness,
        assembled: FirstPrompt,
    ) {
        harness.hand_first_prompt(assembled);
    }

    fn launch_meta<Harness: HarnessFirstPrompt>(
        &self,
        harness: &mut Harness,
        assembled: MetaFirstPrompt,
    ) {
        harness.hand_meta_first_prompt(assembled);
    }
}
