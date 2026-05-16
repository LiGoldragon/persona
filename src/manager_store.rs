use std::path::{Path, PathBuf};

use kameo::actor::{Actor, ActorRef, Spawn};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use sema::{Schema, SchemaVersion, Sema, Table};
use signal_persona::{ComponentHealth, ComponentName, EngineStatus};
use signal_persona_auth::EngineId;

use crate::Result;
use crate::engine_event::{
    ComponentExited, EngineEvent, EngineEventBody, EngineEventDraft, EngineEventSequence,
};

const MANAGER_SCHEMA: Schema = Schema {
    version: SchemaVersion::new(3),
};

const ENGINE_RECORDS: Table<&'static str, StoredEngineRecord> =
    Table::new("manager.engine-records");
const ENGINE_EVENTS: Table<u64, EngineEvent> = Table::new("manager.engine-events");
const ENGINE_LIFECYCLE_SNAPSHOT: Table<&'static str, ComponentLifecycleSnapshotRow> =
    Table::new("manager.engine-lifecycle-snapshot");
const ENGINE_STATUS_SNAPSHOT: Table<&'static str, ComponentStatusSnapshotRow> =
    Table::new("manager.engine-status-snapshot");

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
    engine: EngineId,
    status: EngineStatus,
}

impl StoredEngineRecord {
    pub fn new(engine: EngineId, status: EngineStatus) -> Self {
        Self { engine, status }
    }

    pub fn engine(&self) -> &EngineId {
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
#[derive(
    rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, Copy, PartialEq, Eq,
)]
pub enum ComponentProcessState {
    Launched,
    Ready,
    Stopping,
    Exited,
}

/// Snapshot row stored in `manager.engine-lifecycle-snapshot`, keyed by
/// `engine_id::component_name`. The reducer overwrites the row on each
/// transition; readers project the latest state into `EngineStatus`.
#[derive(
    rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq,
)]
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
/// `engine_id::component_name`. Carries the same closed-enum
/// `ComponentHealth` that `signal_persona::EngineStatus` reports to CLI
/// status queries, with no extra ARCH-aspirational variants.
#[derive(
    rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq,
)]
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

/// Composite key `engine_id::component_name` used by both snapshot tables.
/// The `::` separator is unambiguous: `EngineId` and `ComponentName`
/// values do not contain it in any current shape.
pub struct SnapshotKey(String);

impl SnapshotKey {
    pub fn new(engine: &EngineId, component: &ComponentName) -> Self {
        Self(format!("{}::{}", engine.as_str(), component.as_str()))
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn engine_prefix(engine: &EngineId) -> String {
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

    fn engine_record(&self, engine: &EngineId) -> Result<Option<StoredEngineRecord>> {
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

    fn engine_events(&self, engine: &EngineId) -> Result<Vec<EngineEvent>> {
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
        engine: &EngineId,
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
        engine: &EngineId,
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
        self.database.write(|transaction| {
            for key in &lifecycle_keys {
                ENGINE_LIFECYCLE_SNAPSHOT.remove(transaction, key.as_str())?;
            }
            for key in &status_keys {
                ENGINE_STATUS_SNAPSHOT.remove(transaction, key.as_str())?;
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
            EngineEventBody::RestartScheduled(_) => {}
            EngineEventBody::RestartExhausted(restart) => Self::write_component_state(
                transaction,
                engine,
                restart.component().clone(),
                ComponentProcessState::Exited,
                ComponentHealth::Failed,
            )?,
            EngineEventBody::ComponentUnimplemented(_)
            | EngineEventBody::EngineStateChanged(_) => {}
        }
        Ok(())
    }

    fn write_component_state(
        transaction: &redb::WriteTransaction,
        engine: &EngineId,
        component: ComponentName,
        process_state: ComponentProcessState,
        health: ComponentHealth,
    ) -> sema::Result<()> {
        let key = SnapshotKey::new(engine, &component);
        let lifecycle_row =
            ComponentLifecycleSnapshotRow::new(component.clone(), process_state);
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

    fn tables(&self) -> Result<&ManagerTables> {
        self.tables
            .as_ref()
            .ok_or(crate::Error::ManagerStoreClosed)
    }

    fn persist_engine_record(&mut self, record: StoredEngineRecord) -> Result<ManagerStoreReceipt> {
        self.tables()?.write_engine_record(&record)?;
        self.write_count = self.write_count.saturating_add(1);
        Ok(ManagerStoreReceipt::new(record.engine, self.write_count))
    }

    fn read_engine_record(&self, engine: &EngineId) -> Result<Option<StoredEngineRecord>> {
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

    fn read_engine_events(&self, engine: &EngineId) -> Result<Vec<EngineEvent>> {
        self.tables()?.engine_events(engine)
    }

    fn read_engine_lifecycle_snapshot(
        &self,
        engine: &EngineId,
    ) -> Result<Vec<ComponentLifecycleSnapshotRow>> {
        self.tables()?.engine_lifecycle_snapshot(engine)
    }

    fn read_engine_status_snapshot(
        &self,
        engine: &EngineId,
    ) -> Result<Vec<ComponentStatusSnapshotRow>> {
        self.tables()?.engine_status_snapshot(engine)
    }

    fn force_rebuild_snapshots(&mut self) -> Result<()> {
        self.tables()?.truncate_and_rebuild_snapshots()?;
        self.write_count = self.write_count.saturating_add(1);
        Ok(())
    }

    fn write_count(&self) -> u64 {
        self.write_count
    }
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

    /// Drop the typed table handle so the underlying redb file lock releases
    /// during `on_stop`, before the mailbox's last sender clone closes. A
    /// subsequent `ManagerStore::open` on the same path sees an unlocked
    /// database instead of racing the spawn-thread's tear-down.
    async fn on_stop(
        &mut self,
        _actor_reference: kameo::actor::WeakActorRef<Self>,
        _reason: kameo::error::ActorStopReason,
    ) -> std::result::Result<(), Self::Error> {
        self.tables = None;
        Ok(())
    }
}

pub struct PersistEngineRecord {
    record: StoredEngineRecord,
}

impl PersistEngineRecord {
    pub fn new(engine: EngineId, status: EngineStatus) -> Self {
        Self {
            record: StoredEngineRecord::new(engine, status),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerStoreReceipt {
    engine: EngineId,
    write_count: u64,
}

impl ManagerStoreReceipt {
    fn new(engine: EngineId, write_count: u64) -> Self {
        Self {
            engine,
            write_count,
        }
    }

    pub fn engine(&self) -> &EngineId {
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
    engine: EngineId,
}

impl ReadEngineRecord {
    pub fn new(engine: EngineId) -> Self {
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
    engine: EngineId,
}

impl ReadEngineEvents {
    pub fn new(engine: EngineId) -> Self {
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

pub struct ReadEngineLifecycleSnapshot {
    engine: EngineId,
}

impl ReadEngineLifecycleSnapshot {
    pub fn new(engine: EngineId) -> Self {
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
    engine: EngineId,
}

impl ReadEngineStatusSnapshot {
    pub fn new(engine: EngineId) -> Self {
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
