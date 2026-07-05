use meta_signal_persona::{
    ActionAcceptance, ActionRejection, ActionRejectionReason, ComponentDesiredState,
    ComponentHealth, ComponentName, ComponentShutdown, ComponentStartup, EngineGeneration,
    EnginePhase, EngineStatus, EngineStatusReport, LifecycleComponentStatus, Reply,
};

use crate::engine::EngineComponent;
use crate::generated_contract::EngineGenerationValue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineState {
    status: EngineStatusReport,
}

impl EngineState {
    pub fn default_catalog() -> Self {
        Self {
            status: EngineStatusReport {
                generation: EngineGeneration::new(0),
                phase: EnginePhase::Starting,
                components: EngineComponent::prototype_supervised_components()
                    .into_iter()
                    .map(|component| LifecycleComponentStatus {
                        component_name: component.component_name(),
                        component_kind: component.component_kind(),
                        component_desired_state: ComponentDesiredState::Running,
                        component_health: ComponentHealth::Starting,
                    })
                    .collect(),
            },
        }
    }

    pub fn from_status(status: EngineStatus) -> Self {
        Self {
            status: status.into_payload(),
        }
    }

    pub fn snapshot(&self) -> &EngineStatusReport {
        &self.status
    }

    pub fn status(&self) -> EngineStatus {
        EngineStatus::new(self.status.clone())
    }

    pub fn engine_status(&self) -> Reply {
        Reply::EngineStatus(self.status().into())
    }

    pub fn component_status(&self, component: ComponentName) -> Reply {
        self.status
            .components
            .iter()
            .find(|status| status.component_name == component)
            .cloned()
            .map(|status| Reply::ComponentStatus(status.into()))
            .unwrap_or_else(|| Reply::ComponentMissing(component.into()))
    }

    pub fn start_component(&mut self, startup: ComponentStartup) -> Reply {
        let component = startup.into_payload();
        let Some(status) = self.component_mut(&component) else {
            return Reply::ActionRejected(
                ActionRejection {
                    component: component.clone(),
                    reason: ActionRejectionReason::ComponentNotManaged,
                }
                .into(),
            );
        };
        if status.component_desired_state == ComponentDesiredState::Running {
            return Reply::ActionRejected(
                ActionRejection {
                    component: component.clone(),
                    reason: ActionRejectionReason::ComponentAlreadyInDesiredState,
                }
                .into(),
            );
        }
        status.component_desired_state = ComponentDesiredState::Running;
        status.component_health = ComponentHealth::Starting;
        self.advance_generation();
        self.refresh_phase();
        Reply::ActionAccepted(
            ActionAcceptance {
                component,
                desired_state: ComponentDesiredState::Running,
            }
            .into(),
        )
    }

    pub fn stop_component(&mut self, shutdown: ComponentShutdown) -> Reply {
        let component = shutdown.into_payload();
        let Some(status) = self.component_mut(&component) else {
            return Reply::ActionRejected(
                ActionRejection {
                    component: component.clone(),
                    reason: ActionRejectionReason::ComponentNotManaged,
                }
                .into(),
            );
        };
        if status.component_desired_state == ComponentDesiredState::Stopped {
            return Reply::ActionRejected(
                ActionRejection {
                    component: component.clone(),
                    reason: ActionRejectionReason::ComponentAlreadyInDesiredState,
                }
                .into(),
            );
        }
        status.component_desired_state = ComponentDesiredState::Stopped;
        status.component_health = ComponentHealth::Stopped;
        self.advance_generation();
        self.refresh_phase();
        Reply::ActionAccepted(
            ActionAcceptance {
                component,
                desired_state: ComponentDesiredState::Stopped,
            }
            .into(),
        )
    }

    fn component_mut(
        &mut self,
        component: &ComponentName,
    ) -> Option<&mut LifecycleComponentStatus> {
        self.status
            .components
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
        self.status.generation =
            EngineGeneration::new(self.status.generation.clone().into_u64().saturating_add(1));
    }

    fn refresh_phase(&mut self) {
        self.status.phase = if self
            .status
            .components
            .iter()
            .all(|status| status.component_desired_state == ComponentDesiredState::Stopped)
        {
            EnginePhase::Stopped
        } else if self.status.components.iter().any(|status| {
            matches!(
                status.component_health,
                ComponentHealth::Failed | ComponentHealth::Degraded
            )
        }) {
            EnginePhase::Degraded
        } else if self
            .status
            .components
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
