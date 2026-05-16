use persona::engine_event::{
    ComponentLifecycleEvent, ComponentOperation, ComponentUnimplemented,
    ComponentUnimplementedInput, EngineEventBody, EngineEventDraft, EngineEventDraftInput,
    EngineEventSource, HarnessOperationKind, UnimplementedReason,
};
use persona::manager::EngineManager;
use persona::manager_store::{
    AppendEngineEvent, ComponentLifecycleSnapshotRow, ComponentProcessState,
    ComponentStatusSnapshotRow, ManagerStore, ManagerStoreLocation, PersistEngineRecord,
    ReadEngineEvents, ReadEngineLifecycleSnapshot, ReadEngineRecord, ReadEngineStatusSnapshot,
    ReadManagerStoreWriteCount, RebuildSnapshotsFromEventLog,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_manager_store_reduces_lifecycle_events_into_snapshot_tables() {
    let fixture = StoreFixture::new("persona-manager-store-snapshot-reduce");
    let engine = EngineId::new("engine-snapshot-reduce");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");

    store
        .ask(AppendEngineEvent::new(StoreFixture::spawned_event(
            engine.clone(),
            "persona-router",
        )))
        .await
        .expect("spawn event appends");

    let lifecycle_after_spawn = store
        .ask(ReadEngineLifecycleSnapshot::new(engine.clone()))
        .await
        .expect("lifecycle snapshot reads");
    assert_eq!(lifecycle_after_spawn.len(), 1);
    assert_eq!(lifecycle_after_spawn[0].component().as_str(), "persona-router");
    assert_eq!(
        lifecycle_after_spawn[0].process_state(),
        ComponentProcessState::Launched
    );

    let status_after_spawn = store
        .ask(ReadEngineStatusSnapshot::new(engine.clone()))
        .await
        .expect("status snapshot reads");
    assert_eq!(status_after_spawn.len(), 1);
    assert_eq!(status_after_spawn[0].component().as_str(), "persona-router");
    assert_eq!(
        status_after_spawn[0].health(),
        signal_persona::ComponentHealth::Starting
    );

    let ready_draft = EngineEventDraft::from_input(EngineEventDraftInput {
        engine: engine.clone(),
        source: EngineEventSource::Manager,
        body: EngineEventBody::ComponentReady(ComponentLifecycleEvent::new(
            ComponentName::new("persona-router"),
        )),
    });
    store
        .ask(AppendEngineEvent::new(ready_draft))
        .await
        .expect("ready event appends");

    let lifecycle_after_ready = store
        .ask(ReadEngineLifecycleSnapshot::new(engine.clone()))
        .await
        .expect("lifecycle snapshot reads");
    assert_eq!(lifecycle_after_ready.len(), 1);
    assert_eq!(
        lifecycle_after_ready[0].process_state(),
        ComponentProcessState::Ready
    );

    let status_after_ready = store
        .ask(ReadEngineStatusSnapshot::new(engine.clone()))
        .await
        .expect("status snapshot reads");
    assert_eq!(
        status_after_ready[0].health(),
        signal_persona::ComponentHealth::Running
    );

    store.stop_gracefully().await.expect("manager store stops");
    store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_hydrates_component_health_from_snapshot() {
    let fixture = StoreFixture::new("persona-engine-manager-snapshot-hydrate");
    let engine = EngineId::new("engine-snapshot-hydrate");

    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    store
        .ask(AppendEngineEvent::new(StoreFixture::spawned_event(
            engine.clone(),
            "persona-terminal",
        )))
        .await
        .expect("spawn event appends");
    let ready_draft = EngineEventDraft::from_input(EngineEventDraftInput {
        engine: engine.clone(),
        source: EngineEventSource::Manager,
        body: EngineEventBody::ComponentReady(ComponentLifecycleEvent::new(
            ComponentName::new("persona-terminal"),
        )),
    });
    store
        .ask(AppendEngineEvent::new(ready_draft))
        .await
        .expect("ready event appends");

    // The first manager runs with the live event stream; its in-memory state
    // already reflects the appended events. A second manager starting against
    // the same store proves the snapshot tables — not the in-memory writer —
    // carry the `Running` health across the restart boundary.
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("manager starts from snapshot");

    let reply = manager
        .ask(persona::manager::HandleEngineRequest::new(
            EngineRequest::ComponentStatusQuery(ComponentStatusQuery {
                component: ComponentName::new("persona-terminal"),
            }),
        ))
        .await
        .expect("status query handled through manager");
    let EngineReply::ComponentStatus(status) = reply else {
        panic!("expected terminal component status, got {reply:?}");
    };
    assert_eq!(status.health, signal_persona::ComponentHealth::Running);

    EngineManager::stop(manager)
        .await
        .expect("manager stops after snapshot witness");
    store.stop_gracefully().await.expect("manager store stops");
    store.wait_for_shutdown().await;
}

/// Architectural-truth witness: the event log is authoritative, the
/// snapshot tables are projections. A `RebuildSnapshotsFromEventLog`
/// drops every snapshot row and replays the event log; the post-rebuild
/// snapshot contents equal the pre-rebuild contents because the event
/// log carries the truth.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_manager_store_rebuilds_snapshots_from_event_log_after_snapshot_truncation() {
    let fixture = StoreFixture::new("persona-manager-store-snapshot-rebuild");
    let engine = EngineId::new("engine-snapshot-rebuild");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");

    // Append a lifecycle arc that fully exercises both reducers.
    store
        .ask(AppendEngineEvent::new(StoreFixture::spawned_event(
            engine.clone(),
            "persona-router",
        )))
        .await
        .expect("router spawn event appends");

    let ready_router = EngineEventDraft::from_input(EngineEventDraftInput {
        engine: engine.clone(),
        source: EngineEventSource::Manager,
        body: EngineEventBody::ComponentReady(ComponentLifecycleEvent::new(
            ComponentName::new("persona-router"),
        )),
    });
    store
        .ask(AppendEngineEvent::new(ready_router))
        .await
        .expect("router ready event appends");

    store
        .ask(AppendEngineEvent::new(StoreFixture::spawned_event(
            engine.clone(),
            "persona-terminal",
        )))
        .await
        .expect("terminal spawn event appends");

    // Capture the snapshot the reducer produced as it absorbed each event.
    let lifecycle_before = sorted_lifecycle(
        store
            .ask(ReadEngineLifecycleSnapshot::new(engine.clone()))
            .await
            .expect("lifecycle snapshot reads before rebuild"),
    );
    let status_before = sorted_status(
        store
            .ask(ReadEngineStatusSnapshot::new(engine.clone()))
            .await
            .expect("status snapshot reads before rebuild"),
    );
    assert_eq!(lifecycle_before.len(), 2, "two components in lifecycle");
    assert_eq!(status_before.len(), 2, "two components in status");

    // Force a truncate-and-rebuild. If the reducer-on-append shape were the
    // only source of snapshot state, this would leave both tables empty.
    // The event log replay is what restores them.
    store
        .ask(RebuildSnapshotsFromEventLog)
        .await
        .expect("rebuild from event log succeeds");

    let lifecycle_after = sorted_lifecycle(
        store
            .ask(ReadEngineLifecycleSnapshot::new(engine.clone()))
            .await
            .expect("lifecycle snapshot reads after rebuild"),
    );
    let status_after = sorted_status(
        store
            .ask(ReadEngineStatusSnapshot::new(engine.clone()))
            .await
            .expect("status snapshot reads after rebuild"),
    );

    assert_eq!(
        lifecycle_after, lifecycle_before,
        "lifecycle snapshot after rebuild equals pre-rebuild content"
    );
    assert_eq!(
        status_after, status_before,
        "status snapshot after rebuild equals pre-rebuild content"
    );

    // The router component should be Ready/Running, terminal should be
    // Launched/Starting — proof the reducer absorbed the per-event arc.
    let router_lifecycle = lifecycle_after
        .iter()
        .find(|row| row.component().as_str() == "persona-router")
        .expect("router lifecycle row present");
    assert_eq!(
        router_lifecycle.process_state(),
        ComponentProcessState::Ready
    );
    let router_status = status_after
        .iter()
        .find(|row| row.component().as_str() == "persona-router")
        .expect("router status row present");
    assert_eq!(
        router_status.health(),
        signal_persona::ComponentHealth::Running
    );

    let terminal_lifecycle = lifecycle_after
        .iter()
        .find(|row| row.component().as_str() == "persona-terminal")
        .expect("terminal lifecycle row present");
    assert_eq!(
        terminal_lifecycle.process_state(),
        ComponentProcessState::Launched
    );
    let terminal_status = status_after
        .iter()
        .find(|row| row.component().as_str() == "persona-terminal")
        .expect("terminal status row present");
    assert_eq!(
        terminal_status.health(),
        signal_persona::ComponentHealth::Starting
    );

    store.stop_gracefully().await.expect("manager store stops");
    store.wait_for_shutdown().await;
}

fn sorted_lifecycle(
    mut rows: Vec<ComponentLifecycleSnapshotRow>,
) -> Vec<ComponentLifecycleSnapshotRow> {
    rows.sort_by(|a, b| a.component().as_str().cmp(b.component().as_str()));
    rows
}

fn sorted_status(mut rows: Vec<ComponentStatusSnapshotRow>) -> Vec<ComponentStatusSnapshotRow> {
    rows.sort_by(|a, b| a.component().as_str().cmp(b.component().as_str()));
    rows
}
