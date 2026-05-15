use persona::engine_event::{
    ComponentLifecycleEvent, ComponentOperation, ComponentUnimplemented,
    ComponentUnimplementedInput, EngineEventBody, EngineEventDraft, EngineEventDraftInput,
    EngineEventSource, HarnessOperationKind, UnimplementedReason,
};
use persona::manager::EngineManager;
use persona::manager_store::{
    AppendEngineEvent, ManagerStore, ManagerStoreLocation, PersistEngineRecord, ReadEngineEvents,
    ReadEngineRecord, ReadManagerStoreWriteCount,
};
use persona::schema::{
    ComponentOperationReport, EngineEventBodyReport, EngineEventReport, EngineEventSourceKind,
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

    fn spawned_event(engine: EngineId, component: &str) -> EngineEventDraft {
        let component = ComponentName::new(component);
        EngineEventDraft::from_input(EngineEventDraftInput {
            engine,
            source: EngineEventSource::Manager,
            body: EngineEventBody::ComponentSpawned(ComponentLifecycleEvent::new(component)),
        })
    }

    fn unimplemented_event(engine: EngineId, component: &str) -> EngineEventDraft {
        let component = ComponentName::new(component);
        EngineEventDraft::from_input(EngineEventDraftInput {
            engine,
            source: EngineEventSource::Component(component.clone()),
            body: EngineEventBody::ComponentUnimplemented(ComponentUnimplemented::from_input(
                ComponentUnimplementedInput {
                    component,
                    operation: ComponentOperation::Harness(HarnessOperationKind::MessageDelivery),
                    reason: UnimplementedReason::NotBuiltYet,
                },
            )),
        })
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_restores_persisted_snapshot_before_answering_status() {
    let fixture = StoreFixture::new("persona-engine-manager-restore");
    let engine = EngineId::new("engine-manager-restore");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("engine manager starts with store");

    let reply = manager
        .ask(persona::manager::HandleEngineRequest::new(
            EngineRequest::ComponentShutdown(ComponentShutdown {
                component: ComponentName::new("persona-terminal"),
            }),
        ))
        .await
        .expect("shutdown handled through manager actor");
    assert!(matches!(reply, EngineReply::SupervisorActionAccepted(_)));

    EngineManager::stop(manager)
        .await
        .expect("first engine manager stops");

    let restored = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("engine manager restores from store");
    let status = restored
        .ask(persona::manager::HandleEngineRequest::new(
            EngineRequest::ComponentStatusQuery(ComponentStatusQuery {
                component: ComponentName::new("persona-terminal"),
            }),
        ))
        .await
        .expect("status handled through restored manager actor");

    let EngineReply::ComponentStatus(status) = status else {
        panic!("expected restored component status");
    };
    assert_eq!(status.desired_state, ComponentDesiredState::Stopped);
    assert_eq!(status.health, ComponentHealth::Stopped);

    let record = store
        .ask(ReadEngineRecord::new(engine.clone()))
        .await
        .expect("stored record read through store actor")
        .expect("stored engine record exists");
    assert_eq!(record.status().generation.into_u64(), 1);

    EngineManager::stop(restored)
        .await
        .expect("restored engine manager stops");
    store.stop_gracefully().await.expect("manager store stops");
    store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_event_log_records_typed_manager_events() {
    let fixture = StoreFixture::new("persona-manager-event-log");
    let engine = EngineId::new("engine-event-log");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");

    let first = store
        .ask(AppendEngineEvent::new(StoreFixture::spawned_event(
            engine.clone(),
            "persona-router",
        )))
        .await
        .expect("component spawned event appends through store actor");
    assert_eq!(first.sequence().into_u64(), 1);
    assert_eq!(first.write_count(), 1);

    let second = store
        .ask(AppendEngineEvent::new(StoreFixture::unimplemented_event(
            engine.clone(),
            "persona-harness",
        )))
        .await
        .expect("component unimplemented event appends through store actor");
    assert_eq!(second.sequence().into_u64(), 2);

    let events = store
        .ask(ReadEngineEvents::new(engine.clone()))
        .await
        .expect("events read through store actor");
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0].body(),
        EngineEventBody::ComponentSpawned(_)
    ));
    assert!(matches!(
        events[1].body(),
        EngineEventBody::ComponentUnimplemented(unimplemented)
            if unimplemented.operation()
                == &ComponentOperation::Harness(HarnessOperationKind::MessageDelivery)
    ));

    store.stop_gracefully().await.expect("manager store stops");
    store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_event_log_nota_projection_is_view() {
    let fixture = StoreFixture::new("persona-manager-event-log-projection");
    let engine = EngineId::new("engine-event-projection");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");

    store
        .ask(AppendEngineEvent::new(StoreFixture::unimplemented_event(
            engine.clone(),
            "persona-harness",
        )))
        .await
        .expect("component spawned event appends through store actor");
    let events = store
        .ask(ReadEngineEvents::new(engine.clone()))
        .await
        .expect("events read through store actor");
    let projection = EngineEventReport::from_event(&events[0]);
    let nota = projection.to_nota().expect("event projection encodes");
    let recovered = EngineEventReport::from_nota(&nota).expect("event projection decodes");

    assert_eq!(recovered, projection);
    assert_eq!(projection.sequence, 1);
    assert_eq!(projection.engine.as_str(), "engine-event-projection");
    assert_eq!(projection.source, EngineEventSourceKind::Component);
    assert_eq!(
        projection
            .source_component
            .as_ref()
            .expect("component source projected")
            .as_str(),
        "persona-harness"
    );
    assert!(matches!(
        projection.body,
        EngineEventBodyReport::ComponentUnimplemented(ref unimplemented)
            if unimplemented.component.as_str() == "persona-harness"
                && unimplemented.operation
                    == (ComponentOperationReport::Harness {
                        operation: HarnessOperationKind::MessageDelivery,
                    })
                && unimplemented.reason == UnimplementedReason::NotBuiltYet
    ));
    assert!(
        nota.starts_with("(EngineEventReport 1 engine-event-projection Component persona-harness (ComponentUnimplemented")
    );

    store.stop_gracefully().await.expect("manager store stops");
    store.wait_for_shutdown().await;
}
