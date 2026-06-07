use owner_signal_persona::Reply;
use owner_signal_persona::{
    ActionAcceptance, ActionRejection, ActionRejectionReason, ComponentDesiredState,
    ComponentHealth, ComponentName, ComponentShutdown, ComponentStartup, ComponentStatus,
    EngineGeneration, EnginePhase, EngineStatus,
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
                components: EngineComponent::prototype_supervised_components()
                    .into_iter()
                    .map(|component| ComponentStatus {
                        name: component.component_name(),
                        kind: component.component_kind(),
                        desired_state: ComponentDesiredState::Running,
                        health: ComponentHealth::Starting,
                    })
                    .collect(),
            },
        }
    }

    pub fn from_status(status: EngineStatus) -> Self {
        Self { status }
    }

    pub fn snapshot(&self) -> &EngineStatus {
        &self.status
    }

    pub fn engine_status(&self) -> Reply {
        Reply::EngineStatus(self.status.clone())
    }

    pub fn component_status(&self, component: ComponentName) -> Reply {
        self.status
            .components
            .iter()
            .find(|status| status.name == component)
            .cloned()
            .map(Reply::ComponentStatus)
            .unwrap_or(Reply::ComponentMissing(component))
    }

    pub fn start_component(&mut self, startup: ComponentStartup) -> Reply {
        let component = startup.component;
        let Some(status) = self.component_mut(&component) else {
            return Reply::ActionRejected(ActionRejection {
                component,
                reason: ActionRejectionReason::ComponentNotManaged,
            });
        };
        if status.desired_state == ComponentDesiredState::Running {
            return Reply::ActionRejected(ActionRejection {
                component,
                reason: ActionRejectionReason::ComponentAlreadyInDesiredState,
            });
        }
        status.desired_state = ComponentDesiredState::Running;
        status.health = ComponentHealth::Starting;
        self.advance_generation();
        self.refresh_phase();
        Reply::ActionAccepted(ActionAcceptance {
            component,
            desired_state: ComponentDesiredState::Running,
        })
    }

    pub fn stop_component(&mut self, shutdown: ComponentShutdown) -> Reply {
        let component = shutdown.component;
        let Some(status) = self.component_mut(&component) else {
            return Reply::ActionRejected(ActionRejection {
                component,
                reason: ActionRejectionReason::ComponentNotManaged,
            });
        };
        if status.desired_state == ComponentDesiredState::Stopped {
            return Reply::ActionRejected(ActionRejection {
                component,
                reason: ActionRejectionReason::ComponentAlreadyInDesiredState,
            });
        }
        status.desired_state = ComponentDesiredState::Stopped;
        status.health = ComponentHealth::Stopped;
        self.advance_generation();
        self.refresh_phase();
        Reply::ActionAccepted(ActionAcceptance {
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

    /// Overwrite one component's `health` field from a manager snapshot row.
    /// `desired_state` and other fields stay untouched: snapshots reflect the
    /// observed runtime, while `desired_state` is operator intent.
    pub fn set_component_health(&mut self, component: &ComponentName, health: ComponentHealth) {
        if let Some(status) = self.component_mut(component) {
            status.health = health;
            self.refresh_phase();
        }
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
