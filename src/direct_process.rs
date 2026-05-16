use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::path::{Path, PathBuf};

use kameo::actor::{Actor, ActorRef};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use nota_codec::{Encoder, NotaEncode};
use signal_persona_auth::EngineId;
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::oneshot;

use crate::engine::{ComponentSpawnEnvelope, EngineComponent};
use crate::engine_event::{
    ComponentExited, ComponentExitedInput, EngineEventBody, EngineEventDraft, EngineEventDraftInput,
    EngineEventSource,
};
use crate::manager_store::{AppendEngineEvent, ManagerStore};

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
    natural_exit_count: u64,
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
            natural_exit_count: input.natural_exit_count,
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

    pub fn natural_exit_count(&self) -> u64 {
        self.natural_exit_count
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LauncherSnapshotInput {
    running: Vec<LaunchedComponent>,
    launch_count: u64,
    stop_count: u64,
    natural_exit_count: u64,
}

/// Shared state between the launcher actor and one child's watcher task.
/// When `stop` runs in the launcher, it drops a `oneshot::Sender` here; the
/// watcher task, after `child.wait().await` returns, lifts the sender out
/// of the mutex. If present, the watcher fulfills the stop waiter directly
/// (bypassing the launcher's mailbox); if absent, the watcher routes the
/// exit through the launcher's mailbox as a natural-exit notification.
///
/// The mutex stays held for the millisecond it takes to read/write the
/// `Option`; no async work happens under it. This is short-lived
/// coordination between an actor and its detached worker task, not an
/// `Arc<Mutex>`-as-ownership shape between two actors.
type StopHandoff = Arc<Mutex<Option<oneshot::Sender<StopComponentReceipt>>>>;

struct RunningChild {
    process: ChildProcessId,
    /// Watcher task that owns the `tokio::process::Child` and awaits its
    /// exit. Aborted on launcher drop so an unsupervised teardown still
    /// reaps watchers.
    watcher: tokio::task::JoinHandle<()>,
    stop_handoff: StopHandoff,
}

pub struct DirectProcessLauncher {
    children: HashMap<EngineComponent, RunningChild>,
    launch_count: u64,
    stop_count: u64,
    /// Count of children observed exiting without an explicit stop. Used by
    /// tests to witness the natural-exit observer pipeline.
    natural_exit_count: u64,
    graceful_timeout: Duration,
    /// Optional path back into the manager event log. Present when the
    /// launcher is wired by a supervisor that knows the engine; absent in
    /// unit tests that only exercise the process plane.
    exit_notifier: Option<ExitNotifier>,
}

#[derive(Debug, Clone)]
pub struct ExitNotifier {
    engine: EngineId,
    store: ActorRef<ManagerStore>,
}

impl ExitNotifier {
    pub fn new(engine: EngineId, store: ActorRef<ManagerStore>) -> Self {
        Self { engine, store }
    }
}

impl DirectProcessLauncher {
    pub fn new() -> Self {
        Self {
            children: HashMap::new(),
            launch_count: 0,
            stop_count: 0,
            natural_exit_count: 0,
            graceful_timeout: Duration::from_millis(500),
            exit_notifier: None,
        }
    }

    pub fn with_exit_notifier(mut self, notifier: ExitNotifier) -> Self {
        self.exit_notifier = Some(notifier);
        self
    }

    fn launch(
        &mut self,
        envelope: ComponentSpawnEnvelope,
        launcher_ref: ActorRef<Self>,
    ) -> Result<LaunchComponentReceipt, DirectProcessFailure> {
        let component = envelope.component();
        if self.children.contains_key(&component) {
            return Err(DirectProcessFailure::ComponentAlreadyRunning { component });
        }
        Self::write_spawn_envelope_file(&envelope)?;
        let typed_configuration_path = Self::write_typed_configuration_file(&envelope)?;
        let mut command = Self::command_from_envelope(&envelope, typed_configuration_path.as_deref());
        let mut child = command.spawn().map_err(|source| DirectProcessFailure::Io {
            operation: "spawn component process",
            source,
        })?;
        let Some(process) = child.id().map(ChildProcessId::new) else {
            let _ = child.start_kill();
            return Err(DirectProcessFailure::ChildPidMissing { component });
        };
        let stop_handoff: StopHandoff = Arc::new(Mutex::new(None));
        let watcher_handoff = stop_handoff.clone();
        let watcher = tokio::spawn(async move {
            let exit_code = match child.wait().await {
                Ok(status) => status.code(),
                Err(_error) => None,
            };
            let stop_sender = watcher_handoff
                .lock()
                .expect("stop_handoff mutex not poisoned")
                .take();
            match stop_sender {
                Some(sender) => {
                    let _ =
                        sender.send(StopComponentReceipt::new(component, process));
                }
                None => {
                    let _ = launcher_ref
                        .tell(ChildProcessExited {
                            component,
                            process,
                            exit_code,
                        })
                        .await;
                }
            }
        });
        self.children.insert(
            component,
            RunningChild {
                process,
                watcher,
                stop_handoff,
            },
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
        let (receiver, process, handoff) = {
            let running = self
                .children
                .get_mut(&component)
                .ok_or(DirectProcessFailure::ComponentNotRunning { component })?;
            let mut handoff_guard = running
                .stop_handoff
                .lock()
                .expect("stop_handoff mutex not poisoned");
            if handoff_guard.is_some() {
                return Err(DirectProcessFailure::ComponentStopAlreadyInFlight { component });
            }
            let (sender, receiver) = oneshot::channel();
            *handoff_guard = Some(sender);
            drop(handoff_guard);
            Self::terminate_process_group(running.process, libc::SIGTERM)?;
            (receiver, running.process, running.stop_handoff.clone())
        };

        let receipt = self
            .await_stop_receipt(component, process, handoff, receiver)
            .await?;
        // Remove the entry after the watcher signalled exit. The watcher's
        // `JoinHandle` finishes shortly; its abort on Drop is a no-op once
        // the task has already returned.
        self.children.remove(&component);
        self.stop_count = self.stop_count.saturating_add(1);
        Ok(receipt)
    }

    /// Wait on the watcher's stop signal, escalating to SIGKILL if the
    /// graceful timeout elapses. The stop handoff and the watcher both stay
    /// owned by the launcher's `children` map until this method returns;
    /// `await` happens off the per-child borrow so neighbours stay
    /// reachable.
    async fn await_stop_receipt(
        &self,
        component: EngineComponent,
        process: ChildProcessId,
        _handoff: StopHandoff,
        receiver: oneshot::Receiver<StopComponentReceipt>,
    ) -> Result<StopComponentReceipt, DirectProcessFailure> {
        let timeout = tokio::time::sleep(self.graceful_timeout);
        tokio::pin!(timeout);
        tokio::pin!(receiver);
        loop {
            tokio::select! {
                _ = &mut timeout => {
                    Self::terminate_process_group(process, libc::SIGKILL)?;
                    // Re-arm the timeout to a far future point so the next
                    // iteration awaits only on the receiver.
                    timeout
                        .as_mut()
                        .reset(tokio::time::Instant::now() + Duration::from_secs(60));
                }
                result = &mut receiver => {
                    return result.map_err(|_canceled| {
                        DirectProcessFailure::StopWaiterCanceled { component }
                    });
                }
            }
        }
    }

    /// Natural-exit path: the watcher observed `child.wait()` returning with
    /// no stop handoff present. Append `ComponentExited` to the manager
    /// event log (when a notifier is wired) and update bookkeeping.
    async fn handle_child_exited(&mut self, exit: ChildProcessExited) {
        if self.children.remove(&exit.component).is_none() {
            return;
        }
        self.natural_exit_count = self.natural_exit_count.saturating_add(1);
        let Some(notifier) = self.exit_notifier.clone() else {
            return;
        };
        let draft = EngineEventDraft::from_input(EngineEventDraftInput {
            engine: notifier.engine.clone(),
            source: EngineEventSource::Component(exit.component.component_name()),
            body: EngineEventBody::ComponentExited(ComponentExited::from_input(
                ComponentExitedInput {
                    component: exit.component.component_name(),
                    exit_code: exit.exit_code,
                },
            )),
        });
        let _ = notifier.store.ask(AppendEngineEvent::new(draft)).await;
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
            natural_exit_count: self.natural_exit_count,
        })
    }

    /// Write per-daemon typed configuration files (per designer/183).
    ///
    /// Returns `Some(path)` for components that have migrated to the
    /// typed-configuration argv contract; the caller prepends that
    /// path as argv. Returns `None` for components still on the
    /// env-var contract — the launcher's env-var sets still apply.
    fn write_typed_configuration_file(
        envelope: &ComponentSpawnEnvelope,
    ) -> Result<Option<PathBuf>, DirectProcessFailure> {
        match envelope.component() {
            EngineComponent::Message => {
                Self::write_message_daemon_configuration_file(envelope).map(Some)
            }
            EngineComponent::Introspect => {
                Self::write_introspect_daemon_configuration_file(envelope).map(Some)
            }
            EngineComponent::Router => {
                Self::write_router_daemon_configuration_file(envelope).map(Some)
            }
            _ => Ok(None),
        }
    }

    fn write_message_daemon_configuration_file(
        envelope: &ComponentSpawnEnvelope,
    ) -> Result<PathBuf, DirectProcessFailure> {
        let router_socket_path = envelope
            .peers()
            .iter()
            .find(|peer| peer.component() == EngineComponent::Router)
            .ok_or(DirectProcessFailure::MissingRouterPeerForMessage)?
            .domain_socket_path();
        let configuration = signal_persona_message::MessageDaemonConfiguration {
            message_socket_path: signal_persona::WirePath::new(
                envelope.domain_socket_path().to_string_lossy().into_owned(),
            ),
            message_socket_mode: signal_persona::SocketMode::new(
                envelope.domain_socket_mode().as_octal(),
            ),
            supervision_socket_path: signal_persona::WirePath::new(
                envelope.supervision_socket_path().to_string_lossy().into_owned(),
            ),
            supervision_socket_mode: signal_persona::SocketMode::new(
                envelope.supervision_socket_mode().as_octal(),
            ),
            router_socket_path: signal_persona::WirePath::new(
                router_socket_path.to_string_lossy().into_owned(),
            ),
            owner_identity: envelope.owner_identity().clone(),
        };
        let path = envelope
            .envelope_path()
            .with_file_name("message-daemon.nota");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| DirectProcessFailure::Io {
                operation: "create message daemon configuration directory",
                source,
            })?;
        }
        let mut encoder = Encoder::new();
        configuration
            .encode(&mut encoder)
            .map_err(DirectProcessFailure::Nota)?;
        let mut text = encoder.into_string();
        text.push('\n');
        std::fs::write(&path, text).map_err(|source| DirectProcessFailure::Io {
            operation: "write message daemon configuration file",
            source,
        })?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
            |source| DirectProcessFailure::Io {
                operation: "set message daemon configuration file mode",
                source,
            },
        )?;
        Ok(path)
    }

    fn write_router_daemon_configuration_file(
        envelope: &ComponentSpawnEnvelope,
    ) -> Result<PathBuf, DirectProcessFailure> {
        let store_path = envelope
            .state_dir()
            .join(format!("{}.redb", envelope.component().as_str()));
        let configuration = signal_persona_router::RouterDaemonConfiguration {
            router_socket_path: signal_persona::WirePath::new(
                envelope.domain_socket_path().to_string_lossy().into_owned(),
            ),
            router_socket_mode: signal_persona::SocketMode::new(
                envelope.domain_socket_mode().as_octal(),
            ),
            supervision_socket_path: signal_persona::WirePath::new(
                envelope
                    .supervision_socket_path()
                    .to_string_lossy()
                    .into_owned(),
            ),
            supervision_socket_mode: signal_persona::SocketMode::new(
                envelope.supervision_socket_mode().as_octal(),
            ),
            store_path: signal_persona::WirePath::new(store_path.to_string_lossy().into_owned()),
            bootstrap_path: None,
            owner_identity: envelope.owner_identity().clone(),
        };
        let path = envelope
            .envelope_path()
            .with_file_name("router-daemon.nota");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| DirectProcessFailure::Io {
                operation: "create router daemon configuration directory",
                source,
            })?;
        }
        let mut encoder = Encoder::new();
        configuration
            .encode(&mut encoder)
            .map_err(DirectProcessFailure::Nota)?;
        let mut text = encoder.into_string();
        text.push('\n');
        std::fs::write(&path, text).map_err(|source| DirectProcessFailure::Io {
            operation: "write router daemon configuration file",
            source,
        })?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
            |source| DirectProcessFailure::Io {
                operation: "set router daemon configuration file mode",
                source,
            },
        )?;
        Ok(path)
    }

    fn write_introspect_daemon_configuration_file(
        envelope: &ComponentSpawnEnvelope,
    ) -> Result<PathBuf, DirectProcessFailure> {
        let router_socket_path = envelope
            .peers()
            .iter()
            .find(|peer| peer.component() == EngineComponent::Router)
            .ok_or(DirectProcessFailure::MissingRouterPeerForIntrospect)?
            .domain_socket_path()
            .to_path_buf();
        let terminal_socket_path = envelope
            .peers()
            .iter()
            .find(|peer| peer.component() == EngineComponent::Terminal)
            .ok_or(DirectProcessFailure::MissingTerminalPeerForIntrospect)?
            .domain_socket_path()
            .to_path_buf();
        let configuration = signal_persona_introspect::IntrospectDaemonConfiguration {
            introspect_socket_path: signal_persona::WirePath::new(
                envelope.domain_socket_path().to_string_lossy().into_owned(),
            ),
            introspect_socket_mode: signal_persona::SocketMode::new(
                envelope.domain_socket_mode().as_octal(),
            ),
            supervision_socket_path: signal_persona::WirePath::new(
                envelope
                    .supervision_socket_path()
                    .to_string_lossy()
                    .into_owned(),
            ),
            supervision_socket_mode: signal_persona::SocketMode::new(
                envelope.supervision_socket_mode().as_octal(),
            ),
            store_path: signal_persona::WirePath::new(
                envelope.state_path().to_string_lossy().into_owned(),
            ),
            manager_socket_path: signal_persona::WirePath::new(
                envelope.manager_socket().to_string_lossy().into_owned(),
            ),
            router_socket_path: signal_persona::WirePath::new(
                router_socket_path.to_string_lossy().into_owned(),
            ),
            terminal_socket_path: signal_persona::WirePath::new(
                terminal_socket_path.to_string_lossy().into_owned(),
            ),
            owner_identity: envelope.owner_identity().clone(),
        };
        let path = envelope
            .envelope_path()
            .with_file_name("introspect-daemon.nota");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| DirectProcessFailure::Io {
                operation: "create introspect daemon configuration directory",
                source,
            })?;
        }
        let mut encoder = Encoder::new();
        configuration
            .encode(&mut encoder)
            .map_err(DirectProcessFailure::Nota)?;
        let mut text = encoder.into_string();
        text.push('\n');
        std::fs::write(&path, text).map_err(|source| DirectProcessFailure::Io {
            operation: "write introspect daemon configuration file",
            source,
        })?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
            |source| DirectProcessFailure::Io {
                operation: "set introspect daemon configuration file mode",
                source,
            },
        )?;
        Ok(path)
    }

    fn command_from_envelope(
        envelope: &ComponentSpawnEnvelope,
        typed_configuration_path: Option<&Path>,
    ) -> Command {
        let component_command = envelope.command();
        let mut command = Command::new(component_command.executable_path().as_path());
        if let Some(path) = typed_configuration_path {
            command.arg(path);
        }
        for argument in component_command.arguments() {
            command.arg(argument.as_str());
        }
        for variable in component_command.environment() {
            command.env(variable.name().as_str(), variable.value().as_str());
        }
        command.env("PERSONA_ENGINE_ID", envelope.engine().as_str());
        command.env("PERSONA_COMPONENT", envelope.component().as_str());
        command.env("PERSONA_STATE_PATH", envelope.state_path());
        command.env("PERSONA_SOCKET_PATH", envelope.domain_socket_path());
        command.env("PERSONA_DOMAIN_SOCKET_PATH", envelope.domain_socket_path());
        command.env(
            "PERSONA_DOMAIN_SOCKET_MODE",
            format!("{:o}", envelope.domain_socket_mode().as_octal()),
        );
        command.env(
            "PERSONA_SUPERVISION_SOCKET_PATH",
            envelope.supervision_socket_path(),
        );
        command.env(
            "PERSONA_SUPERVISION_SOCKET_MODE",
            format!("{:o}", envelope.supervision_socket_mode().as_octal()),
        );
        command.env("PERSONA_SPAWN_ENVELOPE", envelope.envelope_path());
        command.env("PERSONA_MANAGER_SOCKET", envelope.manager_socket());
        command.env(
            "PERSONA_SOCKET_MODE",
            format!("{:o}", envelope.domain_socket_mode().as_octal()),
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
                peer.domain_socket_path(),
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
            child.watcher.abort();
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
        context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let launcher_ref = context.actor_ref().clone();
        self.launch(message.envelope, launcher_ref)
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

/// Watcher-task notification: the `tokio::process::Child` for this
/// component has exited (or its `wait` errored). Routed by the watcher
/// into the launcher's mailbox so reaping and event-append happen on the
/// supervised mailbox thread, not in the detached watcher.
#[derive(Debug, Clone)]
pub struct ChildProcessExited {
    component: EngineComponent,
    process: ChildProcessId,
    exit_code: Option<i32>,
}

impl ChildProcessExited {
    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn process(&self) -> ChildProcessId {
        self.process
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
}

impl Message<ChildProcessExited> for DirectProcessLauncher {
    type Reply = ();

    async fn handle(
        &mut self,
        message: ChildProcessExited,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.handle_child_exited(message).await
    }
}

#[derive(Debug, Error)]
pub enum DirectProcessFailure {
    #[error("component {component:?} already has a running child process")]
    ComponentAlreadyRunning { component: EngineComponent },
    #[error("component {component:?} has no running child process")]
    ComponentNotRunning { component: EngineComponent },
    #[error("component {component:?} already has a stop in flight")]
    ComponentStopAlreadyInFlight { component: EngineComponent },
    #[error("component {component:?} stop waiter was canceled before exit")]
    StopWaiterCanceled { component: EngineComponent },
    #[error("spawned component {component:?} did not expose a child pid")]
    ChildPidMissing { component: EngineComponent },
    #[error("spawn envelope path for component {component:?} has no parent directory")]
    EnvelopePathMissingParent { component: EngineComponent },
    #[error("message daemon spawn envelope has no Router peer socket")]
    MissingRouterPeerForMessage,
    #[error("introspect daemon spawn envelope has no Router peer socket")]
    MissingRouterPeerForIntrospect,
    #[error("introspect daemon spawn envelope has no Terminal peer socket")]
    MissingTerminalPeerForIntrospect,
    #[error("spawn envelope nota: {0}")]
    Nota(#[from] nota_codec::Error),
    #[error("{operation}: {source}")]
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
}
