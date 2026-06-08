use std::sync::Arc;

use meta_signal_persona::{
    ComponentDesiredState, ComponentHealth, ComponentName, ComponentShutdown, EngineStatusScope,
    Query,
};
use meta_signal_persona::{Operation as EngineRequest, Reply as EngineReply};
use persona::manager::{
    EngineManager, HandleEngineRequest, ManagerEvent, ReadTrace, StartComponentUnit,
};
use persona::manager_store::{ManagerStore, ManagerStoreLocation};
use persona::unit::{ComponentUnit, UnitController, UnitFuture, UnitReceipt, UnitStatusReport};
use persona::upgrade::Version;
use signal_persona::origin::EngineIdentifier;

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
            location: ManagerStoreLocation::new(root.join("manager.sema")),
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_keeps_versioned_unit_start_authority_only() {
    let fixture = StoreFixture::new("persona-manager-start-component-unit");
    let engine = EngineIdentifier::new("engine-start-component-unit");
    let store = ManagerStore::start(fixture.location()).expect("manager store starts");
    let unit_controller = RecordingUnitController::default();
    let manager = EngineManager::start_with_store_and_unit_controller(
        engine.clone(),
        store.clone(),
        Arc::new(unit_controller.clone()),
    )
    .await
    .expect("manager starts with store");

    let receipt = manager
        .ask(StartComponentUnit::new(
            ComponentName::new("persona-spirit"),
            Version::new("v0.1.1"),
        ))
        .await
        .expect("unit start handled");

    assert_eq!(
        receipt.unit().engine().as_str(),
        "engine-start-component-unit"
    );
    assert_eq!(receipt.unit().component().as_str(), "persona-spirit");
    assert_eq!(receipt.unit().version().as_str(), "v0.1.1");
    assert_eq!(
        receipt.unit().name().as_str(),
        "persona-component@persona-spirit:v0.1.1.service"
    );

    let started_units = unit_controller.started_units();
    assert_eq!(started_units.len(), 1);
    assert_eq!(started_units[0], receipt.unit().clone());

    let trace = manager
        .ask(ReadTrace::expecting_at_least(2))
        .await
        .expect("trace read through actor");
    assert!(trace.contains(&ManagerEvent::ComponentUnitStarted));

    EngineManager::stop(manager)
        .await
        .expect("manager stops cleanly");
    ManagerStore::close_and_stop(store)
        .await
        .expect("manager store closes");
}
