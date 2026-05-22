use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode, NotaEnum, NotaRecord};
use owner_signal_version_handover::{ForceReason, QuarantineReason, RollbackReason};
use signal_persona as contract;
use signal_persona_auth::EngineId;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineEventReport {
    pub sequence: u64,
    pub engine: EngineId,
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

    pub fn from_nota(text: &str) -> nota_codec::Result<Self> {
        let mut decoder = Decoder::new(text);
        let report = Self::decode(&mut decoder)?;
        if let Some(token) = decoder.peek_token()? {
            return Err(nota_codec::Error::UnexpectedToken {
                expected: "end of input",
                got: token,
            });
        }
        Ok(report)
    }

    pub fn to_nota(&self) -> nota_codec::Result<String> {
        let mut encoder = Encoder::new();
        self.encode(&mut encoder)?;
        Ok(encoder.into_string())
    }
}

impl NotaEncode for EngineEventReport {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        encoder.start_record("EngineEventReport")?;
        self.sequence.encode(encoder)?;
        self.engine.encode(encoder)?;
        self.source.encode(encoder)?;
        self.source_component.encode(encoder)?;
        self.body.encode(encoder)?;
        encoder.end_record()
    }
}

impl NotaDecode for EngineEventReport {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        decoder.expect_record_head("EngineEventReport")?;
        let sequence = u64::decode(decoder)?;
        let engine = EngineId::decode(decoder)?;
        let source = EngineEventSourceKind::decode(decoder)?;
        let source_component = Option::<ComponentName>::decode(decoder)?;
        let body = EngineEventBodyReport::decode(decoder)?;
        decoder.expect_record_end()?;
        Ok(Self {
            sequence,
            engine,
            source,
            source_component,
            body,
        })
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

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentLifecycleEventReport {
    pub component: ComponentName,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentUnimplementedReport {
    pub component: ComponentName,
    pub operation: ComponentOperationReport,
    pub reason: UnimplementedReason,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentExitedReport {
    pub component: ComponentName,
    pub exit_code: Option<i32>,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentOrphanedReport {
    pub component: ComponentName,
    pub spawned_sequence: u64,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct RestartScheduledReport {
    pub component: ComponentName,
    pub attempt: u32,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct RestartExhaustedReport {
    pub component: ComponentName,
    pub attempts: u32,
}

#[derive(NotaRecord, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineStateChangedReport {
    pub phase: EnginePhase,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct UpgradePreparedReport {
    pub component: ComponentName,
    pub current_version: String,
    pub next_version: String,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ActiveVersionChangedReport {
    pub component: ComponentName,
    pub active_version: String,
    pub source: ActiveVersionChangeSourceReport,
}

#[derive(NotaEnum, Debug, Clone, PartialEq, Eq)]
pub enum ActiveVersionChangeSourceReport {
    HandoverMarker { commit_sequence: u64 },
    ForceFlip { reason: ForceReason },
    Rollback { reason: RollbackReason },
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct VersionQuarantinedReport {
    pub component: ComponentName,
    pub version: String,
    pub reason: QuarantineReason,
}

#[derive(NotaEnum, Debug, Clone, PartialEq, Eq)]
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

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentOperationReport {
    Engine { operation: EngineOperationKind },
    Message { operation: MessageOperationKind },
    Mind { operation: MindOperationKind },
    System { operation: SystemOperationKind },
    Harness { operation: HarnessOperationKind },
    Terminal { operation: TerminalOperationKind },
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

    pub fn from_nota(text: &str) -> nota_codec::Result<Self> {
        let mut decoder = Decoder::new(text);
        let report = Self::decode(&mut decoder)?;
        if let Some(token) = decoder.peek_token()? {
            return Err(nota_codec::Error::UnexpectedToken {
                expected: "end of input",
                got: token,
            });
        }
        Ok(report)
    }

    pub fn to_nota(&self) -> nota_codec::Result<String> {
        let mut encoder = Encoder::new();
        self.encode(&mut encoder)?;
        Ok(encoder.into_string())
    }
}

impl NotaEncode for EngineStatusReport {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        encoder.start_record("EngineStatusReport")?;
        self.generation.encode(encoder)?;
        self.phase.encode(encoder)?;
        self.components.encode(encoder)?;
        encoder.end_record()
    }
}

impl NotaDecode for EngineStatusReport {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        decoder.expect_record_head("EngineStatusReport")?;
        let generation = contract::EngineGeneration::decode(decoder)?;
        let phase = EnginePhase::decode(decoder)?;
        let components = Vec::<contract::ComponentStatus>::decode(decoder)?;
        decoder.expect_record_end()?;
        Ok(Self {
            generation,
            phase,
            components,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusReport {
    pub component: contract::ComponentStatus,
}

impl NotaEncode for ComponentStatusReport {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        encoder.start_record("ComponentStatusReport")?;
        self.component.encode(encoder)?;
        encoder.end_record()
    }
}

impl NotaDecode for ComponentStatusReport {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        decoder.expect_record_head("ComponentStatusReport")?;
        let component = contract::ComponentStatus::decode(decoder)?;
        decoder.expect_record_end()?;
        Ok(Self { component })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusMissingReport {
    pub component: ComponentName,
}

impl NotaEncode for ComponentStatusMissingReport {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        encoder.start_record("ComponentStatusMissingReport")?;
        self.component.encode(encoder)?;
        encoder.end_record()
    }
}

impl NotaDecode for ComponentStatusMissingReport {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        decoder.expect_record_head("ComponentStatusMissingReport")?;
        let component = ComponentName::decode(decoder)?;
        decoder.expect_record_end()?;
        Ok(Self { component })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetirementAcceptanceReport {
    pub engine: EngineId,
}

impl NotaEncode for RetirementAcceptanceReport {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        encoder.start_record("RetirementAcceptanceReport")?;
        self.engine.encode(encoder)?;
        encoder.end_record()
    }
}

impl NotaDecode for RetirementAcceptanceReport {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        decoder.expect_record_head("RetirementAcceptanceReport")?;
        let engine = EngineId::decode(decoder)?;
        decoder.expect_record_end()?;
        Ok(Self { engine })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionAcceptedReport {
    pub component: ComponentName,
    pub desired_state: ComponentDesiredState,
}

impl NotaEncode for ActionAcceptedReport {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        encoder.start_record("ActionAcceptedReport")?;
        self.component.encode(encoder)?;
        self.desired_state.encode(encoder)?;
        encoder.end_record()
    }
}

impl NotaDecode for ActionAcceptedReport {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        decoder.expect_record_head("ActionAcceptedReport")?;
        let component = ComponentName::decode(decoder)?;
        let desired_state = ComponentDesiredState::decode(decoder)?;
        decoder.expect_record_end()?;
        Ok(Self {
            component,
            desired_state,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRejectedReport {
    pub component: ComponentName,
    pub reason: ActionRejectionReason,
}

impl NotaEncode for ActionRejectedReport {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        encoder.start_record("ActionRejectedReport")?;
        self.component.encode(encoder)?;
        self.reason.encode(encoder)?;
        encoder.end_record()
    }
}

impl NotaDecode for ActionRejectedReport {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        decoder.expect_record_head("ActionRejectedReport")?;
        let component = ComponentName::decode(decoder)?;
        let reason = ActionRejectionReason::decode(decoder)?;
        decoder.expect_record_end()?;
        Ok(Self { component, reason })
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
                    exit_code: event.exit_code(),
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
                    attempt: event.attempt(),
                })
            }
            EngineEventBody::RestartExhausted(event) => {
                Self::RestartExhausted(RestartExhaustedReport {
                    component: event.component().clone(),
                    attempts: event.attempts(),
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
            ActiveVersionChangeSource::HandoverMarker { commit_sequence } => Self::HandoverMarker {
                commit_sequence: *commit_sequence,
            },
            ActiveVersionChangeSource::ForceFlip { reason } => Self::ForceFlip { reason: *reason },
            ActiveVersionChangeSource::Rollback { reason } => Self::Rollback { reason: *reason },
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
            ComponentOperation::Engine(kind) => Self::Engine { operation: *kind },
            ComponentOperation::Message(kind) => Self::Message { operation: *kind },
            ComponentOperation::Mind(kind) => Self::Mind { operation: *kind },
            ComponentOperation::System(kind) => Self::System { operation: *kind },
            ComponentOperation::Harness(kind) => Self::Harness { operation: *kind },
            ComponentOperation::Terminal(kind) => Self::Terminal { operation: *kind },
        }
    }
}
