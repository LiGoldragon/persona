use kameo::actor::{Actor, ActorRef, Spawn};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use owner_signal_version_handover::{
    ForcedFlip, Operation as OwnerVersionHandoverOperation, Quarantined,
    Reply as OwnerVersionReply, RequestUnimplemented, RolledBack, UnimplementedReason,
};
use signal_persona::engine::{Operation, Reply};
use signal_persona::{
    ActionRejection, ActionRejectionReason, ComponentName, ComponentShutdown, ComponentStartup,
    EngineCatalog, EngineCatalogEntry, LaunchRejection, LaunchRejectionReason, Query,
    RetirementRejection, RetirementRejectionReason,
};
use signal_persona_auth::EngineId;
use signal_version_handover::HandoverMarker;

use crate::engine_event::{
    EngineEventBody, EngineEventDraft, EngineEventDraftInput, EngineEventSource,
};
use crate::error::{Error, Result};
use crate::manager_store::{
    AppendEngineEvent, AppendOrphansFromEventLog, ComponentStatusSnapshotRow, ManagerStore,
    PersistEngineRecord, ReadEngineRecord, ReadEngineStatusSnapshot,
};
use crate::state::EngineState;
use crate::upgrade::{ActiveVersionChanged, Prepared, PreparedEvent, Target, VersionQuarantined};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerEvent {
    Started,
    EngineRequestAccepted,
    EngineReplyCreated,
    TraceRead,
    UpgradePrepared,
    ActiveVersionChanged,
    VersionAuthorityApplied,
    VersionQuarantined,
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
        let _shutdown_completion = reference.wait_for_shutdown().await;
        Ok(())
    }

    async fn handle_request(&mut self, request: Operation) -> Result<Reply> {
        self.events.push(ManagerEvent::EngineRequestAccepted);
        let should_persist = matches!(request, Operation::Start(_) | Operation::Stop(_));
        let reply = match request {
            Operation::Query(Query::EngineStatus(_)) => self.state.engine_status(),
            Operation::Query(Query::ComponentStatus(component)) => {
                self.state.component_status(component)
            }
            Operation::Start(startup) => self.state.start_component(startup),
            Operation::Stop(shutdown) => self.state.stop_component(shutdown),
            Operation::Launch(proposal) => Reply::LaunchRejected(LaunchRejection {
                label: proposal.label,
                reason: LaunchRejectionReason::LaunchPlanRejected,
            }),
            Operation::Query(Query::Catalog(_)) => Reply::Catalog(EngineCatalog {
                engines: vec![EngineCatalogEntry {
                    engine: self.engine.clone(),
                    label: signal_persona::EngineLabel::new(self.engine.as_str()),
                    phase: self.state.snapshot().phase,
                }],
            }),
            Operation::Retire(engine) => {
                let reason = if engine == self.engine {
                    RetirementRejectionReason::EngineStillRunning
                } else {
                    RetirementRejectionReason::EngineNotFound
                };
                Reply::RetireRejected(RetirementRejection { engine, reason })
            }
            Operation::Tap(_) | Operation::Untap(_) => Reply::ActionRejected(ActionRejection {
                component: ComponentName::new("persona-observer"),
                reason: ActionRejectionReason::ComponentNotManaged,
            }),
        };
        if should_persist && matches!(reply, Reply::ActionAccepted(_)) {
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

    async fn append_event(&self, body: EngineEventBody) -> Result<()> {
        let Some(store) = &self.store else {
            return Err(Error::UpgradeRequiresManagerStore);
        };
        store
            .ask(AppendEngineEvent::new(EngineEventDraft::from_input(
                EngineEventDraftInput {
                    engine: self.engine.clone(),
                    source: EngineEventSource::Manager,
                    body,
                },
            )))
            .await
            .map_err(|error| Error::actor("append manager upgrade event", error))?;
        Ok(())
    }

    async fn prepare_upgrade(&mut self, target: Target) -> Result<Prepared> {
        let prepared = target.prepare();
        self.append_event(EngineEventBody::UpgradePrepared(
            PreparedEvent::from_target(prepared.target()),
        ))
        .await?;
        self.events.push(ManagerEvent::UpgradePrepared);
        Ok(prepared)
    }

    async fn complete_upgrade(
        &mut self,
        target: Target,
        marker: HandoverMarker,
    ) -> Result<ActiveVersionChanged> {
        if marker.component.as_str() != target.component().as_str() {
            return Err(Error::HandoverMarkerComponentMismatch {
                expected: target.component().as_str().to_string(),
                actual: marker.component.as_str().to_string(),
            });
        }
        let change = ActiveVersionChanged::from_marker(&target, &marker);
        self.append_event(EngineEventBody::ActiveVersionChanged(change.clone()))
            .await?;
        self.events.push(ManagerEvent::ActiveVersionChanged);
        Ok(change)
    }

    async fn handle_owner_version_handover(
        &mut self,
        operation: OwnerVersionHandoverOperation,
    ) -> Result<OwnerVersionReply> {
        let reply = match operation {
            OwnerVersionHandoverOperation::ForceFlip(order) => {
                let change = ActiveVersionChanged::from_force_flip(&order);
                self.append_event(EngineEventBody::ActiveVersionChanged(change))
                    .await?;
                self.events.push(ManagerEvent::VersionAuthorityApplied);
                OwnerVersionReply::FlipForced(ForcedFlip {
                    component: order.component,
                    active_version: order.target_version,
                })
            }
            OwnerVersionHandoverOperation::Rollback(order) => {
                let change = ActiveVersionChanged::from_rollback(&order);
                self.append_event(EngineEventBody::ActiveVersionChanged(change))
                    .await?;
                self.events.push(ManagerEvent::VersionAuthorityApplied);
                OwnerVersionReply::RolledBack(RolledBack {
                    component: order.component,
                    active_version: order.restore_version,
                })
            }
            OwnerVersionHandoverOperation::Quarantine(order) => {
                let event = VersionQuarantined::from_quarantine(&order);
                self.append_event(EngineEventBody::VersionQuarantined(event))
                    .await?;
                self.events.push(ManagerEvent::VersionQuarantined);
                OwnerVersionReply::Quarantined(Quarantined {
                    component: order.component,
                    version: order.version,
                })
            }
            OwnerVersionHandoverOperation::Tap(_) | OwnerVersionHandoverOperation::Untap(_) => {
                OwnerVersionReply::RequestUnimplemented(RequestUnimplemented {
                    reason: UnimplementedReason::IntegrationNotLanded,
                })
            }
        };
        Ok(reply)
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
    request: Operation,
}

impl HandleEngineRequest {
    pub fn new(request: Operation) -> Self {
        Self { request }
    }
}

impl Message<HandleEngineRequest> for EngineManager {
    type Reply = Result<Reply>;

    async fn handle(
        &mut self,
        message: HandleEngineRequest,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.handle_request(message.request).await
    }
}

#[derive(Debug, Clone)]
pub struct PrepareUpgrade {
    target: Target,
}

impl PrepareUpgrade {
    pub fn new(target: Target) -> Self {
        Self { target }
    }
}

impl Message<PrepareUpgrade> for EngineManager {
    type Reply = Result<Prepared>;

    async fn handle(
        &mut self,
        message: PrepareUpgrade,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.prepare_upgrade(message.target).await
    }
}

#[derive(Debug, Clone)]
pub struct CompleteUpgrade {
    target: Target,
    marker: HandoverMarker,
}

impl CompleteUpgrade {
    pub fn new(target: Target, marker: HandoverMarker) -> Self {
        Self { target, marker }
    }
}

impl Message<CompleteUpgrade> for EngineManager {
    type Reply = Result<ActiveVersionChanged>;

    async fn handle(
        &mut self,
        message: CompleteUpgrade,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.complete_upgrade(message.target, message.marker).await
    }
}

#[derive(Debug, Clone)]
pub struct HandleOwnerVersionHandover {
    operation: OwnerVersionHandoverOperation,
}

impl HandleOwnerVersionHandover {
    pub fn new(operation: OwnerVersionHandoverOperation) -> Self {
        Self { operation }
    }
}

impl Message<HandleOwnerVersionHandover> for EngineManager {
    type Reply = Result<OwnerVersionReply>;

    async fn handle(
        &mut self,
        message: HandleOwnerVersionHandover,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.handle_owner_version_handover(message.operation).await
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

impl From<ComponentStartup> for HandleEngineRequest {
    fn from(startup: ComponentStartup) -> Self {
        Self::new(Operation::Start(startup))
    }
}

impl From<ComponentShutdown> for HandleEngineRequest {
    fn from(shutdown: ComponentShutdown) -> Self {
        Self::new(Operation::Stop(shutdown))
    }
}
