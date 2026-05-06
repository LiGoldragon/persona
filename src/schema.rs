use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode, NotaEnum, NotaRecord, NotaTransparent};

#[derive(NotaTransparent, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HarnessName(String);

impl HarnessName {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaTransparent, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrincipalName(String);

impl PrincipalName {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaTransparent, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageIdentifier(String);

impl MessageIdentifier {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaTransparent, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeliveryIdentifier(String);

impl DeliveryIdentifier {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaTransparent, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventIdentifier(String);

impl EventIdentifier {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaTransparent, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InteractionIdentifier(String);

impl InteractionIdentifier {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaTransparent, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransitionIdentifier(String);

impl TransitionIdentifier {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaTransparent, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateRevision(u64);

impl StateRevision {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessRole {
    Operator,
    Designer,
    Observer,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessAdapterKind {
    Terminal,
    DirectProcess,
    StructuredProtocol,
    FutureProvider,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePhase {
    Empty,
    Configured,
    Running,
    Draining,
    Halted,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessLifecycleState {
    Declared,
    Starting,
    Running,
    Idle,
    Busy,
    Blocked,
    Suspended,
    Stopped,
    Failed,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryIntent {
    SafeBoundary,
    Immediate,
    FollowUp,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationDecision {
    Allow,
    Deny,
    Hold,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryState {
    Pending,
    Queued,
    Delivered,
    Observed,
    Denied,
    Failed,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionCommandKind {
    DeclareHarness,
    StartHarness,
    ObserveHarness,
    AppendMessage,
    DecideAuthorization,
    QueueDelivery,
    DeliverMessage,
    ObserveOutput,
    ResolveInteraction,
    StopHarness,
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    StateTransitioned,
    MessageAppended,
    DeliveryDecided,
    DeliveryObserved,
    LifecycleObserved,
    InteractionObserved,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct HarnessRecord {
    pub name: HarnessName,
    pub role: HarnessRole,
    pub adapter: HarnessAdapterKind,
    pub command: String,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct AttachmentRecord {
    pub path: String,
    pub media_type: Option<String>,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct MessageRecord {
    pub identifier: MessageIdentifier,
    pub from: PrincipalName,
    pub to: PrincipalName,
    pub body: String,
    pub attachments: Vec<AttachmentRecord>,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRecord {
    pub delivery: DeliveryIdentifier,
    pub message: MessageIdentifier,
    pub decision: AuthorizationDecision,
    pub reason: String,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct DeliveryRecord {
    pub identifier: DeliveryIdentifier,
    pub message: MessageIdentifier,
    pub target: HarnessName,
    pub intent: DeliveryIntent,
    pub state: DeliveryState,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    pub identifier: EventIdentifier,
    pub kind: EventKind,
    pub subject: String,
    pub detail: String,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct StateCursorRecord {
    pub source: String,
    pub next_sequence: StateRevision,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct HarnessObservationRecord {
    pub harness: HarnessName,
    pub lifecycle: HarnessLifecycleState,
    pub event: EventIdentifier,
    pub output_tail: String,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct PendingInteractionRecord {
    pub identifier: InteractionIdentifier,
    pub harness: HarnessName,
    pub kind: String,
    pub prompt: String,
    pub options: Vec<String>,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct StateTransitionRecord {
    pub identifier: TransitionIdentifier,
    pub command: TransitionCommandKind,
    pub subject: String,
    pub before: StateRevision,
    pub after: StateRevision,
    pub event: EventIdentifier,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct PersonaStateSnapshot {
    pub revision: StateRevision,
    pub phase: CorePhase,
    pub harnesses: Vec<HarnessRecord>,
    pub messages: Vec<MessageRecord>,
    pub authorizations: Vec<AuthorizationRecord>,
    pub deliveries: Vec<DeliveryRecord>,
    pub observations: Vec<HarnessObservationRecord>,
    pub interactions: Vec<PendingInteractionRecord>,
    pub cursors: Vec<StateCursorRecord>,
    pub transitions: Vec<StateTransitionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonaObject {
    HarnessRecord(HarnessRecord),
    MessageRecord(MessageRecord),
    AuthorizationRecord(AuthorizationRecord),
    DeliveryRecord(DeliveryRecord),
    EventRecord(EventRecord),
    StateCursorRecord(StateCursorRecord),
    HarnessObservationRecord(HarnessObservationRecord),
    PendingInteractionRecord(PendingInteractionRecord),
    StateTransitionRecord(StateTransitionRecord),
    PersonaStateSnapshot(PersonaStateSnapshot),
}

impl PersonaObject {
    pub fn example_objects() -> Vec<Self> {
        let operator = HarnessRecord {
            name: HarnessName::new("operator"),
            role: HarnessRole::Operator,
            adapter: HarnessAdapterKind::Terminal,
            command: "codex".to_string(),
        };
        let designer = HarnessRecord {
            name: HarnessName::new("designer"),
            role: HarnessRole::Designer,
            adapter: HarnessAdapterKind::Terminal,
            command: "claude".to_string(),
        };
        let message = MessageRecord {
            identifier: MessageIdentifier::new("message-1"),
            from: PrincipalName::new("operator"),
            to: PrincipalName::new("designer"),
            body: "Sketch the harness message fabric.".to_string(),
            attachments: Vec::new(),
        };
        let authorization = AuthorizationRecord {
            delivery: DeliveryIdentifier::new("delivery-1"),
            message: MessageIdentifier::new("message-1"),
            decision: AuthorizationDecision::Allow,
            reason: "operator may send design work to designer".to_string(),
        };
        let delivery = DeliveryRecord {
            identifier: DeliveryIdentifier::new("delivery-1"),
            message: MessageIdentifier::new("message-1"),
            target: HarnessName::new("designer"),
            intent: DeliveryIntent::SafeBoundary,
            state: DeliveryState::Queued,
        };
        let event = EventRecord {
            identifier: EventIdentifier::new("event-1"),
            kind: EventKind::MessageAppended,
            subject: "message-1".to_string(),
            detail: "message appended and awaiting delivery".to_string(),
        };
        let cursor = StateCursorRecord {
            source: "persona-event-log".to_string(),
            next_sequence: StateRevision::new(2),
        };
        let observation = HarnessObservationRecord {
            harness: HarnessName::new("designer"),
            lifecycle: HarnessLifecycleState::Idle,
            event: EventIdentifier::new("event-2"),
            output_tail: "designer is ready for the next message".to_string(),
        };
        let interaction = PendingInteractionRecord {
            identifier: InteractionIdentifier::new("interaction-1"),
            harness: HarnessName::new("designer"),
            kind: "approval".to_string(),
            prompt: "Allow file edit?".to_string(),
            options: vec!["approve".to_string(), "deny".to_string()],
        };
        let transition = StateTransitionRecord {
            identifier: TransitionIdentifier::new("transition-1"),
            command: TransitionCommandKind::AppendMessage,
            subject: "message-1".to_string(),
            before: StateRevision::new(0),
            after: StateRevision::new(1),
            event: EventIdentifier::new("event-1"),
        };
        let snapshot = PersonaStateSnapshot {
            revision: StateRevision::new(1),
            phase: CorePhase::Running,
            harnesses: vec![operator.clone(), designer.clone()],
            messages: vec![message.clone()],
            authorizations: vec![authorization.clone()],
            deliveries: vec![delivery.clone()],
            observations: vec![observation.clone()],
            interactions: vec![interaction.clone()],
            cursors: vec![cursor.clone()],
            transitions: vec![transition.clone()],
        };

        vec![
            Self::HarnessRecord(operator),
            Self::HarnessRecord(designer),
            Self::MessageRecord(message),
            Self::AuthorizationRecord(authorization),
            Self::DeliveryRecord(delivery),
            Self::EventRecord(event),
            Self::StateCursorRecord(cursor),
            Self::HarnessObservationRecord(observation),
            Self::PendingInteractionRecord(interaction),
            Self::StateTransitionRecord(transition),
            Self::PersonaStateSnapshot(snapshot),
        ]
    }
}

impl NotaEncode for PersonaObject {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        match self {
            Self::HarnessRecord(record) => record.encode(encoder),
            Self::MessageRecord(record) => record.encode(encoder),
            Self::AuthorizationRecord(record) => record.encode(encoder),
            Self::DeliveryRecord(record) => record.encode(encoder),
            Self::EventRecord(record) => record.encode(encoder),
            Self::StateCursorRecord(record) => record.encode(encoder),
            Self::HarnessObservationRecord(record) => record.encode(encoder),
            Self::PendingInteractionRecord(record) => record.encode(encoder),
            Self::StateTransitionRecord(record) => record.encode(encoder),
            Self::PersonaStateSnapshot(record) => record.encode(encoder),
        }
    }
}

impl NotaDecode for PersonaObject {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        let head = decoder.peek_record_head()?;
        match head.as_str() {
            "HarnessRecord" => Ok(Self::HarnessRecord(HarnessRecord::decode(decoder)?)),
            "MessageRecord" => Ok(Self::MessageRecord(MessageRecord::decode(decoder)?)),
            "AuthorizationRecord" => Ok(Self::AuthorizationRecord(AuthorizationRecord::decode(
                decoder,
            )?)),
            "DeliveryRecord" => Ok(Self::DeliveryRecord(DeliveryRecord::decode(decoder)?)),
            "EventRecord" => Ok(Self::EventRecord(EventRecord::decode(decoder)?)),
            "StateCursorRecord" => Ok(Self::StateCursorRecord(StateCursorRecord::decode(decoder)?)),
            "HarnessObservationRecord" => Ok(Self::HarnessObservationRecord(
                HarnessObservationRecord::decode(decoder)?,
            )),
            "PendingInteractionRecord" => Ok(Self::PendingInteractionRecord(
                PendingInteractionRecord::decode(decoder)?,
            )),
            "StateTransitionRecord" => Ok(Self::StateTransitionRecord(
                StateTransitionRecord::decode(decoder)?,
            )),
            "PersonaStateSnapshot" => Ok(Self::PersonaStateSnapshot(PersonaStateSnapshot::decode(
                decoder,
            )?)),
            other => Err(nota_codec::Error::UnknownKindForVerb {
                verb: "PersonaObject",
                got: other.to_string(),
            }),
        }
    }
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct PersonaDocument {
    pub objects: Vec<PersonaObject>,
}

impl PersonaDocument {
    pub fn example() -> Self {
        Self {
            objects: PersonaObject::example_objects(),
        }
    }

    pub fn from_nota(text: &str) -> nota_codec::Result<Self> {
        let mut decoder = Decoder::nota(text);
        let document = Self::decode(&mut decoder)?;
        if let Some(token) = decoder.peek_token()? {
            return Err(nota_codec::Error::UnexpectedToken {
                expected: "end of input",
                got: token,
            });
        }
        Ok(document)
    }

    pub fn to_nota(&self) -> nota_codec::Result<String> {
        let mut encoder = Encoder::nota();
        self.encode(&mut encoder)?;
        Ok(encoder.into_string())
    }
}
