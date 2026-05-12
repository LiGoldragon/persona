use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode, NotaEnum, NotaRecord, NotaTransparent};
use signal_persona as contract;

use crate::engine_event::{EngineEvent, EngineEventBody, EngineEventSource};

#[derive(NotaTransparent, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextEngineId(String);

impl TextEngineId {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaTransparent, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextComponentName(String);

impl TextComponentName {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn into_contract(self) -> contract::ComponentName {
        contract::ComponentName::new(self.0)
    }

    pub fn from_contract(component: &contract::ComponentName) -> Self {
        Self::new(component.as_str())
    }
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnginePhase {
    Starting,
    Running,
    Degraded,
    Draining,
    Stopped,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    Mind,
    Router,
    Message,
    System,
    Harness,
    Terminal,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentDesiredState {
    Running,
    Stopped,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentHealth {
    Starting,
    Running,
    Degraded,
    Stopped,
    Failed,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorActionRejectionReason {
    ComponentNotManaged,
    ComponentAlreadyInDesiredState,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineEventSourceKind {
    Manager,
    Component,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineEventBodyKind {
    ComponentSpawned,
    ComponentReady,
    ComponentUnimplemented,
    ComponentExited,
    RestartScheduled,
    RestartExhausted,
    ComponentStopped,
    EngineStateChanged,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct EngineEventReport {
    pub sequence: u64,
    pub engine: TextEngineId,
    pub source: EngineEventSourceKind,
    pub body: EngineEventBodyKind,
}

impl EngineEventReport {
    pub fn from_event(event: &EngineEvent) -> Self {
        Self {
            sequence: event.sequence().into_u64(),
            engine: TextEngineId::new(event.engine().as_str()),
            source: EngineEventSourceKind::from_event_source(event.source()),
            body: EngineEventBodyKind::from_event_body(event.body()),
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

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusRecord {
    pub name: TextComponentName,
    pub kind: ComponentKind,
    pub desired_state: ComponentDesiredState,
    pub health: ComponentHealth,
}

impl ComponentStatusRecord {
    pub fn from_contract(status: contract::ComponentStatus) -> Self {
        Self {
            name: TextComponentName::from_contract(&status.name),
            kind: ComponentKind::from_contract(status.kind),
            desired_state: ComponentDesiredState::from_contract(status.desired_state),
            health: ComponentHealth::from_contract(status.health),
        }
    }
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct EngineStatusReport {
    pub generation: u64,
    pub phase: EnginePhase,
    pub components: Vec<ComponentStatusRecord>,
}

impl EngineStatusReport {
    pub fn from_contract(status: contract::EngineStatus) -> Self {
        Self {
            generation: status.generation.into_u64(),
            phase: EnginePhase::from_contract(status.phase),
            components: status
                .components
                .into_iter()
                .map(ComponentStatusRecord::from_contract)
                .collect(),
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

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusReport {
    pub component: ComponentStatusRecord,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusMissingReport {
    pub component: TextComponentName,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct SupervisorActionAcceptedReport {
    pub component: TextComponentName,
    pub desired_state: ComponentDesiredState,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct SupervisorActionRejectedReport {
    pub component: TextComponentName,
    pub reason: SupervisorActionRejectionReason,
}

impl EnginePhase {
    pub fn from_contract(phase: contract::EnginePhase) -> Self {
        match phase {
            contract::EnginePhase::Starting => Self::Starting,
            contract::EnginePhase::Running => Self::Running,
            contract::EnginePhase::Degraded => Self::Degraded,
            contract::EnginePhase::Draining => Self::Draining,
            contract::EnginePhase::Stopped => Self::Stopped,
        }
    }
}

impl ComponentKind {
    pub fn from_contract(kind: contract::ComponentKind) -> Self {
        match kind {
            contract::ComponentKind::Mind => Self::Mind,
            contract::ComponentKind::Router => Self::Router,
            contract::ComponentKind::Message => Self::Message,
            contract::ComponentKind::System => Self::System,
            contract::ComponentKind::Harness => Self::Harness,
            contract::ComponentKind::Terminal => Self::Terminal,
        }
    }
}

impl ComponentDesiredState {
    pub fn from_contract(state: contract::ComponentDesiredState) -> Self {
        match state {
            contract::ComponentDesiredState::Running => Self::Running,
            contract::ComponentDesiredState::Stopped => Self::Stopped,
        }
    }
}

impl ComponentHealth {
    pub fn from_contract(health: contract::ComponentHealth) -> Self {
        match health {
            contract::ComponentHealth::Starting => Self::Starting,
            contract::ComponentHealth::Running => Self::Running,
            contract::ComponentHealth::Degraded => Self::Degraded,
            contract::ComponentHealth::Stopped => Self::Stopped,
            contract::ComponentHealth::Failed => Self::Failed,
        }
    }
}

impl SupervisorActionRejectionReason {
    pub fn from_contract(reason: contract::SupervisorActionRejectionReason) -> Self {
        match reason {
            contract::SupervisorActionRejectionReason::ComponentNotManaged => {
                Self::ComponentNotManaged
            }
            contract::SupervisorActionRejectionReason::ComponentAlreadyInDesiredState => {
                Self::ComponentAlreadyInDesiredState
            }
        }
    }
}

impl EngineEventSourceKind {
    pub fn from_event_source(source: &EngineEventSource) -> Self {
        match source {
            EngineEventSource::Manager => Self::Manager,
            EngineEventSource::Component(_) => Self::Component,
        }
    }
}

impl EngineEventBodyKind {
    pub fn from_event_body(body: &EngineEventBody) -> Self {
        match body {
            EngineEventBody::ComponentSpawned(_) => Self::ComponentSpawned,
            EngineEventBody::ComponentReady(_) => Self::ComponentReady,
            EngineEventBody::ComponentUnimplemented(_) => Self::ComponentUnimplemented,
            EngineEventBody::ComponentExited(_) => Self::ComponentExited,
            EngineEventBody::RestartScheduled(_) => Self::RestartScheduled,
            EngineEventBody::RestartExhausted(_) => Self::RestartExhausted,
            EngineEventBody::ComponentStopped(_) => Self::ComponentStopped,
            EngineEventBody::EngineStateChanged(_) => Self::EngineStateChanged,
        }
    }
}
