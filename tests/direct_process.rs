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
    CommandArgument, ComponentCommand, ComponentCommandCatalog, ComponentCommandEntry,
    ComponentCommandEntryInput, ComponentCommandInput, ComponentCommandResolver,
    EngineLaunchConfiguration, EnvironmentVariable, EnvironmentVariableInput,
    EnvironmentVariableName, EnvironmentVariableValue, ExecutablePath, ResolveComponentCommands,
    ResolvedComponentCommands,
};
use persona::manager::{EngineManager, HandleEngineRequest};
use persona::manager_store::{ManagerStore, ManagerStoreLocation, ReadEngineEvents};
use signal_persona::{EngineReply, EngineRequest, EngineStatusQuery};
use signal_persona_auth::EngineId;
use signal_persona_router::RouterDaemonConfiguration;

struct DirectProcessFixture {
    root: PathBuf,
    shell: String,
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
        let shell = std::env::var("PERSONA_TEST_SHELL")
            .or_else(|_| std::env::var("SHELL"))
            .unwrap_or_else(|_| "/bin/sh".to_string());
        Self { root, shell }
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

    fn short_running_command(&self) -> ComponentCommand {
        // Exits cleanly after ~250ms so the natural-exit observer fires
        // while the launcher is still alive to record the event.
        ComponentCommand::from_input(ComponentCommandInput {
            executable_path: ExecutablePath::new(self.shell.clone()),
            arguments: vec![
                CommandArgument::new("-c"),
                CommandArgument::new("sleep 0.25; exit 0"),
            ],
            environment: Vec::new(),
        })
    }

    fn long_running_command(&self) -> ComponentCommand {
        ComponentCommand::from_input(ComponentCommandInput {
            executable_path: ExecutablePath::new(self.shell.clone()),
            arguments: vec![
                CommandArgument::new("-c"),
                CommandArgument::new(
                    "trap 'exit 0' TERM; (trap 'exit 0' TERM; while true; do sleep 1; done) & echo \"$!\" > \"$PERSONA_TEST_CHILD_PID_FILE\"; wait",
                ),
            ],
            environment: vec![EnvironmentVariable::from_input(EnvironmentVariableInput {
                name: EnvironmentVariableName::new("PERSONA_TEST_CHILD_PID_FILE"),
                value: EnvironmentVariableValue::new(
                    self.child_pid_file().to_string_lossy().into_owned(),
                ),
            })],
        })
    }

    fn envelope_capture_command(&self) -> ComponentCommand {
        ComponentCommand::from_input(ComponentCommandInput {
            executable_path: ExecutablePath::new(self.shell.clone()),
            arguments: vec![
                CommandArgument::new("-c"),
                CommandArgument::new(
                    "{
  printf 'engine=%s\n' \"$PERSONA_ENGINE_ID\";
  printf 'component=%s\n' \"$PERSONA_COMPONENT\";
  printf 'state=%s\n' \"$PERSONA_STATE_PATH\";
  printf 'domain_socket=%s\n' \"$PERSONA_DOMAIN_SOCKET_PATH\";
  printf 'supervision_socket=%s\n' \"$PERSONA_SUPERVISION_SOCKET_PATH\";
  printf 'spawn_envelope=%s\n' \"$PERSONA_SPAWN_ENVELOPE\";
  printf 'manager_socket=%s\n' \"$PERSONA_MANAGER_SOCKET\";
  printf 'domain_mode=%s\n' \"$PERSONA_DOMAIN_SOCKET_MODE\";
  printf 'supervision_mode=%s\n' \"$PERSONA_SUPERVISION_SOCKET_MODE\";
  printf 'peer_count=%s\n' \"$PERSONA_PEER_SOCKET_COUNT\";
  printf 'peer_0_component=%s\n' \"$PERSONA_PEER_0_COMPONENT\";
  printf 'peer_0_socket=%s\n' \"$PERSONA_PEER_0_SOCKET_PATH\";
} > \"$PERSONA_TEST_ENVELOPE_FILE\";
exec sleep 3600",
                ),
            ],
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
        match launcher.ask(StopComponentProcess::new(component)).await {
            Ok(receipt) => Ok(receipt),
            Err(SendError::HandlerError(failure)) => Err(failure),
            Err(error) => panic!("launcher actor transport failed: {error:?}"),
        }
    }

    fn process_is_alive(process: u32) -> bool {
        let result = unsafe { libc::kill(process as i32, 0) };
        if result == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
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
                return text.trim().parse().expect("child pid is numeric");
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("child pid file was not written");
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

    for name in ["initiator", "responder", "reviewer"] {
        assert!(
            bootstrap_text.contains(&format!("(Actor {name} 0")),
            "bootstrap did not register {name}: {bootstrap_text}"
        );
        assert!(
            bootstrap_text.contains(&format!("{name}.sock")),
            "bootstrap did not include {name} harness socket: {bootstrap_text}"
        );
    }
    for grant in [
        "(GrantDirectMessage owner initiator)",
        "(GrantDirectMessage owner responder)",
        "(GrantDirectMessage owner reviewer)",
        "(GrantDirectMessage initiator responder)",
        "(GrantDirectMessage responder reviewer)",
        "(GrantDirectMessage reviewer owner)",
    ] {
        assert!(
            bootstrap_text.contains(grant),
            "bootstrap did not include grant {grant}: {bootstrap_text}"
        );
    }

    DirectProcessFixture::stop(&launcher, EngineComponent::Router)
        .await
        .expect("router component stops");
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
        .ask(HandleEngineRequest::new(EngineRequest::EngineStatusQuery(
            EngineStatusQuery::whole_engine(),
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
    let envelope = fixture.envelope(EngineComponent::Mind).await;
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
            .supervision_socket_path
            .as_str()
            .ends_with("mind.supervision.sock")
    );
    assert_eq!(signal_envelope.supervision_socket_mode.into_u32(), 0o600);
    assert_eq!(signal_envelope.supervision_protocol_version.into_u16(), 1);

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
