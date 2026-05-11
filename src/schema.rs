use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode, NotaEnum, NotaRecord, NotaTransparent};
use signal_persona as contract;

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
