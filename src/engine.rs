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
        let state_dir = self.state_root.join(engine.as_str());
        let run_dir = self.run_root.join(engine.as_str());
        let components = EngineComponent::prototype_supervised_components()
            .into_iter()
            .map(|component| {
                ComponentLayout::new(component, state_dir.as_path(), run_dir.as_path())
            })
            .collect();
        EngineLayout {
            engine,
            state_dir,
            run_dir,
            manager_store: self.manager_store(),
            manager_socket: self.manager_socket(),
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
            state_path: layout.state_path.clone(),
            socket_path: layout.socket.path.clone(),
            socket_mode: layout.socket.mode,
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

impl EngineComponent {
    pub const fn operational_delivery_components() -> [Self; 6] {
        [
            Self::Mind,
            Self::Router,
            Self::System,
            Self::Harness,
            Self::Terminal,
            Self::Message,
        ]
    }

    pub const fn prototype_supervised_components() -> [Self; 7] {
        [
            Self::Mind,
            Self::Router,
            Self::System,
            Self::Harness,
            Self::Terminal,
            Self::Message,
            Self::Introspect,
        ]
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentLayout {
    component: EngineComponent,
    state_path: PathBuf,
    socket: ComponentSocket,
}

impl ComponentLayout {
    fn new(component: EngineComponent, state_dir: &Path, run_dir: &Path) -> Self {
        Self {
            component,
            state_path: state_dir.join(component.state_file()),
            socket: ComponentSocket {
                component,
                path: run_dir.join(component.socket_file()),
                mode: component.socket_mode(),
            },
        }
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn state_path(&self) -> &Path {
        self.state_path.as_path()
    }

    pub fn socket(&self) -> &ComponentSocket {
        &self.socket
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
    state_path: PathBuf,
    socket_path: PathBuf,
    socket_mode: SocketMode,
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

    pub fn state_path(&self) -> &Path {
        self.state_path.as_path()
    }

    pub fn socket_path(&self) -> &Path {
        self.socket_path.as_path()
    }

    pub fn socket_mode(&self) -> SocketMode {
        self.socket_mode
    }

    pub fn command(&self) -> &ComponentCommand {
        &self.command
    }

    pub fn peers(&self) -> &[ComponentPeerSocket] {
        self.peers.as_slice()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentPeerSocket {
    component: EngineComponent,
    socket_path: PathBuf,
}

impl ComponentPeerSocket {
    fn from_layout(layout: &ComponentLayout) -> Self {
        Self {
            component: layout.component,
            socket_path: layout.socket.path.clone(),
        }
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn socket_path(&self) -> &Path {
        self.socket_path.as_path()
    }
}
