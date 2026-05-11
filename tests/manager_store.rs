use persona::manager::EngineManager;
use persona::manager_store::{
    ManagerStore, ManagerStoreLocation, PersistEngineRecord, ReadEngineRecord,
    ReadManagerStoreWriteCount,
};
use persona::state::EngineState;
use signal_persona::{
    ComponentDesiredState, ComponentHealth, ComponentName, ComponentShutdown, ComponentStatusQuery,
    EngineReply, EngineRequest,
};
use signal_persona_auth::EngineId;

struct StoreFixture {
    root: std::path::PathBuf,
    location: ManagerStoreLocation,
}

impl StoreFixture {
    fn new(name: &str) -> Self {
        let unique = UniqueName::from_system_time().into_string();
        let root = std::env::temp_dir().join(format!("{name}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("store fixture root created");
        Self {
            location: ManagerStoreLocation::new(root.join("manager.redb")),
            root,
        }
    }

    fn location(&self) -> ManagerStoreLocation {
        self.location.clone()
    }
}

impl Drop for StoreFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct UniqueName {
    nanos: u128,
}

impl UniqueName {
    fn from_system_time() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        Self { nanos }
    }

    fn into_string(self) -> String {
        self.nanos.to_string()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_manager_store_writes_engine_status_through_writer_actor() {
    let fixture = StoreFixture::new("persona-manager-store-writer");
    let engine = EngineId::new("engine-store-writer");
    let status = EngineState::default_catalog().snapshot().clone();

    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let receipt = store
        .ask(PersistEngineRecord::new(engine.clone(), status.clone()))
        .await
        .expect("record persisted through actor");
    assert_eq!(receipt.engine(), &engine);
    assert_eq!(receipt.write_count(), 1);
    assert_eq!(
        store
            .ask(ReadManagerStoreWriteCount)
            .await
            .expect("write count read through actor"),
        1
    );
    let record = store
        .ask(ReadEngineRecord::new(engine.clone()))
        .await
        .expect("record read through actor")
        .expect("engine record exists");
    assert_eq!(record.engine(), &engine);
    assert_eq!(record.status(), &status);
    store.stop_gracefully().await.expect("manager store stops");
    store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_persists_component_mutation_through_manager_store() {
    let fixture = StoreFixture::new("persona-engine-manager-store-path");
    let engine = EngineId::new("engine-manager-store-path");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("engine manager starts with store");

    let initial_record = store
        .ask(ReadEngineRecord::new(engine.clone()))
        .await
        .expect("initial record read through store actor")
        .expect("initial record exists");
    assert_eq!(initial_record.status().generation.into_u64(), 0);

    let reply = manager
        .ask(persona::manager::HandleEngineRequest::new(
            EngineRequest::ComponentShutdown(ComponentShutdown {
                component: ComponentName::new("persona-terminal"),
            }),
        ))
        .await
        .expect("shutdown handled through manager actor");
    assert!(matches!(reply, EngineReply::SupervisorActionAccepted(_)));

    let stored_record = store
        .ask(ReadEngineRecord::new(engine.clone()))
        .await
        .expect("stored record read through store actor")
        .expect("stored record exists");
    assert_eq!(stored_record.status().generation.into_u64(), 1);

    let terminal_status = stored_record
        .status()
        .components
        .iter()
        .find(|component| component.name.as_str() == "persona-terminal")
        .expect("terminal component stored");
    assert_eq!(
        terminal_status.desired_state,
        ComponentDesiredState::Stopped
    );
    assert_eq!(terminal_status.health, ComponentHealth::Stopped);

    let query = manager
        .ask(persona::manager::HandleEngineRequest::new(
            EngineRequest::ComponentStatusQuery(ComponentStatusQuery {
                component: ComponentName::new("persona-terminal"),
            }),
        ))
        .await
        .expect("status handled through manager actor");
    assert!(matches!(query, EngineReply::ComponentStatus(_)));

    EngineManager::stop(manager)
        .await
        .expect("engine manager stops");
    store.stop_gracefully().await.expect("manager store stops");
    store.wait_for_shutdown().await;
}
