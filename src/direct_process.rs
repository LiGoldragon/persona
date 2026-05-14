use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use kameo::actor::{Actor, ActorRef};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use nota_codec::{Encoder, NotaEncode};
use thiserror::Error;
use tokio::process::{Child, Command};

use crate::engine::{ComponentSpawnEnvelope, EngineComponent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildProcessId(u32);

impl ChildProcessId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn into_u32(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchedComponent {
    component: EngineComponent,
    process: ChildProcessId,
}

impl LaunchedComponent {
    pub(crate) fn new(component: EngineComponent, process: ChildProcessId) -> Self {
        Self { component, process }
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn process(&self) -> ChildProcessId {
        self.process
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherSnapshot {
    running: Vec<LaunchedComponent>,
    launch_count: u64,
    stop_count: u64,
}

impl kameo::Reply for LauncherSnapshot {
    type Ok = Self;
    type Error = Infallible;
    type Value = Self;

    fn to_result(self) -> Result<Self::Ok, Self::Error> {
        Ok(self)
    }

    fn into_any_err(self) -> Option<Box<dyn kameo::reply::ReplyError>> {
        None
    }

    fn into_value(self) -> Self::Value {
        self
    }
}

impl LauncherSnapshot {
    fn from_input(input: LauncherSnapshotInput) -> Self {
        Self {
            running: input.running,
            launch_count: input.launch_count,
            stop_count: input.stop_count,
        }
    }

    pub fn running(&self) -> &[LaunchedComponent] {
        self.running.as_slice()
    }

    pub fn launch_count(&self) -> u64 {
        self.launch_count
    }

    pub fn stop_count(&self) -> u64 {
        self.stop_count
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LauncherSnapshotInput {
    running: Vec<LaunchedComponent>,
    launch_count: u64,
    stop_count: u64,
}

struct RunningChild {
    process: ChildProcessId,
    child: Child,
}

impl RunningChild {
    fn from_input(input: RunningChildInput) -> Self {
        Self {
            process: input.process,
            child: input.child,
        }
    }
}

struct RunningChildInput {
    process: ChildProcessId,
    child: Child,
}

pub struct DirectProcessLauncher {
    children: HashMap<EngineComponent, RunningChild>,
    launch_count: u64,
    stop_count: u64,
    graceful_timeout: Duration,
}

impl DirectProcessLauncher {
    pub fn new() -> Self {
        Self {
            children: HashMap::new(),
            launch_count: 0,
            stop_count: 0,
            graceful_timeout: Duration::from_millis(500),
        }
    }

    fn launch(
        &mut self,
        envelope: ComponentSpawnEnvelope,
    ) -> Result<LaunchComponentReceipt, DirectProcessFailure> {
        let component = envelope.component();
        if self.children.contains_key(&component) {
            return Err(DirectProcessFailure::ComponentAlreadyRunning { component });
        }
        Self::write_spawn_envelope_file(&envelope)?;
        let mut command = Self::command_from_envelope(&envelope);
        let mut child = command.spawn().map_err(|source| DirectProcessFailure::Io {
            operation: "spawn component process",
            source,
        })?;
        let Some(process) = child.id().map(ChildProcessId::new) else {
            let _ = child.start_kill();
            return Err(DirectProcessFailure::ChildPidMissing { component });
        };
        self.children.insert(
            component,
            RunningChild::from_input(RunningChildInput { process, child }),
        );
        self.launch_count = self.launch_count.saturating_add(1);
        Ok(LaunchComponentReceipt::new(component, process))
    }

    fn write_spawn_envelope_file(
        envelope: &ComponentSpawnEnvelope,
    ) -> Result<(), DirectProcessFailure> {
        let Some(parent) = envelope.envelope_path().parent() else {
            return Err(DirectProcessFailure::EnvelopePathMissingParent {
                component: envelope.component(),
            });
        };
        std::fs::create_dir_all(parent).map_err(|source| DirectProcessFailure::Io {
            operation: "create spawn envelope directory",
            source,
        })?;
        let mut encoder = Encoder::new();
        envelope
            .signal_spawn_envelope()
            .encode(&mut encoder)
            .map_err(DirectProcessFailure::Nota)?;
        let mut text = encoder.into_string();
        text.push('\n');
        std::fs::write(envelope.envelope_path(), text).map_err(|source| {
            DirectProcessFailure::Io {
                operation: "write spawn envelope file",
                source,
            }
        })?;
        std::fs::set_permissions(
            envelope.envelope_path(),
            std::fs::Permissions::from_mode(0o600),
        )
        .map_err(|source| DirectProcessFailure::Io {
            operation: "set spawn envelope file mode",
            source,
        })?;
        Ok(())
    }

    async fn stop(
        &mut self,
        component: EngineComponent,
    ) -> Result<StopComponentReceipt, DirectProcessFailure> {
        let Some(mut running) = self.children.remove(&component) else {
            return Err(DirectProcessFailure::ComponentNotRunning { component });
        };
        Self::terminate_process_group(running.process, libc::SIGTERM)?;
        let wait = tokio::time::timeout(self.graceful_timeout, running.child.wait()).await;
        match wait {
            Ok(Ok(_status)) => {}
            Ok(Err(source)) => {
                return Err(DirectProcessFailure::Io {
                    operation: "wait for component process",
                    source,
                });
            }
            Err(_elapsed) => {
                Self::terminate_process_group(running.process, libc::SIGKILL)?;
                running
                    .child
                    .wait()
                    .await
                    .map_err(|source| DirectProcessFailure::Io {
                        operation: "wait after killing component process",
                        source,
                    })?;
            }
        }
        self.stop_count = self.stop_count.saturating_add(1);
        Ok(StopComponentReceipt::new(component, running.process))
    }

    fn snapshot(&self) -> LauncherSnapshot {
        let running = self
            .children
            .iter()
            .map(|(component, child)| LaunchedComponent::new(*component, child.process))
            .collect();
        LauncherSnapshot::from_input(LauncherSnapshotInput {
            running,
            launch_count: self.launch_count,
            stop_count: self.stop_count,
        })
    }

    fn command_from_envelope(envelope: &ComponentSpawnEnvelope) -> Command {
        let component_command = envelope.command();
        let mut command = Command::new(component_command.executable_path().as_path());
        for argument in component_command.arguments() {
            command.arg(argument.as_str());
        }
        for variable in component_command.environment() {
            command.env(variable.name().as_str(), variable.value().as_str());
        }
        command.env("PERSONA_ENGINE_ID", envelope.engine().as_str());
        command.env("PERSONA_COMPONENT", envelope.component().as_str());
        command.env("PERSONA_STATE_PATH", envelope.state_path());
        command.env("PERSONA_SOCKET_PATH", envelope.socket_path());
        command.env("PERSONA_SPAWN_ENVELOPE", envelope.envelope_path());
        command.env("PERSONA_MANAGER_SOCKET", envelope.manager_socket());
        command.env(
            "PERSONA_SOCKET_MODE",
            format!("{:o}", envelope.socket_mode().as_octal()),
        );
        command.env(
            "PERSONA_PEER_SOCKET_COUNT",
            envelope.peers().len().to_string(),
        );
        for (index, peer) in envelope.peers().iter().enumerate() {
            command.env(
                format!("PERSONA_PEER_{index}_COMPONENT"),
                peer.component().as_str(),
            );
            command.env(
                format!("PERSONA_PEER_{index}_SOCKET_PATH"),
                peer.socket_path(),
            );
        }
        Self::configure_process_group(&mut command);
        command
    }

    fn configure_process_group(command: &mut Command) {
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }

    fn terminate_process_group(
        process: ChildProcessId,
        signal: i32,
    ) -> Result<(), DirectProcessFailure> {
        let process_group = process.into_u32() as i32;
        let result = unsafe { libc::killpg(process_group, signal) };
        if result == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        Err(DirectProcessFailure::Io {
            operation: "signal component process group",
            source: error,
        })
    }
}

impl Default for DirectProcessLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DirectProcessLauncher {
    fn drop(&mut self) {
        for child in self.children.values() {
            let _ = Self::terminate_process_group(child.process, libc::SIGKILL);
        }
    }
}

impl Actor for DirectProcessLauncher {
    type Args = Self;
    type Error = Infallible;

    async fn on_start(
        launcher: Self::Args,
        _actor_reference: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(launcher)
    }
}

#[derive(Debug)]
pub struct LaunchComponent {
    envelope: ComponentSpawnEnvelope,
}

impl LaunchComponent {
    pub fn new(envelope: ComponentSpawnEnvelope) -> Self {
        Self { envelope }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchComponentReceipt {
    component: EngineComponent,
    process: ChildProcessId,
}

impl LaunchComponentReceipt {
    fn new(component: EngineComponent, process: ChildProcessId) -> Self {
        Self { component, process }
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn process(&self) -> ChildProcessId {
        self.process
    }
}

impl Message<LaunchComponent> for DirectProcessLauncher {
    type Reply = Result<LaunchComponentReceipt, DirectProcessFailure>;

    async fn handle(
        &mut self,
        message: LaunchComponent,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.launch(message.envelope)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StopComponentProcess {
    component: EngineComponent,
}

impl StopComponentProcess {
    pub const fn new(component: EngineComponent) -> Self {
        Self { component }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopComponentReceipt {
    component: EngineComponent,
    process: ChildProcessId,
}

impl StopComponentReceipt {
    fn new(component: EngineComponent, process: ChildProcessId) -> Self {
        Self { component, process }
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn process(&self) -> ChildProcessId {
        self.process
    }
}

impl Message<StopComponentProcess> for DirectProcessLauncher {
    type Reply = Result<StopComponentReceipt, DirectProcessFailure>;

    async fn handle(
        &mut self,
        message: StopComponentProcess,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.stop(message.component).await
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadLauncherSnapshot;

impl Message<ReadLauncherSnapshot> for DirectProcessLauncher {
    type Reply = LauncherSnapshot;

    async fn handle(
        &mut self,
        _message: ReadLauncherSnapshot,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.snapshot()
    }
}

#[derive(Debug, Error)]
pub enum DirectProcessFailure {
    #[error("component {component:?} already has a running child process")]
    ComponentAlreadyRunning { component: EngineComponent },
    #[error("component {component:?} has no running child process")]
    ComponentNotRunning { component: EngineComponent },
    #[error("spawned component {component:?} did not expose a child pid")]
    ChildPidMissing { component: EngineComponent },
    #[error("spawn envelope path for component {component:?} has no parent directory")]
    EnvelopePathMissingParent { component: EngineComponent },
    #[error("spawn envelope nota: {0}")]
    Nota(#[from] nota_codec::Error),
    #[error("{operation}: {source}")]
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
}
