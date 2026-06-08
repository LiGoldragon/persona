use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use kameo::actor::Spawn;
use kameo::error::SendError;
use persona::engine::{
    ComponentInstanceName, EngineComponent, EngineTopology, PersonaDaemonPaths, SocketMode,
};
use persona::launch::{
    CommandArgument, CommandResolutionFailure, ComponentCommand, ComponentCommandCatalog,
    ComponentCommandEntry, ComponentCommandEntryInput, ComponentCommandInput,
    ComponentCommandOverride, ComponentCommandOverrideInput, ComponentCommandResolver,
    EngineLaunchConfiguration, EnvironmentVariable, EnvironmentVariableInput,
    EnvironmentVariableName, EnvironmentVariableValue, ExecutablePath,
    ReadCommandResolutionAttemptCount, ResolveComponentCommands, ResolvedComponentCommands,
};
use signal_persona::origin::{EngineIdentifier, OwnerIdentity, UnixUserIdentifier};

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

    fn closure_command(component: EngineComponent) -> ComponentCommand {
        let command_name = match component {
            EngineComponent::Mind => "persona-mind-daemon",
            EngineComponent::Orchestrate => "persona-orchestrate-daemon",
            EngineComponent::Router => "persona-router-daemon",
            EngineComponent::System => "persona-system-daemon",
            EngineComponent::Harness => "persona-harness-daemon",
            EngineComponent::Terminal => "persona-terminal-daemon",
            EngineComponent::Message => "persona-message-daemon",
            EngineComponent::Introspect => "persona-introspect-daemon",
            EngineComponent::Spirit => "persona-spirit-daemon",
        };
        ComponentCommand::executable(ExecutablePath::new(format!(
            "{}/nix-closure/{command_name}/bin/{command_name}",
            std::env::temp_dir().display()
        )))
    }

    fn routed_command_with_environment() -> ComponentCommand {
        ComponentCommand::from_input(ComponentCommandInput {
            executable_path: ExecutablePath::new(
                "/test-overrides/router/bin/persona-router-daemon",
            ),
            arguments: vec![CommandArgument::new("--serve-engine")],
            environment: vec![EnvironmentVariable::from_input(EnvironmentVariableInput {
                name: EnvironmentVariableName::new("PERSONA_ENGINE_ID"),
                value: EnvironmentVariableValue::new("engine-gamma"),
            })],
        })
    }

    fn command_entry(component: EngineComponent) -> ComponentCommandEntry {
        ComponentCommandEntry::from_input(ComponentCommandEntryInput {
            component,
            command: Self::closure_command(component),
        })
    }

    fn command_catalog() -> ComponentCommandCatalog {
        ComponentCommandCatalog::from_entries(
            EngineComponent::prototype_supervised_components()
                .into_iter()
                .map(Self::command_entry)
                .collect(),
        )
    }

    fn message_router_command_catalog() -> ComponentCommandCatalog {
        ComponentCommandCatalog::from_entries_for_components(
            EngineComponent::message_router_components()
                .into_iter()
                .map(Self::command_entry)
                .collect(),
            EngineComponent::message_router_components(),
        )
    }

    fn three_harness_chain_command_catalog() -> ComponentCommandCatalog {
        ComponentCommandCatalog::from_entries_for_components(
            EngineTopology::ThreeHarnessChain
                .components()
                .iter()
                .copied()
                .map(Self::command_entry)
                .collect(),
            EngineTopology::ThreeHarnessChain
                .components()
                .iter()
                .copied(),
        )
    }

    fn mind_orchestrate_command_catalog() -> ComponentCommandCatalog {
        ComponentCommandCatalog::from_entries_for_components(
            EngineTopology::MindOrchestrate
                .components()
                .iter()
                .copied()
                .map(Self::command_entry)
                .collect(),
            EngineTopology::MindOrchestrate.components().iter().copied(),
        )
    }

    async fn resolver_result(
        catalog: ComponentCommandCatalog,
        configuration: EngineLaunchConfiguration,
    ) -> std::result::Result<ResolvedComponentCommands, CommandResolutionFailure> {
        let resolver = ComponentCommandResolver::spawn(ComponentCommandResolver::new(catalog));
        let resolved = match resolver
            .ask(ResolveComponentCommands::new(configuration))
            .await
        {
            Ok(commands) => Ok(commands),
            Err(SendError::HandlerError(failure)) => Err(failure),
            Err(error) => panic!("resolver actor transport failed: {error:?}"),
        };
        let count = resolver
            .ask(ReadCommandResolutionAttemptCount)
            .await
            .expect("resolver actor count replies");
        assert_eq!(count, 1);
        resolved
    }

    async fn resolved_commands() -> ResolvedComponentCommands {
        Self::resolver_result(Self::command_catalog(), EngineLaunchConfiguration::empty())
            .await
            .expect("default component commands resolve")
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
    let layout = paths.engine_layout(EngineIdentifier::new("engine-alpha"));

    assert!(TemporaryEngineRoot::contains(
        layout.state_dir(),
        "state/engine-alpha"
    ));
    assert!(TemporaryEngineRoot::contains(
        layout.run_dir(),
        "run/engine-alpha"
    ));
    assert!(layout.manager_store().ends_with("manager.sema"));
    assert!(layout.manager_socket().ends_with("persona.sock"));

    let router = layout
        .component(EngineComponent::Router)
        .expect("router component layout exists");
    assert!(router.state_path().ends_with("router.sema"));
    assert!(router.envelope_path().ends_with("router.envelope"));
    assert!(router.domain_socket().path().ends_with("router.sock"));
    assert!(
        router
            .supervision_socket()
            .path()
            .ends_with("router.supervision.sock")
    );
    assert!(TemporaryEngineRoot::contains(
        router.state_path(),
        "state/engine-alpha"
    ));
    assert!(TemporaryEngineRoot::contains(
        router.envelope_path(),
        "run/engine-alpha"
    ));
    assert!(TemporaryEngineRoot::contains(
        router.domain_socket().path(),
        "run/engine-alpha"
    ));
    assert!(TemporaryEngineRoot::contains(
        router.supervision_socket().path(),
        "run/engine-alpha"
    ));
}

#[test]
fn constraint_engine_layout_can_select_message_router_topology() {
    let root = TemporaryEngineRoot::new("message-router-layout");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout_with_topology(
        EngineIdentifier::new("engine-message-router"),
        EngineTopology::MessageRouter,
    );

    assert_eq!(layout.components().len(), 2);
    assert!(layout.component(EngineComponent::Message).is_some());
    assert!(layout.component(EngineComponent::Router).is_some());
    assert!(layout.component(EngineComponent::Mind).is_none());
}

#[test]
fn constraint_engine_layout_can_select_mind_orchestrate_topology() {
    let root = TemporaryEngineRoot::new("mind-orchestrate-layout");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout_with_topology(
        EngineIdentifier::new("engine-mind-orchestrate"),
        EngineTopology::MindOrchestrate,
    );

    assert_eq!(layout.components().len(), 2);
    assert!(layout.component(EngineComponent::Mind).is_some());
    assert!(layout.component(EngineComponent::Orchestrate).is_some());
    assert!(layout.component(EngineComponent::Router).is_none());
}

#[test]
fn constraint_three_harness_chain_topology_allocates_distinct_instances() {
    let root = TemporaryEngineRoot::new("three-harness-chain-layout");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout_with_topology(
        EngineIdentifier::new("engine-three-harness-chain"),
        EngineTopology::ThreeHarnessChain,
    );

    let harness_count = layout
        .components()
        .iter()
        .filter(|layout| layout.component() == EngineComponent::Harness)
        .count();
    let terminal_count = layout
        .components()
        .iter()
        .filter(|layout| layout.component() == EngineComponent::Terminal)
        .count();
    let mut socket_paths = layout
        .components()
        .iter()
        .map(|layout| layout.domain_socket().path().to_path_buf())
        .collect::<Vec<_>>();
    socket_paths.sort();
    socket_paths.dedup();

    assert_eq!(layout.components().len(), 8);
    assert_eq!(harness_count, 3);
    assert_eq!(terminal_count, 3);
    assert!(
        layout
            .component_instance(&ComponentInstanceName::new("initiator"))
            .is_some()
    );
    assert!(
        layout
            .component_instance(&ComponentInstanceName::new("responder"))
            .is_some()
    );
    assert!(
        layout
            .component_instance(&ComponentInstanceName::new("reviewer"))
            .is_some()
    );
    assert!(
        layout
            .component_instance(&ComponentInstanceName::new("initiator-terminal"))
            .is_some()
    );
    assert_eq!(
        socket_paths.len(),
        layout.components().len(),
        "each component instance must own a distinct domain socket"
    );
}

#[test]
fn constraint_prototype_supervision_includes_introspect_but_delivery_does_not() {
    assert_eq!(EngineComponent::operational_delivery_components().len(), 6);
    assert_eq!(EngineComponent::prototype_supervised_components().len(), 8);
    assert!(
        !EngineComponent::operational_delivery_components().contains(&EngineComponent::Introspect)
    );
    assert!(
        EngineComponent::prototype_supervised_components().contains(&EngineComponent::Introspect)
    );
    assert!(!EngineComponent::operational_delivery_components().contains(&EngineComponent::Spirit));
    assert!(EngineComponent::prototype_supervised_components().contains(&EngineComponent::Spirit));
}

#[test]
fn constraint_engine_layout_assigns_socket_modes_by_component_boundary() {
    let root = TemporaryEngineRoot::new("socket-mode");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout(EngineIdentifier::new("engine-beta"));

    for component in [
        EngineComponent::Mind,
        EngineComponent::Router,
        EngineComponent::System,
        EngineComponent::Harness,
        EngineComponent::Terminal,
        EngineComponent::Introspect,
        EngineComponent::Spirit,
    ] {
        let socket = layout
            .component(component)
            .expect("component layout exists")
            .domain_socket();
        assert_eq!(socket.mode(), SocketMode::internal_component());
        assert_eq!(socket.mode().as_octal(), 0o600);
        let supervision_socket = layout
            .component(component)
            .expect("component layout exists")
            .supervision_socket();
        assert_eq!(supervision_socket.mode(), SocketMode::internal_component());
        assert_eq!(supervision_socket.mode().as_octal(), 0o600);
    }

    let message = layout
        .component(EngineComponent::Message)
        .expect("message layout exists");
    assert_eq!(
        message.domain_socket().mode(),
        SocketMode::message_ingress()
    );
    assert_eq!(message.domain_socket().mode().as_octal(), 0o660);
    assert_eq!(
        message.supervision_socket().mode(),
        SocketMode::internal_component()
    );
    assert_eq!(message.supervision_socket().mode().as_octal(), 0o600);
}

#[test]
fn constraint_orchestrate_component_uses_internal_socket_modes() {
    let root = TemporaryEngineRoot::new("orchestrate-socket-mode");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout_with_topology(
        EngineIdentifier::new("engine-mind-orchestrate"),
        EngineTopology::MindOrchestrate,
    );
    let orchestrate = layout
        .component(EngineComponent::Orchestrate)
        .expect("orchestrate layout exists");

    assert_eq!(
        orchestrate.domain_socket().mode(),
        SocketMode::internal_component()
    );
    assert_eq!(orchestrate.domain_socket().mode().as_octal(), 0o600);
    assert_eq!(
        orchestrate.supervision_socket().mode(),
        SocketMode::internal_component()
    );
    assert_eq!(orchestrate.supervision_socket().mode().as_octal(), 0o600);
}

#[tokio::test]
async fn constraint_spawn_envelope_carries_component_paths_and_peer_sockets() {
    let root = TemporaryEngineRoot::new("spawn-envelope");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let owner_identity = OwnerIdentity::UnixUser(UnixUserIdentifier::new(4242));
    let layout = paths.engine_layout_with_owner(
        EngineIdentifier::new("engine-gamma"),
        owner_identity.clone(),
    );
    let resolved_commands = TemporaryEngineRoot::resolved_commands().await;
    let envelope = layout
        .spawn_envelope(EngineComponent::Router, &resolved_commands)
        .expect("router spawn envelope exists");

    assert_eq!(envelope.engine().as_str(), "engine-gamma");
    assert_eq!(envelope.owner_identity(), &owner_identity);
    assert_eq!(envelope.component(), EngineComponent::Router);
    assert_eq!(envelope.component_instance().as_str(), "router");
    assert!(envelope.state_dir().ends_with("engine-gamma"));
    assert!(envelope.state_path().ends_with("router.sema"));
    assert!(envelope.domain_socket_path().ends_with("router.sock"));
    assert!(
        envelope
            .supervision_socket_path()
            .ends_with("router.supervision.sock")
    );
    assert!(envelope.envelope_path().ends_with("router.envelope"));
    assert!(envelope.manager_socket().ends_with("persona.sock"));
    assert_eq!(envelope.domain_socket_mode().as_octal(), 0o600);
    assert_eq!(envelope.supervision_socket_mode().as_octal(), 0o600);
    assert_eq!(envelope.peers().len(), 7);
    assert!(
        envelope
            .peers()
            .iter()
            .any(|peer| peer.component() == EngineComponent::Mind
                && peer.domain_socket_path().ends_with("mind.sock"))
    );
    assert!(
        envelope
            .peers()
            .iter()
            .any(|peer| peer.component() == EngineComponent::Message
                && peer.domain_socket_path().ends_with("message.sock"))
    );
    assert!(
        envelope
            .peers()
            .iter()
            .any(|peer| peer.component() == EngineComponent::Introspect
                && peer.domain_socket_path().ends_with("introspect.sock"))
    );
    assert!(
        envelope
            .peers()
            .iter()
            .any(|peer| peer.component() == EngineComponent::Spirit
                && peer.domain_socket_path().ends_with("spirit.sock"))
    );

    let signal_envelope = envelope.signal_spawn_envelope();
    assert_eq!(signal_envelope.engine_identifier.as_str(), "engine-gamma");
    assert_eq!(
        signal_envelope.component_kind,
        signal_persona::ComponentKind::Router
    );
    assert_eq!(
        signal_envelope.component_name,
        signal_persona::origin::ComponentName::Router
    );
    assert_eq!(signal_envelope.owner_identity, owner_identity);
    assert!(
        signal_envelope
            .state_dir
            .as_str()
            .ends_with("state/engine-gamma")
    );
    assert!(
        signal_envelope
            .domain_socket_path
            .as_str()
            .ends_with("router.sock")
    );
    assert_eq!(signal_envelope.domain_socket_mode.into_u32(), 0o600);
    assert!(
        signal_envelope
            .engine_management_socket_path
            .as_str()
            .ends_with("router.supervision.sock")
    );
    assert_eq!(
        signal_envelope.engine_management_socket_mode.into_u32(),
        0o600
    );
    assert_eq!(signal_envelope.peer_sockets.len(), 7);

    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&signal_envelope)
        .expect("encode signal spawn envelope");
    let recovered = rkyv::from_bytes::<signal_persona::SpawnEnvelope, rkyv::rancor::Error>(&bytes)
        .expect("decode signal spawn envelope");
    assert_eq!(recovered, signal_envelope);
}

#[tokio::test]
async fn constraint_mind_orchestrate_topology_spawn_envelope_has_one_peer_socket() {
    let root = TemporaryEngineRoot::new("mind-orchestrate-envelope");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout_with_topology(
        EngineIdentifier::new("engine-mind-orchestrate"),
        EngineTopology::MindOrchestrate,
    );
    let resolved_commands = TemporaryEngineRoot::resolver_result(
        TemporaryEngineRoot::mind_orchestrate_command_catalog(),
        EngineLaunchConfiguration::empty(),
    )
    .await
    .expect("mind-orchestrate commands resolve");
    let envelope = layout
        .spawn_envelope(EngineComponent::Orchestrate, &resolved_commands)
        .expect("orchestrate spawn envelope exists");

    assert_eq!(envelope.component(), EngineComponent::Orchestrate);
    assert_eq!(envelope.component_instance().as_str(), "orchestrate");
    assert!(envelope.state_path().ends_with("orchestrate.sema"));
    assert!(envelope.domain_socket_path().ends_with("orchestrate.sock"));
    assert!(
        envelope
            .supervision_socket_path()
            .ends_with("orchestrate.supervision.sock")
    );
    assert_eq!(envelope.peers().len(), 1);
    assert_eq!(envelope.peers()[0].component(), EngineComponent::Mind);
    assert!(
        envelope.peers()[0]
            .domain_socket_path()
            .ends_with("mind.sock")
    );

    let signal_envelope = envelope.signal_spawn_envelope();
    assert_eq!(
        signal_envelope.component_kind,
        signal_persona::ComponentKind::Orchestrate
    );
    assert_eq!(
        signal_envelope.component_name,
        signal_persona::origin::ComponentName::Orchestrate
    );
}

#[tokio::test]
async fn constraint_message_router_topology_spawn_envelope_has_one_peer_socket() {
    let root = TemporaryEngineRoot::new("message-router-envelope");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout_with_topology(
        EngineIdentifier::new("engine-message-router"),
        EngineTopology::MessageRouter,
    );
    let resolved_commands = TemporaryEngineRoot::resolver_result(
        TemporaryEngineRoot::message_router_command_catalog(),
        EngineLaunchConfiguration::empty(),
    )
    .await
    .expect("message-router commands resolve");
    let envelope = layout
        .spawn_envelope(EngineComponent::Message, &resolved_commands)
        .expect("message spawn envelope exists");

    assert_eq!(envelope.peers().len(), 1);
    assert_eq!(envelope.peers()[0].component(), EngineComponent::Router);
    assert!(
        envelope.peers()[0]
            .domain_socket_path()
            .ends_with("router.sock")
    );
}

#[tokio::test]
async fn constraint_three_harness_chain_spawn_envelope_pairs_harness_with_named_terminal() {
    let root = TemporaryEngineRoot::new("three-harness-chain-envelope");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout_with_topology(
        EngineIdentifier::new("engine-three-harness-chain"),
        EngineTopology::ThreeHarnessChain,
    );
    let resolved_commands = TemporaryEngineRoot::resolver_result(
        TemporaryEngineRoot::three_harness_chain_command_catalog(),
        EngineLaunchConfiguration::empty(),
    )
    .await
    .expect("three-harness-chain commands resolve");

    let responder = layout
        .spawn_envelope_for_instance(&ComponentInstanceName::new("responder"), &resolved_commands)
        .expect("responder harness spawn envelope exists");

    assert_eq!(responder.component(), EngineComponent::Harness);
    assert_eq!(responder.component_instance().as_str(), "responder");
    assert!(responder.domain_socket_path().ends_with("responder.sock"));
    assert!(
        responder
            .supervision_socket_path()
            .ends_with("responder.supervision.sock")
    );
    assert!(
        responder
            .peers()
            .iter()
            .any(|peer| peer.instance_name().as_str() == "responder-terminal"
                && peer.component() == EngineComponent::Terminal),
        "responder harness must see its paired terminal instance"
    );
}

#[tokio::test]
async fn constraint_spawn_envelope_carries_resolved_component_command() {
    let root = TemporaryEngineRoot::new("spawn-envelope-command");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout(EngineIdentifier::new("engine-gamma"));
    let override_command = TemporaryEngineRoot::routed_command_with_environment();
    let resolved_commands = TemporaryEngineRoot::resolver_result(
        TemporaryEngineRoot::command_catalog(),
        EngineLaunchConfiguration::from_overrides(vec![ComponentCommandOverride::from_input(
            ComponentCommandOverrideInput {
                component: EngineComponent::Router,
                command: override_command.clone(),
            },
        )]),
    )
    .await
    .expect("override command resolves");
    let envelope = layout
        .spawn_envelope(EngineComponent::Router, &resolved_commands)
        .expect("router spawn envelope exists");

    assert_eq!(envelope.command(), &override_command);
    assert_eq!(
        envelope.command().executable_path().as_str(),
        "/test-overrides/router/bin/persona-router-daemon"
    );
    assert_eq!(envelope.command().arguments()[0].as_str(), "--serve-engine");
    assert_eq!(
        envelope.command().environment()[0].name().as_str(),
        "PERSONA_ENGINE_ID"
    );
    assert_eq!(
        envelope.command().environment()[0].value().as_str(),
        "engine-gamma"
    );
}

#[test]
fn constraint_engine_layout_prepares_only_engine_scoped_directories() {
    let root = TemporaryEngineRoot::new("prepare");
    let paths = PersonaDaemonPaths::new(root.state_root(), root.run_root());
    let layout = paths.engine_layout(EngineIdentifier::new("engine-delta"));
    let prepared = layout
        .prepare_directories()
        .expect("engine directories are prepared");

    assert!(prepared.state_dir().is_dir());
    assert!(prepared.run_dir().is_dir());
    assert!(prepared.state_dir().ends_with("engine-delta"));
    assert!(prepared.run_dir().ends_with("engine-delta"));
    assert!(!layout.manager_store().exists());
}

#[tokio::test]
async fn constraint_component_commands_resolve_from_nix_closure() {
    let resolved = TemporaryEngineRoot::resolver_result(
        TemporaryEngineRoot::command_catalog(),
        EngineLaunchConfiguration::empty(),
    )
    .await
    .expect("all prototype-supervised commands resolve from explicit catalog");

    assert_eq!(
        resolved.entries().len(),
        EngineComponent::prototype_supervised_components().len()
    );
    let router = resolved
        .command_for(EngineComponent::Router)
        .expect("router command exists");
    assert!(
        router
            .executable_path()
            .as_str()
            .ends_with("/persona-router-daemon/bin/persona-router-daemon")
    );
}

#[tokio::test]
async fn constraint_launch_config_overrides_one_component_command() {
    let override_command = ComponentCommand::executable(ExecutablePath::new(
        "/test-overrides/router/bin/persona-router-daemon",
    ));
    let configuration =
        EngineLaunchConfiguration::from_overrides(vec![ComponentCommandOverride::from_input(
            ComponentCommandOverrideInput {
                component: EngineComponent::Router,
                command: override_command.clone(),
            },
        )]);
    let resolved =
        TemporaryEngineRoot::resolver_result(TemporaryEngineRoot::command_catalog(), configuration)
            .await
            .expect("override configuration resolves");

    assert_eq!(
        resolved.command_for(EngineComponent::Router),
        Some(&override_command)
    );
    assert_ne!(
        resolved.command_for(EngineComponent::Mind),
        Some(&override_command)
    );
}

#[tokio::test]
async fn constraint_component_command_resolution_fails_without_host_path_fallback() {
    let catalog = ComponentCommandCatalog::from_entries(
        EngineComponent::prototype_supervised_components()
            .into_iter()
            .filter(|component| *component != EngineComponent::Router)
            .map(TemporaryEngineRoot::command_entry)
            .collect(),
    );
    let failure = TemporaryEngineRoot::resolver_result(catalog, EngineLaunchConfiguration::empty())
        .await
        .expect_err("missing router command is rejected");

    assert_eq!(
        failure,
        CommandResolutionFailure::MissingRequiredCommand {
            component: EngineComponent::Router
        }
    );
}

#[tokio::test]
async fn constraint_launch_config_rejects_duplicate_component_overrides() {
    let first = ComponentCommand::executable(ExecutablePath::new(
        "/test-overrides/router-one/bin/persona-router-daemon",
    ));
    let second = ComponentCommand::executable(ExecutablePath::new(
        "/test-overrides/router-two/bin/persona-router-daemon",
    ));
    let configuration = EngineLaunchConfiguration::from_overrides(vec![
        ComponentCommandOverride::from_input(ComponentCommandOverrideInput {
            component: EngineComponent::Router,
            command: first,
        }),
        ComponentCommandOverride::from_input(ComponentCommandOverrideInput {
            component: EngineComponent::Router,
            command: second,
        }),
    ]);
    let failure =
        TemporaryEngineRoot::resolver_result(TemporaryEngineRoot::command_catalog(), configuration)
            .await
            .expect_err("duplicate override is rejected");

    assert_eq!(
        failure,
        CommandResolutionFailure::DuplicateOverrideCommand {
            component: EngineComponent::Router
        }
    );
}
