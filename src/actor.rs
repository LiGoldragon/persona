use kameo::actor::{Actor, ActorRef, Spawn};
use kameo::error::Infallible;
use kameo::message::{Context, Message};

use crate::error::{Error, Result};
use crate::request::{PersonaOutput, PersonaRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonaActorEvent {
    Started,
    RequestAccepted,
    OutputCreated,
    TraceRead,
    Stopping,
}

#[derive(Debug)]
pub struct PersonaRequestActor {
    events: Vec<PersonaActorEvent>,
}

impl PersonaRequestActor {
    pub fn new() -> Self {
        Self {
            events: vec![PersonaActorEvent::Started],
        }
    }

    fn handle_request(&mut self, request: PersonaRequest) -> PersonaOutput {
        self.events.push(PersonaActorEvent::RequestAccepted);
        let output = request.into_output();
        self.events.push(PersonaActorEvent::OutputCreated);
        output
    }

    fn read_events(&mut self) -> Vec<PersonaActorEvent> {
        self.events.push(PersonaActorEvent::TraceRead);
        self.events.clone()
    }
}

impl Default for PersonaRequestActor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for PersonaRequestActor {
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
        self.events.push(PersonaActorEvent::Stopping);
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

impl Message<HandlePersonaRequest> for PersonaRequestActor {
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
pub struct ReadPersonaActorTrace;

impl Message<ReadPersonaActorTrace> for PersonaRequestActor {
    type Reply = Vec<PersonaActorEvent>;

    async fn handle(
        &mut self,
        _message: ReadPersonaActorTrace,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.read_events()
    }
}

pub struct PersonaActorRuntime {
    actor: ActorRef<PersonaRequestActor>,
}

impl PersonaActorRuntime {
    pub fn start() -> Self {
        Self {
            actor: PersonaRequestActor::spawn(PersonaRequestActor::new()),
        }
    }

    pub async fn handle(&self, request: PersonaRequest) -> Result<PersonaOutput> {
        self.actor
            .ask(HandlePersonaRequest::new(request))
            .await
            .map_err(|error| Error::actor("handle persona request", error))
    }

    pub async fn trace(&self) -> Result<Vec<PersonaActorEvent>> {
        self.actor
            .ask(ReadPersonaActorTrace)
            .await
            .map_err(|error| Error::actor("read persona actor trace", error))
    }

    pub async fn stop(self) -> Result<()> {
        self.actor
            .stop_gracefully()
            .await
            .map_err(|error| Error::actor("stop persona actor", error))?;
        self.actor.wait_for_shutdown().await;
        Ok(())
    }
}
