use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use kameo::actor::Spawn;
use persona::engine::{EngineComponent, EngineTopology, PersonaDaemonPaths};
use persona::engine_event::EngineEventBody;
use persona::generated_contract::PayloadString;
use persona::launch::{ComponentCommandCatalog, EngineLaunchConfiguration};
use persona::manager_store::{ManagerStore, ManagerStoreLocation, ReadEngineEvents};
use persona::supervisor::{
    EngineSupervisor, EngineSupervisorInput, ReadEngineSupervisorSnapshot,
    StartPrototypeSupervision, StopPrototypeSupervision,
};
use signal_persona::EngineIdentifier;

mod support;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct SupervisorFixture {
    root: PathBuf,
    engine: EngineIdentifier,
}

impl SupervisorFixture {
    fn new(_name: &str) -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from("/tmp").join(format!("ps{}-{sequence}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("fixture root created");
        Self {
            root,
            engine: EngineIdentifier::new("supervisor-test"),
        }
    }

    fn state_root(&self) -> PathBuf {
        self.root.join("state")
    }

    fn run_root(&self) -> PathBuf {
        self.root.join("run")
    }

    fn manager_store(&self) -> PathBuf {
        self.root.join("manager.sema")
    }

    fn component_capture(&self, component: EngineComponent) -> PathBuf {
        self.component_capture_for_engine(&self.engine, component)
    }

    fn component_capture_for_engine(
        &self,
        engine: &EngineIdentifier,
        component: EngineComponent,
    ) -> PathBuf {
        self.state_root()
            .join(engine.as_str())
            .join(format!("{}.env", component.as_str()))
    }

    fn component_instance_capture(&self, instance_name: &str) -> PathBuf {
        self.state_root()
            .join(self.engine.as_str())
            .join(format!("{instance_name}.env"))
    }

    fn command_catalog(&self) -> ComponentCommandCatalog {
        ComponentCommandCatalog::from_repeated_executable(
            support::component_socket_fixture(self.root.as_path())
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn command_catalog_for_topology(&self, topology: EngineTopology) -> ComponentCommandCatalog {
        ComponentCommandCatalog::from_repeated_executable_for_components(
            support::component_socket_fixture(self.root.as_path())
                .to_string_lossy()
                .into_owned(),
            topology.components().iter().copied(),
        )
    }

    fn layout(&self) -> persona::engine::EngineLayout {
        PersonaDaemonPaths::new(self.state_root(), self.run_root())
            .engine_layout(self.engine.clone())
    }

    fn layout_for_topology(&self, topology: EngineTopology) -> persona::engine::EngineLayout {
        PersonaDaemonPaths::new(self.state_root(), self.run_root())
            .engine_layout_with_topology(self.engine.clone(), topology)
    }

    fn layout_for_engine(&self, engine: EngineIdentifier) -> persona::engine::EngineLayout {
        PersonaDaemonPaths::new(self.state_root(), self.run_root()).engine_layout(engine)
    }

    fn capture_value<'a>(capture: &'a str, field: &str) -> &'a str {
        let prefix = format!("{field}=");
        capture
            .lines()
            .find_map(|line| line.strip_prefix(prefix.as_str()))
            .unwrap_or_else(|| panic!("capture field {field} missing: {capture}"))
    }

    async fn wait_for_capture(path: &Path) -> String {
        for _attempt in 0..80 {
            if let Ok(text) = std::fs::read_to_string(path)
                && text.contains("peer_count=")
            {
                return text;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("component capture did not appear at {}", path.display());
    }
}

impl Drop for SupervisorFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_supervisor_launches_three_harness_chain_instances() {
    let fixture = SupervisorFixture::new("three-harness-chain-supervision");
    let topology = EngineTopology::ThreeHarnessChain;
    let store = ManagerStore::start(ManagerStoreLocation::new(fixture.manager_store()))
        .expect("manager store starts");
    let supervisor = EngineSupervisor::spawn(EngineSupervisor::new(EngineSupervisorInput {
        layout: fixture.layout_for_topology(topology),
        command_catalog: fixture.command_catalog_for_topology(topology),
        launch_configuration: EngineLaunchConfiguration::empty(),
        store: Some(store.clone()),
    }));

    let report = supervisor
        .ask(StartPrototypeSupervision)
        .await
        .expect("three-harness-chain supervision starts");
    assert_eq!(report.components().len(), 8);

    for (instance_name, component) in [
        ("message", EngineComponent::Message),
        ("router", EngineComponent::Router),
        ("initiator-terminal", EngineComponent::Terminal),
        ("initiator", EngineComponent::Harness),
        ("responder-terminal", EngineComponent::Terminal),
        ("responder", EngineComponent::Harness),
        ("reviewer-terminal", EngineComponent::Terminal),
        ("reviewer", EngineComponent::Harness),
    ] {
        let capture =
            SupervisorFixture::wait_for_capture(&fixture.component_instance_capture(instance_name))
                .await;
        assert!(capture.contains("engine=supervisor-test"));
        assert!(capture.contains(&format!("component={}", component.as_str())));
        assert!(capture.contains(&format!("component_instance={instance_name}")));
        assert!(capture.contains("peer_count=7"));
    }

    let stopped = supervisor
        .ask(StopPrototypeSupervision)
        .await
        .expect("three-harness-chain supervision stops");
    assert_eq!(stopped.components().len(), 8);

    supervisor
        .stop_gracefully()
        .await
        .expect("supervisor stops");
    let _shutdown_completion = supervisor.wait_for_shutdown().await;
    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_supervisor_launches_message_router_topology_without_full_stack() {
    let fixture = SupervisorFixture::new("message-router-supervision");
    let topology = EngineTopology::MessageRouter;
    let store = ManagerStore::start(ManagerStoreLocation::new(fixture.manager_store()))
        .expect("manager store starts");
    let supervisor = EngineSupervisor::spawn(EngineSupervisor::new(EngineSupervisorInput {
        layout: fixture.layout_for_topology(topology),
        command_catalog: fixture.command_catalog_for_topology(topology),
        launch_configuration: EngineLaunchConfiguration::empty(),
        store: Some(store.clone()),
    }));

    let report = supervisor
        .ask(StartPrototypeSupervision)
        .await
        .expect("message-router supervision starts");
    assert_eq!(report.components().len(), topology.components().len());

    for component in topology.components().iter().copied() {
        let capture =
            SupervisorFixture::wait_for_capture(&fixture.component_capture(component)).await;
        assert!(capture.contains("engine=supervisor-test"));
        assert!(capture.contains(&format!("component={}", component.as_str())));
        assert!(capture.contains("peer_count=1"));
    }
    assert!(
        !fixture.component_capture(EngineComponent::Mind).exists(),
        "message-router topology must not launch mind"
    );

    let events = store
        .ask(ReadEngineEvents::new(fixture.engine.clone()))
        .await
        .expect("engine events read");
    assert_eq!(events.len(), topology.components().len() * 2);

    let stopped = supervisor
        .ask(StopPrototypeSupervision)
        .await
        .expect("message-router supervision stops");
    assert_eq!(stopped.components().len(), topology.components().len());

    supervisor
        .stop_gracefully()
        .await
        .expect("supervisor stops");
    let _shutdown_completion = supervisor.wait_for_shutdown().await;
    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_supervisor_launches_prototype_supervised_components_through_process_launcher()
 {
    let fixture = SupervisorFixture::new("prototype-supervision");
    let store = ManagerStore::start(ManagerStoreLocation::new(fixture.manager_store()))
        .expect("manager store starts");
    let supervisor = EngineSupervisor::spawn(EngineSupervisor::new(EngineSupervisorInput {
        layout: fixture.layout(),
        command_catalog: fixture.command_catalog(),
        launch_configuration: EngineLaunchConfiguration::empty(),
        store: Some(store.clone()),
    }));

    let report = supervisor
        .ask(StartPrototypeSupervision)
        .await
        .expect("prototype supervision starts");
    assert_eq!(
        report.components().len(),
        EngineComponent::prototype_supervised_components().len()
    );

    let snapshot = supervisor
        .ask(ReadEngineSupervisorSnapshot)
        .await
        .expect("supervisor snapshot succeeds");
    assert_eq!(
        snapshot.running().len(),
        EngineComponent::prototype_supervised_components().len()
    );
    assert_eq!(snapshot.started_supervision_count(), 1);

    for component in EngineComponent::prototype_supervised_components() {
        let capture =
            SupervisorFixture::wait_for_capture(&fixture.component_capture(component)).await;
        assert!(capture.contains("engine=supervisor-test"));
        assert!(capture.contains(&format!("component={}", component.as_str())));
        assert!(capture.contains(&format!(
            "spawn_envelope={}",
            fixture
                .run_root()
                .join(fixture.engine.as_str())
                .join(component.envelope_file())
                .display()
        )));
        assert!(capture.contains("manager_socket="));
        assert!(capture.contains("persona.sock"));
        assert!(capture.contains("domain_socket="));
        assert!(capture.contains("supervision_socket="));
        assert!(capture.contains(&format!(
            "domain_mode={:o}",
            component.socket_mode().as_octal()
        )));
        assert!(capture.contains(&format!(
            "supervision_mode={:o}",
            component.supervision_socket_mode().as_octal()
        )));
        assert!(capture.contains("peer_count=7"));
    }

    let events = store
        .ask(ReadEngineEvents::new(fixture.engine.clone()))
        .await
        .expect("engine events read");
    assert_eq!(
        events.len(),
        EngineComponent::prototype_supervised_components().len() * 2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.body(), EngineEventBody::ComponentSpawned(_)))
            .count(),
        EngineComponent::prototype_supervised_components().len()
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.body(), EngineEventBody::ComponentReady(_)))
            .count(),
        EngineComponent::prototype_supervised_components().len()
    );

    let stopped = supervisor
        .ask(StopPrototypeSupervision)
        .await
        .expect("prototype supervision stops");
    assert_eq!(
        stopped.components().len(),
        EngineComponent::prototype_supervised_components().len()
    );

    let events = store
        .ask(ReadEngineEvents::new(fixture.engine.clone()))
        .await
        .expect("engine events read");
    assert_eq!(
        events.len(),
        EngineComponent::prototype_supervised_components().len() * 3
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.body(), EngineEventBody::ComponentStopped(_)))
            .count(),
        EngineComponent::prototype_supervised_components().len()
    );

    supervisor
        .stop_gracefully()
        .await
        .expect("supervisor stops");
    let _shutdown_completion = supervisor.wait_for_shutdown().await;
    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_engine_supervisor_scopes_spirit_per_engine() {
    let fixture = SupervisorFixture::new("spirit-per-engine-supervision");
    let first_engine = EngineIdentifier::new("spirit-engine-one");
    let second_engine = EngineIdentifier::new("spirit-engine-two");
    let store = ManagerStore::start(ManagerStoreLocation::new(fixture.manager_store()))
        .expect("manager store starts");
    let first_supervisor = EngineSupervisor::spawn(EngineSupervisor::new(EngineSupervisorInput {
        layout: fixture.layout_for_engine(first_engine.clone()),
        command_catalog: fixture.command_catalog(),
        launch_configuration: EngineLaunchConfiguration::empty(),
        store: Some(store.clone()),
    }));
    let second_supervisor = EngineSupervisor::spawn(EngineSupervisor::new(EngineSupervisorInput {
        layout: fixture.layout_for_engine(second_engine.clone()),
        command_catalog: fixture.command_catalog(),
        launch_configuration: EngineLaunchConfiguration::empty(),
        store: Some(store.clone()),
    }));

    let first_report = first_supervisor
        .ask(StartPrototypeSupervision)
        .await
        .expect("first engine supervision starts");
    let second_report = second_supervisor
        .ask(StartPrototypeSupervision)
        .await
        .expect("second engine supervision starts");
    assert!(
        first_report
            .components()
            .iter()
            .any(|component| component.component() == EngineComponent::Spirit)
    );
    assert!(
        second_report
            .components()
            .iter()
            .any(|component| component.component() == EngineComponent::Spirit)
    );

    let first_capture = SupervisorFixture::wait_for_capture(
        &fixture.component_capture_for_engine(&first_engine, EngineComponent::Spirit),
    )
    .await;
    let second_capture = SupervisorFixture::wait_for_capture(
        &fixture.component_capture_for_engine(&second_engine, EngineComponent::Spirit),
    )
    .await;

    assert!(first_capture.contains("component=spirit"));
    assert!(second_capture.contains("component=spirit"));
    assert!(first_capture.contains("state_path="));
    assert!(second_capture.contains("state_path="));
    assert!(first_capture.contains("state/spirit-engine-one/spirit.sema"));
    assert!(second_capture.contains("state/spirit-engine-two/spirit.sema"));
    assert!(first_capture.contains("domain_socket="));
    assert!(second_capture.contains("domain_socket="));
    assert!(first_capture.contains("run/spirit-engine-one/spirit.sock"));
    assert!(second_capture.contains("run/spirit-engine-two/spirit.sock"));
    assert_ne!(
        SupervisorFixture::capture_value(&first_capture, "process"),
        SupervisorFixture::capture_value(&second_capture, "process")
    );
    assert_ne!(
        SupervisorFixture::capture_value(&first_capture, "state_path"),
        SupervisorFixture::capture_value(&second_capture, "state_path")
    );

    first_supervisor
        .ask(StopPrototypeSupervision)
        .await
        .expect("first engine supervision stops");
    second_supervisor
        .ask(StopPrototypeSupervision)
        .await
        .expect("second engine supervision stops");
    first_supervisor
        .stop_gracefully()
        .await
        .expect("first supervisor stops");
    second_supervisor
        .stop_gracefully()
        .await
        .expect("second supervisor stops");
    let _shutdown_completion = first_supervisor.wait_for_shutdown().await;
    let _shutdown_completion = second_supervisor.wait_for_shutdown().await;
    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}
