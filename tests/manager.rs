use owner_signal_version_handover::{
    AttemptHandover, ForceFlip, ForceReason, Operation as OwnerVersionOperation, Quarantine,
    QuarantineReason, RejectionReason, Reply as OwnerVersionReply, Rollback, RollbackReason,
    SocketPath, Version as OwnerVersion, VersionEndpoint, VersionLabel,
};
use persona::Error;
use persona::engine_event::EngineEventBody;
use persona::manager::{
    CompleteUpgrade, DriveVersionHandover, EngineManager, HandleEngineRequest,
    HandleOwnerVersionHandover, ManagerEvent, PrepareUpgrade, ReadTrace,
};
use persona::manager_store::{
    ManagerStore, ManagerStoreLocation, ReadActiveVersion, ReadEngineEvents,
};
use persona::unit::{ComponentUnit, UnitController, UnitFuture, UnitReceipt, UnitStatusReport};
use persona::upgrade::{
    ActiveVersionChangeSource, HandoverFrameCodec, Target, TargetInput, Version,
};
use signal_persona::engine::{Operation as EngineRequest, Reply as EngineReply};
use signal_persona::{
    ComponentDesiredState, ComponentHealth, ComponentName, ComponentShutdown, EngineStatusScope,
    Query, WirePath,
};
use signal_persona_auth::EngineId;
use signal_version_handover::{
    Date, HandoverAcceptance, HandoverFinalization, HandoverMarker, HandoverRejection,
    HandoverRejectionReason, Operation as HandoverOperation, RecoveryResult,
    Reply as HandoverReply, Time,
};
use std::sync::Arc;
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

#[derive(Debug, Clone, Default)]
struct RecordingUnitController {
    started: Arc<std::sync::Mutex<Vec<ComponentUnit>>>,
}

impl RecordingUnitController {
    fn started_units(&self) -> Vec<ComponentUnit> {
        self.started
            .lock()
            .expect("recording unit controller lock")
            .clone()
    }
}

impl UnitController for RecordingUnitController {
    fn start<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move {
            self.started
                .lock()
                .expect("recording unit controller lock")
                .push(unit.clone());
            Ok(UnitReceipt::started(unit))
        })
    }

    fn stop<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move { Ok(UnitReceipt::stopped(unit)) })
    }

    fn restart<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move { Ok(UnitReceipt::restarted(unit)) })
    }

    fn status<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitStatusReport> {
        Box::pin(async move {
            Ok(UnitStatusReport::new(
                unit,
                persona::unit::UnitStatus::Active,
            ))
        })
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

fn spirit_upgrade_target_with_upgrade_sockets(
    current_path: &std::path::Path,
    next_path: &std::path::Path,
) -> Target {
    Target::from_input(TargetInput {
        component: ComponentName::new("persona-spirit"),
        current_version: Version::new("v0.1.0"),
        next_version: Version::new("v0.1.1"),
        current_owner_socket_path: WirePath::new("/run/persona/default/spirit/v0.1.0/owner.sock"),
        current_upgrade_socket_path: WirePath::new(current_path.to_string_lossy().into_owned()),
        next_owner_socket_path: WirePath::new("/run/persona/default/spirit/v0.1.1/owner.sock"),
        next_upgrade_socket_path: WirePath::new(next_path.to_string_lossy().into_owned()),
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

fn owner_version_endpoint(
    label: &str,
    byte: u8,
    upgrade_socket_path: &std::path::Path,
) -> VersionEndpoint {
    let root = format!("/run/persona/default/spirit/{label}");
    VersionEndpoint {
        version: owner_version(label, byte),
        owner_socket_path: SocketPath::new(format!("{root}/owner.sock")),
        upgrade_socket_path: SocketPath::new(upgrade_socket_path.to_string_lossy().into_owned()),
    }
}

fn owner_attempt_handover_order_with_current_upgrade_socket(
    current_upgrade_socket_path: &std::path::Path,
) -> AttemptHandover {
    owner_attempt_handover_order_with_upgrade_sockets(
        current_upgrade_socket_path,
        std::path::Path::new("/run/persona/default/spirit/v0.1.1/upgrade.sock"),
    )
}

fn owner_attempt_handover_order_with_upgrade_sockets(
    current_upgrade_socket_path: &std::path::Path,
    next_upgrade_socket_path: &std::path::Path,
) -> AttemptHandover {
    AttemptHandover {
        component: HandoverComponentName::new("persona-spirit"),
        current: owner_version_endpoint("v0.1.0", 1, current_upgrade_socket_path),
        next: owner_version_endpoint("v0.1.1", 2, next_upgrade_socket_path),
    }
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

async fn serve_current_handover_socket(
    path: std::path::PathBuf,
    marker: HandoverMarker,
) -> persona::Result<Vec<HandoverOperation>> {
    let listener = tokio::net::UnixListener::bind(path)?;
    let codec = HandoverFrameCodec::default();
    let mut operations = Vec::new();
    for _ in 0..3 {
        let (mut stream, _) = listener.accept().await?;
        let frame = codec.read_frame(&mut stream).await?;
        let received = codec.request_from_frame(frame)?;
        let exchange = received.exchange();
        let operation = received.into_operation();
        let reply = match &operation {
            HandoverOperation::AskHandoverMarker(request) => {
                assert_eq!(request.component.as_str(), "persona-spirit");
                HandoverReply::HandoverMarker(marker.clone())
            }
            HandoverOperation::ReadyToHandover(report) => {
                assert_eq!(report.component.as_str(), "persona-spirit");
                assert_eq!(report.source_marker.commit_sequence, marker.commit_sequence);
                HandoverReply::HandoverAccepted(HandoverAcceptance {
                    accepted_marker: marker.clone(),
                })
            }
            HandoverOperation::HandoverCompleted(report) => {
                assert_eq!(report.component.as_str(), "persona-spirit");
                assert_eq!(
                    report.accepted_marker.commit_sequence,
                    marker.commit_sequence
                );
                HandoverReply::HandoverFinalized(HandoverFinalization {
                    finalized_marker: marker.clone(),
                })
            }
            other => panic!("unexpected handover operation in test server: {other:?}"),
        };
        let frame = codec.reply_frame(exchange, reply);
        codec.write_frame(&mut stream, &frame).await?;
        operations.push(operation);
    }
    Ok(operations)
}

async fn serve_current_handover_socket_with_completion_rejection(
    path: std::path::PathBuf,
    marker: HandoverMarker,
) -> persona::Result<Vec<HandoverOperation>> {
    let listener = tokio::net::UnixListener::bind(path)?;
    let codec = HandoverFrameCodec::default();
    let mut operations = Vec::new();
    for _ in 0..4 {
        let (mut stream, _) = listener.accept().await?;
        let frame = codec.read_frame(&mut stream).await?;
        let received = codec.request_from_frame(frame)?;
        let exchange = received.exchange();
        let operation = received.into_operation();
        let reply = match &operation {
            HandoverOperation::AskHandoverMarker(request) => {
                assert_eq!(request.component.as_str(), "persona-spirit");
                HandoverReply::HandoverMarker(marker.clone())
            }
            HandoverOperation::ReadyToHandover(report) => {
                assert_eq!(report.component.as_str(), "persona-spirit");
                assert_eq!(report.source_marker.commit_sequence, marker.commit_sequence);
                HandoverReply::HandoverAccepted(HandoverAcceptance {
                    accepted_marker: marker.clone(),
                })
            }
            HandoverOperation::HandoverCompleted(report) => {
                assert_eq!(report.component.as_str(), "persona-spirit");
                HandoverReply::HandoverRejected(HandoverRejection {
                    component: report.component.clone(),
                    reason: HandoverRejectionReason::CommitSequenceAdvanced,
                })
            }
            HandoverOperation::RecoverFromFailure(request) => {
                assert_eq!(request.component.as_str(), "persona-spirit");
                HandoverReply::RecoveryCompleted(RecoveryResult {
                    component: request.component.clone(),
                    recovered: true,
                })
            }
            other => panic!("unexpected handover operation in recovery test server: {other:?}"),
        };
        let frame = codec.reply_frame(exchange, reply);
        codec.write_frame(&mut stream, &frame).await?;
        operations.push(operation);
    }
    Ok(operations)
}

async fn serve_marker_handover_socket(
    path: std::path::PathBuf,
    marker: HandoverMarker,
) -> persona::Result<Vec<HandoverOperation>> {
    let listener = tokio::net::UnixListener::bind(path)?;
    let codec = HandoverFrameCodec::default();
    let (mut stream, _) = listener.accept().await?;
    let frame = codec.read_frame(&mut stream).await?;
    let received = codec.request_from_frame(frame)?;
    let exchange = received.exchange();
    let operation = received.into_operation();
    let reply = match &operation {
        HandoverOperation::AskHandoverMarker(request) => {
            assert_eq!(request.component.as_str(), "persona-spirit");
            HandoverReply::HandoverMarker(marker)
        }
        other => panic!("unexpected next handover operation in test server: {other:?}"),
    };
    let frame = codec.reply_frame(exchange, reply);
    codec.write_frame(&mut stream, &frame).await?;
    Ok(vec![operation])
}

async fn wait_for_socket(path: &std::path::Path) {
    for _attempt in 0..80 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("socket did not appear: {}", path.display());
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
async fn constraint_persona_engine_drives_version_handover_over_component_upgrade_socket() {
    let fixture = StoreFixture::new("persona-manager-upgrade-socket-drive");
    let current_socket = fixture.root.join("spirit-current-upgrade.sock");
    let next_socket = fixture.root.join("spirit-next-upgrade.sock");
    let marker = handover_marker(118);
    let server = tokio::spawn(serve_current_handover_socket(
        current_socket.clone(),
        marker.clone(),
    ));
    let next_server = tokio::spawn(serve_marker_handover_socket(
        next_socket.clone(),
        marker.clone(),
    ));
    wait_for_socket(&current_socket).await;
    wait_for_socket(&next_socket).await;
    let engine = EngineId::new("engine-upgrade-socket-drive");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("manager starts with store");
    let target = spirit_upgrade_target_with_upgrade_sockets(&current_socket, &next_socket);

    let driven = manager
        .ask(DriveVersionHandover::new(target))
        .await
        .expect("manager drives version handover over socket");

    assert_eq!(driven.marker().commit_sequence, 118);
    assert_eq!(driven.acceptance().accepted_marker.commit_sequence, 118);
    assert_eq!(driven.finalization().finalized_marker.commit_sequence, 118);

    let operations = server
        .await
        .expect("handover socket server joins")
        .expect("handover socket server succeeds");
    assert!(matches!(
        operations.as_slice(),
        [
            HandoverOperation::AskHandoverMarker(_),
            HandoverOperation::ReadyToHandover(_),
            HandoverOperation::HandoverCompleted(_)
        ]
    ));
    let next_operations = next_server
        .await
        .expect("next handover marker socket server joins")
        .expect("next handover marker socket server succeeds");
    assert!(matches!(
        next_operations.as_slice(),
        [HandoverOperation::AskHandoverMarker(_)]
    ));

    let active = store
        .ask(ReadActiveVersion::new(
            engine,
            ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version snapshot read")
        .expect("active version persisted");
    assert_eq!(active.active_version().as_str(), "v0.1.1");
    assert_eq!(active.commit_sequence(), Some(118));

    let trace = manager
        .ask(ReadTrace::expecting_at_least(4))
        .await
        .expect("trace read through actor");
    assert!(trace.contains(&ManagerEvent::UpgradePrepared));
    assert!(trace.contains(&ManagerEvent::ActiveVersionChanged));
    assert!(trace.contains(&ManagerEvent::VersionHandoverDriven));

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_starts_next_component_unit_before_handover_socket_probe() {
    let fixture = StoreFixture::new("persona-manager-start-next-unit-before-handover");
    let engine = EngineId::new("engine-start-next-unit-before-handover");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let unit_controller = RecordingUnitController::default();
    let manager = EngineManager::start_with_store_and_unit_controller(
        engine.clone(),
        store.clone(),
        Arc::new(unit_controller.clone()),
    )
    .await
    .expect("manager starts with store");
    let current_socket = fixture.root.join("missing-current-upgrade.sock");
    let next_socket = fixture.root.join("missing-next-upgrade.sock");
    let target = spirit_upgrade_target_with_upgrade_sockets(&current_socket, &next_socket);

    let error = manager
        .ask(DriveVersionHandover::new(target))
        .await
        .expect_err("missing handover socket fails after unit start");

    assert!(matches!(
        error,
        kameo::error::SendError::HandlerError(Error::Io(_))
    ));

    let started_units = unit_controller.started_units();
    assert_eq!(started_units.len(), 1);
    let unit = &started_units[0];
    assert_eq!(
        unit.engine().as_str(),
        "engine-start-next-unit-before-handover"
    );
    assert_eq!(unit.component().as_str(), "persona-spirit");
    assert_eq!(unit.version().as_str(), "v0.1.1");
    assert_eq!(
        unit.name().as_str(),
        "persona-component@persona-spirit:v0.1.1.service"
    );

    let trace = manager
        .ask(ReadTrace::expecting_at_least(3))
        .await
        .expect("trace read through actor");
    assert!(trace.contains(&ManagerEvent::UpgradePrepared));
    assert!(trace.contains(&ManagerEvent::ComponentUnitStarted));
    assert!(!trace.contains(&ManagerEvent::ActiveVersionChanged));

    let active = store
        .ask(ReadActiveVersion::new(
            engine.clone(),
            ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version snapshot read");
    assert!(active.is_none());

    let events = store
        .ask(ReadEngineEvents::new(engine))
        .await
        .expect("engine events read");
    assert!(matches!(
        events.as_slice(),
        [event] if matches!(
            event.body(),
            EngineEventBody::UpgradePrepared(prepared)
                if prepared.component().as_str() == "persona-spirit"
        )
    ));

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_engine_refuses_handover_when_next_marker_is_stale() {
    let fixture = StoreFixture::new("persona-manager-next-marker-stale");
    let current_socket = fixture.root.join("spirit-current-upgrade.sock");
    let next_socket = fixture.root.join("spirit-next-upgrade.sock");
    let current_marker = handover_marker(118);
    let next_marker = handover_marker(117);
    let current_server = tokio::spawn(serve_marker_handover_socket(
        current_socket.clone(),
        current_marker,
    ));
    let next_server = tokio::spawn(serve_marker_handover_socket(
        next_socket.clone(),
        next_marker,
    ));
    wait_for_socket(&current_socket).await;
    wait_for_socket(&next_socket).await;
    let engine = EngineId::new("engine-upgrade-next-marker-stale");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("manager starts with store");
    let target = spirit_upgrade_target_with_upgrade_sockets(&current_socket, &next_socket);

    let error = manager
        .ask(DriveVersionHandover::new(target))
        .await
        .expect_err("stale next marker rejects handover before current enters readiness");

    assert!(matches!(
        error,
        kameo::error::SendError::HandlerError(Error::NextHandoverMarkerMismatch {
            field: "commit_sequence",
            expected,
            actual,
        }) if expected == "118" && actual == "117"
    ));

    let current_operations = current_server
        .await
        .expect("current handover marker socket server joins")
        .expect("current handover marker socket server succeeds");
    assert!(matches!(
        current_operations.as_slice(),
        [HandoverOperation::AskHandoverMarker(_)]
    ));
    let next_operations = next_server
        .await
        .expect("next handover marker socket server joins")
        .expect("next handover marker socket server succeeds");
    assert!(matches!(
        next_operations.as_slice(),
        [HandoverOperation::AskHandoverMarker(_)]
    ));

    let active = store
        .ask(ReadActiveVersion::new(
            engine.clone(),
            ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version snapshot read");
    assert!(active.is_none());

    let events = store
        .ask(ReadEngineEvents::new(engine))
        .await
        .expect("engine events read");
    assert!(matches!(
        events.as_slice(),
        [event] if matches!(
            event.body(),
            EngineEventBody::UpgradePrepared(prepared)
                if prepared.component().as_str() == "persona-spirit"
        )
    ));

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_engine_recovers_current_handover_when_completion_fails() {
    let fixture = StoreFixture::new("persona-manager-handover-completion-recovery");
    let current_socket = fixture.root.join("spirit-current-upgrade.sock");
    let next_socket = fixture.root.join("spirit-next-upgrade.sock");
    let marker = handover_marker(149);
    let current_server = tokio::spawn(serve_current_handover_socket_with_completion_rejection(
        current_socket.clone(),
        marker.clone(),
    ));
    let next_server = tokio::spawn(serve_marker_handover_socket(
        next_socket.clone(),
        marker.clone(),
    ));
    wait_for_socket(&current_socket).await;
    wait_for_socket(&next_socket).await;
    let engine = EngineId::new("engine-handover-completion-recovery");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("manager starts with store");
    let target = spirit_upgrade_target_with_upgrade_sockets(&current_socket, &next_socket);

    let error = manager
        .ask(DriveVersionHandover::new(target))
        .await
        .expect_err("completion rejection keeps active selector unchanged");

    assert!(matches!(
        error,
        kameo::error::SendError::HandlerError(Error::UnexpectedSignalFrame { got })
            if got.contains("HandoverRejected")
    ));

    let current_operations = current_server
        .await
        .expect("current handover recovery server joins")
        .expect("current handover recovery server succeeds");
    assert!(matches!(
        current_operations.as_slice(),
        [
            HandoverOperation::AskHandoverMarker(_),
            HandoverOperation::ReadyToHandover(_),
            HandoverOperation::HandoverCompleted(_),
            HandoverOperation::RecoverFromFailure(_),
        ]
    ));
    let next_operations = next_server
        .await
        .expect("next handover marker socket server joins")
        .expect("next handover marker socket server succeeds");
    assert!(matches!(
        next_operations.as_slice(),
        [HandoverOperation::AskHandoverMarker(_)]
    ));

    let active = store
        .ask(ReadActiveVersion::new(
            engine.clone(),
            ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version snapshot read");
    assert!(active.is_none());

    let events = store
        .ask(ReadEngineEvents::new(engine))
        .await
        .expect("engine events read");
    assert!(matches!(
        events.as_slice(),
        [event] if matches!(
            event.body(),
            EngineEventBody::UpgradePrepared(prepared)
                if prepared.component().as_str() == "persona-spirit"
        )
    ));

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_owner_attempt_handover_drives_component_upgrade_socket() {
    let fixture = StoreFixture::new("persona-manager-owner-attempt-handover");
    let current_socket = fixture.root.join("spirit-current-upgrade.sock");
    let next_socket = fixture.root.join("spirit-next-upgrade.sock");
    let marker = handover_marker(144);
    let server = tokio::spawn(serve_current_handover_socket(
        current_socket.clone(),
        marker.clone(),
    ));
    let next_server = tokio::spawn(serve_marker_handover_socket(
        next_socket.clone(),
        marker.clone(),
    ));
    wait_for_socket(&current_socket).await;
    wait_for_socket(&next_socket).await;
    let engine = EngineId::new("engine-owner-attempt-handover");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine.clone(), store.clone())
        .await
        .expect("manager starts with store");

    let reply = manager
        .ask(HandleOwnerVersionHandover::new(
            OwnerVersionOperation::AttemptHandover(
                owner_attempt_handover_order_with_upgrade_sockets(&current_socket, &next_socket),
            ),
        ))
        .await
        .expect("owner attempt handover succeeds");

    match reply {
        OwnerVersionReply::HandoverSucceeded(success) => {
            assert_eq!(success.component.as_str(), "persona-spirit");
            assert_eq!(success.active_version.label.as_str(), "v0.1.1");
            assert_eq!(success.commit_sequence, 144);
        }
        other => panic!("expected owner handover success, got {other:?}"),
    }

    let operations = server
        .await
        .expect("handover socket server joins")
        .expect("handover socket server succeeds");
    assert!(matches!(
        operations.as_slice(),
        [
            HandoverOperation::AskHandoverMarker(_),
            HandoverOperation::ReadyToHandover(_),
            HandoverOperation::HandoverCompleted(_)
        ]
    ));
    let next_operations = next_server
        .await
        .expect("next handover marker socket server joins")
        .expect("next handover marker socket server succeeds");
    assert!(matches!(
        next_operations.as_slice(),
        [HandoverOperation::AskHandoverMarker(_)]
    ));

    let active = store
        .ask(ReadActiveVersion::new(
            engine,
            ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version snapshot read")
        .expect("active version persisted");
    assert_eq!(active.active_version().as_str(), "v0.1.1");
    assert_eq!(active.commit_sequence(), Some(144));

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_manager_refuses_handover_with_quarantined_version() {
    let fixture = StoreFixture::new("persona-manager-quarantine-gates-handover");
    let engine = EngineId::new("engine-quarantine-gates-handover");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine, store.clone())
        .await
        .expect("manager starts with store");

    manager
        .ask(HandleOwnerVersionHandover::new(
            OwnerVersionOperation::Quarantine(owner_quarantine_order()),
        ))
        .await
        .expect("owner quarantine succeeds");

    let error = manager
        .ask(DriveVersionHandover::new(spirit_upgrade_target()))
        .await
        .expect_err("quarantined target version rejects handover before socket IO");

    assert!(matches!(
        error,
        kameo::error::SendError::HandlerError(Error::ComponentVersionQuarantined {
            component,
            version,
            reason: QuarantineReason::SuspectState,
        }) if component == "persona-spirit" && version == "v0.1.1"
    ));

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_owner_attempt_handover_reports_quarantined_version() {
    let fixture = StoreFixture::new("persona-manager-owner-attempt-quarantined");
    let engine = EngineId::new("engine-owner-attempt-quarantined");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let manager = EngineManager::start_with_store(engine, store.clone())
        .await
        .expect("manager starts with store");

    manager
        .ask(HandleOwnerVersionHandover::new(
            OwnerVersionOperation::Quarantine(owner_quarantine_order()),
        ))
        .await
        .expect("owner quarantine succeeds");

    let missing_socket = fixture.root.join("missing-upgrade.sock");
    let reply = manager
        .ask(HandleOwnerVersionHandover::new(
            OwnerVersionOperation::AttemptHandover(
                owner_attempt_handover_order_with_current_upgrade_socket(&missing_socket),
            ),
        ))
        .await
        .expect("owner attempt returns typed reply");

    match reply {
        OwnerVersionReply::Rejected(rejected) => {
            assert_eq!(rejected.component.as_str(), "persona-spirit");
            assert_eq!(rejected.reason, RejectionReason::VersionQuarantined);
        }
        other => panic!("expected owner rejection reply, got {other:?}"),
    }

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
