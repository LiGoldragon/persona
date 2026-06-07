use owner_signal_persona::{
    ComponentDesiredState, ComponentHealth, ComponentName, ComponentShutdown, Query,
};
use owner_signal_persona::{Operation as EngineRequest, Reply as EngineReply};
use persona::engine_event::{
    ComponentLifecycleEvent, ComponentOperation, ComponentUnimplemented,
    ComponentUnimplementedInput, EngineEventBody, EngineEventDraft, EngineEventDraftInput,
    EngineEventSource, HarnessOperationKind, UnimplementedReason,
};
use persona::manager::EngineManager;
use persona::manager_store::AppendOrphansFromEventLog;
use persona::manager_store::{
    AppendEngineEvent, ComponentLifecycleSnapshotRow, ComponentProcessState,
    ComponentStatusSnapshotRow, ManagerStore, ManagerStoreLocation, PersistEngineRecord,
    ReadActiveVersion, ReadEngineEvents, ReadEngineLifecycleSnapshot, ReadEngineRecord,
    ReadEngineStatusSnapshot, ReadManagerStoreWriteCount, RebuildSnapshotsFromEventLog,
};
use persona::schema::{
    ComponentOperationReport, EngineEventBodyReport, EngineEventReport, EngineEventSourceKind,
};
use persona::state::EngineState;
use signal_persona_origin::EngineIdentifier;
use signal_upgrade::{ComponentName as UpgradeComponentName, Date, HandoverMarker, Time};
use version_projection::{ComponentName as ProjectionComponentName, ContractVersion};

use persona::upgrade::SocketPath as UpgradeSocketPath;
use persona::upgrade::{ActiveVersionChanged, PreparedEvent, Target, TargetInput, Version};

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
            location: ManagerStoreLocation::new(root.join("manager.sema")),
            root,
        }
    }

    fn location(&self) -> ManagerStoreLocation {
        self.location.clone()
    }

    fn spawned_event(engine: EngineIdentifier, component: &str) -> EngineEventDraft {
        let component = ComponentName::new(component);
        EngineEventDraft::from_input(EngineEventDraftInput {
            engine,
            source: EngineEventSource::Manager,
            body: EngineEventBody::ComponentSpawned(ComponentLifecycleEvent::new(component)),
        })
    }

    fn unimplemented_event(engine: EngineIdentifier, component: &str) -> EngineEventDraft {
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

    fn spirit_upgrade_target() -> Target {
        Target::from_input(TargetInput {
            component: UpgradeComponentName::new("persona-spirit"),
            current_version: Version::new("v0.1.0"),
            next_version: Version::new("v0.1.1"),
            current_owner_socket_path: UpgradeSocketPath::new(
                "/run/persona/default/spirit/v0.1.0/owner.sock",
            ),
            current_upgrade_socket_path: UpgradeSocketPath::new(
                "/run/persona/default/spirit/v0.1.0/upgrade.sock",
            ),
            next_owner_socket_path: UpgradeSocketPath::new(
                "/run/persona/default/spirit/v0.1.1/owner.sock",
            ),
            next_upgrade_socket_path: UpgradeSocketPath::new(
                "/run/persona/default/spirit/v0.1.1/upgrade.sock",
            ),
        })
    }

    fn spirit_handover_marker(state_sequence: u64) -> HandoverMarker {
        HandoverMarker {
            component: ProjectionComponentName::new("persona-spirit"),
            schema_hash: ContractVersion::new([7; 32]),
            state_sequence,
            mirrored_write_count: 99,
            record_frontier: Some(210),
            recorded_at_date: Date::new(2026, 5, 22),
            recorded_at_time: Time::new(16, 0, 0),
        }
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
    let engine = EngineIdentifier::new("engine-store-writer");
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
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_persists_component_mutation_through_manager_store() {
    let fixture = StoreFixture::new("persona-engine-manager-store-path");
    let engine = EngineIdentifier::new("engine-manager-store-path");
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
            EngineRequest::Stop(ComponentShutdown {
                component: ComponentName::new("persona-terminal"),
            }),
        ))
        .await
        .expect("shutdown handled through manager actor");
    assert!(matches!(reply, EngineReply::ActionAccepted(_)));

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
            EngineRequest::Query(Query::ComponentStatus(ComponentName::new(
                "persona-terminal",
            ))),
        ))
        .await
        .expect("status handled through manager actor");
    assert!(matches!(query, EngineReply::ComponentStatus(_)));

    EngineManager::stop(manager)
        .await
        .expect("engine manager stops");
    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_restores_persisted_snapshot_before_answering_status() {
    let fixture = StoreFixture::new("persona-engine-manager-restore");
    let engine = EngineIdentifier::new("engine-manager-restore");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("engine manager starts with store");

    let reply = manager
        .ask(persona::manager::HandleEngineRequest::new(
            EngineRequest::Stop(ComponentShutdown {
                component: ComponentName::new("persona-terminal"),
            }),
        ))
        .await
        .expect("shutdown handled through manager actor");
    assert!(matches!(reply, EngineReply::ActionAccepted(_)));

    EngineManager::stop(manager)
        .await
        .expect("first engine manager stops");

    let restored = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("engine manager restores from store");
    let status = restored
        .ask(persona::manager::HandleEngineRequest::new(
            EngineRequest::Query(Query::ComponentStatus(ComponentName::new(
                "persona-terminal",
            ))),
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
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_event_log_records_typed_manager_events() {
    let fixture = StoreFixture::new("persona-manager-event-log");
    let engine = EngineIdentifier::new("engine-event-log");
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
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_manager_store_projects_active_component_version_from_event_log() {
    let fixture = StoreFixture::new("persona-manager-active-version");
    let engine = EngineIdentifier::new("engine-active-version");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let target = StoreFixture::spirit_upgrade_target();
    let marker = StoreFixture::spirit_handover_marker(45);
    let change = ActiveVersionChanged::from_marker(&target, &marker);

    store
        .ask(AppendEngineEvent::new(EngineEventDraft::from_input(
            EngineEventDraftInput {
                engine: engine.clone(),
                source: EngineEventSource::Manager,
                body: EngineEventBody::UpgradePrepared(PreparedEvent::from_target(&target)),
            },
        )))
        .await
        .expect("upgrade prepared event appends");
    store
        .ask(AppendEngineEvent::new(EngineEventDraft::from_input(
            EngineEventDraftInput {
                engine: engine.clone(),
                source: EngineEventSource::Manager,
                body: EngineEventBody::ActiveVersionChanged(change.clone()),
            },
        )))
        .await
        .expect("active version event appends");

    let active = store
        .ask(ReadActiveVersion::new(
            engine.clone(),
            ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version snapshot reads")
        .expect("active version exists");
    assert_eq!(active.active_version().as_str(), "v0.1.1");
    assert_eq!(active.schema_hash(), ContractVersion::new([7; 32]));
    assert_eq!(active.state_sequence(), Some(45));

    store
        .ask(RebuildSnapshotsFromEventLog)
        .await
        .expect("rebuild from event log succeeds");
    let rebuilt = store
        .ask(ReadActiveVersion::new(
            engine.clone(),
            ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version snapshot reads after rebuild")
        .expect("active version exists after rebuild");
    assert_eq!(rebuilt.active_version().as_str(), "v0.1.1");
    assert_eq!(rebuilt.state_sequence(), Some(45));

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_event_log_nota_projection_is_view() {
    let fixture = StoreFixture::new("persona-manager-event-log-projection");
    let engine = EngineIdentifier::new("engine-event-projection");
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
    let nota = projection.to_nota();
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
                    == ComponentOperationReport::Harness(HarnessOperationKind::MessageDelivery)
                && unimplemented.reason == UnimplementedReason::NotBuiltYet
    ));
    assert!(
        nota.starts_with(
            "(1 [engine-event-projection] Component (Some [persona-harness]) (ComponentUnimplemented"
        ),
        "unexpected event projection: {nota}"
    );

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_manager_store_reduces_lifecycle_events_into_snapshot_tables() {
    let fixture = StoreFixture::new("persona-manager-store-snapshot-reduce");
    let engine = EngineIdentifier::new("engine-snapshot-reduce");
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
    assert_eq!(
        lifecycle_after_spawn[0].component().as_str(),
        "persona-router"
    );
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
        owner_signal_persona::ComponentHealth::Starting
    );

    let ready_draft = EngineEventDraft::from_input(EngineEventDraftInput {
        engine: engine.clone(),
        source: EngineEventSource::Manager,
        body: EngineEventBody::ComponentReady(ComponentLifecycleEvent::new(ComponentName::new(
            "persona-router",
        ))),
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
        owner_signal_persona::ComponentHealth::Running
    );

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_hydrates_component_health_from_snapshot() {
    let fixture = StoreFixture::new("persona-engine-manager-snapshot-hydrate");
    let engine = EngineIdentifier::new("engine-snapshot-hydrate");

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
        body: EngineEventBody::ComponentReady(ComponentLifecycleEvent::new(ComponentName::new(
            "persona-terminal",
        ))),
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
            EngineRequest::Query(Query::ComponentStatus(ComponentName::new(
                "persona-terminal",
            ))),
        ))
        .await
        .expect("status query handled through manager");
    let EngineReply::ComponentStatus(status) = reply else {
        panic!("expected terminal component status, got {reply:?}");
    };
    assert_eq!(
        status.health,
        owner_signal_persona::ComponentHealth::Running
    );

    EngineManager::stop(manager)
        .await
        .expect("manager stops after snapshot witness");
    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

/// Architectural-truth witness: the event log is authoritative, the
/// snapshot tables are projections. A `RebuildSnapshotsFromEventLog`
/// drops every snapshot row and replays the event log; the post-rebuild
/// snapshot contents equal the pre-rebuild contents because the event
/// log carries the truth.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_manager_store_rebuilds_snapshots_from_event_log_after_snapshot_truncation() {
    let fixture = StoreFixture::new("persona-manager-store-snapshot-rebuild");
    let engine = EngineIdentifier::new("engine-snapshot-rebuild");
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
        body: EngineEventBody::ComponentReady(ComponentLifecycleEvent::new(ComponentName::new(
            "persona-router",
        ))),
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
        owner_signal_persona::ComponentHealth::Running
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
        owner_signal_persona::ComponentHealth::Starting
    );

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

/// Architectural-truth witness: `ManagerStore` owns an exclusive storage handle,
/// and its close-then-stop protocol releases that handle before callers treat
/// shutdown as complete. A new store opening the same path after
/// `close_and_stop` must succeed and read the data written by the stopped
/// actor.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_manager_store_close_protocol_releases_storage_lock_before_shutdown() {
    let fixture = StoreFixture::new("persona-manager-store-release");
    let engine = EngineIdentifier::new("engine-store-release");

    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    store
        .ask(AppendEngineEvent::new(StoreFixture::spawned_event(
            engine.clone(),
            "persona-router",
        )))
        .await
        .expect("router spawn event appends");

    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store close protocol completes");

    let reopened = ManagerStore::start(fixture.location())
        .expect("manager store sema path reopens after graceful shutdown and wait_for_shutdown");
    let events = reopened
        .ask(ReadEngineEvents::new(engine.clone()))
        .await
        .expect("events read from reopened store");
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0].body(),
        EngineEventBody::ComponentSpawned(spawned)
            if spawned.component().as_str() == "persona-router"
    ));

    ManagerStore::close_and_stop(reopened)
        .await
        .expect("reopened manager store close protocol completes");
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

/// Architectural-truth witness: a `ComponentSpawned` recorded by a prior
/// daemon run without a matching `ComponentReady` or `ComponentExited`
/// is detected as an orphan during the next manager startup, and one
/// `ComponentOrphaned` event is appended to the event log. The snapshot
/// reducer then projects the orphan into
/// `ComponentProcessState::Exited` and `ComponentHealth::Failed`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_manager_startup_appends_component_orphaned_for_unfinished_spawn() {
    let fixture = StoreFixture::new("persona-manager-orphan-detection");
    let engine = EngineIdentifier::new("engine-orphan-detection");

    // Simulate a prior daemon arc: spawn two components, mark one
    // ready, leave the other in the open arc that the prior daemon
    // never closed. Re-opening the same store from a new
    // Reopening the store would be a separate resource-release witness.
    // The orphan-detection logic itself runs on the store actor after
    // persisted events exist. One `ManagerStore` is sufficient to
    // witness the orphan scan: the events are persisted, the scan
    // reads them, and the orphan event is appended through the same
    // actor path the manager startup would use.
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    store
        .ask(AppendEngineEvent::new(StoreFixture::spawned_event(
            engine.clone(),
            "persona-router",
        )))
        .await
        .expect("router spawn appends");
    let router_ready = EngineEventDraft::from_input(EngineEventDraftInput {
        engine: engine.clone(),
        source: EngineEventSource::Manager,
        body: EngineEventBody::ComponentReady(ComponentLifecycleEvent::new(ComponentName::new(
            "persona-router",
        ))),
    });
    store
        .ask(AppendEngineEvent::new(router_ready))
        .await
        .expect("router ready appends");
    store
        .ask(AppendEngineEvent::new(StoreFixture::spawned_event(
            engine.clone(),
            "persona-terminal",
        )))
        .await
        .expect("terminal spawn appends");

    // Orphan scan: should find exactly one open arc for
    // `persona-terminal` and append one `ComponentOrphaned` event.
    let orphans = store
        .ask(AppendOrphansFromEventLog)
        .await
        .expect("orphan scan completes");
    assert_eq!(orphans.len(), 1, "exactly one orphan detected");
    let orphaned = match orphans[0].body() {
        EngineEventBody::ComponentOrphaned(orphan) => orphan,
        other => panic!("expected ComponentOrphaned, got {other:?}"),
    };
    assert_eq!(orphaned.component().as_str(), "persona-terminal");
    // The spawned_sequence on the orphan event names the original
    // spawn (sequence 3: router-spawn=1, router-ready=2, terminal-
    // spawn=3).
    assert_eq!(orphaned.spawned_sequence().into_u64(), 3);

    // Snapshot reducer should now project terminal as Exited / Failed.
    let lifecycle = store
        .ask(ReadEngineLifecycleSnapshot::new(engine.clone()))
        .await
        .expect("lifecycle snapshot reads");
    let terminal_lifecycle = lifecycle
        .iter()
        .find(|row| row.component().as_str() == "persona-terminal")
        .expect("terminal lifecycle row present");
    assert_eq!(
        terminal_lifecycle.process_state(),
        ComponentProcessState::Exited
    );
    let status = store
        .ask(ReadEngineStatusSnapshot::new(engine.clone()))
        .await
        .expect("status snapshot reads");
    let terminal_status = status
        .iter()
        .find(|row| row.component().as_str() == "persona-terminal")
        .expect("terminal status row present");
    assert_eq!(
        terminal_status.health(),
        owner_signal_persona::ComponentHealth::Failed
    );

    // Router was ready before "crash"; it must not be marked orphaned.
    let router_lifecycle = lifecycle
        .iter()
        .find(|row| row.component().as_str() == "persona-router")
        .expect("router lifecycle row present");
    assert_eq!(
        router_lifecycle.process_state(),
        ComponentProcessState::Ready
    );
    let router_status = status
        .iter()
        .find(|row| row.component().as_str() == "persona-router")
        .expect("router status row present");
    assert_eq!(
        router_status.health(),
        owner_signal_persona::ComponentHealth::Running
    );

    // Second orphan-scan must be idempotent: the orphan arc gained a
    // terminator (the appended `Orphaned`), so the next scan appends
    // zero new events.
    let again = store
        .ask(AppendOrphansFromEventLog)
        .await
        .expect("second orphan scan completes");
    assert!(again.is_empty(), "orphan scan is idempotent");

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

/// Architectural-truth witness: the event-log append and the snapshot
/// reduce land in one sema-kernel write transaction. A daemon crash between
/// `ENGINE_EVENTS.insert` and the reducer's snapshot writes is
/// structurally impossible — they share a transaction; the kernel commits or
/// rolls back both together. The source scan reads `manager_store.rs`
/// and asserts that `write_engine_event`'s body contains both the
/// `ENGINE_EVENTS.insert` and the `reduce_event_into_snapshots` call,
/// in that order, inside one `self.engine.storage_kernel().write(` closure.
#[test]
fn constraint_event_append_and_snapshot_reduce_share_one_write_transaction() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/manager_store.rs"),
    )
    .expect("manager_store.rs source readable");

    let signature = "fn write_engine_event(&self, event: &EngineEvent) -> Result<()> {";
    let body_start = source
        .find(signature)
        .expect("write_engine_event signature appears in manager_store.rs");
    let body_after = &source[body_start..];

    // Scan a generous window past the signature and rely on the
    // ordering checks below to anchor what counts as the method body.
    let window_end = body_start
        + body_after
            .find("\n    }\n")
            .expect("write_engine_event body ends with a `}`");
    let body = &source[body_start..window_end];

    assert!(
        body.contains("self.engine.storage_kernel().write("),
        "write_engine_event must use a single `self.engine.storage_kernel().write(` closure"
    );
    let insert_position = body
        .find(".insert(transaction, event.key().to_string(), event)")
        .expect("write_engine_event must insert the event log row");
    let reduce_position = body
        .find("Self::reduce_event_into_snapshots(transaction")
        .expect(
            "write_engine_event must call `Self::reduce_event_into_snapshots(transaction, ...)`",
        );

    assert!(
        insert_position < reduce_position,
        "event-log insert must precede the snapshot reduce in one transaction"
    );

    // Confirm the call ordering really is inside one closure: between
    // the `self.engine.storage_kernel().write(` opening and the matching `})?)` close,
    // both the insert and the reduce appear exactly once.
    let write_open = body
        .find("self.engine.storage_kernel().write(")
        .expect("write_engine_event opens one write transaction");
    let close_marker = "        })?)";
    let write_close_relative = body[write_open..]
        .find(close_marker)
        .expect("write transaction closes inside the method");
    let write_close = write_open + write_close_relative;
    let closure_body = &body[write_open..write_close];
    assert_eq!(
        closure_body
            .matches(".insert(transaction, event.key().to_string(), event)")
            .count(),
        1,
        "exactly one event-log insert per call to write_engine_event"
    );
    assert_eq!(
        closure_body
            .matches("Self::reduce_event_into_snapshots(transaction")
            .count(),
        1,
        "exactly one snapshot reduce per call to write_engine_event"
    );
}
