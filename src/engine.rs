use std::path::{Path, PathBuf};

use nota_next::{NotaDecode, NotaEncode};
use signal_persona::origin::{
    ComponentName as SignalComponentName, EngineIdentifier, OwnerIdentity, UnixUserIdentifier,
};

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
        self.state_root.join("manager.sema")
    }

    pub fn manager_socket(&self) -> PathBuf {
        self.run_root.join("persona.sock")
    }

    pub fn engine_layout(&self, engine: EngineIdentifier) -> EngineLayout {
        self.engine_layout_with_owner(engine, Self::current_owner_identity())
    }

    pub fn engine_layout_with_owner(
        &self,
        engine: EngineIdentifier,
        owner_identity: OwnerIdentity,
    ) -> EngineLayout {
        self.engine_layout_with_manager_socket_and_owner(
            engine,
            self.manager_socket(),
            EngineTopology::FullPrototype,
            owner_identity,
        )
    }

    pub fn engine_layout_with_manager_socket(
        &self,
        engine: EngineIdentifier,
        manager_socket: impl Into<PathBuf>,
    ) -> EngineLayout {
        self.engine_layout_with_manager_socket_and_owner(
            engine,
            manager_socket,
            EngineTopology::FullPrototype,
            Self::current_owner_identity(),
        )
    }

    pub fn engine_layout_with_topology(
        &self,
        engine: EngineIdentifier,
        topology: EngineTopology,
    ) -> EngineLayout {
        self.engine_layout_with_manager_socket_and_owner(
            engine,
            self.manager_socket(),
            topology,
            Self::current_owner_identity(),
        )
    }

    pub fn engine_layout_with_manager_socket_and_topology(
        &self,
        engine: EngineIdentifier,
        manager_socket: impl Into<PathBuf>,
        topology: EngineTopology,
    ) -> EngineLayout {
        self.engine_layout_with_manager_socket_and_owner(
            engine,
            manager_socket,
            topology,
            Self::current_owner_identity(),
        )
    }

    pub fn engine_layout_with_manager_socket_and_owner(
        &self,
        engine: EngineIdentifier,
        manager_socket: impl Into<PathBuf>,
        topology: EngineTopology,
        owner_identity: OwnerIdentity,
    ) -> EngineLayout {
        let state_dir = self.state_root.join(engine.as_str());
        let run_dir = self.run_root.join(engine.as_str());
        let components = topology
            .component_topology_entries()
            .iter()
            .copied()
            .map(|entry| {
                ComponentLayout::from_topology_entry(entry, state_dir.as_path(), run_dir.as_path())
            })
            .collect();
        EngineLayout {
            engine,
            owner_identity,
            state_dir,
            run_dir,
            manager_store: self.manager_store(),
            manager_socket: manager_socket.into(),
            components,
        }
    }

    fn current_owner_identity() -> OwnerIdentity {
        OwnerIdentity::UnixUser(UnixUserIdentifier::new(unsafe { libc::geteuid() }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineLayout {
    engine: EngineIdentifier,
    owner_identity: OwnerIdentity,
    state_dir: PathBuf,
    run_dir: PathBuf,
    manager_store: PathBuf,
    manager_socket: PathBuf,
    components: Vec<ComponentLayout>,
}

impl EngineLayout {
    pub fn engine(&self) -> &EngineIdentifier {
        &self.engine
    }

    pub fn owner_identity(&self) -> &OwnerIdentity {
        &self.owner_identity
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

    pub fn component_instance(
        &self,
        instance_name: &ComponentInstanceName,
    ) -> Option<&ComponentLayout> {
        self.components
            .iter()
            .find(|layout| &layout.instance_name == instance_name)
    }

    pub fn spawn_envelope(
        &self,
        component: EngineComponent,
        resolved_commands: &ResolvedComponentCommands,
    ) -> Option<ComponentSpawnEnvelope> {
        let layout = self.component(component)?;
        self.spawn_envelope_for_layout(layout, resolved_commands)
    }

    pub fn spawn_envelope_for_instance(
        &self,
        instance_name: &ComponentInstanceName,
        resolved_commands: &ResolvedComponentCommands,
    ) -> Option<ComponentSpawnEnvelope> {
        let layout = self.component_instance(instance_name)?;
        self.spawn_envelope_for_layout(layout, resolved_commands)
    }

    fn spawn_envelope_for_layout(
        &self,
        layout: &ComponentLayout,
        resolved_commands: &ResolvedComponentCommands,
    ) -> Option<ComponentSpawnEnvelope> {
        let component = layout.component;
        let command = resolved_commands.command_for(component)?.clone();
        let peers = self
            .components
            .iter()
            .filter(|peer| peer.instance_name != layout.instance_name)
            .map(ComponentPeerSocket::from_layout)
            .collect();
        Some(ComponentSpawnEnvelope {
            engine: self.engine.clone(),
            owner_identity: self.owner_identity.clone(),
            component_instance: layout.instance_name.clone(),
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

    pub fn prepare_directories(&self) -> crate::Result<PreparedEngineLayout> {
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
    MindOrchestrate,
    ThreeHarnessChain,
}

impl EngineTopology {
    pub const fn components(self) -> &'static [EngineComponent] {
        match self {
            Self::FullPrototype => &PROTOTYPE_SUPERVISED_COMPONENTS,
            Self::MessageRouter => &MESSAGE_ROUTER_COMPONENTS,
            Self::MindOrchestrate => &MIND_ORCHESTRATE_COMPONENTS,
            Self::ThreeHarnessChain => &THREE_HARNESS_CHAIN_COMPONENTS,
        }
    }

    pub const fn component_topology_entries(self) -> &'static [ComponentTopologyEntry] {
        match self {
            Self::FullPrototype => &PROTOTYPE_SUPERVISED_COMPONENT_ENTRIES,
            Self::MessageRouter => &MESSAGE_ROUTER_COMPONENT_ENTRIES,
            Self::MindOrchestrate => &MIND_ORCHESTRATE_COMPONENT_ENTRIES,
            Self::ThreeHarnessChain => &THREE_HARNESS_CHAIN_COMPONENT_ENTRIES,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullPrototype => "full-prototype",
            Self::MessageRouter => "message-router",
            Self::MindOrchestrate => "mind-orchestrate",
            Self::ThreeHarnessChain => "three-harness-chain",
        }
    }

    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "full-prototype" => Some(Self::FullPrototype),
            "message-router" => Some(Self::MessageRouter),
            "mind-orchestrate" => Some(Self::MindOrchestrate),
            "three-harness-chain" => Some(Self::ThreeHarnessChain),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentTopologyEntry {
    component: EngineComponent,
    instance_name: &'static str,
}

impl ComponentTopologyEntry {
    pub const fn new(component: EngineComponent, instance_name: &'static str) -> Self {
        Self {
            component,
            instance_name,
        }
    }

    pub const fn for_component(component: EngineComponent) -> Self {
        Self::new(component, component.as_str())
    }

    pub const fn component(self) -> EngineComponent {
        self.component
    }

    pub const fn instance_name(self) -> &'static str {
        self.instance_name
    }
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineComponent {
    Mind,
    Orchestrate,
    Router,
    System,
    Harness,
    Terminal,
    Message,
    Introspect,
    Spirit,
}

const OPERATIONAL_DELIVERY_COMPONENTS: [EngineComponent; 6] = [
    EngineComponent::Mind,
    EngineComponent::Router,
    EngineComponent::System,
    EngineComponent::Harness,
    EngineComponent::Terminal,
    EngineComponent::Message,
];

const PROTOTYPE_SUPERVISED_COMPONENTS: [EngineComponent; 8] = [
    EngineComponent::Mind,
    EngineComponent::Router,
    EngineComponent::System,
    EngineComponent::Harness,
    EngineComponent::Terminal,
    EngineComponent::Message,
    EngineComponent::Introspect,
    EngineComponent::Spirit,
];

const MESSAGE_ROUTER_COMPONENTS: [EngineComponent; 2] =
    [EngineComponent::Message, EngineComponent::Router];

const MIND_ORCHESTRATE_COMPONENTS: [EngineComponent; 2] =
    [EngineComponent::Mind, EngineComponent::Orchestrate];

const THREE_HARNESS_CHAIN_COMPONENTS: [EngineComponent; 4] = [
    EngineComponent::Message,
    EngineComponent::Router,
    EngineComponent::Harness,
    EngineComponent::Terminal,
];

const PROTOTYPE_SUPERVISED_COMPONENT_ENTRIES: [ComponentTopologyEntry; 8] = [
    ComponentTopologyEntry::for_component(EngineComponent::Mind),
    ComponentTopologyEntry::for_component(EngineComponent::Router),
    ComponentTopologyEntry::for_component(EngineComponent::System),
    ComponentTopologyEntry::for_component(EngineComponent::Harness),
    ComponentTopologyEntry::for_component(EngineComponent::Terminal),
    ComponentTopologyEntry::for_component(EngineComponent::Message),
    ComponentTopologyEntry::for_component(EngineComponent::Introspect),
    ComponentTopologyEntry::for_component(EngineComponent::Spirit),
];

const MESSAGE_ROUTER_COMPONENT_ENTRIES: [ComponentTopologyEntry; 2] = [
    ComponentTopologyEntry::for_component(EngineComponent::Message),
    ComponentTopologyEntry::for_component(EngineComponent::Router),
];

const MIND_ORCHESTRATE_COMPONENT_ENTRIES: [ComponentTopologyEntry; 2] = [
    ComponentTopologyEntry::for_component(EngineComponent::Mind),
    ComponentTopologyEntry::for_component(EngineComponent::Orchestrate),
];

const THREE_HARNESS_CHAIN_COMPONENT_ENTRIES: [ComponentTopologyEntry; 8] = [
    ComponentTopologyEntry::for_component(EngineComponent::Message),
    ComponentTopologyEntry::for_component(EngineComponent::Router),
    ComponentTopologyEntry::new(EngineComponent::Terminal, "initiator-terminal"),
    ComponentTopologyEntry::new(EngineComponent::Harness, "initiator"),
    ComponentTopologyEntry::new(EngineComponent::Terminal, "responder-terminal"),
    ComponentTopologyEntry::new(EngineComponent::Harness, "responder"),
    ComponentTopologyEntry::new(EngineComponent::Terminal, "reviewer-terminal"),
    ComponentTopologyEntry::new(EngineComponent::Harness, "reviewer"),
];

impl EngineComponent {
    pub const fn operational_delivery_components() -> [Self; 6] {
        OPERATIONAL_DELIVERY_COMPONENTS
    }

    pub const fn prototype_supervised_components() -> [Self; 8] {
        PROTOTYPE_SUPERVISED_COMPONENTS
    }

    pub const fn message_router_components() -> [Self; 2] {
        MESSAGE_ROUTER_COMPONENTS
    }

    pub const fn component_kind(self) -> signal_persona::ComponentKind {
        match self {
            Self::Mind => signal_persona::ComponentKind::Mind,
            Self::Orchestrate => signal_persona::ComponentKind::Orchestrate,
            Self::Router => signal_persona::ComponentKind::Router,
            Self::System => signal_persona::ComponentKind::System,
            Self::Harness => signal_persona::ComponentKind::Harness,
            Self::Terminal => signal_persona::ComponentKind::Terminal,
            Self::Message => signal_persona::ComponentKind::Message,
            Self::Introspect => signal_persona::ComponentKind::Introspect,
            Self::Spirit => signal_persona::ComponentKind::Spirit,
        }
    }

    pub fn component_name(self) -> signal_persona::ComponentName {
        signal_persona::ComponentName::new(self.as_component_name())
    }

    pub fn from_component_name(component: &signal_persona::ComponentName) -> Option<Self> {
        match component.as_str() {
            "persona-mind" => Some(Self::Mind),
            "persona-orchestrate" => Some(Self::Orchestrate),
            "persona-router" => Some(Self::Router),
            "persona-system" => Some(Self::System),
            "persona-harness" => Some(Self::Harness),
            "persona-terminal" => Some(Self::Terminal),
            "persona-message" => Some(Self::Message),
            "persona-introspect" => Some(Self::Introspect),
            "persona-spirit" => Some(Self::Spirit),
            _ => None,
        }
    }

    pub const fn signal_name(self) -> SignalComponentName {
        match self {
            Self::Mind => SignalComponentName::Mind,
            Self::Orchestrate => SignalComponentName::Orchestrate,
            Self::Router => SignalComponentName::Router,
            Self::System => SignalComponentName::System,
            Self::Harness => SignalComponentName::Harness,
            Self::Terminal => SignalComponentName::Terminal,
            Self::Message => SignalComponentName::Message,
            Self::Introspect => SignalComponentName::Introspect,
            Self::Spirit => SignalComponentName::Spirit,
        }
    }

    pub const fn socket_file(self) -> &'static str {
        match self {
            Self::Mind => "mind.sock",
            Self::Orchestrate => "orchestrate.sock",
            Self::Router => "router.sock",
            Self::System => "system.sock",
            Self::Harness => "harness.sock",
            Self::Terminal => "terminal.sock",
            Self::Message => "message.sock",
            Self::Introspect => "introspect.sock",
            Self::Spirit => "spirit.sock",
        }
    }

    pub const fn supervision_socket_file(self) -> &'static str {
        match self {
            Self::Mind => "mind.supervision.sock",
            Self::Orchestrate => "orchestrate.supervision.sock",
            Self::Router => "router.supervision.sock",
            Self::System => "system.supervision.sock",
            Self::Harness => "harness.supervision.sock",
            Self::Terminal => "terminal.supervision.sock",
            Self::Message => "message.supervision.sock",
            Self::Introspect => "introspect.supervision.sock",
            Self::Spirit => "spirit.supervision.sock",
        }
    }

    pub const fn envelope_file(self) -> &'static str {
        match self {
            Self::Mind => "mind.envelope",
            Self::Orchestrate => "orchestrate.envelope",
            Self::Router => "router.envelope",
            Self::System => "system.envelope",
            Self::Harness => "harness.envelope",
            Self::Terminal => "terminal.envelope",
            Self::Message => "message.envelope",
            Self::Introspect => "introspect.envelope",
            Self::Spirit => "spirit.envelope",
        }
    }

    pub const fn state_file(self) -> &'static str {
        match self {
            Self::Mind => "mind.sema",
            Self::Orchestrate => "orchestrate.sema",
            Self::Router => "router.sema",
            Self::System => "system.sema",
            Self::Harness => "harness.sema",
            Self::Terminal => "terminal.sema",
            Self::Message => "message.sema",
            Self::Introspect => "introspect.sema",
            Self::Spirit => "spirit.sema",
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mind => "mind",
            Self::Orchestrate => "orchestrate",
            Self::Router => "router",
            Self::System => "system",
            Self::Harness => "harness",
            Self::Terminal => "terminal",
            Self::Message => "message",
            Self::Introspect => "introspect",
            Self::Spirit => "spirit",
        }
    }

    pub const fn as_component_name(self) -> &'static str {
        match self {
            Self::Mind => "persona-mind",
            Self::Orchestrate => "persona-orchestrate",
            Self::Router => "persona-router",
            Self::System => "persona-system",
            Self::Harness => "persona-harness",
            Self::Terminal => "persona-terminal",
            Self::Message => "persona-message",
            Self::Introspect => "persona-introspect",
            Self::Spirit => "persona-spirit",
        }
    }

    pub const fn executable_environment_variable(self) -> &'static str {
        match self {
            Self::Mind => "PERSONA_MIND_EXECUTABLE",
            Self::Orchestrate => "PERSONA_ORCHESTRATE_EXECUTABLE",
            Self::Router => "PERSONA_ROUTER_EXECUTABLE",
            Self::System => "PERSONA_SYSTEM_EXECUTABLE",
            Self::Harness => "PERSONA_HARNESS_EXECUTABLE",
            Self::Terminal => "PERSONA_TERMINAL_EXECUTABLE",
            Self::Message => "PERSONA_MESSAGE_DAEMON_EXECUTABLE",
            Self::Introspect => "PERSONA_INTROSPECT_DAEMON_EXECUTABLE",
            Self::Spirit => "PERSONA_SPIRIT_DAEMON_EXECUTABLE",
        }
    }

    pub const fn socket_mode(self) -> SocketMode {
        match self {
            Self::Message => SocketMode::message_ingress(),
            Self::Mind
            | Self::Orchestrate
            | Self::Router
            | Self::System
            | Self::Harness
            | Self::Terminal
            | Self::Introspect
            | Self::Spirit => SocketMode::internal_component(),
        }
    }

    pub const fn supervision_socket_mode(self) -> SocketMode {
        Self::internal_supervision_socket_mode()
    }

    const fn internal_supervision_socket_mode() -> SocketMode {
        SocketMode::internal_component()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentInstanceName(String);

impl ComponentInstanceName {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn from_component(component: EngineComponent) -> Self {
        Self::new(component.as_str())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentLayout {
    instance_name: ComponentInstanceName,
    component: EngineComponent,
    state_path: PathBuf,
    envelope_path: PathBuf,
    domain_socket: ComponentSocket,
    supervision_socket: ComponentSocket,
}

impl ComponentLayout {
    fn from_topology_entry(
        entry: ComponentTopologyEntry,
        state_dir: &Path,
        run_dir: &Path,
    ) -> Self {
        let component = entry.component();
        let instance_name = ComponentInstanceName::new(entry.instance_name());
        Self {
            state_path: state_dir.join(format!("{}.sema", instance_name.as_str())),
            envelope_path: run_dir.join(format!("{}.envelope", instance_name.as_str())),
            domain_socket: ComponentSocket {
                component,
                path: run_dir.join(format!("{}.sock", instance_name.as_str())),
                mode: component.socket_mode(),
            },
            supervision_socket: ComponentSocket {
                component,
                path: run_dir.join(format!("{}.supervision.sock", instance_name.as_str())),
                mode: component.supervision_socket_mode(),
            },
            instance_name,
            component,
        }
    }

    pub fn instance_name(&self) -> &ComponentInstanceName {
        &self.instance_name
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
    engine: EngineIdentifier,
    owner_identity: OwnerIdentity,
    component_instance: ComponentInstanceName,
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
    pub fn engine(&self) -> &EngineIdentifier {
        &self.engine
    }

    pub fn owner_identity(&self) -> &OwnerIdentity {
        &self.owner_identity
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn component_instance(&self) -> &ComponentInstanceName {
        &self.component_instance
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
            engine_identifier: self.engine.clone(),
            component_kind: self.component.component_kind(),
            component_name: self.component.signal_name(),
            owner_identity: self.owner_identity.clone(),
            state_dir: signal_persona::WirePath::new(self.state_dir.to_string_lossy().into_owned()),
            domain_socket_path: signal_persona::WirePath::new(
                self.domain_socket_path.to_string_lossy().into_owned(),
            ),
            domain_socket_mode: signal_persona::SocketMode::new(self.domain_socket_mode.as_octal()),
            engine_management_socket_path: signal_persona::WirePath::new(
                self.supervision_socket_path.to_string_lossy().into_owned(),
            ),
            engine_management_socket_mode: signal_persona::SocketMode::new(
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
            engine_management_protocol_version:
                signal_persona::EngineManagementProtocolVersion::new(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentPeerSocket {
    instance_name: ComponentInstanceName,
    component: EngineComponent,
    domain_socket_path: PathBuf,
}

impl ComponentPeerSocket {
    fn from_layout(layout: &ComponentLayout) -> Self {
        Self {
            instance_name: layout.instance_name.clone(),
            component: layout.component,
            domain_socket_path: layout.domain_socket.path.clone(),
        }
    }

    pub fn instance_name(&self) -> &ComponentInstanceName {
        &self.instance_name
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
