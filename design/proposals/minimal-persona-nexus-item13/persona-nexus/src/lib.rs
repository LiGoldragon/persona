//! Item 13 draft: typed socket boundaries, a quota-ledger seam, and one-call launch seams.
//! No socket, process, provider, account, or Sema store is opened by this crate.

use std::collections::BTreeMap;

use meta_signal_persona::{FirstPrompt as MetaFirstPrompt, MetaSignal};
use nexus::SocketAuthority;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use sema_engine::{
    CommitRequest, Engine, EngineRecord, FamilyName, RecordKey, SchemaHash, TableDescriptor,
    TableName, TableReference,
};
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
    StorageUnavailable,
}

#[derive(Clone, Debug, Default)]
pub struct MemoryQuotaLedger {
    windows: BTreeMap<SubscriptionId, QuotaWindow>,
}

/// The narrow storage surface the ledger needs. The concrete adapter below
/// calls sema-engine's registered-table, read-only reader, and commit APIs.
pub trait QuotaWindowStorage {
    fn load_window(
        &self,
        subscription: &SubscriptionId,
    ) -> Result<Option<QuotaWindow>, sema_engine::Error>;
    fn store_window(
        &mut self,
        prior: Option<&QuotaWindow>,
        window: QuotaWindow,
    ) -> Result<(), sema_engine::Error>;
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Clone, Debug)]
#[rkyv(derive(Debug))]
struct StoredQuotaWindow {
    subscription: String,
    started_at: u64,
    source_timestamp: u64,
    reset_interval: u64,
    remaining: u64,
}

impl StoredQuotaWindow {
    fn from_window(window: QuotaWindow) -> Self {
        Self {
            subscription: window.sample.subscription.0,
            started_at: window.started_at.0,
            source_timestamp: window.sample.source_timestamp.0,
            reset_interval: window.sample.reset_interval.0,
            remaining: window.sample.remaining.0,
        }
    }

    fn into_window(self) -> QuotaWindow {
        QuotaWindow {
            started_at: SourceTimestamp(self.started_at),
            sample: QuotaSample {
                subscription: SubscriptionId(self.subscription),
                source_timestamp: SourceTimestamp(self.source_timestamp),
                reset_interval: ResetInterval(self.reset_interval),
                remaining: signal_persona::RemainingAmount(self.remaining),
            },
        }
    }

    fn table() -> TableReference<Self> {
        TableReference::new(TableName::new("persona_quota_windows"))
    }

    fn descriptor() -> TableDescriptor<Self> {
        TableDescriptor::new(
            TableName::new("persona_quota_windows"),
            FamilyName::new("persona_quota_window"),
            SchemaHash::for_label("persona-quota-window-v1"),
        )
    }
}

impl EngineRecord for StoredQuotaWindow {
    fn record_key(&self) -> RecordKey {
        RecordKey::domain(self.subscription.clone())
    }
}

/// A durable adapter. Registration is explicit; opening an `Engine` remains
/// the owning Nexus's runtime work and is intentionally absent from this draft.
pub struct SemaEngineQuotaStorage {
    engine: Engine,
}

impl SemaEngineQuotaStorage {
    pub fn register(mut engine: Engine) -> Result<Self, sema_engine::Error> {
        engine.register_table(StoredQuotaWindow::descriptor())?;
        Ok(Self { engine })
    }
}

impl QuotaWindowStorage for SemaEngineQuotaStorage {
    fn load_window(
        &self,
        subscription: &SubscriptionId,
    ) -> Result<Option<QuotaWindow>, sema_engine::Error> {
        let key = subscription.0.clone();
        let stored = self.engine.storage_reader().read(|transaction| {
            StoredQuotaWindow::table()
                .sema_table()
                .get(transaction, key)
        })?;
        Ok(stored.map(StoredQuotaWindow::into_window))
    }

    fn store_window(
        &mut self,
        prior: Option<&QuotaWindow>,
        window: QuotaWindow,
    ) -> Result<(), sema_engine::Error> {
        let request = CommitRequest::new(StoredQuotaWindow::table());
        let record = StoredQuotaWindow::from_window(window);
        let request = if prior.is_some() {
            request.mutate(record)
        } else {
            request.assert(record)
        };
        self.engine.commit(request)?;
        Ok(())
    }
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

pub struct SemaEngineQuotaLedger<Storage> {
    pub storage: Storage,
}

impl<Storage: QuotaWindowStorage> QuotaReader for SemaEngineQuotaLedger<Storage> {
    fn quota_state(&self, subscription: &SubscriptionId) -> Option<QuotaState> {
        self.storage
            .load_window(subscription)
            .ok()
            .flatten()
            .map(QuotaState::Known)
    }
}

impl<Storage: QuotaWindowStorage> QuotaLedger for SemaEngineQuotaLedger<Storage> {
    fn record_quota(&mut self, observed_at: ObservedAt, sample: QuotaSample) -> QuotaState {
        let prior = match self.storage.load_window(&sample.subscription) {
            Ok(prior) => prior,
            Err(_) => return QuotaState::StorageUnavailable,
        };
        let mut decision = MemoryQuotaLedger::default();
        if let Some(window) = prior.clone() {
            decision.windows.insert(sample.subscription.clone(), window);
        }
        let state = decision.record_quota(observed_at, sample);
        if let QuotaState::Known(window) = &state {
            if self
                .storage
                .store_window(prior.as_ref(), window.clone())
                .is_err()
            {
                return QuotaState::StorageUnavailable;
            }
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
    pub ordinary_authority: SocketAuthority,
    pub meta_authority: SocketAuthority,
}

impl<Ledger: QuotaLedger> OrdinarySocket for PersonaNexus<Ledger> {
    fn receive_signal(&mut self, input: Signal, observed_at: ObservedAt) -> Signal {
        match input {
            Signal::Query(Query::RecordQuota(sample)) => {
                match self.ledger.record_quota(observed_at, sample.clone()) {
                    QuotaState::Known(_) => Signal::Response(Response::QuotaRecorded(sample)),
                    QuotaState::Unknown(reason) => Signal::Response(Response::QuotaUnknown(reason)),
                    QuotaState::StorageUnavailable => {
                        Signal::Response(Response::QuotaUnknown(UnknownQuota::StorageUnavailable))
                    }
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
