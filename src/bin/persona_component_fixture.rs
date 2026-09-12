use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use signal_persona::{
    ByteViewable, ComponentHealth, ComponentIdentity, ComponentKind, ComponentName, LifecycleQuery,
    Query as EngineManagementRequest, Response as EngineManagementReply, Restorable, Signal,
    Signalizable,
};

struct FixtureProcess {
    state_path: PathBuf,
    component: FixtureComponent,
    domain_socket: FixtureSocket,
    supervision_socket: FixtureSocket,
    spawn_envelope: String,
    manager_socket: String,
    peer_count: String,
}

impl FixtureProcess {
    fn from_environment() -> Self {
        let state_path = PathBuf::from(Self::required_env("PERSONA_STATE_PATH"));
        let component = FixtureComponent::from_name(Self::required_env("PERSONA_COMPONENT"));
        let domain_socket = FixtureSocket::domain_from_environment();
        let supervision_socket = FixtureSocket::supervision_from_environment();
        Self {
            state_path,
            component,
            domain_socket,
            supervision_socket,
            spawn_envelope: Self::required_env("PERSONA_SPAWN_ENVELOPE"),
            manager_socket: Self::required_env("PERSONA_MANAGER_SOCKET"),
            peer_count: Self::required_env("PERSONA_PEER_SOCKET_COUNT"),
        }
    }

    fn supervision_only_from_environment() -> SupervisionOnlyProcess {
        SupervisionOnlyProcess {
            component: FixtureComponent::from_name(Self::required_env("PERSONA_COMPONENT")),
            supervision_socket: FixtureSocket::supervision_from_environment(),
        }
    }

    fn run(self) {
        let state_dir = self.state_path.parent().expect("state path parent");
        fs::create_dir_all(state_dir).expect("state dir created");

        let domain_listener = self.domain_socket.bind();
        let supervision_listener = self.supervision_socket.bind();
        let supervision_server =
            SupervisionServer::new(self.component.clone(), supervision_listener);
        let _supervision_thread = thread::spawn(move || supervision_server.run());

        self.write_capture(state_dir);

        loop {
            let _ = &domain_listener;
            thread::sleep(Duration::from_secs(1));
        }
    }

    fn write_capture(&self, state_dir: &Path) {
        let component_instance = Self::required_env("PERSONA_COMPONENT_INSTANCE");
        let capture = state_dir.join(format!("{component_instance}.env"));
        let text = format!(
            "engine={}\ncomponent={}\ncomponent_instance={}\nprocess={}\nstate_path={}\ndomain_socket={}\nsupervision_socket={}\nspawn_envelope={}\nmanager_socket={}\ndomain_mode={}\nsupervision_mode={}\npeer_count={}\n",
            Self::required_env("PERSONA_ENGINE_ID"),
            self.component.as_str(),
            component_instance,
            std::process::id(),
            Self::required_env("PERSONA_STATE_PATH"),
            self.domain_socket.path().display(),
            self.supervision_socket.path().display(),
            self.spawn_envelope,
            self.manager_socket,
            self.domain_socket.mode_text(),
            self.supervision_socket.mode_text(),
            self.peer_count,
        );
        let text = match env::var("PERSONA_ACTUAL_EXECUTABLE") {
            Ok(actual) => format!("{text}actual={actual}\n"),
            Err(_) => text,
        };
        fs::write(capture, text).expect("component capture written");
    }

    fn required_env(name: &'static str) -> String {
        env::var(name).unwrap_or_else(|_| panic!("{name} environment variable is required"))
    }
}

struct SupervisionOnlyProcess {
    component: FixtureComponent,
    supervision_socket: FixtureSocket,
}

impl SupervisionOnlyProcess {
    fn run(self) {
        let supervision_listener = self.supervision_socket.bind();
        SupervisionServer::new(self.component, supervision_listener).run();
    }
}

#[derive(Debug, Clone)]
struct FixtureComponent {
    name: String,
    signal_name: ComponentName,
    kind: ComponentKind,
}

impl FixtureComponent {
    fn from_name(name: String) -> Self {
        let kind = match name.as_str() {
            "mind" => ComponentKind::Mind,
            "orchestrate" => ComponentKind::Orchestrate,
            "router" => ComponentKind::Router,
            "system" => ComponentKind::System,
            "harness" => ComponentKind::Harness,
            "terminal" => ComponentKind::Terminal,
            "message" => ComponentKind::Message,
            "introspect" => ComponentKind::Introspect,
            "spirit" => ComponentKind::Spirit,
            other => panic!("unknown component name: {other}"),
        };
        let signal_name = match name.as_str() {
            "mind" => String::from("mind"),
            _ => format!("persona-{name}"),
        };
        Self {
            signal_name,
            name,
            kind,
        }
    }

    fn as_str(&self) -> &str {
        self.name.as_str()
    }
}

struct FixtureSocket {
    path: PathBuf,
    mode_text: String,
}

impl FixtureSocket {
    fn domain_from_environment() -> Self {
        let path = env::var("PERSONA_DOMAIN_SOCKET_PATH")
            .or_else(|_| env::var("PERSONA_SOCKET_PATH"))
            .expect("domain socket path");
        let mode_text = env::var("PERSONA_DOMAIN_SOCKET_MODE")
            .or_else(|_| env::var("PERSONA_SOCKET_MODE"))
            .expect("domain socket mode");
        Self {
            path: PathBuf::from(path),
            mode_text,
        }
    }

    fn supervision_from_environment() -> Self {
        Self {
            path: PathBuf::from(
                env::var("PERSONA_SUPERVISION_SOCKET_PATH").expect("supervision socket path"),
            ),
            mode_text: env::var("PERSONA_SUPERVISION_SOCKET_MODE")
                .expect("supervision socket mode"),
        }
    }

    fn bind(&self) -> std::os::unix::net::UnixListener {
        let _ = fs::remove_file(&self.path);
        let listener =
            std::os::unix::net::UnixListener::bind(&self.path).expect("fixture socket bound");
        fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode()))
            .expect("fixture socket mode applied");
        listener
    }

    fn path(&self) -> &Path {
        self.path.as_path()
    }

    fn mode_text(&self) -> &str {
        self.mode_text.as_str()
    }

    fn mode(&self) -> u32 {
        u32::from_str_radix(self.mode_text.as_str(), 8).expect("octal socket mode")
    }
}

struct SupervisionServer {
    component: FixtureComponent,
    listener: std::os::unix::net::UnixListener,
    codec: BlockingSupervisionCodec,
}

impl SupervisionServer {
    fn new(component: FixtureComponent, listener: std::os::unix::net::UnixListener) -> Self {
        Self {
            component,
            listener,
            codec: BlockingSupervisionCodec::new(1024 * 1024),
        }
    }

    fn run(self) {
        for incoming in self.listener.incoming() {
            match incoming {
                Ok(mut stream) => self.serve_connection(&mut stream),
                Err(error) => panic!("accept supervision connection: {error}"),
            }
        }
    }

    fn serve_connection(&self, stream: &mut std::os::unix::net::UnixStream) {
        while let Ok(request) = self.codec.read_request(stream) {
            let reply = self.reply_to(request);
            self.codec
                .write_reply(stream, reply)
                .expect("write supervision reply");
        }
    }

    fn reply_to(&self, request: EngineManagementRequest) -> EngineManagementReply {
        match request {
            EngineManagementRequest::Announce(_) => {
                EngineManagementReply::Identified(ComponentIdentity {
                    component_name: self.component.signal_name.clone(),
                    component_kind: self.component.kind.clone(),
                    engine_management_protocol_version: 1,
                    component_startup_error_option: None,
                })
            }
            EngineManagementRequest::Query(query) => match query {
                LifecycleQuery::ReadinessStatus(_) => EngineManagementReply::Ready(None),
                LifecycleQuery::HealthStatus(_) => {
                    EngineManagementReply::HealthReport(ComponentHealth::Running)
                }
            },
            EngineManagementRequest::Stop(_) => EngineManagementReply::StopAcknowledged(None),
        }
    }
}

#[derive(Clone, Copy)]
struct BlockingSupervisionCodec {
    maximum_frame_bytes: usize,
}

impl BlockingSupervisionCodec {
    const fn new(maximum_frame_bytes: usize) -> Self {
        Self {
            maximum_frame_bytes,
        }
    }

    fn read_request(
        &self,
        stream: &mut std::os::unix::net::UnixStream,
    ) -> std::io::Result<EngineManagementRequest> {
        use std::io::Read;

        let mut prefix = [0_u8; 4];
        stream.read_exact(&mut prefix)?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length > self.maximum_frame_bytes {
            panic!("supervision frame too large: {length}");
        }

        let mut bytes = vec![0_u8; length];
        stream.read_exact(&mut bytes)?;
        Signal::<EngineManagementRequest>::from(bytes)
            .restore()
            .map_err(io_error)
    }

    fn write_reply(
        &self,
        stream: &mut std::os::unix::net::UnixStream,
        reply: EngineManagementReply,
    ) -> std::io::Result<()> {
        use std::io::Write;

        let signal = reply.signalize().map_err(io_error)?;
        let bytes = signal.bytes();
        let length = u32::try_from(bytes.len()).map_err(io_error)?;
        stream.write_all(&length.to_be_bytes())?;
        stream.write_all(bytes)?;
        stream.flush()
    }
}

fn io_error(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
}

fn main() {
    match env::var("PERSONA_COMPONENT_FIXTURE_MODE")
        .unwrap_or_else(|_| "component".to_string())
        .as_str()
    {
        "component" => FixtureProcess::from_environment().run(),
        "supervision-only" => FixtureProcess::supervision_only_from_environment().run(),
        other => panic!("unknown PERSONA_COMPONENT_FIXTURE_MODE: {other}"),
    }
}
