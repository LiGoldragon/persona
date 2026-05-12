use std::path::{Path, PathBuf};

use kameo::actor::{Actor, ActorRef, Spawn};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use sema::{Schema, SchemaVersion, Sema, Table};
use signal_persona::EngineStatus;
use signal_persona_auth::EngineId;

use crate::Result;
use crate::engine_event::{EngineEvent, EngineEventDraft, EngineEventSequence};

const MANAGER_SCHEMA: Schema = Schema {
    version: SchemaVersion::new(2),
};

const ENGINE_RECORDS: Table<&'static str, StoredEngineRecord> =
    Table::new("manager.engine-records");
const ENGINE_EVENTS: Table<u64, EngineEvent> = Table::new("manager.engine-events");

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

struct ManagerTables {
    database: Sema,
}

impl ManagerTables {
    fn open(location: &ManagerStoreLocation) -> Result<Self> {
        let database = Sema::open_with_schema(location.as_path(), &MANAGER_SCHEMA)?;
        database.write(|transaction| {
            ENGINE_RECORDS.ensure(transaction)?;
            ENGINE_EVENTS.ensure(transaction)?;
            Ok(())
        })?;
        Ok(Self { database })
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

    fn write_engine_event(&self, event: &EngineEvent) -> Result<()> {
        Ok(self.database.write(|transaction| {
            ENGINE_EVENTS.insert(transaction, event.key(), event)?;
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
}

pub struct ManagerStore {
    tables: ManagerTables,
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
            tables,
            write_count: 0,
            event_sequence,
        })
    }

    pub fn start(location: ManagerStoreLocation) -> Result<ActorRef<Self>> {
        let store = Self::open(location)?;
        Ok(Self::spawn_in_thread(store))
    }

    fn persist_engine_record(&mut self, record: StoredEngineRecord) -> Result<ManagerStoreReceipt> {
        self.tables.write_engine_record(&record)?;
        self.write_count = self.write_count.saturating_add(1);
        Ok(ManagerStoreReceipt::new(record.engine, self.write_count))
    }

    fn read_engine_record(&self, engine: &EngineId) -> Result<Option<StoredEngineRecord>> {
        self.tables.engine_record(engine)
    }

    fn append_engine_event(&mut self, draft: EngineEventDraft) -> Result<EngineEventReceipt> {
        let sequence = self.event_sequence.next();
        let event = draft.into_event(sequence);
        self.tables.write_engine_event(&event)?;
        self.event_sequence = sequence;
        self.write_count = self.write_count.saturating_add(1);
        Ok(EngineEventReceipt::new(sequence, self.write_count))
    }

    fn read_engine_events(&self, engine: &EngineId) -> Result<Vec<EngineEvent>> {
        self.tables.engine_events(engine)
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
