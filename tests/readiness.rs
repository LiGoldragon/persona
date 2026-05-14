use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kameo::actor::Spawn;
use kameo::error::SendError;
use persona::engine::{EngineComponent, SocketMode};
use persona::readiness::{
    ComponentSocketExpectation, ComponentSocketReadiness, ComponentSocketReadinessFailure,
    VerifyComponentSocket,
};

struct ReadinessFixture {
    root: PathBuf,
}

impl ReadinessFixture {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let root =
            PathBuf::from("/tmp").join(format!("p-ready-{name}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(root.as_path());
        std::fs::create_dir_all(root.as_path()).expect("readiness fixture root created");
        Self { root }
    }

    fn socket_path(&self) -> PathBuf {
        self.root.join("router.sock")
    }

    fn bind_socket(&self, mode: u32) -> UnixListener {
        let socket = self.socket_path();
        let listener = UnixListener::bind(socket.as_path()).expect("test socket bound");
        std::fs::set_permissions(socket, std::fs::Permissions::from_mode(mode))
            .expect("test socket mode applied");
        listener
    }
}

impl Drop for ReadinessFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.root.as_path());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_component_ready_requires_socket_metadata_from_spawn_envelope() {
    let fixture = ReadinessFixture::new("ready");
    let _listener = fixture.bind_socket(0o600);
    let readiness =
        ComponentSocketReadiness::spawn(ComponentSocketReadiness::new(1, Duration::ZERO));

    let ready = readiness
        .ask(VerifyComponentSocket::new(ComponentSocketExpectation::new(
            EngineComponent::Router,
            fixture.socket_path(),
            SocketMode::internal_component(),
        )))
        .await
        .expect("socket is ready");

    assert_eq!(ready.component(), EngineComponent::Router);
    assert_eq!(ready.mode(), SocketMode::internal_component());
    readiness.stop_gracefully().await.expect("readiness stops");
    readiness.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_component_ready_rejects_wrong_socket_mode() {
    let fixture = ReadinessFixture::new("wrong-mode");
    let _listener = fixture.bind_socket(0o600);
    let readiness =
        ComponentSocketReadiness::spawn(ComponentSocketReadiness::new(1, Duration::ZERO));

    let error = readiness
        .ask(VerifyComponentSocket::new(ComponentSocketExpectation::new(
            EngineComponent::Message,
            fixture.socket_path(),
            SocketMode::message_ingress(),
        )))
        .await
        .expect_err("wrong mode reaches handler error");

    match error {
        SendError::HandlerError(ComponentSocketReadinessFailure::WrongMode {
            component,
            expected,
            actual,
            ..
        }) => {
            assert_eq!(component, EngineComponent::Message);
            assert_eq!(expected, 0o660);
            assert_eq!(actual, 0o600);
        }
        other => panic!("unexpected readiness failure: {other:?}"),
    }
    readiness.stop_gracefully().await.expect("readiness stops");
    readiness.wait_for_shutdown().await;
}
