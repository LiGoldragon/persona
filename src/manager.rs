use kameo::actor::{Actor, ActorRef, Spawn};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use signal_persona::{
    ComponentShutdown, ComponentStartup, ComponentStatusQuery, EngineCatalog, EngineCatalogEntry,
    EngineLaunchRejection, EngineLaunchRejectionReason, EngineReply, EngineRequest,
    EngineRetirementRejection, EngineRetirementRejectionReason, EngineStatusQuery,
};
use signal_persona_auth::EngineId;

use crate::error::{Error, Result};
use crate::manager_store::{
    AppendOrphansFromEventLog, ComponentStatusSnapshotRow, ManagerStore, PersistEngineRecord,
    ReadEngineRecord, ReadEngineStatusSnapshot,
};
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
    engine: EngineId,
    state: EngineState,
    store: Option<ActorRef<ManagerStore>>,
    events: Vec<ManagerEvent>,
}

impl EngineManager {
    pub fn new(state: EngineState) -> Self {
        Self {
            engine: EngineId::new("default"),
            state,
            store: None,
            events: vec![ManagerEvent::Started],
        }
    }

    pub fn with_store(engine: EngineId, state: EngineState, store: ActorRef<ManagerStore>) -> Self {
        Self {
            engine,
            state,
            store: Some(store),
            events: vec![ManagerEvent::Started],
        }
    }

    pub async fn start() -> ActorRef<Self> {
        let reference = Self::spawn(Self::new(EngineState::default_catalog()));
        reference.wait_for_startup().await;
        reference
    }

    pub async fn start_with_store(
        engine: EngineId,
        store: ActorRef<ManagerStore>,
    ) -> Result<ActorRef<Self>> {
        let state = Self::initial_state_from_store(&engine, &store).await?;
        let reference = Self::spawn(Self::with_store(engine, state, store));
        reference.wait_for_startup().await;
        reference
            .ask(SynchronizeManagerState)
            .await
            .map_err(|error| Error::actor("synchronize manager state", error))?;
        Ok(reference)
    }

    async fn initial_state_from_store(
        engine: &EngineId,
        store: &ActorRef<ManagerStore>,
    ) -> Result<EngineState> {
        // Detect orphan arcs from the prior daemon run and append typed
        // `ComponentOrphaned` events before reading the status snapshot.
        // The reducer projects each orphan into `Exited / Failed`, so
        // the snapshot the manager hydrates from already reflects every
        // arc the prior daemon failed to close. `ask` collapses
        // `Reply = Result<_, _>` into the outer `SendError`, so one
        // `?` unwraps both layers; the `Vec<EngineEvent>` it returns is
        // informational and ignored here.
        let _appended_orphans = store
            .ask(AppendOrphansFromEventLog)
            .await
            .map_err(|error| Error::actor("scan event log for orphan components", error))?;
        let record = store
            .ask(ReadEngineRecord::new(engine.clone()))
            .await
            .map_err(|error| Error::actor("read persisted manager engine record", error))?;
        let status_snapshot = store
            .ask(ReadEngineStatusSnapshot::new(engine.clone()))
            .await
            .map_err(|error| Error::actor("read manager status snapshot", error))?;
        let base_state = record
            .map(|record| EngineState::from_status(record.status().clone()))
            .unwrap_or_else(EngineState::default_catalog);
        Ok(Self::overlay_status_snapshot(base_state, status_snapshot))
    }

    fn overlay_status_snapshot(
        mut state: EngineState,
        snapshot_rows: Vec<ComponentStatusSnapshotRow>,
    ) -> EngineState {
        for row in snapshot_rows {
            state.set_component_health(row.component(), row.health());
        }
        state
    }

    pub async fn stop(reference: ActorRef<Self>) -> Result<()> {
        reference
            .stop_gracefully()
            .await
            .map_err(|error| Error::actor("stop engine manager", error))?;
        reference.wait_for_shutdown().await;
        Ok(())
    }

    async fn handle_request(&mut self, request: EngineRequest) -> Result<EngineReply> {
        self.events.push(ManagerEvent::EngineRequestAccepted);
        let should_persist = matches!(
            request,
            EngineRequest::ComponentStartup(_) | EngineRequest::ComponentShutdown(_)
        );
        let reply = match request {
            EngineRequest::EngineStatusQuery(EngineStatusQuery { .. }) => {
                self.state.engine_status()
            }
            EngineRequest::ComponentStatusQuery(query) => self.state.component_status(query),
            EngineRequest::ComponentStartup(startup) => self.state.start_component(startup),
            EngineRequest::ComponentShutdown(shutdown) => self.state.stop_component(shutdown),
            EngineRequest::EngineLaunchProposal(proposal) => {
                EngineReply::EngineLaunchRejected(EngineLaunchRejection {
                    label: proposal.label,
                    reason: EngineLaunchRejectionReason::LaunchPlanRejected,
                })
            }
            EngineRequest::EngineCatalogQuery(_) => EngineReply::EngineCatalog(EngineCatalog {
                engines: vec![EngineCatalogEntry {
                    engine: self.engine.clone(),
                    label: signal_persona::EngineLabel::new(self.engine.as_str()),
                    phase: self.state.snapshot().phase,
                }],
            }),
            EngineRequest::EngineRetirement(retirement) => {
                let reason = if retirement.engine == self.engine {
                    EngineRetirementRejectionReason::EngineStillRunning
                } else {
                    EngineRetirementRejectionReason::EngineNotFound
                };
                EngineReply::EngineRetirementRejected(EngineRetirementRejection {
                    engine: retirement.engine,
                    reason,
                })
            }
        };
        if should_persist && matches!(reply, EngineReply::SupervisorActionAccepted(_)) {
            self.persist_state().await?;
        }
        self.events.push(ManagerEvent::EngineReplyCreated);
        Ok(reply)
    }

    async fn persist_state(&self) -> Result<()> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        store
            .ask(PersistEngineRecord::new(
                self.engine.clone(),
                self.state.snapshot().clone(),
            ))
            .await
            .map_err(|error| Error::actor("persist manager engine record", error))?;
        Ok(())
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
    type Reply = Result<EngineReply>;

    async fn handle(
        &mut self,
        message: HandleEngineRequest,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.handle_request(message.request).await
    }
}

pub struct SynchronizeManagerState;

impl Message<SynchronizeManagerState> for EngineManager {
    type Reply = Result<()>;

    async fn handle(
        &mut self,
        _message: SynchronizeManagerState,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.persist_state().await
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
