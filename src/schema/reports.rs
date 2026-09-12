use crate::datom_text::{DatomActualizable, DatomTextualizable};
use meta_signal_persona as contract;
use meta_signal_upgrade::{ForceReason, QuarantineReason, RollbackReason};
use signal_persona::EngineIdentifier;

pub use crate::engine_event::{EngineEventBodyKind, EngineEventSourceKind};
pub use contract::EnginePhase;
pub use signal_persona::{ComponentDesiredState, ComponentHealth, ComponentKind, ComponentName};

use crate::engine_event::{
    ComponentOperation, EngineEvent, EngineEventBody, EngineEventSource, HarnessOperationKind,
    MessageOperationKind, SystemOperationKind, TerminalOperationKind, UnimplementedReason,
};
use crate::upgrade::ActiveVersionChangeSource;

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct EngineEventReport {
    pub sequence: i64,
    pub engine: EngineIdentifier,
    pub source: EngineEventSourceKind,
    pub source_component: Option<ComponentName>,
    pub body: EngineEventBodyReport,
}

impl EngineEventReport {
    pub fn from_event(event: &EngineEvent) -> Self {
        Self {
            sequence: event.sequence().into_u64() as i64,
            engine: event.engine().clone(),
            source: event.source().into(),
            source_component: EngineEventSourceComponent::from_event_source(event.source())
                .into_option(),
            body: EngineEventBodyReport::from_event_body(event.body()),
        }
    }

    pub fn actualize(text: &str) -> Result<Self, datom_codec::Error> {
        <Self as DatomActualizable>::actualize_text(text)
    }

    pub fn textualize(&self) -> String {
        DatomTextualizable::textualize(self)
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

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct ComponentLifecycleEventReport {
    pub component: ComponentName,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct ComponentUnimplementedReport {
    pub component: ComponentName,
    pub operation: ComponentOperationReport,
    pub reason: UnimplementedReason,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct ComponentExitedReport {
    pub component: ComponentName,
    pub exit_code: Option<i64>,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct ComponentOrphanedReport {
    pub component: ComponentName,
    pub spawned_sequence: i64,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct RestartScheduledReport {
    pub component: ComponentName,
    pub attempt: i64,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct RestartExhaustedReport {
    pub component: ComponentName,
    pub attempts: i64,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct EngineStateChangedReport {
    pub phase: String,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct UpgradePreparedReport {
    pub component: ComponentName,
    pub current_version: String,
    pub next_version: String,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct ActiveVersionChangedReport {
    pub component: ComponentName,
    pub active_version: String,
    pub source: ActiveVersionChangeSourceReport,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub enum ActiveVersionChangeSourceReport {
    HandoverMarker(HandoverMarkerSourceReport),
    ForceFlip(ForceFlipSourceReport),
    Rollback(RollbackSourceReport),
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct HandoverMarkerSourceReport {
    pub state_sequence: i64,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct ForceFlipSourceReport {
    pub reason: ForceReason,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct RollbackSourceReport {
    pub reason: RollbackReason,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct VersionQuarantinedReport {
    pub component: ComponentName,
    pub version: String,
    pub reason: QuarantineReason,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
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

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub enum ComponentOperationReport {
    Message(MessageOperationKind),
    System(SystemOperationKind),
    Harness(HarnessOperationKind),
    Terminal(TerminalOperationKind),
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct EngineStatusReport {
    pub generation: i64,
    pub phase: String,
    pub components: Vec<LifecycleComponentStatusReport>,
}

impl EngineStatusReport {
    pub fn from_contract(status: contract::EngineStatusReport) -> Self {
        Self {
            generation: status.engine_generation,
            phase: format!("{:?}", status.engine_phase),
            components: status
                .component_status_vector
                .into_iter()
                .map(LifecycleComponentStatusReport::from_contract)
                .collect(),
        }
    }

    pub fn actualize(text: &str) -> Result<Self, datom_codec::Error> {
        <Self as DatomActualizable>::actualize_text(text)
    }

    pub fn textualize(&self) -> String {
        DatomTextualizable::textualize(self)
    }
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct ComponentStatusReport {
    pub component: LifecycleComponentStatusReport,
}

impl ComponentStatusReport {
    pub fn from_contract(status: signal_persona::ComponentStatus) -> Self {
        Self {
            component: LifecycleComponentStatusReport::from_contract(status),
        }
    }
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct ComponentStatusMissingReport {
    pub component: ComponentName,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct RetirementAcceptanceReport {
    pub engine: EngineIdentifier,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct ActionAcceptedReport {
    pub component: ComponentName,
    pub desired_state: String,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct ActionRejectedReport {
    pub component: ComponentName,
    pub reason: String,
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct LifecycleComponentStatusReport {
    pub component: ComponentName,
    pub kind: String,
    pub desired_state: String,
    pub health: String,
}

impl LifecycleComponentStatusReport {
    pub fn from_contract(status: signal_persona::ComponentStatus) -> Self {
        Self {
            component: status.component_name,
            kind: format!("{:?}", status.component_kind),
            desired_state: format!("{:?}", status.component_desired_state),
            health: format!("{:?}", status.component_health),
        }
    }
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct LaunchAcceptanceReport {
    pub engine: EngineIdentifier,
    pub label: String,
}

impl LaunchAcceptanceReport {
    pub fn from_contract(acceptance: contract::LaunchAcceptance) -> Self {
        Self {
            engine: acceptance.engine_identifier,
            label: acceptance.engine_label,
        }
    }
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct LaunchRejectionReport {
    pub label: String,
    pub reason: String,
}

impl LaunchRejectionReport {
    pub fn from_contract(rejection: contract::LaunchRejection) -> Self {
        Self {
            label: rejection.engine_label,
            reason: format!("{:?}", rejection.launch_rejection_reason),
        }
    }
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct EngineCatalogReport {
    pub engines: Vec<EngineCatalogEntryReport>,
}

impl EngineCatalogReport {
    pub fn from_contract(catalog: contract::EngineCatalog) -> Self {
        Self {
            engines: catalog
                .into_iter()
                .map(EngineCatalogEntryReport::from_contract)
                .collect(),
        }
    }
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct EngineCatalogEntryReport {
    pub engine: EngineIdentifier,
    pub label: String,
    pub phase: String,
}

impl EngineCatalogEntryReport {
    pub fn from_contract(entry: contract::EngineCatalogEntry) -> Self {
        Self {
            engine: entry.engine_identifier,
            label: entry.engine_label,
            phase: format!("{:?}", entry.engine_phase),
        }
    }
}

#[derive(datom_codec::Datomizable, datom_codec::Composing, Debug, Clone, PartialEq)]
pub struct RetirementRejectionReport {
    pub engine: EngineIdentifier,
    pub reason: String,
}

impl RetirementRejectionReport {
    pub fn from_contract(rejection: contract::RetirementRejection) -> Self {
        Self {
            engine: rejection.engine_identifier,
            reason: format!("{:?}", rejection.retirement_rejection_reason),
        }
    }
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
                    exit_code: event.exit_code().map(i64::from),
                })
            }
            EngineEventBody::ComponentOrphaned(event) => {
                Self::ComponentOrphaned(ComponentOrphanedReport {
                    component: event.component().clone(),
                    spawned_sequence: event.spawned_sequence().into_u64() as i64,
                })
            }
            EngineEventBody::RestartScheduled(event) => {
                Self::RestartScheduled(RestartScheduledReport {
                    component: event.component().clone(),
                    attempt: i64::from(event.attempt()),
                })
            }
            EngineEventBody::RestartExhausted(event) => {
                Self::RestartExhausted(RestartExhaustedReport {
                    component: event.component().clone(),
                    attempts: i64::from(event.attempts()),
                })
            }
            EngineEventBody::ComponentStopped(event) => Self::ComponentStopped(
                ComponentLifecycleEventReport::from_component(event.component()),
            ),
            EngineEventBody::EngineStateChanged(event) => {
                Self::EngineStateChanged(EngineStateChangedReport {
                    phase: format!("{:?}", event.phase()),
                })
            }
            EngineEventBody::UpgradePrepared(event) => {
                Self::UpgradePrepared(UpgradePreparedReport {
                    component: event.component().clone(),
                    current_version: event.current_version().as_str().to_string(),
                    next_version: event.next_version().as_str().to_string(),
                })
            }
            EngineEventBody::ActiveVersionChanged(event) => {
                Self::ActiveVersionChanged(ActiveVersionChangedReport {
                    component: event.component().clone(),
                    active_version: event.active_version().as_str().to_string(),
                    source: ActiveVersionChangeSourceReport::from_source(event.source()),
                })
            }
            EngineEventBody::VersionQuarantined(event) => {
                Self::VersionQuarantined(VersionQuarantinedReport {
                    component: event.component().clone(),
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
                Self::ForceFlip(ForceFlipSourceReport {
                    reason: reason.clone(),
                })
            }
            ActiveVersionChangeSource::Rollback { reason } => {
                Self::Rollback(RollbackSourceReport {
                    reason: reason.clone(),
                })
            }
        }
    }
}

impl ComponentLifecycleEventReport {
    pub fn from_component(component: &ComponentName) -> Self {
        Self {
            component: component.clone(),
        }
    }
}

impl ComponentOperationReport {
    pub fn from_operation(operation: &ComponentOperation) -> Self {
        match operation {
            ComponentOperation::Message(kind) => Self::Message(kind.clone()),
            ComponentOperation::System(kind) => Self::System(kind.clone()),
            ComponentOperation::Harness(kind) => Self::Harness(kind.clone()),
            ComponentOperation::Terminal(kind) => Self::Terminal(kind.clone()),
        }
    }
}
