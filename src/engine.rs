use std::path::{Path, PathBuf};

use nota_codec::NotaEnum;
use signal_persona_auth::{ComponentName as SignalComponentName, EngineId};

use crate::Result;
use crate::launch::{ComponentCommand, ResolvedComponentCommands};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaDaemonPaths {
    state_root: PathBuf,
    run_root: PathBuf,
}

impl PersonaDaemonPaths {
    pub fn production() -> Self {
        Self::new("/var/lib/persona", "/var/run/persona")
    }

    pub fn new(state_root: impl Into<PathBuf>, run_root: impl Into<PathBuf>) -> Self {
        Self {
            state_root: state_root.into(),
            run_root: run_root.into(),
        }
    }

    pub fn manager_store(&self) -> PathBuf {
        self.state_root.join("manager.redb")
    }

    pub fn manager_socket(&self) -> PathBuf {
        self.run_root.join("persona.sock")
    }

    pub fn engine_layout(&self, engine: EngineId) -> EngineLayout {
        self.engine_layout_with_manager_socket(engine, self.manager_socket())
    }

    pub fn engine_layout_with_manager_socket(
        &self,
        engine: EngineId,
        manager_socket: impl Into<PathBuf>,
    ) -> EngineLayout {
        self.engine_layout_with_manager_socket_and_topology(
            engine,
            manager_socket,
            EngineTopology::FullPrototype,
        )
    }

    pub fn engine_layout_with_topology(
        &self,
        engine: EngineId,
        topology: EngineTopology,
    ) -> EngineLayout {
        self.engine_layout_with_manager_socket_and_topology(engine, self.manager_socket(), topology)
    }

    pub fn engine_layout_with_manager_socket_and_topology(
        &self,
        engine: EngineId,
        manager_socket: impl Into<PathBuf>,
        topology: EngineTopology,
    ) -> EngineLayout {
        let state_dir = self.state_root.join(engine.as_str());
        let run_dir = self.run_root.join(engine.as_str());
        let components = topology
            .components()
            .iter()
            .copied()
            .map(|component| {
                ComponentLayout::new(component, state_dir.as_path(), run_dir.as_path())
            })
            .collect();
        EngineLayout {
            engine,
            state_dir,
            run_dir,
            manager_store: self.manager_store(),
            manager_socket: manager_socket.into(),
            components,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineLayout {
    engine: EngineId,
    state_dir: PathBuf,
    run_dir: PathBuf,
    manager_store: PathBuf,
    manager_socket: PathBuf,
    components: Vec<ComponentLayout>,
}

impl EngineLayout {
    pub fn engine(&self) -> &EngineId {
        &self.engine
    }

    pub fn state_dir(&self) -> &Path {
        self.state_dir.as_path()
    }

    pub fn run_dir(&self) -> &Path {
        self.run_dir.as_path()
    }

    pub fn manager_store(&self) -> &Path {
        self.manager_store.as_path()
    }

    pub fn manager_socket(&self) -> &Path {
        self.manager_socket.as_path()
    }

    pub fn components(&self) -> &[ComponentLayout] {
        self.components.as_slice()
    }

    pub fn component(&self, component: EngineComponent) -> Option<&ComponentLayout> {
        self.components
            .iter()
            .find(|layout| layout.component == component)
    }

    pub fn spawn_envelope(
        &self,
        component: EngineComponent,
        resolved_commands: &ResolvedComponentCommands,
    ) -> Option<ComponentSpawnEnvelope> {
        let layout = self.component(component)?;
        let command = resolved_commands.command_for(component)?.clone();
        let peers = self
            .components
            .iter()
            .filter(|peer| peer.component != component)
            .map(ComponentPeerSocket::from_layout)
            .collect();
        Some(ComponentSpawnEnvelope {
            engine: self.engine.clone(),
            component,
            state_dir: self.state_dir.clone(),
            state_path: layout.state_path.clone(),
            domain_socket_path: layout.domain_socket.path.clone(),
            domain_socket_mode: layout.domain_socket.mode,
            supervision_socket_path: layout.supervision_socket.path.clone(),
            supervision_socket_mode: layout.supervision_socket.mode,
            envelope_path: layout.envelope_path.clone(),
            manager_socket: self.manager_socket.clone(),
            command,
            peers,
        })
    }

    pub fn prepare_directories(&self) -> Result<PreparedEngineLayout> {
        std::fs::create_dir_all(&self.state_dir)?;
        std::fs::create_dir_all(&self.run_dir)?;
        Ok(PreparedEngineLayout {
            state_dir: self.state_dir.clone(),
            run_dir: self.run_dir.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedEngineLayout {
    state_dir: PathBuf,
    run_dir: PathBuf,
}

impl PreparedEngineLayout {
    pub fn state_dir(&self) -> &Path {
        self.state_dir.as_path()
    }

    pub fn run_dir(&self) -> &Path {
        self.run_dir.as_path()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineTopology {
    FullPrototype,
    MessageRouter,
}

impl EngineTopology {
    pub const fn components(self) -> &'static [EngineComponent] {
        match self {
            Self::FullPrototype => &PROTOTYPE_SUPERVISED_COMPONENTS,
            Self::MessageRouter => &MESSAGE_ROUTER_COMPONENTS,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullPrototype => "full-prototype",
            Self::MessageRouter => "message-router",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "full-prototype" => Some(Self::FullPrototype),
            "message-router" => Some(Self::MessageRouter),
            _ => None,
        }
    }
}

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineComponent {
    Mind,
    Router,
    System,
    Harness,
    Terminal,
    Message,
    Introspect,
}

const OPERATIONAL_DELIVERY_COMPONENTS: [EngineComponent; 6] = [
    EngineComponent::Mind,
    EngineComponent::Router,
    EngineComponent::System,
    EngineComponent::Harness,
    EngineComponent::Terminal,
    EngineComponent::Message,
];

const PROTOTYPE_SUPERVISED_COMPONENTS: [EngineComponent; 7] = [
    EngineComponent::Mind,
    EngineComponent::Router,
    EngineComponent::System,
    EngineComponent::Harness,
    EngineComponent::Terminal,
    EngineComponent::Message,
    EngineComponent::Introspect,
];

const MESSAGE_ROUTER_COMPONENTS: [EngineComponent; 2] =
    [EngineComponent::Message, EngineComponent::Router];

impl EngineComponent {
    pub const fn operational_delivery_components() -> [Self; 6] {
        OPERATIONAL_DELIVERY_COMPONENTS
    }

    pub const fn prototype_supervised_components() -> [Self; 7] {
        PROTOTYPE_SUPERVISED_COMPONENTS
    }

    pub const fn message_router_components() -> [Self; 2] {
        MESSAGE_ROUTER_COMPONENTS
    }

    pub const fn component_kind(self) -> signal_persona::ComponentKind {
        match self {
            Self::Mind => signal_persona::ComponentKind::Mind,
            Self::Router => signal_persona::ComponentKind::Router,
            Self::System => signal_persona::ComponentKind::System,
            Self::Harness => signal_persona::ComponentKind::Harness,
            Self::Terminal => signal_persona::ComponentKind::Terminal,
            Self::Message => signal_persona::ComponentKind::Message,
            Self::Introspect => signal_persona::ComponentKind::Introspect,
        }
    }

    pub fn component_name(self) -> signal_persona::ComponentName {
        signal_persona::ComponentName::new(self.as_component_name())
    }

    pub fn from_component_name(component: &signal_persona::ComponentName) -> Option<Self> {
        match component.as_str() {
            "persona-mind" => Some(Self::Mind),
            "persona-router" => Some(Self::Router),
            "persona-system" => Some(Self::System),
            "persona-harness" => Some(Self::Harness),
            "persona-terminal" => Some(Self::Terminal),
            "persona-message" => Some(Self::Message),
            "persona-introspect" => Some(Self::Introspect),
            _ => None,
        }
    }

    pub const fn signal_name(self) -> SignalComponentName {
        match self {
            Self::Mind => SignalComponentName::Mind,
            Self::Router => SignalComponentName::Router,
            Self::System => SignalComponentName::System,
            Self::Harness => SignalComponentName::Harness,
            Self::Terminal => SignalComponentName::Terminal,
            Self::Message => SignalComponentName::Message,
            Self::Introspect => SignalComponentName::Introspect,
        }
    }

    pub const fn socket_file(self) -> &'static str {
        match self {
            Self::Mind => "mind.sock",
            Self::Router => "router.sock",
            Self::System => "system.sock",
            Self::Harness => "harness.sock",
            Self::Terminal => "terminal.sock",
            Self::Message => "message.sock",
            Self::Introspect => "introspect.sock",
        }
    }

    pub const fn supervision_socket_file(self) -> &'static str {
        match self {
            Self::Mind => "mind.supervision.sock",
            Self::Router => "router.supervision.sock",
            Self::System => "system.supervision.sock",
            Self::Harness => "harness.supervision.sock",
            Self::Terminal => "terminal.supervision.sock",
            Self::Message => "message.supervision.sock",
            Self::Introspect => "introspect.supervision.sock",
        }
    }

    pub const fn envelope_file(self) -> &'static str {
        match self {
            Self::Mind => "mind.envelope",
            Self::Router => "router.envelope",
            Self::System => "system.envelope",
            Self::Harness => "harness.envelope",
            Self::Terminal => "terminal.envelope",
            Self::Message => "message.envelope",
            Self::Introspect => "introspect.envelope",
        }
    }

    pub const fn state_file(self) -> &'static str {
        match self {
            Self::Mind => "mind.redb",
            Self::Router => "router.redb",
            Self::System => "system.redb",
            Self::Harness => "harness.redb",
            Self::Terminal => "terminal.redb",
            Self::Message => "message.redb",
            Self::Introspect => "introspect.redb",
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mind => "mind",
            Self::Router => "router",
            Self::System => "system",
            Self::Harness => "harness",
            Self::Terminal => "terminal",
            Self::Message => "message",
            Self::Introspect => "introspect",
        }
    }

    pub const fn as_component_name(self) -> &'static str {
        match self {
            Self::Mind => "persona-mind",
            Self::Router => "persona-router",
            Self::System => "persona-system",
            Self::Harness => "persona-harness",
            Self::Terminal => "persona-terminal",
            Self::Message => "persona-message",
            Self::Introspect => "persona-introspect",
        }
    }

    pub const fn executable_environment_variable(self) -> &'static str {
        match self {
            Self::Mind => "PERSONA_MIND_EXECUTABLE",
            Self::Router => "PERSONA_ROUTER_EXECUTABLE",
            Self::System => "PERSONA_SYSTEM_EXECUTABLE",
            Self::Harness => "PERSONA_HARNESS_EXECUTABLE",
            Self::Terminal => "PERSONA_TERMINAL_EXECUTABLE",
            Self::Message => "PERSONA_MESSAGE_DAEMON_EXECUTABLE",
            Self::Introspect => "PERSONA_INTROSPECT_DAEMON_EXECUTABLE",
        }
    }

    pub const fn socket_mode(self) -> SocketMode {
        match self {
            Self::Message => SocketMode::message_ingress(),
            Self::Mind
            | Self::Router
            | Self::System
            | Self::Harness
            | Self::Terminal
            | Self::Introspect => SocketMode::internal_component(),
        }
    }

    pub const fn supervision_socket_mode(self) -> SocketMode {
        Self::internal_supervision_socket_mode()
    }

    const fn internal_supervision_socket_mode() -> SocketMode {
        SocketMode::internal_component()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentLayout {
    component: EngineComponent,
    state_path: PathBuf,
    envelope_path: PathBuf,
    domain_socket: ComponentSocket,
    supervision_socket: ComponentSocket,
}

impl ComponentLayout {
    fn new(component: EngineComponent, state_dir: &Path, run_dir: &Path) -> Self {
        Self {
            component,
            state_path: state_dir.join(component.state_file()),
            envelope_path: run_dir.join(component.envelope_file()),
            domain_socket: ComponentSocket {
                component,
                path: run_dir.join(component.socket_file()),
                mode: component.socket_mode(),
            },
            supervision_socket: ComponentSocket {
                component,
                path: run_dir.join(component.supervision_socket_file()),
                mode: component.supervision_socket_mode(),
            },
        }
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn state_path(&self) -> &Path {
        self.state_path.as_path()
    }

    pub fn envelope_path(&self) -> &Path {
        self.envelope_path.as_path()
    }

    pub fn domain_socket(&self) -> &ComponentSocket {
        &self.domain_socket
    }

    pub fn supervision_socket(&self) -> &ComponentSocket {
        &self.supervision_socket
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSocket {
    component: EngineComponent,
    path: PathBuf,
    mode: SocketMode,
}

impl ComponentSocket {
    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn mode(&self) -> SocketMode {
        self.mode
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketMode(u32);

impl SocketMode {
    pub const fn internal_component() -> Self {
        Self(0o600)
    }

    pub const fn message_ingress() -> Self {
        Self(0o660)
    }

    pub const fn as_octal(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSpawnEnvelope {
    engine: EngineId,
    component: EngineComponent,
    state_dir: PathBuf,
    state_path: PathBuf,
    domain_socket_path: PathBuf,
    domain_socket_mode: SocketMode,
    supervision_socket_path: PathBuf,
    supervision_socket_mode: SocketMode,
    envelope_path: PathBuf,
    manager_socket: PathBuf,
    command: ComponentCommand,
    peers: Vec<ComponentPeerSocket>,
}

impl ComponentSpawnEnvelope {
    pub fn engine(&self) -> &EngineId {
        &self.engine
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn state_dir(&self) -> &Path {
        self.state_dir.as_path()
    }

    pub fn state_path(&self) -> &Path {
        self.state_path.as_path()
    }

    pub fn domain_socket_path(&self) -> &Path {
        self.domain_socket_path.as_path()
    }

    pub fn domain_socket_mode(&self) -> SocketMode {
        self.domain_socket_mode
    }

    pub fn supervision_socket_path(&self) -> &Path {
        self.supervision_socket_path.as_path()
    }

    pub fn supervision_socket_mode(&self) -> SocketMode {
        self.supervision_socket_mode
    }

    pub fn envelope_path(&self) -> &Path {
        self.envelope_path.as_path()
    }

    pub fn manager_socket(&self) -> &Path {
        self.manager_socket.as_path()
    }

    pub fn command(&self) -> &ComponentCommand {
        &self.command
    }

    pub fn peers(&self) -> &[ComponentPeerSocket] {
        self.peers.as_slice()
    }

    pub fn signal_spawn_envelope(&self) -> signal_persona::SpawnEnvelope {
        signal_persona::SpawnEnvelope {
            engine_id: self.engine.clone(),
            component_kind: self.component.component_kind(),
            component_name: self.component.signal_name(),
            state_dir: signal_persona::WirePath::new(self.state_dir.to_string_lossy().into_owned()),
            domain_socket_path: signal_persona::WirePath::new(
                self.domain_socket_path.to_string_lossy().into_owned(),
            ),
            domain_socket_mode: signal_persona::SocketMode::new(self.domain_socket_mode.as_octal()),
            supervision_socket_path: signal_persona::WirePath::new(
                self.supervision_socket_path.to_string_lossy().into_owned(),
            ),
            supervision_socket_mode: signal_persona::SocketMode::new(
                self.supervision_socket_mode.as_octal(),
            ),
            peer_sockets: self
                .peers
                .iter()
                .map(ComponentPeerSocket::signal_peer_socket)
                .collect(),
            manager_socket: signal_persona::WirePath::new(
                self.manager_socket.to_string_lossy().into_owned(),
            ),
            supervision_protocol_version: signal_persona::SupervisionProtocolVersion::new(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentPeerSocket {
    component: EngineComponent,
    domain_socket_path: PathBuf,
}

impl ComponentPeerSocket {
    fn from_layout(layout: &ComponentLayout) -> Self {
        Self {
            component: layout.component,
            domain_socket_path: layout.domain_socket.path.clone(),
        }
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn domain_socket_path(&self) -> &Path {
        self.domain_socket_path.as_path()
    }

    pub fn signal_peer_socket(&self) -> signal_persona::PeerSocket {
        signal_persona::PeerSocket {
            component_name: self.component.signal_name(),
            domain_socket_path: signal_persona::WirePath::new(
                self.domain_socket_path.to_string_lossy().into_owned(),
            ),
        }
    }
}
