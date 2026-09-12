use meta_signal_persona::{
    ActionAcceptance, ActionRejection, ActionRejectionReason, ComponentShutdown, ComponentStartup,
    EnginePhase, EngineStatusReport, Response,
};
use signal_persona::{
    ComponentDesiredState, ComponentHealth, ComponentName, ComponentStatus,
};

use crate::engine::EngineComponent;

#[derive(Debug, Clone, PartialEq)]
pub struct EngineState {
    status: EngineStatusReport,
}

impl EngineState {
    pub fn default_catalog() -> Self {
        Self {
            status: EngineStatusReport {
                engine_generation: 0,
                engine_phase: EnginePhase::Starting,
                component_status_vector: EngineComponent::prototype_supervised_components()
                    .into_iter()
                    .map(|component| ComponentStatus {
                        component_name: component.component_name(),
                        component_kind: component.component_kind(),
                        component_desired_state: ComponentDesiredState::Running,
                        component_health: ComponentHealth::Starting,
                    })
                    .collect(),
            },
        }
    }

    pub fn from_status(status: EngineStatusReport) -> Self {
        Self { status }
    }

    pub fn snapshot(&self) -> &EngineStatusReport {
        &self.status
    }

    pub fn status(&self) -> EngineStatusReport {
        self.status.clone()
    }

    pub fn engine_status(&self) -> Response {
        Response::EngineStatus(self.status())
    }

    pub fn component_status(&self, component: ComponentName) -> Response {
        self.status
            .component_status_vector
            .iter()
            .find(|status| status.component_name == component)
            .cloned()
            .map(Response::ComponentStatus)
            .unwrap_or(Response::ComponentMissing(component))
    }

    pub fn start_component(&mut self, startup: ComponentStartup) -> Response {
        let component = startup;
        let Some(status) = self.component_mut(&component) else {
            return Response::ActionRejected(
                ActionRejection {
                    component_name: component.clone(),
                    action_rejection_reason: ActionRejectionReason::ComponentNotManaged,
                },
            );
        };
        if status.component_desired_state == ComponentDesiredState::Running {
            return Response::ActionRejected(
                ActionRejection {
                    component_name: component.clone(),
                    action_rejection_reason:
                        ActionRejectionReason::ComponentAlreadyInDesiredState,
                },
            );
        }
        status.component_desired_state = ComponentDesiredState::Running;
        status.component_health = ComponentHealth::Starting;
        self.advance_generation();
        self.refresh_phase();
        Response::ActionAccepted(
            ActionAcceptance {
                component_name: component,
                component_desired_state: ComponentDesiredState::Running,
            },
        )
    }

    pub fn stop_component(&mut self, shutdown: ComponentShutdown) -> Response {
        let component = shutdown;
        let Some(status) = self.component_mut(&component) else {
            return Response::ActionRejected(
                ActionRejection {
                    component_name: component.clone(),
                    action_rejection_reason: ActionRejectionReason::ComponentNotManaged,
                },
            );
        };
        if status.component_desired_state == ComponentDesiredState::Stopped {
            return Response::ActionRejected(
                ActionRejection {
                    component_name: component.clone(),
                    action_rejection_reason:
                        ActionRejectionReason::ComponentAlreadyInDesiredState,
                },
            );
        }
        status.component_desired_state = ComponentDesiredState::Stopped;
        status.component_health = ComponentHealth::Stopped;
        self.advance_generation();
        self.refresh_phase();
        Response::ActionAccepted(
            ActionAcceptance {
                component_name: component,
                component_desired_state: ComponentDesiredState::Stopped,
            },
        )
    }

    fn component_mut(
        &mut self,
        component: &ComponentName,
    ) -> Option<&mut ComponentStatus> {
        self.status
            .component_status_vector
            .iter_mut()
            .find(|status| status.component_name == *component)
    }

    /// Overwrite one component's `health` field from a manager snapshot row.
    /// `desired_state` and other fields stay untouched: snapshots reflect the
    /// observed runtime, while `desired_state` is operator intent.
    pub fn set_component_health(&mut self, component: &ComponentName, health: ComponentHealth) {
        if let Some(status) = self.component_mut(component) {
            status.component_health = health;
            self.refresh_phase();
        }
    }

    fn advance_generation(&mut self) {
        self.status.engine_generation = self.status.engine_generation.saturating_add(1);
    }

    fn refresh_phase(&mut self) {
        self.status.engine_phase = if self
            .status
            .component_status_vector
            .iter()
            .all(|status| status.component_desired_state == ComponentDesiredState::Stopped)
        {
            EnginePhase::Stopped
        } else if self.status.component_status_vector.iter().any(|status| {
            matches!(
                status.component_health,
                ComponentHealth::Failed | ComponentHealth::Degraded
            )
        }) {
            EnginePhase::Degraded
        } else if self
            .status
            .component_status_vector
            .iter()
            .any(|status| status.component_health == ComponentHealth::Starting)
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
