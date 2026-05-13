use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use persona::engine::EngineComponent;
use persona::engine_event::EngineEventBody;

struct DaemonFixture {
    root: PathBuf,
    socket: PathBuf,
    manager_store: PathBuf,
    daemon: Child,
}

impl DaemonFixture {
    fn start() -> Self {
        let root = Self::unique_root();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("test root created");
        let socket = root.join("persona.sock");
        let manager_store = root.join("manager.redb");
        let mut daemon = Command::new(env!("CARGO_BIN_EXE_persona-daemon"))
            .arg(&socket)
            .env("PERSONA_MANAGER_STORE", &manager_store)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("persona-daemon starts");

        let stdout = daemon.stdout.take().expect("daemon stdout is piped");
        let mut reader = BufReader::new(stdout);
        let mut readiness = String::new();
        reader
            .read_line(&mut readiness)
            .expect("daemon reports readiness");

        assert_eq!(
            readiness.trim(),
            format!("persona-daemon socket={}", socket.display())
        );

        Self {
            root,
            socket,
            manager_store,
            daemon,
        }
    }

    fn start_with_prototype_supervised_components() -> Self {
        let root = Self::unique_root();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("test root created");
        let socket = root.join("persona.sock");
        let manager_store = root.join("manager.redb");
        let script = Self::component_skeleton_script(&root);
        let mut daemon = Command::new(env!("CARGO_BIN_EXE_persona-daemon"))
            .arg(&socket)
            .env("PERSONA_MANAGER_STORE", &manager_store)
            .env("PERSONA_STATE_ROOT", root.join("state"))
            .env("PERSONA_RUN_ROOT", root.join("run"))
            .env("PERSONA_PROTOTYPE_STACK_EXECUTABLE", script)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("persona-daemon starts");

        let stdout = daemon.stdout.take().expect("daemon stdout is piped");
        let mut reader = BufReader::new(stdout);
        let mut readiness = String::new();
        reader
            .read_line(&mut readiness)
            .expect("daemon reports readiness");

        assert_eq!(
            readiness.trim(),
            format!("persona-daemon socket={}", socket.display())
        );

        Self {
            root,
            socket,
            manager_store,
            daemon,
        }
    }

    fn unique_root() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "persona-daemon-test-{}-{unique}",
            std::process::id()
        ))
    }

    fn component_skeleton_script(root: &std::path::Path) -> PathBuf {
        let script = root.join("component-skeleton");
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
  printf 'process=%s\n' "$$"
  printf 'socket=%s\n' "$PERSONA_SOCKET_PATH"
  printf 'mode=%s\n' "$PERSONA_SOCKET_MODE"
  printf 'peer_count=%s\n' "$PERSONA_PEER_SOCKET_COUNT"
} > "$state_dir/$PERSONA_COMPONENT.env"
trap 'exit 0' TERM
while true; do sleep 1; done
"#
        );
        std::fs::write(&script, script_text).expect("component skeleton written");
        let mut permissions = std::fs::metadata(&script)
            .expect("component skeleton metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("component skeleton executable");
        script
    }

    fn stop_daemon(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        self.stop_component_process_groups();
    }

    fn persona(&self, request: &str) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_persona"))
            .arg(request)
            .env("PERSONA_SOCKET", &self.socket)
            .output()
            .expect("persona client runs");

        assert!(
            output.status.success(),
            "persona failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8(output.stdout).expect("persona output is utf8")
    }

    fn component_capture(&self, component: EngineComponent) -> PathBuf {
        self.root
            .join("state")
            .join("default")
            .join(format!("{}.env", component.as_str()))
    }

    fn wait_for_component_capture(&self, component: EngineComponent) -> String {
        let path = self.component_capture(component);
        for _attempt in 0..80 {
            if let Ok(text) = std::fs::read_to_string(&path)
                && text.contains("peer_count=")
            {
                return text;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("component capture did not appear: {}", path.display());
    }

    fn stop_component_process_groups(&self) {
        for component in EngineComponent::prototype_supervised_components() {
            let Ok(text) = std::fs::read_to_string(self.component_capture(component)) else {
                continue;
            };
            let Some(process) = text.lines().find_map(|line| {
                line.strip_prefix("process=")
                    .and_then(|value| value.parse::<i32>().ok())
            }) else {
                continue;
            };
            unsafe {
                libc::killpg(process, libc::SIGTERM);
                libc::killpg(process, libc::SIGKILL);
            }
        }
    }
}

impl Drop for DaemonFixture {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        self.stop_component_process_groups();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn constraint_persona_cli_talks_to_persona_daemon_over_socket() {
    let fixture = DaemonFixture::start();

    let shutdown = fixture.persona("(ComponentShutdown persona-terminal)");
    assert!(shutdown.contains("(SupervisorActionAcceptedReport persona-terminal Stopped)"));

    let status = fixture.persona("(ComponentStatusQuery persona-terminal)");
    assert!(status.contains("(ComponentStatusReport "));
    assert!(status.contains("(ComponentStatus persona-terminal Terminal Stopped Stopped)"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_persists_cli_mutation_to_manager_store() {
    let mut fixture = DaemonFixture::start();

    let shutdown = fixture.persona("(ComponentShutdown persona-terminal)");
    assert!(shutdown.contains("(SupervisorActionAcceptedReport persona-terminal Stopped)"));

    fixture.stop_daemon();

    let store = persona::manager_store::ManagerStore::start(
        persona::manager_store::ManagerStoreLocation::new(&fixture.manager_store),
    )
    .expect("manager store starts for inspection");
    let record = store
        .ask(persona::manager_store::ReadEngineRecord::new(
            signal_persona_auth::EngineId::new("default"),
        ))
        .await
        .expect("stored record read through manager store actor")
        .expect("default engine record exists");
    let terminal = record
        .status()
        .components
        .iter()
        .find(|component| component.name.as_str() == "persona-terminal")
        .expect("terminal component stored");
    assert_eq!(
        terminal.desired_state,
        signal_persona::ComponentDesiredState::Stopped
    );

    store.stop_gracefully().await.expect("manager store stops");
    store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_launches_prototype_supervised_components_through_engine_supervisor()
 {
    let mut fixture = DaemonFixture::start_with_prototype_supervised_components();

    for component in EngineComponent::prototype_supervised_components() {
        let capture = fixture.wait_for_component_capture(component);
        assert!(
            capture.contains("engine=default"),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains(&format!("component={}", component.as_str())),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains(&format!("mode={:o}", component.socket_mode().as_octal())),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains("peer_count=6"),
            "capture for {component:?}: {capture}"
        );
    }

    fixture.stop_daemon();

    let store = persona::manager_store::ManagerStore::start(
        persona::manager_store::ManagerStoreLocation::new(&fixture.manager_store),
    )
    .expect("manager store starts for inspection");
    let events = store
        .ask(persona::manager_store::ReadEngineEvents::new(
            signal_persona_auth::EngineId::new("default"),
        ))
        .await
        .expect("default engine events read through manager store actor");
    assert_eq!(
        events.len(),
        EngineComponent::prototype_supervised_components().len()
    );
    assert!(
        events
            .iter()
            .all(|event| matches!(event.body(), EngineEventBody::ComponentSpawned(_)))
    );

    store.stop_gracefully().await.expect("manager store stops");
    store.wait_for_shutdown().await;
}

#[test]
fn constraint_persona_daemon_does_not_delete_non_socket_endpoint_path() {
    let root = std::env::temp_dir().join(format!(
        "persona-daemon-occupied-path-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("test root created");
    let endpoint = root.join("persona.sock");
    std::fs::write(&endpoint, "not a socket").expect("regular file created");

    let output = Command::new(env!("CARGO_BIN_EXE_persona-daemon"))
        .arg(&endpoint)
        .output()
        .expect("persona-daemon runs");

    assert!(
        !output.status.success(),
        "persona-daemon should reject occupied path"
    );
    assert_eq!(
        std::fs::read_to_string(&endpoint).expect("regular file preserved"),
        "not a socket"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("non-socket file"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(&root);
}
