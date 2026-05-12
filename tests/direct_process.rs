use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use kameo::actor::{ActorRef, Spawn};
use kameo::error::SendError;
use persona::direct_process::{
    DirectProcessFailure, DirectProcessLauncher, LaunchComponent, LaunchComponentReceipt,
    ReadLauncherSnapshot, StopComponentProcess, StopComponentReceipt,
};
use persona::engine::{ComponentSpawnEnvelope, EngineComponent, PersonaDaemonPaths};
use persona::launch::{
    CommandArgument, ComponentCommand, ComponentCommandCatalog, ComponentCommandEntry,
    ComponentCommandEntryInput, ComponentCommandInput, ComponentCommandResolver,
    EngineLaunchConfiguration, EnvironmentVariable, EnvironmentVariableInput,
    EnvironmentVariableName, EnvironmentVariableValue, ExecutablePath, ResolveComponentCommands,
    ResolvedComponentCommands,
};
use persona::manager::{EngineManager, HandleEngineRequest};
use signal_persona::{EngineReply, EngineRequest, EngineStatusQuery};
use signal_persona_auth::EngineId;

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

    fn long_running_command(&self) -> ComponentCommand {
        ComponentCommand::from_input(ComponentCommandInput {
            executable_path: ExecutablePath::new(self.shell.clone()),
            arguments: vec![
                CommandArgument::new("-c"),
                CommandArgument::new(
                    "trap 'exit 0' TERM; (trap 'exit 0' TERM; echo \"$BASHPID\" > \"$PERSONA_TEST_CHILD_PID_FILE\"; while true; do sleep 1; done) & wait",
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
  printf 'socket=%s\n' \"$PERSONA_SOCKET_PATH\";
  printf 'mode=%s\n' \"$PERSONA_SOCKET_MODE\";
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
            EngineComponent::first_stack()
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
        let mut entries: Vec<ComponentCommandEntry> = EngineComponent::first_stack()
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
            if let Ok(text) = std::fs::read_to_string(self.envelope_capture_file()) {
                return text;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("spawn envelope capture file was not written");
    }
}

impl Drop for DirectProcessFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_component_launcher_does_not_block_manager_mailbox() {
    let fixture = DirectProcessFixture::new("mailbox");
    let launcher = DirectProcessLauncher::spawn(DirectProcessLauncher::new());
    let manager = EngineManager::start().await;
    let envelope = fixture.envelope(EngineComponent::Router).await;

    let receipt = DirectProcessFixture::launch(&launcher, envelope)
        .await
        .expect("component process launches");
    assert_eq!(receipt.component(), EngineComponent::Router);

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

    DirectProcessFixture::stop(&launcher, EngineComponent::Router)
        .await
        .expect("component process stops");
    launcher.stop_gracefully().await.expect("launcher stops");
    launcher.wait_for_shutdown().await;
    EngineManager::stop(manager)
        .await
        .expect("manager stops after launcher witness");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_component_launcher_reaps_process_group() {
    let fixture = DirectProcessFixture::new("reap");
    let launcher = DirectProcessLauncher::spawn(DirectProcessLauncher::new());
    let envelope = fixture.envelope(EngineComponent::Router).await;
    let receipt = DirectProcessFixture::launch(&launcher, envelope)
        .await
        .expect("component process launches");
    let process = receipt.process().into_u32();
    let child_process = fixture.read_child_process().await;
    assert!(DirectProcessFixture::process_is_alive(process));
    assert!(DirectProcessFixture::process_is_alive(child_process));

    let stopped = DirectProcessFixture::stop(&launcher, EngineComponent::Router)
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
    launcher.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_component_launcher_passes_spawn_envelope_to_child_environment() {
    let fixture = DirectProcessFixture::new("envelope");
    let launcher = DirectProcessLauncher::spawn(DirectProcessLauncher::new());
    let envelope = fixture
        .envelope_with_command(EngineComponent::Router, fixture.envelope_capture_command())
        .await;

    DirectProcessFixture::launch(&launcher, envelope)
        .await
        .expect("component process launches");
    let captured = fixture.read_envelope_capture().await;
    assert!(captured.contains("engine=engine-direct-process"));
    assert!(captured.contains("component=router"));
    assert!(captured.contains("state="));
    assert!(captured.contains("router.redb"));
    assert!(captured.contains("socket="));
    assert!(captured.contains("router.sock"));
    assert!(captured.contains("mode=600"));
    assert!(captured.contains("peer_count=5"));
    assert!(captured.contains("peer_0_component="));
    assert!(captured.contains("peer_0_socket="));

    DirectProcessFixture::stop(&launcher, EngineComponent::Router)
        .await
        .expect("component process stops");
    launcher.stop_gracefully().await.expect("launcher stops");
    launcher.wait_for_shutdown().await;
}
