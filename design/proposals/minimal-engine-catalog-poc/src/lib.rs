//! Draft-only in-memory catalog. No provider or runtime integration belongs here.

use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EngineId(pub String);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Generation(pub u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ObservedAt(pub u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceTimestamp(pub u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ResetInterval(pub u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RemainingAmount(pub u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PaceAmount(pub u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PaceInterval(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetInterpretation {
    FixedWindow,
    RollingWindow,
    ProviderDefined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuotaPace {
    pub amount: PaceAmount,
    pub interval: PaceInterval,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaSnapshot {
    pub source_timestamp: SourceTimestamp,
    pub reset_interval: ResetInterval,
    pub remaining: RemainingAmount,
    pub pace: QuotaPace,
    pub reset_interpretation: ResetInterpretation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesiredEngine {
    pub id: EngineId,
    pub generation: Generation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedEngine {
    pub id: EngineId,
    pub generation: Generation,
    pub observed_at: ObservedAt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservationOutcome {
    Current,
    GenerationMismatch,
    StaleObservation,
    UnknownEngine,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationReport {
    pub desired: Generation,
    pub observed: Option<ObservedEngine>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaReport {
    pub engine: EngineId,
    pub snapshot: QuotaSnapshot,
}

#[derive(Clone, Debug, Default)]
pub struct InMemoryCatalog {
    desired: BTreeMap<EngineId, DesiredEngine>,
    observed: BTreeMap<EngineId, ObservedEngine>,
    quotas: BTreeMap<EngineId, QuotaSnapshot>,
}

pub trait DesiredCatalog {
    fn set_desired(&mut self, desired: DesiredEngine);
}

pub trait ObservedCatalog {
    fn record_observation(&mut self, observation: ObservedEngine) -> ObservationOutcome;
}

pub trait GenerationReporting {
    fn generation_report(&self, engine: &EngineId) -> Option<GenerationReport>;
}

pub trait QuotaCatalog {
    fn record_quota(&mut self, engine: EngineId, snapshot: QuotaSnapshot);
}

pub trait QuotaReporting {
    fn quota_report(&self, engine: &EngineId) -> Option<QuotaReport>;
}

impl DesiredCatalog for InMemoryCatalog {
    fn set_desired(&mut self, desired: DesiredEngine) {
        self.desired.insert(desired.id.clone(), desired);
    }
}

impl ObservedCatalog for InMemoryCatalog {
    fn record_observation(&mut self, observation: ObservedEngine) -> ObservationOutcome {
        let Some(desired) = self.desired.get(&observation.id) else {
            return ObservationOutcome::UnknownEngine;
        };
        if self
            .observed
            .get(&observation.id)
            .is_some_and(|stored| stored.observed_at >= observation.observed_at)
        {
            return ObservationOutcome::StaleObservation;
        }
        let outcome = if desired.generation == observation.generation {
            ObservationOutcome::Current
        } else {
            ObservationOutcome::GenerationMismatch
        };
        self.observed.insert(observation.id.clone(), observation);
        outcome
    }
}

impl GenerationReporting for InMemoryCatalog {
    fn generation_report(&self, engine: &EngineId) -> Option<GenerationReport> {
        self.desired.get(engine).map(|desired| GenerationReport {
            desired: desired.generation,
            observed: self.observed.get(engine).cloned(),
        })
    }
}

impl QuotaCatalog for InMemoryCatalog {
    fn record_quota(&mut self, engine: EngineId, snapshot: QuotaSnapshot) {
        self.quotas.insert(engine, snapshot);
    }
}

impl QuotaReporting for InMemoryCatalog {
    fn quota_report(&self, engine: &EngineId) -> Option<QuotaReport> {
        self.quotas
            .get(engine)
            .cloned()
            .map(|snapshot| QuotaReport {
                engine: engine.clone(),
                snapshot,
            })
    }
}
