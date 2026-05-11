use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

struct DaemonFixture {
    root: PathBuf,
    socket: PathBuf,
    daemon: Child,
}

impl DaemonFixture {
    fn start() -> Self {
        let root = std::env::temp_dir().join(format!("persona-daemon-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("test root created");
        let socket = root.join("persona.sock");
        let mut daemon = Command::new(env!("CARGO_BIN_EXE_personad"))
            .arg(&socket)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("personad starts");

        let stdout = daemon.stdout.take().expect("daemon stdout is piped");
        let mut reader = BufReader::new(stdout);
        let mut readiness = String::new();
        reader
            .read_line(&mut readiness)
            .expect("daemon reports readiness");

        assert_eq!(
            readiness.trim(),
            format!("personad socket={}", socket.display())
        );

        Self {
            root,
            socket,
            daemon,
        }
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
fn constraint_persona_cli_talks_to_personad_over_socket() {
    let fixture = DaemonFixture::start();

    let shutdown = fixture.persona("(ComponentShutdown persona-terminal)");
    assert!(shutdown.contains("(SupervisorActionAcceptedReport persona-terminal Stopped)"));

    let status = fixture.persona("(ComponentStatusQuery persona-terminal)");
    assert!(status.contains("(ComponentStatusReport "));
    assert!(status.contains("(ComponentStatusRecord persona-terminal Terminal Stopped Stopped)"));
}

#[test]
fn constraint_personad_does_not_delete_non_socket_endpoint_path() {
    let root = std::env::temp_dir().join(format!(
        "persona-daemon-occupied-path-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("test root created");
    let endpoint = root.join("persona.sock");
    std::fs::write(&endpoint, "not a socket").expect("regular file created");

    let output = Command::new(env!("CARGO_BIN_EXE_personad"))
        .arg(&endpoint)
        .output()
        .expect("personad runs");

    assert!(
        !output.status.success(),
        "personad should reject occupied path"
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
