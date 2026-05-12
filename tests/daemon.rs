use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

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

    fn stop_daemon(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
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
}

impl Drop for DaemonFixture {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
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
