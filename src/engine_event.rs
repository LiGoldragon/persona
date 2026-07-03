pub use meta_signal_persona::OperationKind as EngineOperationKind;
use meta_signal_persona::{ComponentName, EnginePhase};
use nota::{NotaDecode, NotaEncode};
pub use signal_harness::HarnessOperationKind;
pub use signal_message::MessageOperationKind;
pub use signal_mind::MindOperationKind;
use signal_persona::origin::EngineIdentifier;
pub use signal_system::SystemOperationKind;
pub use signal_terminal::TerminalOperationKind;
use strum::EnumDiscriminants;

use crate::upgrade::{ActiveVersionChanged, PreparedEvent, VersionQuarantined};

/// Monotonic event key scoped to one manager catalog.
///
/// The sequence is not per engine. It gives the manager log one total order
/// across every engine whose events are stored in the same `manager.sema`.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineEventSequence(u64);

impl EngineEventSequence {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn into_u64(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct EngineEvent {
    sequence: EngineEventSequence,
    engine: EngineIdentifier,
    source: EngineEventSource,
    body: EngineEventBody,
}

impl EngineEvent {
    pub fn from_input(input: EngineEventInput) -> Self {
        Self {
            sequence: input.sequence,
            engine: input.engine,
            source: input.source,
            body: input.body,
        }
    }

    pub fn sequence(&self) -> EngineEventSequence {
        self.sequence
    }

    pub fn engine(&self) -> &EngineIdentifier {
        &self.engine
    }

    pub fn source(&self) -> &EngineEventSource {
        &self.source
    }

    pub fn body(&self) -> &EngineEventBody {
        &self.body
    }

    pub fn key(&self) -> u64 {
        self.sequence.into_u64()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineEventInput {
    pub sequence: EngineEventSequence,
    pub engine: EngineIdentifier,
    pub source: EngineEventSource,
    pub body: EngineEventBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineEventDraft {
    engine: EngineIdentifier,
    source: EngineEventSource,
    body: EngineEventBody,
}

impl EngineEventDraft {
    pub fn from_input(input: EngineEventDraftInput) -> Self {
        Self {
            engine: input.engine,
            source: input.source,
            body: input.body,
        }
    }

    pub fn into_event(self, sequence: EngineEventSequence) -> EngineEvent {
        EngineEvent::from_input(EngineEventInput {
            sequence,
            engine: self.engine,
            source: self.source,
            body: self.body,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineEventDraftInput {
    pub engine: EngineIdentifier,
    pub source: EngineEventSource,
    pub body: EngineEventBody,
}

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
    EnumDiscriminants,
)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
#[strum_discriminants(name(EngineEventSourceKind))]
#[strum_discriminants(derive(NotaEncode, NotaDecode))]
pub enum EngineEventSource {
    Manager,
    /// Manager-observed component fact. The component does not write the log.
    Component(ComponentName),
}

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
    EnumDiscriminants,
)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
#[strum_discriminants(name(EngineEventBodyKind))]
#[strum_discriminants(derive(NotaEncode, NotaDecode))]
pub enum EngineEventBody {
    ComponentSpawned(ComponentLifecycleEvent),
    ComponentReady(ComponentLifecycleEvent),
    ComponentUnimplemented(ComponentUnimplemented),
    ComponentExited(ComponentExited),
    /// A `ComponentSpawned` recorded by a prior daemon run had no
    /// matching `ComponentReady` or `ComponentExited` before the
    /// daemon stopped. The current daemon's startup event-log replay
    /// found the open arc and is recording the orphan. Supervisor
    /// restart policy decides what happens next; this event is the
    /// audit witness that the orphan was detected, not silently lost.
    ComponentOrphaned(ComponentOrphaned),
    RestartScheduled(RestartScheduled),
    RestartExhausted(RestartExhausted),
    ComponentStopped(ComponentLifecycleEvent),
    EngineStateChanged(EngineStateChanged),
    UpgradePrepared(PreparedEvent),
    ActiveVersionChanged(ActiveVersionChanged),
    VersionQuarantined(VersionQuarantined),
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct ComponentLifecycleEvent {
    component: ComponentName,
}

impl ComponentLifecycleEvent {
    pub fn new(component: ComponentName) -> Self {
        Self { component }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct ComponentUnimplemented {
    component: ComponentName,
    operation: ComponentOperation,
    reason: UnimplementedReason,
}

impl ComponentUnimplemented {
    pub fn from_input(input: ComponentUnimplementedInput) -> Self {
        Self {
            component: input.component,
            operation: input.operation,
            reason: input.reason,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn operation(&self) -> &ComponentOperation {
        &self.operation
    }

    pub fn reason(&self) -> UnimplementedReason {
        self.reason
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentUnimplementedInput {
    pub component: ComponentName,
    pub operation: ComponentOperation,
    pub reason: UnimplementedReason,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub enum ComponentOperation {
    Engine(EngineOperationKind),
    Message(MessageOperationKind),
    Mind(MindOperationKind),
    System(SystemOperationKind),
    Harness(HarnessOperationKind),
    Terminal(TerminalOperationKind),
}

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    NotaEncode,
    NotaDecode,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
)]
pub enum UnimplementedReason {
    NotBuiltYet,
    /// Cross-cutting prerequisite work is not landed in the current stack.
    ///
    /// This does not mean a downstream component rejected the request; use a
    /// future component-specific variant if that runtime fact becomes needed.
    DependencyTrackNotLanded,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct ComponentExited {
    component: ComponentName,
    exit_code: Option<i32>,
}

impl ComponentExited {
    pub fn from_input(input: ComponentExitedInput) -> Self {
        Self {
            component: input.component,
            exit_code: input.exit_code,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentExitedInput {
    pub component: ComponentName,
    pub exit_code: Option<i32>,
}

/// The `(engine, component)` pair was last seen in
/// `ComponentProcessState::Launched` by a prior daemon run that never
/// recorded readiness or exit. The current daemon observed the open arc
/// during startup event-log replay and is recording the orphan. Carries
/// the sequence number of the `ComponentSpawned` event that started the
/// orphan arc so audit can correlate the two events.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct ComponentOrphaned {
    component: ComponentName,
    spawned_sequence: EngineEventSequence,
}

impl ComponentOrphaned {
    pub fn from_input(input: ComponentOrphanedInput) -> Self {
        Self {
            component: input.component,
            spawned_sequence: input.spawned_sequence,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn spawned_sequence(&self) -> EngineEventSequence {
        self.spawned_sequence
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentOrphanedInput {
    pub component: ComponentName,
    pub spawned_sequence: EngineEventSequence,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct RestartScheduled {
    component: ComponentName,
    attempt: u32,
}

impl RestartScheduled {
    pub fn from_input(input: RestartScheduledInput) -> Self {
        Self {
            component: input.component,
            attempt: input.attempt,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartScheduledInput {
    pub component: ComponentName,
    pub attempt: u32,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct RestartExhausted {
    component: ComponentName,
    attempts: u32,
}

impl RestartExhausted {
    pub fn from_input(input: RestartExhaustedInput) -> Self {
        Self {
            component: input.component,
            attempts: input.attempts,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartExhaustedInput {
    pub component: ComponentName,
    pub attempts: u32,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct EngineStateChanged {
    phase: EnginePhase,
}

impl EngineStateChanged {
    pub const fn new(phase: EnginePhase) -> Self {
        Self { phase }
    }

    pub const fn phase(self) -> EnginePhase {
        self.phase
    }
}
