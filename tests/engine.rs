use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use persona::engine::{EngineComponent, PersonaDaemonPaths, SocketMode};
use signal_persona_auth::EngineId;

struct TemporaryEngineRoot {
    root: PathBuf,
}

impl TemporaryEngineRoot {
    fn new(name: &str) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "persona-engine-{name}-{}-{now}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        Self { root }
    }

    fn state_root(&self) -> PathBuf {
        self.root.join("state")
    }

    fn run_root(&self) -> PathBuf {
        self.root.join("run")
    }

    fn contains(path: &Path, expected: &str) -> bool {
        path.to_string_lossy().contains(expected)
    }
}

impl Drop for TemporaryEngineRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn constraint_engine_layout_uses_engine_id_scoped_paths() {
    let root = TemporaryEngineRoot::new("layout");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout(EngineId::new("engine-alpha"));

    assert!(TemporaryEngineRoot::contains(
        layout.state_dir(),
        "state/engine-alpha"
    ));
    assert!(TemporaryEngineRoot::contains(
        layout.run_dir(),
        "run/engine-alpha"
    ));
    assert!(layout.manager_store().ends_with("manager.redb"));
    assert!(layout.manager_socket().ends_with("persona.sock"));

    let router = layout
        .component(EngineComponent::Router)
        .expect("router component layout exists");
    assert!(router.state_path().ends_with("router.redb"));
    assert!(router.socket().path().ends_with("router.sock"));
    assert!(TemporaryEngineRoot::contains(
        router.state_path(),
        "state/engine-alpha"
    ));
    assert!(TemporaryEngineRoot::contains(
        router.socket().path(),
        "run/engine-alpha"
    ));
}

#[test]
fn constraint_engine_layout_assigns_socket_modes_by_component_boundary() {
    let root = TemporaryEngineRoot::new("socket-mode");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout(EngineId::new("engine-beta"));

    for component in [
        EngineComponent::Mind,
        EngineComponent::Router,
        EngineComponent::System,
        EngineComponent::Harness,
        EngineComponent::Terminal,
    ] {
        let socket = layout
            .component(component)
            .expect("component layout exists")
            .socket();
        assert_eq!(socket.mode(), SocketMode::internal_component());
        assert_eq!(socket.mode().as_octal(), 0o600);
    }

    let message_proxy = layout
        .component(EngineComponent::MessageProxy)
        .expect("message proxy layout exists");
    assert_eq!(message_proxy.socket().mode(), SocketMode::message_proxy());
    assert_eq!(message_proxy.socket().mode().as_octal(), 0o660);
}

#[test]
fn constraint_spawn_envelope_carries_component_paths_and_peer_sockets() {
    let root = TemporaryEngineRoot::new("spawn-envelope");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout(EngineId::new("engine-gamma"));
    let envelope = layout
        .spawn_envelope(EngineComponent::Router)
        .expect("router spawn envelope exists");

    assert_eq!(envelope.engine().as_str(), "engine-gamma");
    assert_eq!(envelope.component(), EngineComponent::Router);
    assert!(envelope.state_path().ends_with("router.redb"));
    assert!(envelope.socket_path().ends_with("router.sock"));
    assert_eq!(envelope.socket_mode().as_octal(), 0o600);
    assert_eq!(envelope.peers().len(), 5);
    assert!(
        envelope
            .peers()
            .iter()
            .any(|peer| peer.component() == EngineComponent::Mind
                && peer.socket_path().ends_with("mind.sock"))
    );
    assert!(
        envelope
            .peers()
            .iter()
            .any(|peer| peer.component() == EngineComponent::MessageProxy
                && peer.socket_path().ends_with("message-proxy.sock"))
    );
}

#[test]
fn constraint_engine_layout_prepares_only_engine_scoped_directories() {
    let root = TemporaryEngineRoot::new("prepare");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout(EngineId::new("engine-delta"));
    let prepared = layout
        .prepare_directories()
        .expect("engine directories are prepared");

    assert!(prepared.state_dir().is_dir());
    assert!(prepared.run_dir().is_dir());
    assert!(prepared.state_dir().ends_with("engine-delta"));
    assert!(prepared.run_dir().ends_with("engine-delta"));
    assert!(!layout.manager_store().exists());
}
