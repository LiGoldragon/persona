use nota_codec::{
    Decoder, Encoder, NotaDecode, NotaEncode, NotaEnum, NotaRecord, NotaSum, NotaTransparent,
};
use signal_persona as contract;

pub use crate::engine_event::{EngineEventBodyKind, EngineEventSourceKind};
pub use contract::{
    ComponentDesiredState, ComponentHealth, ComponentKind, ComponentName, EnginePhase,
    SupervisorActionRejectionReason,
};

use crate::engine_event::{
    ComponentOperation, EngineEvent, EngineEventBody, EngineEventSource, EngineOperationKind,
    HarnessOperationKind, MessageOperationKind, MindOperationKind, SystemOperationKind,
    TerminalOperationKind, UnimplementedReason,
};

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

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct EngineEventReport {
    pub sequence: u64,
    pub engine: TextEngineId,
    pub source: EngineEventSourceKind,
    pub source_component: Option<ComponentName>,
    pub body: EngineEventBodyReport,
}

impl EngineEventReport {
    pub fn from_event(event: &EngineEvent) -> Self {
        Self {
            sequence: event.sequence().into_u64(),
            engine: TextEngineId::new(event.engine().as_str()),
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
    pub reason: UnimplementedReasonReport,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentExitedReport {
    pub component: ComponentName,
    pub exit_code: Option<i32>,
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

#[derive(NotaSum, Debug, Clone, PartialEq, Eq)]
pub enum EngineEventBodyReport {
    ComponentSpawned(ComponentLifecycleEventReport),
    ComponentReady(ComponentLifecycleEventReport),
    ComponentUnimplemented(ComponentUnimplementedReport),
    ComponentExited(ComponentExitedReport),
    RestartScheduled(RestartScheduledReport),
    RestartExhausted(RestartExhaustedReport),
    ComponentStopped(ComponentLifecycleEventReport),
    EngineStateChanged(EngineStateChangedReport),
}

#[derive(NotaSum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentOperationReport {
    Engine {
        operation: EngineOperationKindReport,
    },
    Message {
        operation: MessageOperationKindReport,
    },
    Mind {
        operation: MindOperationKindReport,
    },
    System {
        operation: SystemOperationKindReport,
    },
    Harness {
        operation: HarnessOperationKindReport,
    },
    Terminal {
        operation: TerminalOperationKindReport,
    },
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineOperationKindReport {
    EngineStatusQuery,
    ComponentStatusQuery,
    ComponentStartup,
    ComponentShutdown,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageOperationKindReport {
    MessageSubmission,
    InboxQuery,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MindOperationKindReport {
    RoleClaim,
    RoleRelease,
    RoleHandoff,
    RoleObservation,
    ActivitySubmission,
    ActivityQuery,
    Opening,
    NoteSubmission,
    Link,
    StatusChange,
    AliasAssignment,
    Query,
    AdjudicationRequest,
    ChannelGrant,
    ChannelExtend,
    ChannelRetract,
    AdjudicationDeny,
    ChannelList,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemOperationKindReport {
    FocusSubscription,
    FocusUnsubscription,
    FocusSnapshot,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessOperationKindReport {
    MessageDelivery,
    InteractionPrompt,
    DeliveryCancellation,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalOperationKindReport {
    TerminalConnection,
    TerminalInput,
    TerminalResize,
    TerminalDetachment,
    TerminalCapture,
    RegisterPromptPattern,
    UnregisterPromptPattern,
    ListPromptPatterns,
    AcquireInputGate,
    ReleaseInputGate,
    WriteInjection,
    SubscribeTerminalWorkerLifecycle,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnimplementedReasonReport {
    NotBuiltYet,
    DependencyTrackNotLanded,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
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

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusReport {
    pub component: contract::ComponentStatus,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusMissingReport {
    pub component: ComponentName,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct SupervisorActionAcceptedReport {
    pub component: ComponentName,
    pub desired_state: ComponentDesiredState,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct SupervisorActionRejectedReport {
    pub component: ComponentName,
    pub reason: SupervisorActionRejectionReason,
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
                    reason: UnimplementedReasonReport::from_reason(event.reason()),
                })
            }
            EngineEventBody::ComponentExited(event) => {
                Self::ComponentExited(ComponentExitedReport {
                    component: event.component().clone(),
                    exit_code: event.exit_code(),
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
            ComponentOperation::Engine(kind) => Self::Engine {
                operation: EngineOperationKindReport::from_kind(*kind),
            },
            ComponentOperation::Message(kind) => Self::Message {
                operation: MessageOperationKindReport::from_kind(*kind),
            },
            ComponentOperation::Mind(kind) => Self::Mind {
                operation: MindOperationKindReport::from_kind(*kind),
            },
            ComponentOperation::System(kind) => Self::System {
                operation: SystemOperationKindReport::from_kind(*kind),
            },
            ComponentOperation::Harness(kind) => Self::Harness {
                operation: HarnessOperationKindReport::from_kind(*kind),
            },
            ComponentOperation::Terminal(kind) => Self::Terminal {
                operation: TerminalOperationKindReport::from_kind(*kind),
            },
        }
    }
}

impl EngineOperationKindReport {
    pub fn from_kind(kind: EngineOperationKind) -> Self {
        match kind {
            EngineOperationKind::EngineStatusQuery => Self::EngineStatusQuery,
            EngineOperationKind::ComponentStatusQuery => Self::ComponentStatusQuery,
            EngineOperationKind::ComponentStartup => Self::ComponentStartup,
            EngineOperationKind::ComponentShutdown => Self::ComponentShutdown,
        }
    }
}

impl MessageOperationKindReport {
    pub fn from_kind(kind: MessageOperationKind) -> Self {
        match kind {
            MessageOperationKind::MessageSubmission => Self::MessageSubmission,
            MessageOperationKind::InboxQuery => Self::InboxQuery,
        }
    }
}

impl MindOperationKindReport {
    pub fn from_kind(kind: MindOperationKind) -> Self {
        match kind {
            MindOperationKind::RoleClaim => Self::RoleClaim,
            MindOperationKind::RoleRelease => Self::RoleRelease,
            MindOperationKind::RoleHandoff => Self::RoleHandoff,
            MindOperationKind::RoleObservation => Self::RoleObservation,
            MindOperationKind::ActivitySubmission => Self::ActivitySubmission,
            MindOperationKind::ActivityQuery => Self::ActivityQuery,
            MindOperationKind::Opening => Self::Opening,
            MindOperationKind::NoteSubmission => Self::NoteSubmission,
            MindOperationKind::Link => Self::Link,
            MindOperationKind::StatusChange => Self::StatusChange,
            MindOperationKind::AliasAssignment => Self::AliasAssignment,
            MindOperationKind::Query => Self::Query,
            MindOperationKind::AdjudicationRequest => Self::AdjudicationRequest,
            MindOperationKind::ChannelGrant => Self::ChannelGrant,
            MindOperationKind::ChannelExtend => Self::ChannelExtend,
            MindOperationKind::ChannelRetract => Self::ChannelRetract,
            MindOperationKind::AdjudicationDeny => Self::AdjudicationDeny,
            MindOperationKind::ChannelList => Self::ChannelList,
        }
    }
}

impl SystemOperationKindReport {
    pub fn from_kind(kind: SystemOperationKind) -> Self {
        match kind {
            SystemOperationKind::FocusSubscription => Self::FocusSubscription,
            SystemOperationKind::FocusUnsubscription => Self::FocusUnsubscription,
            SystemOperationKind::FocusSnapshot => Self::FocusSnapshot,
        }
    }
}

impl HarnessOperationKindReport {
    pub fn from_kind(kind: HarnessOperationKind) -> Self {
        match kind {
            HarnessOperationKind::MessageDelivery => Self::MessageDelivery,
            HarnessOperationKind::InteractionPrompt => Self::InteractionPrompt,
            HarnessOperationKind::DeliveryCancellation => Self::DeliveryCancellation,
        }
    }
}

impl TerminalOperationKindReport {
    pub fn from_kind(kind: TerminalOperationKind) -> Self {
        match kind {
            TerminalOperationKind::TerminalConnection => Self::TerminalConnection,
            TerminalOperationKind::TerminalInput => Self::TerminalInput,
            TerminalOperationKind::TerminalResize => Self::TerminalResize,
            TerminalOperationKind::TerminalDetachment => Self::TerminalDetachment,
            TerminalOperationKind::TerminalCapture => Self::TerminalCapture,
            TerminalOperationKind::RegisterPromptPattern => Self::RegisterPromptPattern,
            TerminalOperationKind::UnregisterPromptPattern => Self::UnregisterPromptPattern,
            TerminalOperationKind::ListPromptPatterns => Self::ListPromptPatterns,
            TerminalOperationKind::AcquireInputGate => Self::AcquireInputGate,
            TerminalOperationKind::ReleaseInputGate => Self::ReleaseInputGate,
            TerminalOperationKind::WriteInjection => Self::WriteInjection,
            TerminalOperationKind::SubscribeTerminalWorkerLifecycle => {
                Self::SubscribeTerminalWorkerLifecycle
            }
        }
    }
}

impl UnimplementedReasonReport {
    pub fn from_reason(reason: UnimplementedReason) -> Self {
        match reason {
            UnimplementedReason::NotBuiltYet => Self::NotBuiltYet,
            UnimplementedReason::DependencyTrackNotLanded => Self::DependencyTrackNotLanded,
        }
    }
}
