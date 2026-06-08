use meta_signal_persona as contract;
use meta_signal_upgrade::{ForceReason, QuarantineReason, RollbackReason};
use nota_next::{NotaDecode, NotaEncode, NotaSource};
use signal_persona_origin::EngineIdentifier;

pub use crate::engine_event::{EngineEventBodyKind, EngineEventSourceKind};
pub use contract::{
    ActionRejectionReason, ComponentDesiredState, ComponentHealth, ComponentKind, ComponentName,
    EnginePhase,
};

use crate::engine_event::{
    ComponentOperation, EngineEvent, EngineEventBody, EngineEventSource, EngineOperationKind,
    HarnessOperationKind, MessageOperationKind, MindOperationKind, SystemOperationKind,
    TerminalOperationKind, UnimplementedReason,
};
use crate::upgrade::ActiveVersionChangeSource;

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct EngineEventReport {
    pub sequence: u64,
    pub engine: EngineIdentifier,
    pub source: EngineEventSourceKind,
    pub source_component: Option<ComponentName>,
    pub body: EngineEventBodyReport,
}

impl EngineEventReport {
    pub fn from_event(event: &EngineEvent) -> Self {
        Self {
            sequence: event.sequence().into_u64(),
            engine: event.engine().clone(),
            source: event.source().into(),
            source_component: EngineEventSourceComponent::from_event_source(event.source())
                .into_option(),
            body: EngineEventBodyReport::from_event_body(event.body()),
        }
    }

    pub fn from_nota(text: &str) -> Result<Self, nota_next::NotaDecodeError> {
        NotaSource::new(text).parse::<Self>()
    }

    pub fn to_nota(&self) -> String {
        NotaEncode::to_nota(self)
    }
}

struct EngineEventSourceComponent {
    component: Option<ComponentName>,
}

impl EngineEventSourceComponent {
    fn from_event_source(source: &EngineEventSource) -> Self {
        let component = match source {
            EngineEventSource::Manager => None,
            EngineEventSource::Component(component) => Some(component.clone()),
        };
        Self { component }
    }

    fn into_option(self) -> Option<ComponentName> {
        self.component
    }
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ComponentLifecycleEventReport {
    pub component: ComponentName,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ComponentUnimplementedReport {
    pub component: ComponentName,
    pub operation: ComponentOperationReport,
    pub reason: UnimplementedReason,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ComponentExitedReport {
    pub component: ComponentName,
    pub exit_code: Option<u64>,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ComponentOrphanedReport {
    pub component: ComponentName,
    pub spawned_sequence: u64,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct RestartScheduledReport {
    pub component: ComponentName,
    pub attempt: u64,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct RestartExhaustedReport {
    pub component: ComponentName,
    pub attempts: u64,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineStateChangedReport {
    pub phase: EnginePhase,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct UpgradePreparedReport {
    pub component: ComponentName,
    pub current_version: String,
    pub next_version: String,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ActiveVersionChangedReport {
    pub component: ComponentName,
    pub active_version: String,
    pub source: ActiveVersionChangeSourceReport,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub enum ActiveVersionChangeSourceReport {
    HandoverMarker(HandoverMarkerSourceReport),
    ForceFlip(ForceFlipSourceReport),
    Rollback(RollbackSourceReport),
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct HandoverMarkerSourceReport {
    pub state_sequence: u64,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ForceFlipSourceReport {
    pub reason: ForceReason,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct RollbackSourceReport {
    pub reason: RollbackReason,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct VersionQuarantinedReport {
    pub component: ComponentName,
    pub version: String,
    pub reason: QuarantineReason,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub enum EngineEventBodyReport {
    ComponentSpawned(ComponentLifecycleEventReport),
    ComponentReady(ComponentLifecycleEventReport),
    ComponentUnimplemented(ComponentUnimplementedReport),
    ComponentExited(ComponentExitedReport),
    ComponentOrphaned(ComponentOrphanedReport),
    RestartScheduled(RestartScheduledReport),
    RestartExhausted(RestartExhaustedReport),
    ComponentStopped(ComponentLifecycleEventReport),
    EngineStateChanged(EngineStateChangedReport),
    UpgradePrepared(UpgradePreparedReport),
    ActiveVersionChanged(ActiveVersionChangedReport),
    VersionQuarantined(VersionQuarantinedReport),
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentOperationReport {
    Engine(EngineOperationKind),
    Message(MessageOperationKind),
    Mind(MindOperationKind),
    System(SystemOperationKind),
    Harness(HarnessOperationKind),
    Terminal(TerminalOperationKind),
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct EngineStatusReport {
    pub generation: contract::EngineGeneration,
    pub phase: EnginePhase,
    pub components: Vec<contract::ComponentStatus>,
}

impl EngineStatusReport {
    pub fn from_contract(status: contract::EngineStatus) -> Self {
        Self {
            generation: status.generation,
            phase: status.phase,
            components: status.components,
        }
    }

    pub fn from_nota(text: &str) -> Result<Self, nota_next::NotaDecodeError> {
        NotaSource::new(text).parse::<Self>()
    }

    pub fn to_nota(&self) -> String {
        NotaEncode::to_nota(self)
    }
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusReport {
    pub component: contract::ComponentStatus,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusMissingReport {
    pub component: ComponentName,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct RetirementAcceptanceReport {
    pub engine: EngineIdentifier,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ActionAcceptedReport {
    pub component: ComponentName,
    pub desired_state: ComponentDesiredState,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ActionRejectedReport {
    pub component: ComponentName,
    pub reason: ActionRejectionReason,
}

impl EngineEventBodyReport {
    pub fn from_event_body(body: &EngineEventBody) -> Self {
        match body {
            EngineEventBody::ComponentSpawned(event) => Self::ComponentSpawned(
                ComponentLifecycleEventReport::from_component(event.component()),
            ),
            EngineEventBody::ComponentReady(event) => Self::ComponentReady(
                ComponentLifecycleEventReport::from_component(event.component()),
            ),
            EngineEventBody::ComponentUnimplemented(event) => {
                Self::ComponentUnimplemented(ComponentUnimplementedReport {
                    component: event.component().clone(),
                    operation: ComponentOperationReport::from_operation(event.operation()),
                    reason: event.reason(),
                })
            }
            EngineEventBody::ComponentExited(event) => {
                Self::ComponentExited(ComponentExitedReport {
                    component: event.component().clone(),
                    exit_code: event.exit_code().and_then(|code| u64::try_from(code).ok()),
                })
            }
            EngineEventBody::ComponentOrphaned(event) => {
                Self::ComponentOrphaned(ComponentOrphanedReport {
                    component: event.component().clone(),
                    spawned_sequence: event.spawned_sequence().into_u64(),
                })
            }
            EngineEventBody::RestartScheduled(event) => {
                Self::RestartScheduled(RestartScheduledReport {
                    component: event.component().clone(),
                    attempt: u64::from(event.attempt()),
                })
            }
            EngineEventBody::RestartExhausted(event) => {
                Self::RestartExhausted(RestartExhaustedReport {
                    component: event.component().clone(),
                    attempts: u64::from(event.attempts()),
                })
            }
            EngineEventBody::ComponentStopped(event) => Self::ComponentStopped(
                ComponentLifecycleEventReport::from_component(event.component()),
            ),
            EngineEventBody::EngineStateChanged(event) => {
                Self::EngineStateChanged(EngineStateChangedReport {
                    phase: event.phase(),
                })
            }
            EngineEventBody::UpgradePrepared(event) => {
                Self::UpgradePrepared(UpgradePreparedReport {
                    component: ComponentName::new(event.component().as_str()),
                    current_version: event.current_version().as_str().to_string(),
                    next_version: event.next_version().as_str().to_string(),
                })
            }
            EngineEventBody::ActiveVersionChanged(event) => {
                Self::ActiveVersionChanged(ActiveVersionChangedReport {
                    component: ComponentName::new(event.component().as_str()),
                    active_version: event.active_version().as_str().to_string(),
                    source: ActiveVersionChangeSourceReport::from_source(event.source()),
                })
            }
            EngineEventBody::VersionQuarantined(event) => {
                Self::VersionQuarantined(VersionQuarantinedReport {
                    component: ComponentName::new(event.component().as_str()),
                    version: event.version().as_str().to_string(),
                    reason: event.reason(),
                })
            }
        }
    }
}

impl ActiveVersionChangeSourceReport {
    fn from_source(source: &ActiveVersionChangeSource) -> Self {
        match source {
            ActiveVersionChangeSource::HandoverMarker { state_sequence } => {
                Self::HandoverMarker(HandoverMarkerSourceReport {
                    state_sequence: *state_sequence,
                })
            }
            ActiveVersionChangeSource::ForceFlip { reason } => {
                Self::ForceFlip(ForceFlipSourceReport { reason: *reason })
            }
            ActiveVersionChangeSource::Rollback { reason } => {
                Self::Rollback(RollbackSourceReport { reason: *reason })
            }
        }
    }
}

impl ComponentLifecycleEventReport {
    pub fn from_component(component: &contract::ComponentName) -> Self {
        Self {
            component: component.clone(),
        }
    }
}

impl ComponentOperationReport {
    pub fn from_operation(operation: &ComponentOperation) -> Self {
        match operation {
            ComponentOperation::Engine(kind) => Self::Engine(*kind),
            ComponentOperation::Message(kind) => Self::Message(*kind),
            ComponentOperation::Mind(kind) => Self::Mind(*kind),
            ComponentOperation::System(kind) => Self::System(*kind),
            ComponentOperation::Harness(kind) => Self::Harness(*kind),
            ComponentOperation::Terminal(kind) => Self::Terminal(*kind),
        }
    }
}
