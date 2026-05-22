use owner_signal_version_handover::{
    ForceFlip, ForceReason, Operation as OwnerVersionOperation, Quarantine, QuarantineReason,
    Reply as OwnerVersionReply, Rollback, RollbackReason, Version as OwnerVersion, VersionLabel,
};
use persona::engine_event::EngineEventBody;
use persona::manager::{
    CompleteUpgrade, EngineManager, HandleEngineRequest, HandleOwnerVersionHandover, ManagerEvent,
    PrepareUpgrade, ReadTrace,
};
use persona::manager_store::{
    ManagerStore, ManagerStoreLocation, ReadActiveVersion, ReadEngineEvents,
};
use persona::upgrade::{ActiveVersionChangeSource, Target, TargetInput, Version};
use signal_persona::engine::{Operation as EngineRequest, Reply as EngineReply};
use signal_persona::{
    ComponentDesiredState, ComponentHealth, ComponentName, ComponentShutdown, EngineStatusScope,
    Query, WirePath,
};
use signal_persona_auth::EngineId;
use signal_version_handover::{Date, HandoverMarker, Operation as HandoverOperation, Time};
use version_projection::{ComponentName as HandoverComponentName, ContractVersion};

struct StoreFixture {
    root: std::path::PathBuf,
    location: ManagerStoreLocation,
}

impl StoreFixture {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()));
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

fn spirit_upgrade_target() -> Target {
    Target::from_input(TargetInput {
        component: ComponentName::new("persona-spirit"),
        current_version: Version::new("v0.1.0"),
        next_version: Version::new("v0.1.1"),
        current_owner_socket_path: WirePath::new("/run/persona/default/spirit/v0.1.0/owner.sock"),
        current_upgrade_socket_path: WirePath::new(
            "/run/persona/default/spirit/v0.1.0/upgrade.sock",
        ),
        next_owner_socket_path: WirePath::new("/run/persona/default/spirit/v0.1.1/owner.sock"),
        next_upgrade_socket_path: WirePath::new("/run/persona/default/spirit/v0.1.1/upgrade.sock"),
    })
}

fn handover_marker(commit_sequence: u64) -> HandoverMarker {
    HandoverMarker {
        component: HandoverComponentName::new("persona-spirit"),
        schema_hash: ContractVersion::new([9; 32]),
        commit_sequence,
        write_counter: 3,
        last_record_identifier: Some(210),
        recorded_at_date: Date::new(2026, 5, 22),
        recorded_at_time: Time::new(16, 30, 0),
    }
}

fn owner_version(label: &str, byte: u8) -> OwnerVersion {
    OwnerVersion::new(VersionLabel::new(label), ContractVersion::new([byte; 32]))
}

fn owner_force_flip_order() -> ForceFlip {
    ForceFlip {
        component: HandoverComponentName::new("persona-spirit"),
        current_version: owner_version("v0.1.0", 1),
        target_version: owner_version("v0.1.1", 2),
        reason: ForceReason::OperatorOverride,
    }
}

fn owner_rollback_order() -> Rollback {
    Rollback {
        component: HandoverComponentName::new("persona-spirit"),
        active_version: owner_version("v0.1.1", 2),
        restore_version: owner_version("v0.1.0", 1),
        reason: RollbackReason::PostCutoverFailure,
    }
}

fn owner_quarantine_order() -> Quarantine {
    Quarantine {
        component: HandoverComponentName::new("persona-spirit"),
        version: owner_version("v0.1.1", 2),
        reason: QuarantineReason::SuspectState,
    }
}

#[tokio::test]
async fn constraint_engine_request_reply_is_created_by_kameo_manager_path() {
    let manager = EngineManager::start().await;

    let reply = manager
        .ask(HandleEngineRequest::new(EngineRequest::Query(
            Query::EngineStatus(EngineStatusScope::WholeEngine),
        )))
        .await
        .expect("request handled by actor");

    assert!(matches!(reply, EngineReply::EngineStatus(_)));

    let trace = manager
        .ask(ReadTrace::expecting_at_least(3))
        .await
        .expect("trace read through actor");
    assert_eq!(
        trace,
        vec![
            ManagerEvent::Started,
            ManagerEvent::EngineRequestAccepted,
            ManagerEvent::EngineReplyCreated,
            ManagerEvent::TraceRead,
        ]
    );

    EngineManager::stop(manager)
        .await
        .expect("actor stops cleanly");
}

#[tokio::test]
async fn constraint_engine_manager_keeps_component_state_between_messages() {
    let manager = EngineManager::start().await;

    let shutdown = ComponentShutdown {
        component: ComponentName::new("persona-terminal"),
    };
    let acceptance = manager
        .ask(HandleEngineRequest::new(EngineRequest::Stop(shutdown)))
        .await
        .expect("shutdown handled by actor");

    assert!(matches!(acceptance, EngineReply::ActionAccepted(_)));

    let status = manager
        .ask(HandleEngineRequest::new(EngineRequest::Query(
            Query::ComponentStatus(ComponentName::new("persona-terminal")),
        )))
        .await
        .expect("status handled by actor");

    match status {
        EngineReply::ComponentStatus(component) => {
            assert_eq!(component.desired_state, ComponentDesiredState::Stopped);
            assert_eq!(component.health, ComponentHealth::Stopped);
        }
        other => panic!("expected terminal component status, got {other:?}"),
    }

    EngineManager::stop(manager)
        .await
        .expect("actor stops cleanly");
}

#[test]
fn constraint_engine_manager_is_not_a_zst_actor() {
    assert!(std::mem::size_of::<EngineManager>() > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_prepares_upgrade_with_version_handover_request() {
    let fixture = StoreFixture::new("persona-manager-upgrade-prepare");
    let engine = EngineId::new("engine-upgrade-prepare");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine, store.clone())
        .await
        .expect("manager starts with store");
    let target = spirit_upgrade_target();

    let prepared = manager
        .ask(PrepareUpgrade::new(target))
        .await
        .expect("prepare upgrade succeeds");

    let HandoverOperation::AskHandoverMarker(marker_request) = prepared.first_handover_operation()
    else {
        panic!(
            "expected AskHandoverMarker, got {:?}",
            prepared.first_handover_operation()
        );
    };
    assert_eq!(marker_request.component.as_str(), "persona-spirit");
    assert_eq!(
        prepared.target().current_owner_socket_path().as_str(),
        "/run/persona/default/spirit/v0.1.0/owner.sock"
    );

    let trace = manager
        .ask(ReadTrace::expecting_at_least(2))
        .await
        .expect("trace read through actor");
    assert!(trace.contains(&ManagerEvent::UpgradePrepared));

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_records_active_version_after_handover_completion() {
    let fixture = StoreFixture::new("persona-manager-upgrade-complete");
    let engine = EngineId::new("engine-upgrade-complete");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("manager starts with store");
    let target = spirit_upgrade_target();

    let change = manager
        .ask(CompleteUpgrade::new(target, handover_marker(77)))
        .await
        .expect("complete upgrade succeeds");
    assert_eq!(change.active_version().as_str(), "v0.1.1");
    assert_eq!(change.commit_sequence(), Some(77));

    let active = store
        .ask(ReadActiveVersion::new(
            engine,
            ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version snapshot read")
        .expect("active version persisted");
    assert_eq!(active.active_version().as_str(), "v0.1.1");
    assert_eq!(active.schema_hash(), ContractVersion::new([9; 32]));
    assert_eq!(active.commit_sequence(), Some(77));

    let trace = manager
        .ask(ReadTrace::expecting_at_least(2))
        .await
        .expect("trace read through actor");
    assert!(trace.contains(&ManagerEvent::ActiveVersionChanged));

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_applies_owner_force_flip_to_active_selector() {
    let fixture = StoreFixture::new("persona-manager-owner-force-flip");
    let engine = EngineId::new("engine-owner-force-flip");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("manager starts with store");

    let reply = manager
        .ask(HandleOwnerVersionHandover::new(
            OwnerVersionOperation::ForceFlip(owner_force_flip_order()),
        ))
        .await
        .expect("owner force flip succeeds");
    let OwnerVersionReply::FlipForced(accepted) = reply else {
        panic!("expected FlipForced reply, got {reply:?}");
    };
    assert_eq!(accepted.component.as_str(), "persona-spirit");
    assert_eq!(accepted.active_version.label.as_str(), "v0.1.1");

    let active = store
        .ask(ReadActiveVersion::new(
            engine,
            ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version snapshot read")
        .expect("active version persisted");
    assert_eq!(active.active_version().as_str(), "v0.1.1");
    assert_eq!(active.schema_hash(), ContractVersion::new([2; 32]));
    assert_eq!(active.commit_sequence(), None);
    assert_eq!(
        active.source(),
        &ActiveVersionChangeSource::ForceFlip {
            reason: ForceReason::OperatorOverride
        }
    );

    let trace = manager
        .ask(ReadTrace::expecting_at_least(2))
        .await
        .expect("trace read through actor");
    assert!(trace.contains(&ManagerEvent::VersionAuthorityApplied));

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_applies_owner_rollback_to_active_selector() {
    let fixture = StoreFixture::new("persona-manager-owner-rollback");
    let engine = EngineId::new("engine-owner-rollback");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("manager starts with store");

    let reply = manager
        .ask(HandleOwnerVersionHandover::new(
            OwnerVersionOperation::Rollback(owner_rollback_order()),
        ))
        .await
        .expect("owner rollback succeeds");
    let OwnerVersionReply::RolledBack(accepted) = reply else {
        panic!("expected RolledBack reply, got {reply:?}");
    };
    assert_eq!(accepted.component.as_str(), "persona-spirit");
    assert_eq!(accepted.active_version.label.as_str(), "v0.1.0");

    let active = store
        .ask(ReadActiveVersion::new(
            engine,
            ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version snapshot read")
        .expect("active version persisted");
    assert_eq!(active.active_version().as_str(), "v0.1.0");
    assert_eq!(active.schema_hash(), ContractVersion::new([1; 32]));
    assert_eq!(
        active.source(),
        &ActiveVersionChangeSource::Rollback {
            reason: RollbackReason::PostCutoverFailure
        }
    );

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_records_owner_quarantine_event() {
    let fixture = StoreFixture::new("persona-manager-owner-quarantine");
    let engine = EngineId::new("engine-owner-quarantine");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("manager starts with store");

    let reply = manager
        .ask(HandleOwnerVersionHandover::new(
            OwnerVersionOperation::Quarantine(owner_quarantine_order()),
        ))
        .await
        .expect("owner quarantine succeeds");
    let OwnerVersionReply::Quarantined(accepted) = reply else {
        panic!("expected Quarantined reply, got {reply:?}");
    };
    assert_eq!(accepted.component.as_str(), "persona-spirit");
    assert_eq!(accepted.version.label.as_str(), "v0.1.1");

    let events = store
        .ask(ReadEngineEvents::new(engine))
        .await
        .expect("engine events read");
    assert!(matches!(
        events.last().map(|event| event.body()),
        Some(EngineEventBody::VersionQuarantined(event))
            if event.component().as_str() == "persona-spirit"
                && event.version().as_str() == "v0.1.1"
                && event.schema_hash() == ContractVersion::new([2; 32])
                && event.reason() == QuarantineReason::SuspectState
    ));

    let trace = manager
        .ask(ReadTrace::expecting_at_least(2))
        .await
        .expect("trace read through actor");
    assert!(trace.contains(&ManagerEvent::VersionQuarantined));

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}
