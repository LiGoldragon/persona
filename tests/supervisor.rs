use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use kameo::actor::Spawn;
use persona::engine::{EngineComponent, PersonaDaemonPaths};
use persona::engine_event::EngineEventBody;
use persona::launch::{ComponentCommandCatalog, EngineLaunchConfiguration};
use persona::manager_store::{ManagerStore, ManagerStoreLocation, ReadEngineEvents};
use persona::supervisor::{
    EngineSupervisor, EngineSupervisorInput, ReadEngineSupervisorSnapshot,
    StartPrototypeSupervision, StopPrototypeSupervision,
};
use signal_persona_auth::EngineId;

struct SupervisorFixture {
    root: PathBuf,
    engine: EngineId,
}

impl SupervisorFixture {
    fn new(name: &str) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "persona-supervisor-{name}-{}-{now}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("fixture root created");
        Self {
            root,
            engine: EngineId::new("supervisor-test"),
        }
    }

    fn state_root(&self) -> PathBuf {
        self.root.join("state")
    }

    fn run_root(&self) -> PathBuf {
        self.root.join("run")
    }

    fn manager_store(&self) -> PathBuf {
        self.root.join("manager.redb")
    }

    fn component_capture(&self, component: EngineComponent) -> PathBuf {
        self.state_root()
            .join(self.engine.as_str())
            .join(format!("{}.env", component.as_str()))
    }

    fn launcher_script(&self) -> PathBuf {
        let script = self.root.join("component-skeleton");
        let shell = std::env::var("PERSONA_TEST_SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let script_text = format!(
            "{}{}",
            format!("#!{shell}\n"),
            r#"set -eu
state_dir="$(dirname "$PERSONA_STATE_PATH")"
mkdir -p "$state_dir"
{
  printf 'engine=%s\n' "$PERSONA_ENGINE_ID"
  printf 'component=%s\n' "$PERSONA_COMPONENT"
  printf 'state=%s\n' "$PERSONA_STATE_PATH"
  printf 'socket=%s\n' "$PERSONA_SOCKET_PATH"
  printf 'mode=%s\n' "$PERSONA_SOCKET_MODE"
  printf 'peer_count=%s\n' "$PERSONA_PEER_SOCKET_COUNT"
} > "$state_dir/$PERSONA_COMPONENT.env"
trap 'exit 0' TERM
while true; do sleep 1; done
"#
        );
        std::fs::write(&script, script_text).expect("launcher script written");
        let mut permissions = std::fs::metadata(&script)
            .expect("launcher script metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("launcher script executable");
        script
    }

    fn command_catalog(&self) -> ComponentCommandCatalog {
        ComponentCommandCatalog::from_repeated_executable(
            self.launcher_script().to_string_lossy().into_owned(),
        )
    }

    fn layout(&self) -> persona::engine::EngineLayout {
        PersonaDaemonPaths::new(self.state_root(), self.run_root())
            .engine_layout(self.engine.clone())
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
        assert!(capture.contains(&format!("mode={:o}", component.socket_mode().as_octal())));
        assert!(capture.contains("peer_count=6"));
    }

    let events = store
        .ask(ReadEngineEvents::new(fixture.engine.clone()))
        .await
        .expect("engine events read");
    assert_eq!(
        events.len(),
        EngineComponent::prototype_supervised_components().len()
    );
    assert!(
        events
            .iter()
            .all(|event| matches!(event.body(), EngineEventBody::ComponentSpawned(_)))
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
        EngineComponent::prototype_supervised_components().len() * 2
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
    supervisor.wait_for_shutdown().await;
    store.stop_gracefully().await.expect("manager store stops");
    store.wait_for_shutdown().await;
}
