use std::path::{Path, PathBuf};

use kameo::actor::{Actor, ActorRef, Spawn};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use sema::{Schema, SchemaVersion, Sema, Table};
use signal_persona::{ComponentHealth, ComponentName, EngineStatus};
use signal_persona_origin::EngineIdentifier;

use crate::Result;
use crate::engine_event::{
    ComponentExited, ComponentOrphaned, ComponentOrphanedInput, EngineEvent, EngineEventBody,
    EngineEventDraft, EngineEventDraftInput, EngineEventSequence, EngineEventSource,
};
use crate::upgrade::ActiveVersion;

const MANAGER_SCHEMA: Schema = Schema {
    version: SchemaVersion::new(4),
};

const ENGINE_RECORDS: Table<&'static str, StoredEngineRecord> =
    Table::new("manager.engine-records");
const ENGINE_EVENTS: Table<u64, EngineEvent> = Table::new("manager.engine-events");
const ENGINE_LIFECYCLE_SNAPSHOT: Table<&'static str, ComponentLifecycleSnapshotRow> =
    Table::new("manager.engine-lifecycle-snapshot");
const ENGINE_STATUS_SNAPSHOT: Table<&'static str, ComponentStatusSnapshotRow> =
    Table::new("manager.engine-status-snapshot");
const ACTIVE_VERSION_SNAPSHOT: Table<&'static str, ActiveVersion> =
    Table::new("manager.active-version-snapshot");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerStoreLocation {
    path: PathBuf,
}

impl ManagerStoreLocation {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn from_environment() -> Option<Self> {
        std::env::var_os("PERSONA_MANAGER_STORE").map(Self::new)
    }

    pub fn from_endpoint(endpoint: &Path) -> Result<Self> {
        let Some(parent) = endpoint.parent() else {
            return Err(crate::Error::ManagerStorePathMissingParent {
                path: endpoint.to_path_buf(),
            });
        };
        Ok(Self::new(parent.join("manager.redb")))
    }

    pub fn as_path(&self) -> &Path {
        self.path.as_path()
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct StoredEngineRecord {
    engine: EngineIdentifier,
    status: EngineStatus,
}

impl StoredEngineRecord {
    pub fn new(engine: EngineIdentifier, status: EngineStatus) -> Self {
        Self { engine, status }
    }

    pub fn engine(&self) -> &EngineIdentifier {
        &self.engine
    }

    pub fn status(&self) -> &EngineStatus {
        &self.status
    }

    fn key(&self) -> &str {
        self.engine.as_str()
    }
}

/// Closed-set process-lifecycle stage for one supervised component within one
/// engine, materialised by the engine-lifecycle reducer over the
/// `manager.engine-events` log per `ARCHITECTURE.md` §1.7.
///
/// Transitions today: `ComponentSpawned` lifts a row to `Launched`,
/// `ComponentReady` to `Ready`, `ComponentStopped` to `Stopping`,
/// `ComponentExited` / `RestartExhausted` to `Exited`. The `SocketBound`
/// intermediate stage ARCH names is reserved for the future
/// `ComponentSocketBound` event; the prototype reducer does not emit it
/// today.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentProcessState {
    Launched,
    Ready,
    Stopping,
    Exited,
}

/// Snapshot row stored in `manager.engine-lifecycle-snapshot`, keyed by
/// `engine_identifier::component_name`. The reducer overwrites the row on each
/// transition; readers project the latest state into `EngineStatus`.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ComponentLifecycleSnapshotRow {
    component: ComponentName,
    process_state: ComponentProcessState,
}

impl ComponentLifecycleSnapshotRow {
    pub fn new(component: ComponentName, process_state: ComponentProcessState) -> Self {
        Self {
            component,
            process_state,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn process_state(&self) -> ComponentProcessState {
        self.process_state
    }
}

/// Snapshot row stored in `manager.engine-status-snapshot`, keyed by
/// `engine_identifier::component_name`. Carries the same closed-enum
/// `ComponentHealth` that `signal_persona::EngineStatus` reports to CLI
/// status queries, with no extra ARCH-aspirational variants.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusSnapshotRow {
    component: ComponentName,
    health: ComponentHealth,
}

impl ComponentStatusSnapshotRow {
    pub fn new(component: ComponentName, health: ComponentHealth) -> Self {
        Self { component, health }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn health(&self) -> ComponentHealth {
        self.health
    }
}

/// Composite key `engine_identifier::component_name` used by both snapshot tables.
/// The `::` separator is unambiguous: `EngineIdentifier` and `ComponentName`
/// values do not contain it in any current shape.
pub struct SnapshotKey(String);

impl SnapshotKey {
    pub fn new(engine: &EngineIdentifier, component: &ComponentName) -> Self {
        Self(format!("{}::{}", engine.as_str(), component.as_str()))
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn engine_prefix(engine: &EngineIdentifier) -> String {
        format!("{}::", engine.as_str())
    }
}

struct ManagerTables {
    database: Sema,
}

impl ManagerTables {
    fn open(location: &ManagerStoreLocation) -> Result<Self> {
        let database = Sema::open_with_schema(location.as_path(), &MANAGER_SCHEMA)?;
        database.write(|transaction| {
            ENGINE_RECORDS.ensure(transaction)?;
            ENGINE_EVENTS.ensure(transaction)?;
            ENGINE_LIFECYCLE_SNAPSHOT.ensure(transaction)?;
            ENGINE_STATUS_SNAPSHOT.ensure(transaction)?;
            ACTIVE_VERSION_SNAPSHOT.ensure(transaction)?;
            Ok(())
        })?;
        let tables = Self { database };
        tables.rebuild_snapshots_from_event_log()?;
        Ok(tables)
    }

    fn write_engine_record(&self, record: &StoredEngineRecord) -> Result<()> {
        Ok(self.database.write(|transaction| {
            ENGINE_RECORDS.insert(transaction, record.key(), record)?;
            Ok(())
        })?)
    }

    fn engine_record(&self, engine: &EngineIdentifier) -> Result<Option<StoredEngineRecord>> {
        Ok(self
            .database
            .read(|transaction| ENGINE_RECORDS.get(transaction, engine.as_str()))?)
    }

    /// Append one event and reduce it into both snapshot tables in the same
    /// write transaction, so the event log and the materialised snapshot
    /// move together or not at all.
    fn write_engine_event(&self, event: &EngineEvent) -> Result<()> {
        Ok(self.database.write(|transaction| {
            ENGINE_EVENTS.insert(transaction, event.key(), event)?;
            Self::reduce_event_into_snapshots(transaction, event)?;
            Ok(())
        })?)
    }

    fn engine_events(&self, engine: &EngineIdentifier) -> Result<Vec<EngineEvent>> {
        Ok(self.database.read(|transaction| {
            let events = ENGINE_EVENTS
                .iter(transaction)?
                .into_iter()
                .map(|(_sequence, event)| event)
                .filter(|event| event.engine() == engine)
                .collect();
            Ok(events)
        })?)
    }

    /// Every persisted event, regardless of engine, in sequence order.
    /// Used by orphan detection so a single scan covers every engine
    /// the manager has launched against this catalog.
    fn all_engine_events(&self) -> Result<Vec<EngineEvent>> {
        Ok(self.database.read(|transaction| {
            Ok(ENGINE_EVENTS
                .iter(transaction)?
                .into_iter()
                .map(|(_sequence, event)| event)
                .collect())
        })?)
    }

    fn highest_event_sequence(&self) -> Result<Option<EngineEventSequence>> {
        Ok(self.database.read(|transaction| {
            let sequence = ENGINE_EVENTS
                .iter(transaction)?
                .into_iter()
                .map(|(sequence, _event)| EngineEventSequence::new(sequence))
                .last();
            Ok(sequence)
        })?)
    }

    fn engine_lifecycle_snapshot(
        &self,
        engine: &EngineIdentifier,
    ) -> Result<Vec<ComponentLifecycleSnapshotRow>> {
        let prefix = SnapshotKey::engine_prefix(engine);
        Ok(self.database.read(|transaction| {
            let rows = ENGINE_LIFECYCLE_SNAPSHOT
                .iter(transaction)?
                .into_iter()
                .filter(|(key, _row)| key.starts_with(&prefix))
                .map(|(_key, row)| row)
                .collect();
            Ok(rows)
        })?)
    }

    fn engine_status_snapshot(
        &self,
        engine: &EngineIdentifier,
    ) -> Result<Vec<ComponentStatusSnapshotRow>> {
        let prefix = SnapshotKey::engine_prefix(engine);
        Ok(self.database.read(|transaction| {
            let rows = ENGINE_STATUS_SNAPSHOT
                .iter(transaction)?
                .into_iter()
                .filter(|(key, _row)| key.starts_with(&prefix))
                .map(|(_key, row)| row)
                .collect();
            Ok(rows)
        })?)
    }

    fn active_version(
        &self,
        engine: &EngineIdentifier,
        component: &ComponentName,
    ) -> Result<Option<ActiveVersion>> {
        let key = SnapshotKey::new(engine, component);
        Ok(self
            .database
            .read(|transaction| ACTIVE_VERSION_SNAPSHOT.get(transaction, key.as_str()))?)
    }

    /// Replay every persisted `EngineEvent` into both snapshot tables. Run
    /// once per `open` so a manager that crashes mid-append still presents
    /// a snapshot consistent with the event log.
    fn rebuild_snapshots_from_event_log(&self) -> Result<()> {
        let events: Vec<EngineEvent> = self.database.read(|transaction| {
            Ok(ENGINE_EVENTS
                .iter(transaction)?
                .into_iter()
                .map(|(_sequence, event)| event)
                .collect())
        })?;
        Ok(self.database.write(|transaction| {
            for event in &events {
                Self::reduce_event_into_snapshots(transaction, event)?;
            }
            Ok(())
        })?)
    }

    /// Drop every row in both snapshot tables, then replay the event log
    /// to materialise them again. Used by maintenance paths and by
    /// architectural-truth tests that prove the event log is the
    /// authoritative source — the snapshot rows must reappear with the
    /// same contents after a forced truncation. The snapshot tables are
    /// always projections; this operation never loses durable state.
    fn truncate_and_rebuild_snapshots(&self) -> Result<()> {
        let lifecycle_keys: Vec<String> = self.database.read(|transaction| {
            Ok(ENGINE_LIFECYCLE_SNAPSHOT
                .iter(transaction)?
                .into_iter()
                .map(|(key, _row)| key)
                .collect())
        })?;
        let status_keys: Vec<String> = self.database.read(|transaction| {
            Ok(ENGINE_STATUS_SNAPSHOT
                .iter(transaction)?
                .into_iter()
                .map(|(key, _row)| key)
                .collect())
        })?;
        let active_version_keys: Vec<String> = self.database.read(|transaction| {
            Ok(ACTIVE_VERSION_SNAPSHOT
                .iter(transaction)?
                .into_iter()
                .map(|(key, _row)| key)
                .collect())
        })?;
        self.database.write(|transaction| {
            for key in &lifecycle_keys {
                ENGINE_LIFECYCLE_SNAPSHOT.remove(transaction, key.as_str())?;
            }
            for key in &status_keys {
                ENGINE_STATUS_SNAPSHOT.remove(transaction, key.as_str())?;
            }
            for key in &active_version_keys {
                ACTIVE_VERSION_SNAPSHOT.remove(transaction, key.as_str())?;
            }
            Ok(())
        })?;
        self.rebuild_snapshots_from_event_log()
    }

    fn reduce_event_into_snapshots(
        transaction: &redb::WriteTransaction,
        event: &EngineEvent,
    ) -> sema::Result<()> {
        let engine = event.engine();
        match event.body() {
            EngineEventBody::ComponentSpawned(lifecycle) => Self::write_component_state(
                transaction,
                engine,
                lifecycle.component().clone(),
                ComponentProcessState::Launched,
                ComponentHealth::Starting,
            )?,
            EngineEventBody::ComponentReady(lifecycle) => Self::write_component_state(
                transaction,
                engine,
                lifecycle.component().clone(),
                ComponentProcessState::Ready,
                ComponentHealth::Running,
            )?,
            EngineEventBody::ComponentStopped(lifecycle) => Self::write_component_state(
                transaction,
                engine,
                lifecycle.component().clone(),
                ComponentProcessState::Stopping,
                ComponentHealth::Stopped,
            )?,
            EngineEventBody::ComponentExited(exit) => Self::write_component_state(
                transaction,
                engine,
                exit.component().clone(),
                ComponentProcessState::Exited,
                Self::health_from_exit(exit),
            )?,
            EngineEventBody::ComponentOrphaned(orphan) => Self::write_component_state(
                transaction,
                engine,
                orphan.component().clone(),
                ComponentProcessState::Exited,
                ComponentHealth::Failed,
            )?,
            EngineEventBody::RestartScheduled(_) => {}
            EngineEventBody::RestartExhausted(restart) => Self::write_component_state(
                transaction,
                engine,
                restart.component().clone(),
                ComponentProcessState::Exited,
                ComponentHealth::Failed,
            )?,
            EngineEventBody::ComponentUnimplemented(_)
            | EngineEventBody::EngineStateChanged(_)
            | EngineEventBody::UpgradePrepared(_)
            | EngineEventBody::VersionQuarantined(_) => {}
            EngineEventBody::ActiveVersionChanged(change) => {
                let row = ActiveVersion::from_change(change);
                let key = SnapshotKey::new(engine, row.component());
                ACTIVE_VERSION_SNAPSHOT.insert(transaction, key.as_str(), &row)?;
            }
        }
        Ok(())
    }

    fn write_component_state(
        transaction: &redb::WriteTransaction,
        engine: &EngineIdentifier,
        component: ComponentName,
        process_state: ComponentProcessState,
        health: ComponentHealth,
    ) -> sema::Result<()> {
        let key = SnapshotKey::new(engine, &component);
        let lifecycle_row = ComponentLifecycleSnapshotRow::new(component.clone(), process_state);
        ENGINE_LIFECYCLE_SNAPSHOT.insert(transaction, key.as_str(), &lifecycle_row)?;
        let status_row = ComponentStatusSnapshotRow::new(component, health);
        ENGINE_STATUS_SNAPSHOT.insert(transaction, key.as_str(), &status_row)?;
        Ok(())
    }

    fn health_from_exit(exit: &ComponentExited) -> ComponentHealth {
        match exit.exit_code() {
            Some(0) => ComponentHealth::Stopped,
            _ => ComponentHealth::Failed,
        }
    }
}

pub struct ManagerStore {
    tables: Option<ManagerTables>,
    write_count: u64,
    event_sequence: EngineEventSequence,
}

impl ManagerStore {
    pub fn open(location: ManagerStoreLocation) -> Result<Self> {
        let tables = ManagerTables::open(&location)?;
        let event_sequence = tables
            .highest_event_sequence()?
            .unwrap_or(EngineEventSequence::new(0));
        Ok(Self {
            tables: Some(tables),
            write_count: 0,
            event_sequence,
        })
    }

    pub fn start(location: ManagerStoreLocation) -> Result<ActorRef<Self>> {
        let store = Self::open(location)?;
        Ok(Self::spawn_in_thread(store))
    }

    pub async fn close_and_stop(reference: ActorRef<Self>) -> Result<()> {
        reference
            .ask(CloseManagerStore)
            .await
            .map_err(|error| crate::Error::actor("close manager store", error))?;
        reference
            .stop_gracefully()
            .await
            .map_err(|error| crate::Error::actor("stop manager store", error))?;
        let _shutdown_completion = reference.wait_for_shutdown().await;
        Ok(())
    }

    fn tables(&self) -> Result<&ManagerTables> {
        self.tables.as_ref().ok_or(crate::Error::ManagerStoreClosed)
    }

    fn close_tables(&mut self) {
        self.tables = None;
    }

    fn persist_engine_record(&mut self, record: StoredEngineRecord) -> Result<ManagerStoreReceipt> {
        self.tables()?.write_engine_record(&record)?;
        self.write_count = self.write_count.saturating_add(1);
        Ok(ManagerStoreReceipt::new(record.engine, self.write_count))
    }

    fn read_engine_record(&self, engine: &EngineIdentifier) -> Result<Option<StoredEngineRecord>> {
        self.tables()?.engine_record(engine)
    }

    fn append_engine_event(&mut self, draft: EngineEventDraft) -> Result<EngineEventReceipt> {
        let sequence = self.event_sequence.next();
        let event = draft.into_event(sequence);
        self.tables()?.write_engine_event(&event)?;
        self.event_sequence = sequence;
        self.write_count = self.write_count.saturating_add(1);
        Ok(EngineEventReceipt::new(sequence, self.write_count))
    }

    fn read_engine_events(&self, engine: &EngineIdentifier) -> Result<Vec<EngineEvent>> {
        self.tables()?.engine_events(engine)
    }

    fn read_engine_lifecycle_snapshot(
        &self,
        engine: &EngineIdentifier,
    ) -> Result<Vec<ComponentLifecycleSnapshotRow>> {
        self.tables()?.engine_lifecycle_snapshot(engine)
    }

    fn read_engine_status_snapshot(
        &self,
        engine: &EngineIdentifier,
    ) -> Result<Vec<ComponentStatusSnapshotRow>> {
        self.tables()?.engine_status_snapshot(engine)
    }

    fn read_active_version(
        &self,
        engine: &EngineIdentifier,
        component: &ComponentName,
    ) -> Result<Option<ActiveVersion>> {
        self.tables()?.active_version(engine, component)
    }

    fn force_rebuild_snapshots(&mut self) -> Result<()> {
        self.tables()?.truncate_and_rebuild_snapshots()?;
        self.write_count = self.write_count.saturating_add(1);
        Ok(())
    }

    /// Scan the engine event log for orphan arcs — `(engine, component)`
    /// pairs whose most recent lifecycle event is `ComponentSpawned`
    /// without a matching `ComponentReady`, `ComponentExited`,
    /// `ComponentOrphaned`, or `RestartExhausted` terminator. For each
    /// such pair, append one `ComponentOrphaned` event so the snapshot
    /// reducer projects the component into `Exited / Failed`. Returns
    /// the orphan events appended; safe to call repeatedly because a
    /// freshly-orphaned arc gains a terminator (`Orphaned`) and stops
    /// matching the orphan predicate.
    pub fn append_orphans_from_event_log(&mut self) -> Result<Vec<EngineEvent>> {
        let events = self.tables()?.all_engine_events()?;
        let orphans = Self::orphan_candidates(&events);
        let mut appended = Vec::with_capacity(orphans.len());
        for orphan in orphans {
            let draft = EngineEventDraft::from_input(EngineEventDraftInput {
                engine: orphan.engine,
                source: EngineEventSource::Manager,
                body: EngineEventBody::ComponentOrphaned(ComponentOrphaned::from_input(
                    ComponentOrphanedInput {
                        component: orphan.component,
                        spawned_sequence: orphan.spawned_sequence,
                    },
                )),
            });
            let sequence = self.event_sequence.next();
            let event = draft.into_event(sequence);
            self.tables()?.write_engine_event(&event)?;
            self.event_sequence = sequence;
            self.write_count = self.write_count.saturating_add(1);
            appended.push(event);
        }
        Ok(appended)
    }

    /// Scan the event sequence and return one `OrphanCandidate` per
    /// `(engine, component)` pair whose most-recent lifecycle-arc event
    /// is `ComponentSpawned` — the prior daemon recorded the spawn but
    /// no terminator. Events are visited in sequence order so the most
    /// recent lifecycle event determines arc state. `EngineIdentifier` does not
    /// implement `Hash`, so the working map keys on owned strings.
    fn orphan_candidates(events: &[EngineEvent]) -> Vec<OrphanCandidate> {
        use std::collections::BTreeMap;
        #[derive(Clone)]
        struct ArcState {
            engine: EngineIdentifier,
            component: ComponentName,
            spawned_sequence: EngineEventSequence,
            in_flight: bool,
        }
        let mut arcs: BTreeMap<(String, String), ArcState> = BTreeMap::new();
        for event in events {
            let engine = event.engine().clone();
            match event.body() {
                EngineEventBody::ComponentSpawned(lifecycle) => {
                    let key = (
                        engine.as_str().to_string(),
                        lifecycle.component().as_str().to_string(),
                    );
                    arcs.insert(
                        key,
                        ArcState {
                            engine,
                            component: lifecycle.component().clone(),
                            spawned_sequence: event.sequence(),
                            in_flight: true,
                        },
                    );
                }
                EngineEventBody::ComponentReady(lifecycle) => {
                    let key = (
                        engine.as_str().to_string(),
                        lifecycle.component().as_str().to_string(),
                    );
                    if let Some(arc) = arcs.get_mut(&key) {
                        arc.in_flight = false;
                    }
                }
                EngineEventBody::ComponentExited(exit) => {
                    let key = (
                        engine.as_str().to_string(),
                        exit.component().as_str().to_string(),
                    );
                    if let Some(arc) = arcs.get_mut(&key) {
                        arc.in_flight = false;
                    }
                }
                EngineEventBody::ComponentOrphaned(orphan) => {
                    let key = (
                        engine.as_str().to_string(),
                        orphan.component().as_str().to_string(),
                    );
                    if let Some(arc) = arcs.get_mut(&key) {
                        arc.in_flight = false;
                    }
                }
                EngineEventBody::RestartExhausted(restart) => {
                    let key = (
                        engine.as_str().to_string(),
                        restart.component().as_str().to_string(),
                    );
                    if let Some(arc) = arcs.get_mut(&key) {
                        arc.in_flight = false;
                    }
                }
                EngineEventBody::ComponentStopped(_)
                | EngineEventBody::ComponentUnimplemented(_)
                | EngineEventBody::RestartScheduled(_)
                | EngineEventBody::EngineStateChanged(_)
                | EngineEventBody::UpgradePrepared(_)
                | EngineEventBody::ActiveVersionChanged(_)
                | EngineEventBody::VersionQuarantined(_) => {}
            }
        }
        let mut candidates: Vec<OrphanCandidate> = arcs
            .into_values()
            .filter(|arc| arc.in_flight)
            .map(|arc| OrphanCandidate {
                engine: arc.engine,
                component: arc.component,
                spawned_sequence: arc.spawned_sequence,
            })
            .collect();
        candidates.sort_by_key(|candidate| candidate.spawned_sequence.into_u64());
        candidates
    }

    fn write_count(&self) -> u64 {
        self.write_count
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OrphanCandidate {
    engine: EngineIdentifier,
    component: ComponentName,
    spawned_sequence: EngineEventSequence,
}

impl Actor for ManagerStore {
    type Args = Self;
    type Error = Infallible;

    async fn on_start(
        store: Self::Args,
        _actor_reference: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(store)
    }

    /// Best-effort fallback for plain actor stops. Callers that need a
    /// release witness use `CloseManagerStore` / `close_and_stop` so the
    /// redb handle is dropped through the actor mailbox before shutdown
    /// completion is observed.
    async fn on_stop(
        &mut self,
        _actor_reference: kameo::actor::WeakActorRef<Self>,
        _reason: kameo::error::ActorStopReason,
    ) -> std::result::Result<(), Self::Error> {
        self.close_tables();
        Ok(())
    }
}

pub struct PersistEngineRecord {
    record: StoredEngineRecord,
}

impl PersistEngineRecord {
    pub fn new(engine: EngineIdentifier, status: EngineStatus) -> Self {
        Self {
            record: StoredEngineRecord::new(engine, status),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerStoreReceipt {
    engine: EngineIdentifier,
    write_count: u64,
}

impl ManagerStoreReceipt {
    fn new(engine: EngineIdentifier, write_count: u64) -> Self {
        Self {
            engine,
            write_count,
        }
    }

    pub fn engine(&self) -> &EngineIdentifier {
        &self.engine
    }

    pub fn write_count(&self) -> u64 {
        self.write_count
    }
}

impl Message<PersistEngineRecord> for ManagerStore {
    type Reply = Result<ManagerStoreReceipt>;

    async fn handle(
        &mut self,
        message: PersistEngineRecord,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.persist_engine_record(message.record)
    }
}

pub struct AppendEngineEvent {
    draft: EngineEventDraft,
}

impl AppendEngineEvent {
    pub fn new(draft: EngineEventDraft) -> Self {
        Self { draft }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineEventReceipt {
    sequence: EngineEventSequence,
    write_count: u64,
}

impl EngineEventReceipt {
    fn new(sequence: EngineEventSequence, write_count: u64) -> Self {
        Self {
            sequence,
            write_count,
        }
    }

    pub fn sequence(&self) -> EngineEventSequence {
        self.sequence
    }

    pub fn write_count(&self) -> u64 {
        self.write_count
    }
}

impl Message<AppendEngineEvent> for ManagerStore {
    type Reply = Result<EngineEventReceipt>;

    async fn handle(
        &mut self,
        message: AppendEngineEvent,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.append_engine_event(message.draft)
    }
}

pub struct ReadEngineRecord {
    engine: EngineIdentifier,
}

impl ReadEngineRecord {
    pub fn new(engine: EngineIdentifier) -> Self {
        Self { engine }
    }
}

impl Message<ReadEngineRecord> for ManagerStore {
    type Reply = Result<Option<StoredEngineRecord>>;

    async fn handle(
        &mut self,
        message: ReadEngineRecord,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.read_engine_record(&message.engine)
    }
}

pub struct ReadEngineEvents {
    engine: EngineIdentifier,
}

impl ReadEngineEvents {
    pub fn new(engine: EngineIdentifier) -> Self {
        Self { engine }
    }
}

impl Message<ReadEngineEvents> for ManagerStore {
    type Reply = Result<Vec<EngineEvent>>;

    async fn handle(
        &mut self,
        message: ReadEngineEvents,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.read_engine_events(&message.engine)
    }
}

pub struct ReadManagerStoreWriteCount;

impl Message<ReadManagerStoreWriteCount> for ManagerStore {
    type Reply = u64;

    async fn handle(
        &mut self,
        _message: ReadManagerStoreWriteCount,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.write_count()
    }
}

pub struct CloseManagerStore;

impl Message<CloseManagerStore> for ManagerStore {
    type Reply = Result<()>;

    async fn handle(
        &mut self,
        _message: CloseManagerStore,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.close_tables();
        Ok(())
    }
}

pub struct ReadEngineLifecycleSnapshot {
    engine: EngineIdentifier,
}

impl ReadEngineLifecycleSnapshot {
    pub fn new(engine: EngineIdentifier) -> Self {
        Self { engine }
    }
}

impl Message<ReadEngineLifecycleSnapshot> for ManagerStore {
    type Reply = Result<Vec<ComponentLifecycleSnapshotRow>>;

    async fn handle(
        &mut self,
        message: ReadEngineLifecycleSnapshot,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.read_engine_lifecycle_snapshot(&message.engine)
    }
}

pub struct ReadEngineStatusSnapshot {
    engine: EngineIdentifier,
}

impl ReadEngineStatusSnapshot {
    pub fn new(engine: EngineIdentifier) -> Self {
        Self { engine }
    }
}

impl Message<ReadEngineStatusSnapshot> for ManagerStore {
    type Reply = Result<Vec<ComponentStatusSnapshotRow>>;

    async fn handle(
        &mut self,
        message: ReadEngineStatusSnapshot,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.read_engine_status_snapshot(&message.engine)
    }
}

pub struct ReadActiveVersion {
    engine: EngineIdentifier,
    component: ComponentName,
}

impl ReadActiveVersion {
    pub fn new(engine: EngineIdentifier, component: ComponentName) -> Self {
        Self { engine, component }
    }
}

impl Message<ReadActiveVersion> for ManagerStore {
    type Reply = Result<Option<ActiveVersion>>;

    async fn handle(
        &mut self,
        message: ReadActiveVersion,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.read_active_version(&message.engine, &message.component)
    }
}

/// Operational and architectural-truth verb: drop every row in both
/// snapshot tables and rebuild them from the event log. The event log
/// is the authoritative source; snapshots are projections. After this
/// call, both snapshot tables have exactly the same rows the reducer
/// would produce from the persisted event sequence.
pub struct RebuildSnapshotsFromEventLog;

impl Message<RebuildSnapshotsFromEventLog> for ManagerStore {
    type Reply = Result<()>;

    async fn handle(
        &mut self,
        _message: RebuildSnapshotsFromEventLog,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.force_rebuild_snapshots()
    }
}

/// Manager-startup verb: scan the event log for orphan arcs — pairs of
/// `(engine, component)` whose most recent lifecycle event is
/// `ComponentSpawned` with no matching `Ready`, `Exited`, `Orphaned`, or
/// `RestartExhausted`. For each such pair, append one
/// `ComponentOrphaned` event so the snapshot reducer projects the
/// component into `Exited / Failed`. Returns the orphan events appended,
/// in the order their `ComponentSpawned` rows were recorded.
pub struct AppendOrphansFromEventLog;

impl Message<AppendOrphansFromEventLog> for ManagerStore {
    type Reply = Result<Vec<EngineEvent>>;

    async fn handle(
        &mut self,
        _message: AppendOrphansFromEventLog,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.append_orphans_from_event_log()
    }
}
