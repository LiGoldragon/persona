use kameo::actor::{Actor, ActorRef, Spawn};
use kameo::error::{Infallible, SendError};
use kameo::message::{Context, Message};
use thiserror::Error;

use crate::direct_process::{
    DirectProcessFailure, DirectProcessLauncher, LaunchComponent, LaunchedComponent,
    ReadLauncherSnapshot, StopComponentProcess,
};
use crate::engine::{EngineComponent, EngineLayout};
use crate::engine_event::{
    ComponentLifecycleEvent, EngineEventBody, EngineEventDraft, EngineEventDraftInput,
    EngineEventSource,
};
use crate::launch::{
    CommandResolutionFailure, ComponentCommandCatalog, ComponentCommandResolver,
    EngineLaunchConfiguration, ResolveComponentCommands,
};
use crate::manager_store::{AppendEngineEvent, ManagerStore};

#[derive(Debug)]
pub struct EngineSupervisor {
    layout: EngineLayout,
    launch_configuration: EngineLaunchConfiguration,
    resolver: ActorRef<ComponentCommandResolver>,
    launcher: ActorRef<DirectProcessLauncher>,
    store: Option<ActorRef<ManagerStore>>,
    started_stack_count: u64,
    stopped_stack_count: u64,
}

impl EngineSupervisor {
    pub fn new(input: EngineSupervisorInput) -> Self {
        Self {
            layout: input.layout,
            launch_configuration: input.launch_configuration,
            resolver: ComponentCommandResolver::spawn(ComponentCommandResolver::new(
                input.command_catalog,
            )),
            launcher: DirectProcessLauncher::spawn(DirectProcessLauncher::new()),
            store: input.store,
            started_stack_count: 0,
            stopped_stack_count: 0,
        }
    }

    pub fn start(input: EngineSupervisorInput) -> ActorRef<Self> {
        let reference = Self::spawn(Self::new(input));
        reference
    }

    async fn start_first_stack(&mut self) -> Result<FirstStackReport, EngineSupervisorFailure> {
        self.layout
            .prepare_directories()
            .map_err(EngineSupervisorFailure::PrepareEngineLayout)?;
        let resolved = match self
            .resolver
            .ask(ResolveComponentCommands::new(
                self.launch_configuration.clone(),
            ))
            .await
        {
            Ok(resolved) => resolved,
            Err(SendError::HandlerError(failure)) => {
                return Err(EngineSupervisorFailure::CommandResolution(failure));
            }
            Err(error) => {
                return Err(EngineSupervisorFailure::Actor {
                    operation: "resolve component commands",
                    detail: format!("{error:?}"),
                });
            }
        };

        let mut launched = Vec::new();
        for component in EngineComponent::first_stack() {
            let envelope = self
                .layout
                .spawn_envelope(component, &resolved)
                .ok_or(EngineSupervisorFailure::MissingSpawnEnvelope { component })?;
            let receipt = match self.launcher.ask(LaunchComponent::new(envelope)).await {
                Ok(receipt) => receipt,
                Err(SendError::HandlerError(failure)) => {
                    return Err(EngineSupervisorFailure::DirectProcess(failure));
                }
                Err(error) => {
                    return Err(EngineSupervisorFailure::Actor {
                        operation: "launch component process",
                        detail: format!("{error:?}"),
                    });
                }
            };
            self.append_component_event(EngineEventBody::ComponentSpawned(
                ComponentLifecycleEvent::new(receipt.component().component_name()),
            ))
            .await?;
            launched.push(LaunchedComponent::new(
                receipt.component(),
                receipt.process(),
            ));
        }

        self.started_stack_count = self.started_stack_count.saturating_add(1);
        Ok(FirstStackReport::new(launched))
    }

    async fn stop_first_stack(&mut self) -> Result<FirstStackReport, EngineSupervisorFailure> {
        let snapshot = self
            .launcher
            .ask(ReadLauncherSnapshot)
            .await
            .map_err(|error| EngineSupervisorFailure::Actor {
                operation: "read launcher snapshot",
                detail: format!("{error:?}"),
            })?;
        let mut stopped = Vec::new();
        for launched in snapshot.running().iter().rev() {
            let receipt = match self
                .launcher
                .ask(StopComponentProcess::new(launched.component()))
                .await
            {
                Ok(receipt) => receipt,
                Err(SendError::HandlerError(failure)) => {
                    return Err(EngineSupervisorFailure::DirectProcess(failure));
                }
                Err(error) => {
                    return Err(EngineSupervisorFailure::Actor {
                        operation: "stop component process",
                        detail: format!("{error:?}"),
                    });
                }
            };
            self.append_component_event(EngineEventBody::ComponentStopped(
                ComponentLifecycleEvent::new(receipt.component().component_name()),
            ))
            .await?;
            stopped.push(LaunchedComponent::new(
                receipt.component(),
                receipt.process(),
            ));
        }
        self.stopped_stack_count = self.stopped_stack_count.saturating_add(1);
        Ok(FirstStackReport::new(stopped))
    }

    async fn append_component_event(
        &self,
        body: EngineEventBody,
    ) -> Result<(), EngineSupervisorFailure> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        let draft = EngineEventDraft::from_input(EngineEventDraftInput {
            engine: self.layout.engine().clone(),
            source: EngineEventSource::Manager,
            body,
        });
        match store.ask(AppendEngineEvent::new(draft)).await {
            Ok(_) => {}
            Err(SendError::HandlerError(error)) => {
                return Err(EngineSupervisorFailure::ManagerStore {
                    detail: error.to_string(),
                });
            }
            Err(error) => {
                return Err(EngineSupervisorFailure::Actor {
                    operation: "append manager lifecycle event",
                    detail: format!("{error:?}"),
                });
            }
        }
        Ok(())
    }

    async fn read_snapshot(&self) -> Result<EngineSupervisorSnapshot, EngineSupervisorFailure> {
        let launcher = self
            .launcher
            .ask(ReadLauncherSnapshot)
            .await
            .map_err(|error| EngineSupervisorFailure::Actor {
                operation: "read launcher snapshot",
                detail: format!("{error:?}"),
            })?;
        Ok(EngineSupervisorSnapshot {
            running: launcher.running().to_vec(),
            started_stack_count: self.started_stack_count,
            stopped_stack_count: self.stopped_stack_count,
        })
    }
}

#[derive(Debug)]
pub struct EngineSupervisorInput {
    pub layout: EngineLayout,
    pub command_catalog: ComponentCommandCatalog,
    pub launch_configuration: EngineLaunchConfiguration,
    pub store: Option<ActorRef<ManagerStore>>,
}

impl Actor for EngineSupervisor {
    type Args = Self;
    type Error = Infallible;

    async fn on_start(
        supervisor: Self::Args,
        _actor_reference: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(supervisor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstStackReport {
    components: Vec<LaunchedComponent>,
}

impl FirstStackReport {
    fn new(components: Vec<LaunchedComponent>) -> Self {
        Self { components }
    }

    pub fn components(&self) -> &[LaunchedComponent] {
        self.components.as_slice()
    }
}

pub struct StartFirstStack;

impl Message<StartFirstStack> for EngineSupervisor {
    type Reply = Result<FirstStackReport, EngineSupervisorFailure>;

    async fn handle(
        &mut self,
        _message: StartFirstStack,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.start_first_stack().await
    }
}

pub struct StopFirstStack;

impl Message<StopFirstStack> for EngineSupervisor {
    type Reply = Result<FirstStackReport, EngineSupervisorFailure>;

    async fn handle(
        &mut self,
        _message: StopFirstStack,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.stop_first_stack().await
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadEngineSupervisorSnapshot;

impl Message<ReadEngineSupervisorSnapshot> for EngineSupervisor {
    type Reply = Result<EngineSupervisorSnapshot, EngineSupervisorFailure>;

    async fn handle(
        &mut self,
        _message: ReadEngineSupervisorSnapshot,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.read_snapshot().await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSupervisorSnapshot {
    running: Vec<LaunchedComponent>,
    started_stack_count: u64,
    stopped_stack_count: u64,
}

impl EngineSupervisorSnapshot {
    pub fn running(&self) -> &[LaunchedComponent] {
        self.running.as_slice()
    }

    pub fn started_stack_count(&self) -> u64 {
        self.started_stack_count
    }

    pub fn stopped_stack_count(&self) -> u64 {
        self.stopped_stack_count
    }
}

#[derive(Debug, Error)]
pub enum EngineSupervisorFailure {
    #[error("prepare engine layout: {0}")]
    PrepareEngineLayout(#[from] crate::Error),

    #[error("component command resolution: {0}")]
    CommandResolution(#[from] CommandResolutionFailure),

    #[error("missing spawn envelope for component {component:?}")]
    MissingSpawnEnvelope { component: EngineComponent },

    #[error("direct process launcher: {0}")]
    DirectProcess(#[from] DirectProcessFailure),

    #[error("manager store: {detail}")]
    ManagerStore { detail: String },

    #[error("actor failed during {operation}: {detail}")]
    Actor {
        operation: &'static str,
        detail: String,
    },
}
