use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use kameo::actor::{ActorRef, Spawn};
use kameo::error::SendError;
use nota_codec::{Decoder, NotaDecode};
use persona::direct_process::{
    DirectProcessFailure, DirectProcessLauncher, ExitNotifier, LaunchComponent,
    LaunchComponentReceipt, ReadLauncherSnapshot, StopComponentProcess, StopComponentReceipt,
};
use persona::engine::{
    ComponentInstanceName, ComponentSpawnEnvelope, EngineComponent, EngineTopology,
    PersonaDaemonPaths,
};
use persona::engine_event::EngineEventBody;
use persona::launch::{
    ComponentCommand, ComponentCommandCatalog, ComponentCommandEntry, ComponentCommandEntryInput,
    ComponentCommandInput, ComponentCommandResolver, EngineLaunchConfiguration,
    EnvironmentVariable, EnvironmentVariableInput, EnvironmentVariableName,
    EnvironmentVariableValue, ExecutablePath, ResolveComponentCommands, ResolvedComponentCommands,
};
use persona::manager::{EngineManager, HandleEngineRequest};
use persona::manager_store::{ManagerStore, ManagerStoreLocation, ReadEngineEvents};
use signal_persona::engine::{Operation as EngineRequest, Reply as EngineReply};
use signal_persona::{EngineStatusScope, Query};
use signal_persona_auth::EngineId;
use signal_persona_harness::{HarnessDaemonConfiguration, HarnessKind};
use signal_persona_message::MessageDaemonConfiguration;
use signal_persona_router::{
    EndpointKind, RouterBootstrapDocument, RouterBootstrapOperation, RouterDaemonConfiguration,
};
use signal_persona_terminal::TerminalDaemonConfiguration;

struct DirectProcessFixture {
    root: PathBuf,
}

impl DirectProcessFixture {
    fn new(name: &str) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "persona-direct-process-{name}-{}-{now}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("fixture root created");
        Self { root }
    }

    fn state_root(&self) -> PathBuf {
        self.root.join("state")
    }

    fn run_root(&self) -> PathBuf {
        self.root.join("run")
    }

    fn child_pid_file(&self) -> PathBuf {
        self.root.join("child.pid")
    }

    fn envelope_capture_file(&self) -> PathBuf {
        self.root.join("spawn-envelope.env")
    }

    fn script_path(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.sh"))
    }

    fn write_script(&self, name: &str, body: &str) -> String {
        let path = self.script_path(name);
        std::fs::write(&path, body).expect("test script writes");
        let mut permissions = std::fs::metadata(&path)
            .expect("test script metadata")
            .permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o700);
        std::fs::set_permissions(&path, permissions).expect("test script permissions set");
        path.to_string_lossy().into_owned()
    }

    fn short_running_command(&self) -> ComponentCommand {
        let script = self.write_script("short-running", "#!/bin/sh\nsleep 0.25\n");
        // Exits cleanly after ~250ms so the natural-exit observer fires
        // while the launcher is still alive to record the event.
        ComponentCommand::from_input(ComponentCommandInput {
            executable_path: ExecutablePath::new(script),
            arguments: Vec::new(),
            environment: Vec::new(),
        })
    }

    fn long_running_command(&self) -> ComponentCommand {
        let script = self.write_script(
            "long-running",
            "#!/bin/sh\ntrap 'exit 0' TERM\nwhile true; do sleep 1; done\n",
        );
        ComponentCommand::from_input(ComponentCommandInput {
            executable_path: ExecutablePath::new(script),
            arguments: Vec::new(),
            environment: Vec::new(),
        })
    }

    fn process_group_command(&self) -> ComponentCommand {
        let script = self.write_script(
            "process-group",
            "#!/bin/sh\ntrap 'exit 0' TERM\n(trap 'exit 0' TERM; while true; do sleep 1; done) &\nchild=\"$!\"\necho \"$child\" > \"$PERSONA_TEST_CHILD_PID_FILE\"\nwait \"$child\" 2>/dev/null || true\n",
        );
        ComponentCommand::from_input(ComponentCommandInput {
            executable_path: ExecutablePath::new(script),
            arguments: Vec::new(),
            environment: vec![EnvironmentVariable::from_input(EnvironmentVariableInput {
                name: EnvironmentVariableName::new("PERSONA_TEST_CHILD_PID_FILE"),
                value: EnvironmentVariableValue::new(
                    self.child_pid_file().to_string_lossy().into_owned(),
                ),
            })],
        })
    }

    fn envelope_capture_command(&self) -> ComponentCommand {
        let script = self.write_script(
            "envelope-capture",
            "#!/bin/sh\n{\n  printf 'engine=%s\\n' \"$PERSONA_ENGINE_ID\";\n  printf 'component=%s\\n' \"$PERSONA_COMPONENT\";\n  printf 'state=%s\\n' \"$PERSONA_STATE_PATH\";\n  printf 'domain_socket=%s\\n' \"$PERSONA_DOMAIN_SOCKET_PATH\";\n  printf 'supervision_socket=%s\\n' \"$PERSONA_SUPERVISION_SOCKET_PATH\";\n  printf 'spawn_envelope=%s\\n' \"$PERSONA_SPAWN_ENVELOPE\";\n  printf 'manager_socket=%s\\n' \"$PERSONA_MANAGER_SOCKET\";\n  printf 'domain_mode=%s\\n' \"$PERSONA_DOMAIN_SOCKET_MODE\";\n  printf 'supervision_mode=%s\\n' \"$PERSONA_SUPERVISION_SOCKET_MODE\";\n  printf 'peer_count=%s\\n' \"$PERSONA_PEER_SOCKET_COUNT\";\n  printf 'peer_0_component=%s\\n' \"$PERSONA_PEER_0_COMPONENT\";\n  printf 'peer_0_socket=%s\\n' \"$PERSONA_PEER_0_SOCKET_PATH\";\n} > \"$PERSONA_TEST_ENVELOPE_FILE\";\nexec sleep 3600\n",
        );
        ComponentCommand::from_input(ComponentCommandInput {
            executable_path: ExecutablePath::new(script),
            arguments: Vec::new(),
            environment: vec![EnvironmentVariable::from_input(EnvironmentVariableInput {
                name: EnvironmentVariableName::new("PERSONA_TEST_ENVELOPE_FILE"),
                value: EnvironmentVariableValue::new(
                    self.envelope_capture_file().to_string_lossy().into_owned(),
                ),
            })],
        })
    }

    fn command_catalog(&self) -> ComponentCommandCatalog {
        ComponentCommandCatalog::from_entries(
            EngineComponent::prototype_supervised_components()
                .into_iter()
                .map(|component| {
                    ComponentCommandEntry::from_input(ComponentCommandEntryInput {
                        component,
                        command: self.long_running_command(),
                    })
                })
                .collect(),
        )
    }

    fn command_catalog_for_topology(&self, topology: EngineTopology) -> ComponentCommandCatalog {
        ComponentCommandCatalog::from_entries_for_components(
            topology
                .components()
                .iter()
                .copied()
                .map(|component| {
                    ComponentCommandEntry::from_input(ComponentCommandEntryInput {
                        component,
                        command: self.long_running_command(),
                    })
                })
                .collect(),
            topology.components().iter().copied(),
        )
    }

    async fn resolved_commands(&self) -> ResolvedComponentCommands {
        let resolver =
            ComponentCommandResolver::spawn(ComponentCommandResolver::new(self.command_catalog()));
        resolver
            .ask(ResolveComponentCommands::new(
                EngineLaunchConfiguration::empty(),
            ))
            .await
            .expect("component commands resolve")
    }

    async fn resolved_commands_for_topology(
        &self,
        topology: EngineTopology,
    ) -> ResolvedComponentCommands {
        let resolver = ComponentCommandResolver::spawn(ComponentCommandResolver::new(
            self.command_catalog_for_topology(topology),
        ));
        resolver
            .ask(ResolveComponentCommands::new(
                EngineLaunchConfiguration::empty(),
            ))
            .await
            .expect("component commands resolve")
    }

    async fn envelope(&self, component: EngineComponent) -> ComponentSpawnEnvelope {
        let paths = PersonaDaemonPaths::new(self.state_root(), self.run_root());
        let layout = paths.engine_layout(EngineId::new("engine-direct-process"));
        layout
            .prepare_directories()
            .expect("engine directories prepared");
        layout
            .spawn_envelope(component, &self.resolved_commands().await)
            .expect("component spawn envelope exists")
    }

    async fn envelope_with_command(
        &self,
        component: EngineComponent,
        command: ComponentCommand,
    ) -> ComponentSpawnEnvelope {
        let paths = PersonaDaemonPaths::new(self.state_root(), self.run_root());
        let layout = paths.engine_layout(EngineId::new("engine-direct-process"));
        layout
            .prepare_directories()
            .expect("engine directories prepared");
        let mut entries: Vec<ComponentCommandEntry> =
            EngineComponent::prototype_supervised_components()
                .into_iter()
                .map(|entry_component| {
                    let entry_command = if entry_component == component {
                        command.clone()
                    } else {
                        self.long_running_command()
                    };
                    ComponentCommandEntry::from_input(ComponentCommandEntryInput {
                        component: entry_component,
                        command: entry_command,
                    })
                })
                .collect();
        entries.sort_by_key(|entry| entry.component().as_str());
        let catalog = ComponentCommandCatalog::from_entries(entries);
        let resolver = ComponentCommandResolver::spawn(ComponentCommandResolver::new(catalog));
        let resolved = resolver
            .ask(ResolveComponentCommands::new(
                EngineLaunchConfiguration::empty(),
            ))
            .await
            .expect("component commands resolve");
        layout
            .spawn_envelope(component, &resolved)
            .expect("component spawn envelope exists")
    }

    async fn launch(
        launcher: &ActorRef<DirectProcessLauncher>,
        envelope: ComponentSpawnEnvelope,
    ) -> Result<LaunchComponentReceipt, DirectProcessFailure> {
        match launcher.ask(LaunchComponent::new(envelope)).await {
            Ok(receipt) => Ok(receipt),
            Err(SendError::HandlerError(failure)) => Err(failure),
            Err(error) => panic!("launcher actor transport failed: {error:?}"),
        }
    }

    async fn stop(
        launcher: &ActorRef<DirectProcessLauncher>,
        component: EngineComponent,
    ) -> Result<StopComponentReceipt, DirectProcessFailure> {
        Self::stop_instance(launcher, ComponentInstanceName::from_component(component)).await
    }

    async fn stop_instance(
        launcher: &ActorRef<DirectProcessLauncher>,
        component_instance: ComponentInstanceName,
    ) -> Result<StopComponentReceipt, DirectProcessFailure> {
        match launcher
            .ask(StopComponentProcess::for_instance(component_instance))
            .await
        {
            Ok(receipt) => Ok(receipt),
            Err(SendError::HandlerError(failure)) => Err(failure),
            Err(error) => panic!("launcher actor transport failed: {error:?}"),
        }
    }

    fn process_is_alive(process: u32) -> bool {
        let result = unsafe { libc::kill(process as i32, 0) };
        if result == 0 {
            return !Self::process_is_zombie(process);
        }
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    fn process_is_zombie(process: u32) -> bool {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{process}/stat")) else {
            return false;
        };
        let Some(after_name) = stat.rsplit_once(") ") else {
            return false;
        };
        after_name.1.starts_with("Z ")
    }

    async fn wait_until_process_exits(process: u32) {
        for _attempt in 0..40 {
            if !Self::process_is_alive(process) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }

    async fn read_child_process(&self) -> u32 {
        for _attempt in 0..40 {
            if let Ok(text) = std::fs::read_to_string(self.child_pid_file()) {
                let trimmed = text.trim();
                if let Ok(process) = trimmed.parse() {
                    return process;
                }
                if !trimmed.is_empty() {
                    panic!("child pid is numeric: {trimmed:?}");
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("child pid file was not written with a numeric pid");
    }

    async fn read_envelope_capture(&self) -> String {
        for _attempt in 0..40 {
            if let Ok(text) = std::fs::read_to_string(self.envelope_capture_file())
                && text.contains("peer_count=")
                && text.contains("peer_0_socket=")
                && text.contains("supervision_socket=")
                && text.contains("spawn_envelope=")
            {
                return text;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("spawn envelope capture file was not fully written");
    }
}

impl Drop for DirectProcessFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn constraint_three_harness_chain_router_launch_writes_bootstrap_for_named_harnesses() {
    let fixture = DirectProcessFixture::new("three-harness-router-bootstrap");
    let launcher = DirectProcessLauncher::spawn(DirectProcessLauncher::new());
    let paths = PersonaDaemonPaths::new(fixture.state_root(), fixture.run_root());
    let layout = paths.engine_layout_with_topology(
        EngineId::new("engine-three-harness-router-bootstrap"),
        EngineTopology::ThreeHarnessChain,
    );
    layout
        .prepare_directories()
        .expect("engine directories prepared");
    let resolved = fixture
        .resolved_commands_for_topology(EngineTopology::ThreeHarnessChain)
        .await;
    let envelope = layout
        .spawn_envelope_for_instance(&ComponentInstanceName::new("router"), &resolved)
        .expect("router spawn envelope exists");
    let envelope_path = envelope.envelope_path().to_path_buf();

    DirectProcessFixture::launch(&launcher, envelope)
        .await
        .expect("router component launches");

    let configuration_path = envelope_path.with_file_name("router-daemon.nota");
    let configuration_text =
        std::fs::read_to_string(&configuration_path).expect("router configuration was written");
    let mut decoder = Decoder::new(&configuration_text);
    let configuration =
        RouterDaemonConfiguration::decode(&mut decoder).expect("router configuration decodes");
    let bootstrap_path = configuration
        .bootstrap_path
        .expect("three-harness topology writes a router bootstrap");
    let bootstrap_text =
        std::fs::read_to_string(bootstrap_path.as_str()).expect("router bootstrap was written");

    let bootstrap = RouterBootstrapDocument::from_nota_lines(&bootstrap_text)
        .expect("router bootstrap decodes through contract vocabulary");
    assert_eq!(bootstrap.operations().len(), 9);

    for name in ["initiator", "responder", "reviewer"] {
        assert!(
            bootstrap.operations().iter().any(|operation| matches!(
                operation,
                RouterBootstrapOperation::RegisterActor(registration)
                    if registration.actor.name.as_str() == name
                        && registration.actor.process == 0
                        && matches!(
                            &registration.actor.endpoint,
                            Some(endpoint)
                                if endpoint.kind == EndpointKind::HarnessSocket
                                    && endpoint.target.ends_with(format!("{name}.sock").as_str())
                        )
            )),
            "bootstrap did not register {name}: {bootstrap_text}"
        );
    }
    for (from, to) in [
        ("owner", "initiator"),
        ("owner", "responder"),
        ("owner", "reviewer"),
        ("initiator", "responder"),
        ("responder", "reviewer"),
        ("reviewer", "owner"),
    ] {
        assert!(
            bootstrap.operations().iter().any(|operation| matches!(
                operation,
                RouterBootstrapOperation::GrantDirectMessage(grant)
                    if grant.from.as_str() == from && grant.to.as_str() == to
            )),
            "bootstrap did not include grant {from}->{to}: {bootstrap_text}"
        );
    }

    DirectProcessFixture::stop(&launcher, EngineComponent::Router)
        .await
        .expect("router component stops");
    launcher.stop_gracefully().await.expect("launcher stops");
    let _shutdown_completion = launcher.wait_for_shutdown().await;
}

#[tokio::test]
async fn constraint_three_harness_chain_message_launch_writes_component_ingress_sockets() {
    let fixture = DirectProcessFixture::new("three-harness-message-ingress");
    let launcher = DirectProcessLauncher::spawn(DirectProcessLauncher::new());
    let paths = PersonaDaemonPaths::new(fixture.state_root(), fixture.run_root());
    let layout = paths.engine_layout_with_topology(
        EngineId::new("engine-three-harness-message-ingress"),
        EngineTopology::ThreeHarnessChain,
    );
    layout
        .prepare_directories()
        .expect("engine directories prepared");
    let resolved = fixture
        .resolved_commands_for_topology(EngineTopology::ThreeHarnessChain)
        .await;
    let envelope = layout
        .spawn_envelope_for_instance(&ComponentInstanceName::new("message"), &resolved)
        .expect("message spawn envelope exists");
    let envelope_path = envelope.envelope_path().to_path_buf();

    DirectProcessFixture::launch(&launcher, envelope)
        .await
        .expect("message component launches");

    let configuration_path = envelope_path.with_file_name("message-daemon.nota");
    let configuration_text =
        std::fs::read_to_string(&configuration_path).expect("message configuration was written");
    let mut decoder = Decoder::new(&configuration_text);
    let configuration =
        MessageDaemonConfiguration::decode(&mut decoder).expect("message configuration decodes");

    let ingresses = configuration.component_ingresses;
    assert_eq!(ingresses.len(), 3);
    for name in ["initiator", "responder", "reviewer"] {
        let ingress = ingresses
            .iter()
            .find(|entry| entry.origin.instance().as_str() == name)
            .unwrap_or_else(|| panic!("missing component ingress for {name}"));
        assert_eq!(
            ingress.origin.component(),
            signal_persona_auth::ComponentName::Harness
        );
        assert!(
            ingress
                .socket_path
                .as_str()
                .ends_with(&format!("message-ingress/{name}.sock")),
            "unexpected ingress socket path for {name}: {}",
            ingress.socket_path.as_str()
        );
        assert_eq!(ingress.socket_mode.into_u32(), 0o600);
    }

    DirectProcessFixture::stop(&launcher, EngineComponent::Message)
        .await
        .expect("message component stops");
    launcher.stop_gracefully().await.expect("launcher stops");
    let _shutdown_completion = launcher.wait_for_shutdown().await;
}

#[tokio::test]
async fn constraint_three_harness_chain_writes_instance_specific_daemon_configurations() {
    let fixture = DirectProcessFixture::new("three-harness-instance-configurations");
    let launcher = DirectProcessLauncher::spawn(DirectProcessLauncher::new());
    let paths = PersonaDaemonPaths::new(fixture.state_root(), fixture.run_root());
    let layout = paths.engine_layout_with_topology(
        EngineId::new("engine-three-harness-instance-configurations"),
        EngineTopology::ThreeHarnessChain,
    );
    layout
        .prepare_directories()
        .expect("engine directories prepared");
    let resolved = fixture
        .resolved_commands_for_topology(EngineTopology::ThreeHarnessChain)
        .await;
    let instance_names = [
        "message",
        "router",
        "initiator-terminal",
        "initiator",
        "responder-terminal",
        "responder",
        "reviewer-terminal",
        "reviewer",
    ];

    for instance_name in instance_names {
        let envelope = layout
            .spawn_envelope_for_instance(&ComponentInstanceName::new(instance_name), &resolved)
            .unwrap_or_else(|| panic!("spawn envelope exists for {instance_name}"));
        DirectProcessFixture::launch(&launcher, envelope)
            .await
            .unwrap_or_else(|error| panic!("{instance_name} component launches: {error:?}"));
    }

    let engine_run_root = fixture
        .run_root()
        .join("engine-three-harness-instance-configurations");
    let engine_state_root = fixture
        .state_root()
        .join("engine-three-harness-instance-configurations");

    for instance_name in instance_names {
        let path = engine_run_root.join(format!("{instance_name}-daemon.nota"));
        assert!(
            path.exists(),
            "missing instance-specific configuration: {}",
            path.display()
        );
    }

    let message_configuration_text =
        std::fs::read_to_string(engine_run_root.join("message-daemon.nota"))
            .expect("message configuration reads");
    let mut message_decoder = Decoder::new(&message_configuration_text);
    let message_configuration = MessageDaemonConfiguration::decode(&mut message_decoder)
        .expect("message configuration decodes");
    assert_eq!(message_configuration.component_ingresses.len(), 3);

    for agent_name in ["initiator", "responder", "reviewer"] {
        let ingress = message_configuration
            .component_ingresses
            .iter()
            .find(|entry| entry.origin.instance().as_str() == agent_name)
            .unwrap_or_else(|| panic!("message ingress exists for {agent_name}"));
        assert_eq!(
            ingress.origin.component(),
            signal_persona_auth::ComponentName::Harness
        );
        assert!(
            ingress
                .socket_path
                .as_str()
                .ends_with(&format!("message-ingress/{agent_name}.sock")),
            "message ingress path belongs to {agent_name}: {}",
            ingress.socket_path.as_str()
        );
        assert_eq!(ingress.socket_mode.into_u32(), 0o600);
    }

    for agent_name in ["initiator", "responder", "reviewer"] {
        let terminal_instance_name = format!("{agent_name}-terminal");
        let terminal_configuration_text = std::fs::read_to_string(
            engine_run_root.join(format!("{terminal_instance_name}-daemon.nota")),
        )
        .unwrap_or_else(|error| {
            panic!("terminal configuration reads for {terminal_instance_name}: {error}")
        });
        let mut terminal_decoder = Decoder::new(&terminal_configuration_text);
        let terminal_configuration = TerminalDaemonConfiguration::decode(&mut terminal_decoder)
            .unwrap_or_else(|error| {
                panic!("terminal configuration decodes for {terminal_instance_name}: {error:?}")
            });
        assert!(
            terminal_configuration
                .terminal_socket_path
                .as_str()
                .ends_with(&format!("{terminal_instance_name}.sock")),
            "terminal socket path belongs to {terminal_instance_name}: {}",
            terminal_configuration.terminal_socket_path.as_str()
        );
        assert_eq!(
            terminal_configuration.terminal_socket_mode.into_u32(),
            0o600
        );
        assert!(
            terminal_configuration
                .supervision_socket_path
                .as_str()
                .ends_with(&format!("{terminal_instance_name}.supervision.sock")),
            "terminal supervision socket belongs to {terminal_instance_name}: {}",
            terminal_configuration.supervision_socket_path.as_str()
        );
        assert_eq!(
            terminal_configuration.supervision_socket_mode.into_u32(),
            0o600
        );
        assert!(
            terminal_configuration
                .store_path
                .as_str()
                .ends_with(&format!("{terminal_instance_name}.redb")),
            "terminal store path belongs to {terminal_instance_name}: {}",
            terminal_configuration.store_path.as_str()
        );
        assert!(
            terminal_configuration
                .store_path
                .as_str()
                .starts_with(engine_state_root.to_string_lossy().as_ref()),
            "terminal store path stays in engine state root: {}",
            terminal_configuration.store_path.as_str()
        );

        let harness_configuration_text =
            std::fs::read_to_string(engine_run_root.join(format!("{agent_name}-daemon.nota")))
                .unwrap_or_else(|error| {
                    panic!("harness configuration reads for {agent_name}: {error}")
                });
        let mut harness_decoder = Decoder::new(&harness_configuration_text);
        let harness_configuration = HarnessDaemonConfiguration::decode(&mut harness_decoder)
            .unwrap_or_else(|error| {
                panic!("harness configuration decodes for {agent_name}: {error:?}")
            });
        assert_eq!(harness_configuration.harness_name.as_str(), agent_name);
        assert_eq!(harness_configuration.harness_kind, HarnessKind::Fixture);
        assert!(
            harness_configuration
                .harness_socket_path
                .as_str()
                .ends_with(&format!("{agent_name}.sock")),
            "harness socket path belongs to {agent_name}: {}",
            harness_configuration.harness_socket_path.as_str()
        );
        assert_eq!(harness_configuration.harness_socket_mode.into_u32(), 0o600);
        assert!(
            harness_configuration
                .supervision_socket_path
                .as_str()
                .ends_with(&format!("{agent_name}.supervision.sock")),
            "harness supervision socket belongs to {agent_name}: {}",
            harness_configuration.supervision_socket_path.as_str()
        );
        assert_eq!(
            harness_configuration.supervision_socket_mode.into_u32(),
            0o600
        );
        let terminal_socket_path = harness_configuration
            .terminal_socket_path
            .as_ref()
            .unwrap_or_else(|| panic!("harness {agent_name} has paired terminal socket"));
        assert!(
            terminal_socket_path
                .as_str()
                .ends_with(&format!("{terminal_instance_name}.sock")),
            "harness {agent_name} pairs with {terminal_instance_name}: {}",
            terminal_socket_path.as_str()
        );
    }

    assert!(
        !fixture
            .run_root()
            .join("engine-three-harness-instance-configurations")
            .join("terminal-daemon.nota")
            .exists(),
        "multi-terminal topology must not collapse terminal configurations into one shared file"
    );
    assert!(
        !fixture
            .run_root()
            .join("engine-three-harness-instance-configurations")
            .join("harness-daemon.nota")
            .exists(),
        "multi-harness topology must not collapse harness configurations into one shared file"
    );

    for instance_name in instance_names.into_iter().rev() {
        DirectProcessFixture::stop_instance(&launcher, ComponentInstanceName::new(instance_name))
            .await
            .unwrap_or_else(|error| panic!("{instance_name} component stops: {error:?}"));
    }
    launcher.stop_gracefully().await.expect("launcher stops");
    let _shutdown_completion = launcher.wait_for_shutdown().await;
}

#[tokio::test]
async fn constraint_spirit_launch_writes_engine_scoped_daemon_configuration() {
    let fixture = DirectProcessFixture::new("spirit-daemon-configuration");
    let launcher = DirectProcessLauncher::spawn(DirectProcessLauncher::new());
    let envelope = fixture.envelope(EngineComponent::Spirit).await;
    let envelope_path = envelope.envelope_path().to_path_buf();

    DirectProcessFixture::launch(&launcher, envelope)
        .await
        .expect("spirit component launches");

    let configuration_path = envelope_path.with_file_name("spirit-daemon.nota");
    let configuration_text =
        std::fs::read_to_string(&configuration_path).expect("spirit configuration was written");
    assert!(configuration_text.contains("spirit.sock"));
    assert!(configuration_text.contains("owner-spirit.sock"));
    assert!(configuration_text.contains("spirit-upgrade.sock"));
    assert!(configuration_text.contains("spirit.redb"));
    assert!(configuration_text.contains("spirit.supervision.sock"));
    assert!(
        configuration_text.contains("384"),
        "spirit sockets must be internal-only in prototype supervision: {configuration_text}"
    );

    DirectProcessFixture::stop(&launcher, EngineComponent::Spirit)
        .await
        .expect("spirit component stops");
    launcher.stop_gracefully().await.expect("launcher stops");
    let _shutdown_completion = launcher.wait_for_shutdown().await;
}

#[tokio::test]
async fn constraint_component_launcher_does_not_block_manager_mailbox() {
    let fixture = DirectProcessFixture::new("mailbox");
    let launcher = DirectProcessLauncher::spawn(DirectProcessLauncher::new());
    let manager = EngineManager::start().await;
    let envelope = fixture.envelope(EngineComponent::Mind).await;

    let receipt = DirectProcessFixture::launch(&launcher, envelope)
        .await
        .expect("component process launches");
    assert_eq!(receipt.component(), EngineComponent::Mind);

    let snapshot = launcher
        .ask(ReadLauncherSnapshot)
        .await
        .expect("launcher snapshot replies while child runs");
    assert_eq!(snapshot.running().len(), 1);
    assert_eq!(snapshot.launch_count(), 1);

    let manager_reply = manager
        .ask(HandleEngineRequest::new(EngineRequest::Query(
            Query::EngineStatus(EngineStatusScope::WholeEngine),
        )))
        .await
        .expect("manager mailbox replies while launched child runs");
    assert!(matches!(manager_reply, EngineReply::EngineStatus(_)));

    DirectProcessFixture::stop(&launcher, EngineComponent::Mind)
        .await
        .expect("component process stops");
    launcher.stop_gracefully().await.expect("launcher stops");
    let _shutdown_completion = launcher.wait_for_shutdown().await;
    EngineManager::stop(manager)
        .await
        .expect("manager stops after launcher witness");
}

#[tokio::test]
async fn constraint_component_launcher_reaps_process_group() {
    let fixture = DirectProcessFixture::new("reap");
    let launcher = DirectProcessLauncher::spawn(DirectProcessLauncher::new());
    let envelope = fixture
        .envelope_with_command(EngineComponent::Mind, fixture.process_group_command())
        .await;
    let receipt = DirectProcessFixture::launch(&launcher, envelope)
        .await
        .expect("component process launches");
    let process = receipt.process().into_u32();
    let child_process = fixture.read_child_process().await;
    assert!(DirectProcessFixture::process_is_alive(process));
    assert!(DirectProcessFixture::process_is_alive(child_process));

    let stopped = DirectProcessFixture::stop(&launcher, EngineComponent::Mind)
        .await
        .expect("component process stops");
    assert_eq!(stopped.process().into_u32(), process);
    DirectProcessFixture::wait_until_process_exits(process).await;
    DirectProcessFixture::wait_until_process_exits(child_process).await;
    assert!(!DirectProcessFixture::process_is_alive(process));
    assert!(!DirectProcessFixture::process_is_alive(child_process));

    let snapshot = launcher
        .ask(ReadLauncherSnapshot)
        .await
        .expect("launcher snapshot replies after stop");
    assert!(snapshot.running().is_empty());
    assert_eq!(snapshot.stop_count(), 1);

    launcher.stop_gracefully().await.expect("launcher stops");
    let _shutdown_completion = launcher.wait_for_shutdown().await;
}

#[tokio::test]
async fn constraint_component_launcher_passes_spawn_envelope_to_child_environment() {
    let fixture = DirectProcessFixture::new("envelope");
    let launcher = DirectProcessLauncher::spawn(DirectProcessLauncher::new());
    let envelope = fixture
        .envelope_with_command(EngineComponent::Mind, fixture.envelope_capture_command())
        .await;
    let envelope_path = envelope.envelope_path().to_path_buf();
    let owner_identity = envelope.owner_identity().clone();

    DirectProcessFixture::launch(&launcher, envelope)
        .await
        .expect("component process launches");
    let captured = fixture.read_envelope_capture().await;
    assert!(captured.contains("engine=engine-direct-process"));
    assert!(captured.contains("component=mind"));
    assert!(captured.contains("state="));
    assert!(captured.contains("mind.redb"));
    assert!(captured.contains("domain_socket="));
    assert!(captured.contains("mind.sock"));
    assert!(captured.contains("supervision_socket="));
    assert!(captured.contains("mind.supervision.sock"));
    assert!(captured.contains("spawn_envelope="));
    assert!(captured.contains("mind.envelope"));
    assert!(captured.contains("manager_socket="));
    assert!(captured.contains("persona.sock"));
    assert!(captured.contains("domain_mode=600"));
    assert!(captured.contains("supervision_mode=600"));
    let expected_peer_count = EngineComponent::prototype_supervised_components().len() - 1;
    assert!(
        captured.contains(&format!("peer_count={expected_peer_count}")),
        "capture did not contain expected peer count: {captured}"
    );
    assert!(captured.contains("peer_0_component="));
    assert!(captured.contains("peer_0_socket="));

    let envelope_text =
        std::fs::read_to_string(&envelope_path).expect("typed spawn envelope file exists");
    let mut decoder = Decoder::new(&envelope_text);
    let signal_envelope =
        signal_persona::SpawnEnvelope::decode(&mut decoder).expect("spawn envelope decodes");
    assert_eq!(signal_envelope.engine_id.as_str(), "engine-direct-process");
    assert_eq!(
        signal_envelope.component_kind,
        signal_persona::ComponentKind::Mind
    );
    assert_eq!(
        signal_envelope.component_name,
        signal_persona_auth::ComponentName::Mind
    );
    assert_eq!(signal_envelope.owner_identity, owner_identity);
    assert!(
        signal_envelope
            .state_dir
            .as_str()
            .ends_with("state/engine-direct-process")
    );
    assert!(
        signal_envelope
            .domain_socket_path
            .as_str()
            .ends_with("mind.sock")
    );
    assert_eq!(signal_envelope.domain_socket_mode.into_u32(), 0o600);
    assert!(
        signal_envelope
            .engine_management_socket_path
            .as_str()
            .ends_with("mind.supervision.sock")
    );
    assert_eq!(
        signal_envelope.engine_management_socket_mode.into_u32(),
        0o600
    );
    assert_eq!(
        signal_envelope
            .engine_management_protocol_version
            .into_u16(),
        1
    );

    DirectProcessFixture::stop(&launcher, EngineComponent::Mind)
        .await
        .expect("component process stops");
    launcher.stop_gracefully().await.expect("launcher stops");
    let _shutdown_completion = launcher.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_component_launcher_observes_natural_child_exit_and_appends_event() {
    let fixture = DirectProcessFixture::new("natural-exit");
    let engine = EngineId::new("engine-direct-process");
    let manager_store_path = fixture.root.join("manager.redb");
    let store = ManagerStore::start(ManagerStoreLocation::new(&manager_store_path))
        .expect("manager store starts");
    let launcher = DirectProcessLauncher::spawn(
        DirectProcessLauncher::new()
            .with_exit_notifier(ExitNotifier::new(engine.clone(), store.clone())),
    );
    let envelope = fixture
        .envelope_with_command(EngineComponent::Mind, fixture.short_running_command())
        .await;
    let receipt = DirectProcessFixture::launch(&launcher, envelope)
        .await
        .expect("component launches");
    let process = receipt.process().into_u32();

    DirectProcessFixture::wait_until_process_exits(process).await;
    assert!(!DirectProcessFixture::process_is_alive(process));

    // Pump the launcher's mailbox until the watcher's ChildProcessExited
    // notification has been processed and the natural-exit counter has
    // ticked. The launcher's snapshot is a pushed reply via its mailbox;
    // bounded retries here are mailbox-completion ordering, not polling
    // for state-change on a remote producer.
    let mut snapshot = launcher
        .ask(ReadLauncherSnapshot)
        .await
        .expect("launcher snapshot replies");
    for _attempt in 0..40 {
        if snapshot.natural_exit_count() >= 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        snapshot = launcher
            .ask(ReadLauncherSnapshot)
            .await
            .expect("launcher snapshot replies");
    }
    assert_eq!(snapshot.natural_exit_count(), 1);
    assert!(snapshot.running().is_empty());

    let events = store
        .ask(ReadEngineEvents::new(engine.clone()))
        .await
        .expect("manager events read");
    let exited_events: Vec<_> = events
        .iter()
        .filter(|event| matches!(event.body(), EngineEventBody::ComponentExited(_)))
        .collect();
    assert_eq!(exited_events.len(), 1);
    let EngineEventBody::ComponentExited(exited) = exited_events[0].body() else {
        unreachable!();
    };
    assert_eq!(exited.component().as_str(), "persona-mind");
    assert_eq!(exited.exit_code(), Some(0));

    launcher.stop_gracefully().await.expect("launcher stops");
    let _shutdown_completion = launcher.wait_for_shutdown().await;
    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}
