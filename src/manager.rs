use kameo::actor::{Actor, ActorRef, Spawn};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use signal_persona::{
    ComponentShutdown, ComponentStartup, ComponentStatusQuery, EngineReply, EngineRequest,
    EngineStatusQuery,
};

use crate::error::{Error, Result};
use crate::state::EngineState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerEvent {
    Started,
    EngineRequestAccepted,
    EngineReplyCreated,
    TraceRead,
    Stopping,
}

#[derive(Debug)]
pub struct EngineManager {
    state: EngineState,
    events: Vec<ManagerEvent>,
}

impl EngineManager {
    pub fn new(state: EngineState) -> Self {
        Self {
            state,
            events: vec![ManagerEvent::Started],
        }
    }

    pub async fn start() -> ActorRef<Self> {
        let reference = Self::spawn(Self::new(EngineState::default_catalog()));
        reference.wait_for_startup().await;
        reference
    }

    pub async fn stop(reference: ActorRef<Self>) -> Result<()> {
        reference
            .stop_gracefully()
            .await
            .map_err(|error| Error::actor("stop engine manager", error))?;
        reference.wait_for_shutdown().await;
        Ok(())
    }

    fn handle_request(&mut self, request: EngineRequest) -> EngineReply {
        self.events.push(ManagerEvent::EngineRequestAccepted);
        let reply = match request {
            EngineRequest::EngineStatusQuery(EngineStatusQuery { .. }) => {
                self.state.engine_status()
            }
            EngineRequest::ComponentStatusQuery(query) => self.state.component_status(query),
            EngineRequest::ComponentStartup(startup) => self.state.start_component(startup),
            EngineRequest::ComponentShutdown(shutdown) => self.state.stop_component(shutdown),
        };
        self.events.push(ManagerEvent::EngineReplyCreated);
        reply
    }

    fn read_events(&mut self, probe: TraceProbe) -> Vec<ManagerEvent> {
        let _satisfied = self.events.len() >= probe.minimum_events;
        self.events.push(ManagerEvent::TraceRead);
        self.events.clone()
    }
}

impl Default for EngineManager {
    fn default() -> Self {
        Self::new(EngineState::default_catalog())
    }
}

impl Actor for EngineManager {
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
        self.events.push(ManagerEvent::Stopping);
        Ok(())
    }
}

#[derive(Debug)]
pub struct HandleEngineRequest {
    request: EngineRequest,
}

impl HandleEngineRequest {
    pub fn new(request: EngineRequest) -> Self {
        Self { request }
    }
}

impl Message<HandleEngineRequest> for EngineManager {
    type Reply = std::result::Result<EngineReply, Infallible>;

    async fn handle(
        &mut self,
        message: HandleEngineRequest,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        Ok(self.handle_request(message.request))
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

impl Message<ReadTrace> for EngineManager {
    type Reply = Vec<ManagerEvent>;

    async fn handle(
        &mut self,
        message: ReadTrace,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.read_events(message.probe)
    }
}

impl From<ComponentStatusQuery> for HandleEngineRequest {
    fn from(query: ComponentStatusQuery) -> Self {
        Self::new(EngineRequest::ComponentStatusQuery(query))
    }
}

impl From<ComponentStartup> for HandleEngineRequest {
    fn from(startup: ComponentStartup) -> Self {
        Self::new(EngineRequest::ComponentStartup(startup))
    }
}

impl From<ComponentShutdown> for HandleEngineRequest {
    fn from(shutdown: ComponentShutdown) -> Self {
        Self::new(EngineRequest::ComponentShutdown(shutdown))
    }
}
