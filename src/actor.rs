use kameo::actor::{Actor, ActorRef, Spawn};
use kameo::error::Infallible;
use kameo::message::{Context, Message};

use crate::error::{Error, Result};
use crate::request::{PersonaOutput, PersonaRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeEvent {
    Started,
    RequestAccepted,
    OutputCreated,
    TraceRead,
    Stopping,
}

#[derive(Debug)]
pub struct PersonaRuntime {
    events: Vec<RuntimeEvent>,
}

impl PersonaRuntime {
    pub fn new() -> Self {
        Self {
            events: vec![RuntimeEvent::Started],
        }
    }

    pub async fn start() -> ActorRef<Self> {
        let reference = Self::spawn(Self::new());
        reference.wait_for_startup().await;
        reference
    }

    pub async fn stop(reference: ActorRef<Self>) -> Result<()> {
        reference
            .stop_gracefully()
            .await
            .map_err(|error| Error::actor("stop persona runtime", error))?;
        reference.wait_for_shutdown().await;
        Ok(())
    }

    fn handle_request(&mut self, request: PersonaRequest) -> PersonaOutput {
        self.events.push(RuntimeEvent::RequestAccepted);
        let output = request.into_output();
        self.events.push(RuntimeEvent::OutputCreated);
        output
    }

    fn read_events(&mut self, probe: TraceProbe) -> Vec<RuntimeEvent> {
        let _satisfied = self.events.len() >= probe.minimum_events;
        self.events.push(RuntimeEvent::TraceRead);
        self.events.clone()
    }
}

impl Default for PersonaRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for PersonaRuntime {
    type Args = Self;
    type Error = Infallible;

    async fn on_start(
        actor: Self::Args,
        _actor_reference: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(actor)
    }

    async fn on_stop(
        &mut self,
        _actor_reference: kameo::actor::WeakActorRef<Self>,
        _reason: kameo::error::ActorStopReason,
    ) -> std::result::Result<(), Self::Error> {
        self.events.push(RuntimeEvent::Stopping);
        Ok(())
    }
}

#[derive(Debug)]
pub struct HandlePersonaRequest {
    request: PersonaRequest,
}

impl HandlePersonaRequest {
    pub fn new(request: PersonaRequest) -> Self {
        Self { request }
    }
}

impl Message<HandlePersonaRequest> for PersonaRuntime {
    type Reply = PersonaOutput;

    async fn handle(
        &mut self,
        message: HandlePersonaRequest,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.handle_request(message.request)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadTrace {
    pub probe: TraceProbe,
}

impl ReadTrace {
    pub fn expecting_at_least(minimum_events: usize) -> Self {
        Self {
            probe: TraceProbe { minimum_events },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceProbe {
    minimum_events: usize,
}

impl Message<ReadTrace> for PersonaRuntime {
    type Reply = Vec<RuntimeEvent>;

    async fn handle(
        &mut self,
        message: ReadTrace,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.read_events(message.probe)
    }
}
