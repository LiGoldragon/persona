use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::path::{Path, PathBuf};

use kameo::actor::{Actor, ActorRef};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use nota_codec::{Encoder, NotaEncode};
use signal_persona_origin::EngineIdentifier;
use signal_persona_router::{
    Actor as RouterBootstrapActor, ActorIdentifier as RouterBootstrapActorIdentifier,
    EndpointKind as RouterBootstrapEndpointKind,
    EndpointTransport as RouterBootstrapEndpointTransport,
    GrantDirectMessage as RouterBootstrapGrantDirectMessage,
    RegisterActor as RouterBootstrapRegisterActor, RouterBootstrapDocument,
    RouterBootstrapOperation,
};
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::oneshot;

use crate::engine::{ComponentInstanceName, ComponentSpawnEnvelope, EngineComponent};
use crate::engine_event::{
    ComponentExited, ComponentExitedInput, EngineEventBody, EngineEventDraft,
    EngineEventDraftInput, EngineEventSource,
};
use crate::manager_store::{AppendEngineEvent, ManagerStore};

mod spirit_daemon_configuration {
    #[derive(Debug, Clone, PartialEq, Eq, nota_codec::NotaRecord)]
    pub struct DaemonConfiguration {
        pub ordinary_socket_path: SocketPath,
        pub owner_socket_path: SocketPath,
        pub upgrade_socket_path: SocketPath,
        pub store_path: StorePath,
        pub socket_mode: SocketMode,
        pub bootstrap_policy_path: Option<BootstrapPolicyPath>,
        pub handoff_control_socket_path: Option<SocketPath>,
        pub engine_management_socket_path: Option<SocketPath>,
        pub engine_management_socket_mode: Option<SocketMode>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, nota_codec::NotaTransparent)]
    pub struct SocketPath(String);

    #[derive(Debug, Clone, PartialEq, Eq, nota_codec::NotaTransparent)]
    pub struct StorePath(String);

    #[derive(Debug, Clone, PartialEq, Eq, nota_codec::NotaTransparent)]
    pub struct BootstrapPolicyPath(String);

    #[derive(Debug, Clone, Copy, PartialEq, Eq, nota_codec::NotaTransparent)]
    pub struct SocketMode(u32);

    impl SocketPath {
        pub fn new(value: impl Into<String>) -> Self {
            Self(value.into())
        }
    }

    impl StorePath {
        pub fn new(value: impl Into<String>) -> Self {
            Self(value.into())
        }
    }

    impl SocketMode {
        pub const fn new(value: u32) -> Self {
            Self(value)
        }
    }
}

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
    component_instance: ComponentInstanceName,
    component: EngineComponent,
    process: ChildProcessId,
}

impl LaunchedComponent {
    pub(crate) fn new_instance(
        component_instance: ComponentInstanceName,
        component: EngineComponent,
        process: ChildProcessId,
    ) -> Self {
        Self {
            component_instance,
            component,
            process,
        }
    }

    pub fn component_instance(&self) -> &ComponentInstanceName {
        &self.component_instance
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
    component_instance: ComponentInstanceName,
    component: EngineComponent,
    process: ChildProcessId,
    /// Watcher task that owns the `tokio::process::Child` and awaits its
    /// exit. Aborted on launcher drop so an unsupervised teardown still
    /// reaps watchers.
    watcher: tokio::task::JoinHandle<()>,
    stop_handoff: StopHandoff,
}

pub struct DirectProcessLauncher {
    children: HashMap<ComponentInstanceName, RunningChild>,
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
    engine: EngineIdentifier,
    store: ActorRef<ManagerStore>,
}

impl ExitNotifier {
    pub fn new(engine: EngineIdentifier, store: ActorRef<ManagerStore>) -> Self {
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
        let component_instance = envelope.component_instance().clone();
        if self.children.contains_key(&component_instance) {
            return Err(DirectProcessFailure::ComponentAlreadyRunning {
                component_instance: component_instance.clone(),
            });
        }
        Self::write_spawn_envelope_file(&envelope)?;
        let typed_configuration_path = Self::write_typed_configuration_file(&envelope)?;
        let mut command =
            Self::command_from_envelope(&envelope, typed_configuration_path.as_deref());
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
        let watcher_component_instance = component_instance.clone();
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
                    let _ = sender.send(StopComponentReceipt::new_instance(
                        watcher_component_instance,
                        component,
                        process,
                    ));
                }
                None => {
                    let _ = launcher_ref
                        .tell(ChildProcessExited {
                            component_instance: watcher_component_instance,
                            component,
                            process,
                            exit_code,
                        })
                        .await;
                }
            }
        });
        self.children.insert(
            component_instance.clone(),
            RunningChild {
                component_instance: component_instance.clone(),
                component,
                process,
                watcher,
                stop_handoff,
            },
        );
        self.launch_count = self.launch_count.saturating_add(1);
        Ok(LaunchComponentReceipt::new_instance(
            component_instance,
            component,
            process,
        ))
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
        component_instance: ComponentInstanceName,
    ) -> Result<StopComponentReceipt, DirectProcessFailure> {
        let (component, receiver, process, handoff) = {
            let running = self.children.get_mut(&component_instance).ok_or_else(|| {
                DirectProcessFailure::ComponentNotRunning {
                    component_instance: component_instance.clone(),
                }
            })?;
            let component = running.component;
            let mut handoff_guard = running
                .stop_handoff
                .lock()
                .expect("stop_handoff mutex not poisoned");
            if handoff_guard.is_some() {
                return Err(DirectProcessFailure::ComponentStopAlreadyInFlight {
                    component_instance,
                });
            }
            let (sender, receiver) = oneshot::channel();
            *handoff_guard = Some(sender);
            drop(handoff_guard);
            Self::terminate_process_group(running.process, libc::SIGTERM)?;
            (
                component,
                receiver,
                running.process,
                running.stop_handoff.clone(),
            )
        };

        let receipt = self
            .await_stop_receipt(component_instance.clone(), process, handoff, receiver)
            .await?;
        // Remove the entry after the watcher signalled exit. The watcher's
        // `JoinHandle` finishes shortly; its abort on Drop is a no-op once
        // the task has already returned.
        self.children.remove(&component_instance);
        self.stop_count = self.stop_count.saturating_add(1);
        debug_assert_eq!(receipt.component(), component);
        Ok(receipt)
    }

    /// Wait on the watcher's stop signal, escalating to SIGKILL if the
    /// graceful timeout elapses. The stop handoff and the watcher both stay
    /// owned by the launcher's `children` map until this method returns;
    /// `await` happens off the per-child borrow so neighbours stay
    /// reachable.
    async fn await_stop_receipt(
        &self,
        component_instance: ComponentInstanceName,
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
                        DirectProcessFailure::StopWaiterCanceled { component_instance }
                    });
                }
            }
        }
    }

    /// Natural-exit path: the watcher observed `child.wait()` returning with
    /// no stop handoff present. Append `ComponentExited` to the manager
    /// event log (when a notifier is wired) and update bookkeeping.
    async fn handle_child_exited(&mut self, exit: ChildProcessExited) {
        if self.children.remove(&exit.component_instance).is_none() {
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
            .values()
            .map(|child| {
                LaunchedComponent::new_instance(
                    child.component_instance.clone(),
                    child.component,
                    child.process,
                )
            })
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
            EngineComponent::Terminal => {
                Self::write_terminal_daemon_configuration_file(envelope).map(Some)
            }
            EngineComponent::Harness => {
                Self::write_harness_daemon_configuration_file(envelope).map(Some)
            }
            EngineComponent::System => {
                Self::write_system_daemon_configuration_file(envelope).map(Some)
            }
            EngineComponent::Spirit => {
                Self::write_spirit_daemon_configuration_file(envelope).map(Some)
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
                envelope
                    .supervision_socket_path()
                    .to_string_lossy()
                    .into_owned(),
            ),
            supervision_socket_mode: signal_persona::SocketMode::new(
                envelope.supervision_socket_mode().as_octal(),
            ),
            router_socket_path: signal_persona::WirePath::new(
                router_socket_path.to_string_lossy().into_owned(),
            ),
            component_ingresses: Self::component_message_ingresses(envelope),
            owner_identity: envelope.owner_identity().clone(),
        };
        let path = Self::daemon_configuration_path(envelope);
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

    fn component_message_ingresses(
        envelope: &ComponentSpawnEnvelope,
    ) -> Vec<signal_persona_message::ComponentMessageIngress> {
        envelope
            .peers()
            .iter()
            .filter(|peer| peer.component() == EngineComponent::Harness)
            .map(|peer| signal_persona_message::ComponentMessageIngress {
                origin: signal_persona_origin::InternalComponentInstanceOrigin::new(
                    signal_persona_origin::ComponentName::Harness,
                    signal_persona_origin::ComponentInstanceName::new(
                        peer.instance_name().as_str(),
                    ),
                ),
                socket_path: signal_persona::WirePath::new(
                    Self::component_message_ingress_socket_path(envelope, peer.instance_name())
                        .to_string_lossy()
                        .into_owned(),
                ),
                socket_mode: signal_persona::SocketMode::new(0o600),
            })
            .collect()
    }

    pub fn component_message_ingress_socket_path(
        envelope: &ComponentSpawnEnvelope,
        component_instance: &ComponentInstanceName,
    ) -> PathBuf {
        envelope
            .domain_socket_path()
            .with_file_name("message-ingress")
            .join(format!("{}.sock", component_instance.as_str()))
    }

    fn write_router_daemon_configuration_file(
        envelope: &ComponentSpawnEnvelope,
    ) -> Result<PathBuf, DirectProcessFailure> {
        let store_path = envelope.state_path().to_path_buf();
        let bootstrap_path = ThreeHarnessRouterBootstrap::for_envelope(envelope)?
            .map(|document| document.write_next_to(envelope.envelope_path()))
            .transpose()?;
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
            bootstrap_path: bootstrap_path
                .map(|path| signal_persona::WirePath::new(path.to_string_lossy().into_owned())),
            owner_identity: envelope.owner_identity().clone(),
        };
        let path = Self::daemon_configuration_path(envelope);
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
        let path = Self::daemon_configuration_path(envelope);
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

    fn write_terminal_daemon_configuration_file(
        envelope: &ComponentSpawnEnvelope,
    ) -> Result<PathBuf, DirectProcessFailure> {
        let configuration = signal_persona_terminal::TerminalDaemonConfiguration {
            terminal_socket_path: signal_persona::WirePath::new(
                envelope.domain_socket_path().to_string_lossy().into_owned(),
            ),
            terminal_socket_mode: signal_persona::SocketMode::new(
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
            owner_identity: envelope.owner_identity().clone(),
        };
        Self::write_configuration_nota_file(envelope, &configuration)
    }

    fn write_harness_daemon_configuration_file(
        envelope: &ComponentSpawnEnvelope,
    ) -> Result<PathBuf, DirectProcessFailure> {
        // The default supervised harness is `Fixture` until the spawn
        // envelope carries a typed harness kind. The supervised
        // production stack will widen this; for the prototype path
        // every supervised harness is fixture-shaped.
        let configuration = signal_persona_harness::HarnessDaemonConfiguration {
            harness_socket_path: signal_persona::WirePath::new(
                envelope.domain_socket_path().to_string_lossy().into_owned(),
            ),
            harness_socket_mode: signal_persona::SocketMode::new(
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
            harness_name: signal_persona_harness::HarnessName::new(
                envelope.component_instance().as_str(),
            ),
            harness_kind: signal_persona_harness::HarnessKind::Fixture,
            terminal_socket_path: Self::paired_terminal_socket_path(envelope),
            owner_identity: envelope.owner_identity().clone(),
        };
        Self::write_configuration_nota_file(envelope, &configuration)
    }

    fn paired_terminal_socket_path(
        envelope: &ComponentSpawnEnvelope,
    ) -> Option<signal_persona::WirePath> {
        let paired_terminal_instance = ComponentInstanceName::new(format!(
            "{}-terminal",
            envelope.component_instance().as_str()
        ));
        let terminal_peer = envelope
            .peers()
            .iter()
            .find(|peer| peer.instance_name() == &paired_terminal_instance)
            .or_else(|| {
                envelope
                    .peers()
                    .iter()
                    .find(|peer| peer.component() == EngineComponent::Terminal)
            })?;
        Some(signal_persona::WirePath::new(
            terminal_peer
                .domain_socket_path()
                .to_string_lossy()
                .into_owned(),
        ))
    }

    fn write_system_daemon_configuration_file(
        envelope: &ComponentSpawnEnvelope,
    ) -> Result<PathBuf, DirectProcessFailure> {
        let configuration = signal_persona_system::SystemDaemonConfiguration {
            system_socket_path: signal_persona::WirePath::new(
                envelope.domain_socket_path().to_string_lossy().into_owned(),
            ),
            system_socket_mode: signal_persona::SocketMode::new(
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
            backend: signal_persona_system::SystemBackend::Niri,
            owner_identity: envelope.owner_identity().clone(),
        };
        Self::write_configuration_nota_file(envelope, &configuration)
    }

    fn write_spirit_daemon_configuration_file(
        envelope: &ComponentSpawnEnvelope,
    ) -> Result<PathBuf, DirectProcessFailure> {
        use spirit_daemon_configuration as spirit;

        let instance = envelope.component_instance().as_str();
        let owner_socket_path = envelope
            .domain_socket_path()
            .with_file_name(format!("owner-{instance}.sock"));
        let upgrade_socket_path = envelope
            .domain_socket_path()
            .with_file_name(format!("{instance}-upgrade.sock"));
        let configuration = spirit::DaemonConfiguration {
            ordinary_socket_path: spirit::SocketPath::new(
                envelope.domain_socket_path().to_string_lossy().into_owned(),
            ),
            owner_socket_path: spirit::SocketPath::new(
                owner_socket_path.to_string_lossy().into_owned(),
            ),
            upgrade_socket_path: spirit::SocketPath::new(
                upgrade_socket_path.to_string_lossy().into_owned(),
            ),
            store_path: spirit::StorePath::new(
                envelope.state_path().to_string_lossy().into_owned(),
            ),
            socket_mode: spirit::SocketMode::new(envelope.domain_socket_mode().as_octal()),
            bootstrap_policy_path: None,
            handoff_control_socket_path: None,
            engine_management_socket_path: Some(spirit::SocketPath::new(
                envelope
                    .supervision_socket_path()
                    .to_string_lossy()
                    .into_owned(),
            )),
            engine_management_socket_mode: Some(spirit::SocketMode::new(
                envelope.supervision_socket_mode().as_octal(),
            )),
        };
        Self::write_configuration_nota_file(envelope, &configuration)
    }

    fn write_configuration_nota_file<C: NotaEncode>(
        envelope: &ComponentSpawnEnvelope,
        configuration: &C,
    ) -> Result<PathBuf, DirectProcessFailure> {
        let path = Self::daemon_configuration_path(envelope);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| DirectProcessFailure::Io {
                operation: "create daemon configuration directory",
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
            operation: "write daemon configuration file",
            source,
        })?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
            |source| DirectProcessFailure::Io {
                operation: "set daemon configuration file mode",
                source,
            },
        )?;
        Ok(path)
    }

    fn daemon_configuration_path(envelope: &ComponentSpawnEnvelope) -> PathBuf {
        envelope.envelope_path().with_file_name(format!(
            "{}-daemon.nota",
            envelope.component_instance().as_str()
        ))
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
        command.env("PERSONA_COMPONENT_KIND", envelope.component().as_str());
        command.env(
            "PERSONA_COMPONENT_INSTANCE",
            envelope.component_instance().as_str(),
        );
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
                format!("PERSONA_PEER_{index}_COMPONENT_INSTANCE"),
                peer.instance_name().as_str(),
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
struct ThreeHarnessRouterBootstrap {
    document: RouterBootstrapDocument,
}

impl ThreeHarnessRouterBootstrap {
    fn for_envelope(
        envelope: &ComponentSpawnEnvelope,
    ) -> Result<Option<Self>, DirectProcessFailure> {
        let required = ["initiator", "responder", "reviewer"];
        let mut operations = Vec::new();
        for name in required {
            let Some(peer) = envelope.peers().iter().find(|peer| {
                peer.component() == EngineComponent::Harness
                    && peer.instance_name().as_str() == name
            }) else {
                return Ok(None);
            };
            operations.push(RouterBootstrapOperation::RegisterActor(
                RouterBootstrapRegisterActor::new(RouterBootstrapActor::new(
                    RouterBootstrapActorIdentifier::new(name),
                    0,
                    Some(RouterBootstrapEndpointTransport::new(
                        RouterBootstrapEndpointKind::HarnessSocket,
                        peer.domain_socket_path().to_string_lossy().into_owned(),
                        None,
                    )),
                )),
            ));
        }

        for (from, to) in [
            ("owner", "initiator"),
            ("owner", "responder"),
            ("owner", "reviewer"),
            ("initiator", "responder"),
            ("responder", "reviewer"),
            ("reviewer", "owner"),
        ] {
            operations.push(RouterBootstrapOperation::GrantDirectMessage(
                RouterBootstrapGrantDirectMessage::new(
                    RouterBootstrapActorIdentifier::new(from),
                    RouterBootstrapActorIdentifier::new(to),
                ),
            ));
        }
        Ok(Some(Self {
            document: RouterBootstrapDocument::new(operations),
        }))
    }

    fn write_next_to(&self, envelope_path: &Path) -> Result<PathBuf, DirectProcessFailure> {
        let path = envelope_path.with_file_name("router-bootstrap.nota");
        let text = self
            .document
            .to_nota_lines()
            .map_err(DirectProcessFailure::Nota)?;
        std::fs::write(&path, text).map_err(|source| DirectProcessFailure::Io {
            operation: "write router bootstrap file",
            source,
        })?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
            |source| DirectProcessFailure::Io {
                operation: "set router bootstrap file mode",
                source,
            },
        )?;
        Ok(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchComponentReceipt {
    component_instance: ComponentInstanceName,
    component: EngineComponent,
    process: ChildProcessId,
}

impl LaunchComponentReceipt {
    fn new_instance(
        component_instance: ComponentInstanceName,
        component: EngineComponent,
        process: ChildProcessId,
    ) -> Self {
        Self {
            component_instance,
            component,
            process,
        }
    }

    pub fn component_instance(&self) -> &ComponentInstanceName {
        &self.component_instance
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

#[derive(Debug, Clone)]
pub struct StopComponentProcess {
    component_instance: ComponentInstanceName,
}

impl StopComponentProcess {
    pub fn new(component: EngineComponent) -> Self {
        Self::for_instance(ComponentInstanceName::from_component(component))
    }

    pub fn for_instance(component_instance: ComponentInstanceName) -> Self {
        Self { component_instance }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopComponentReceipt {
    component_instance: ComponentInstanceName,
    component: EngineComponent,
    process: ChildProcessId,
}

impl StopComponentReceipt {
    fn new_instance(
        component_instance: ComponentInstanceName,
        component: EngineComponent,
        process: ChildProcessId,
    ) -> Self {
        Self {
            component_instance,
            component,
            process,
        }
    }

    pub fn component_instance(&self) -> &ComponentInstanceName {
        &self.component_instance
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
        self.stop(message.component_instance).await
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
    component_instance: ComponentInstanceName,
    component: EngineComponent,
    process: ChildProcessId,
    exit_code: Option<i32>,
}

impl ChildProcessExited {
    pub fn component_instance(&self) -> &ComponentInstanceName {
        &self.component_instance
    }

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
    #[error("component instance {component_instance:?} already has a running child process")]
    ComponentAlreadyRunning {
        component_instance: ComponentInstanceName,
    },
    #[error("component instance {component_instance:?} has no running child process")]
    ComponentNotRunning {
        component_instance: ComponentInstanceName,
    },
    #[error("component instance {component_instance:?} already has a stop in flight")]
    ComponentStopAlreadyInFlight {
        component_instance: ComponentInstanceName,
    },
    #[error("component instance {component_instance:?} stop waiter was canceled before exit")]
    StopWaiterCanceled {
        component_instance: ComponentInstanceName,
    },
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
