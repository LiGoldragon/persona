use signal_persona::{
    ComponentDesiredState, ComponentHealth, ComponentKind, ComponentName, ComponentShutdown,
    ComponentStartup, ComponentStatus, ComponentStatusMissing, ComponentStatusQuery,
    EngineGeneration, EnginePhase, EngineReply, EngineStatus, SupervisorActionAcceptance,
    SupervisorActionRejection, SupervisorActionRejectionReason,
};

use crate::engine::EngineComponent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineState {
    status: EngineStatus,
}

impl EngineState {
    pub fn default_catalog() -> Self {
        Self {
            status: EngineStatus {
                generation: EngineGeneration::new(0),
                phase: EnginePhase::Starting,
                components: vec![
                    ComponentStatus {
                        name: EngineComponent::Mind.component_name(),
                        kind: ComponentKind::Mind,
                        desired_state: ComponentDesiredState::Running,
                        health: ComponentHealth::Starting,
                    },
                    ComponentStatus {
                        name: EngineComponent::Router.component_name(),
                        kind: ComponentKind::Router,
                        desired_state: ComponentDesiredState::Running,
                        health: ComponentHealth::Starting,
                    },
                    ComponentStatus {
                        name: EngineComponent::System.component_name(),
                        kind: ComponentKind::System,
                        desired_state: ComponentDesiredState::Running,
                        health: ComponentHealth::Starting,
                    },
                    ComponentStatus {
                        name: EngineComponent::Harness.component_name(),
                        kind: ComponentKind::Harness,
                        desired_state: ComponentDesiredState::Running,
                        health: ComponentHealth::Starting,
                    },
                    ComponentStatus {
                        name: EngineComponent::Terminal.component_name(),
                        kind: ComponentKind::Terminal,
                        desired_state: ComponentDesiredState::Running,
                        health: ComponentHealth::Starting,
                    },
                    ComponentStatus {
                        name: EngineComponent::MessageProxy.component_name(),
                        kind: ComponentKind::MessageProxy,
                        desired_state: ComponentDesiredState::Running,
                        health: ComponentHealth::Starting,
                    },
                ],
            },
        }
    }

    pub fn snapshot(&self) -> &EngineStatus {
        &self.status
    }

    pub fn engine_status(&self) -> EngineReply {
        EngineReply::EngineStatus(self.status.clone())
    }

    pub fn component_status(&self, query: ComponentStatusQuery) -> EngineReply {
        let component = query.component;
        self.status
            .components
            .iter()
            .find(|status| status.name == component)
            .cloned()
            .map(EngineReply::ComponentStatus)
            .unwrap_or(EngineReply::ComponentStatusMissing(
                ComponentStatusMissing { component },
            ))
    }

    pub fn start_component(&mut self, startup: ComponentStartup) -> EngineReply {
        let component = startup.component;
        let Some(status) = self.component_mut(&component) else {
            return EngineReply::SupervisorActionRejected(SupervisorActionRejection {
                component,
                reason: SupervisorActionRejectionReason::ComponentNotManaged,
            });
        };
        if status.desired_state == ComponentDesiredState::Running {
            return EngineReply::SupervisorActionRejected(SupervisorActionRejection {
                component,
                reason: SupervisorActionRejectionReason::ComponentAlreadyInDesiredState,
            });
        }
        status.desired_state = ComponentDesiredState::Running;
        status.health = ComponentHealth::Starting;
        self.advance_generation();
        self.refresh_phase();
        EngineReply::SupervisorActionAccepted(SupervisorActionAcceptance {
            component,
            desired_state: ComponentDesiredState::Running,
        })
    }

    pub fn stop_component(&mut self, shutdown: ComponentShutdown) -> EngineReply {
        let component = shutdown.component;
        let Some(status) = self.component_mut(&component) else {
            return EngineReply::SupervisorActionRejected(SupervisorActionRejection {
                component,
                reason: SupervisorActionRejectionReason::ComponentNotManaged,
            });
        };
        if status.desired_state == ComponentDesiredState::Stopped {
            return EngineReply::SupervisorActionRejected(SupervisorActionRejection {
                component,
                reason: SupervisorActionRejectionReason::ComponentAlreadyInDesiredState,
            });
        }
        status.desired_state = ComponentDesiredState::Stopped;
        status.health = ComponentHealth::Stopped;
        self.advance_generation();
        self.refresh_phase();
        EngineReply::SupervisorActionAccepted(SupervisorActionAcceptance {
            component,
            desired_state: ComponentDesiredState::Stopped,
        })
    }

    fn component_mut(&mut self, component: &ComponentName) -> Option<&mut ComponentStatus> {
        self.status
            .components
            .iter_mut()
            .find(|status| status.name == *component)
    }

    fn advance_generation(&mut self) {
        self.status.generation =
            EngineGeneration::new(self.status.generation.into_u64().saturating_add(1));
    }

    fn refresh_phase(&mut self) {
        self.status.phase = if self
            .status
            .components
            .iter()
            .all(|status| status.desired_state == ComponentDesiredState::Stopped)
        {
            EnginePhase::Stopped
        } else if self.status.components.iter().any(|status| {
            matches!(
                status.health,
                ComponentHealth::Failed | ComponentHealth::Degraded
            )
        }) {
            EnginePhase::Degraded
        } else if self
            .status
            .components
            .iter()
            .any(|status| status.health == ComponentHealth::Starting)
        {
            EnginePhase::Starting
        } else {
            EnginePhase::Running
        };
    }
}

impl Default for EngineState {
    fn default() -> Self {
        Self::default_catalog()
    }
}
