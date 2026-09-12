use meta_signal_persona::{
    EnginePhase as ContractPhase, EngineStatusReport as ContractEngineStatusReport,
};
use persona::datom_text::DatomActualizable;
use persona::schema::EngineStatusReport;
use signal_persona::{
    ComponentDesiredState as ContractDesiredState, ComponentHealth as ContractHealth,
    ComponentKind as ContractKind, ComponentStatus,
};

struct SchemaFixture {
    status: ContractEngineStatusReport,
}

impl SchemaFixture {
    fn starting_engine() -> Self {
        Self {
            status: ContractEngineStatusReport {
                engine_generation: 3,
                engine_phase: ContractPhase::Starting,
                component_status_vector: vec![ComponentStatus {
                    component_name: "persona-system".to_string(),
                    component_kind: ContractKind::System,
                    component_desired_state: ContractDesiredState::Running,
                    component_health: ContractHealth::Starting,
                }],
            },
        }
    }

    fn message_engine() -> Self {
        Self {
            status: ContractEngineStatusReport {
                engine_generation: 4,
                engine_phase: ContractPhase::Running,
                component_status_vector: vec![ComponentStatus {
                    component_name: "persona-message".to_string(),
                    component_kind: ContractKind::Message,
                    component_desired_state: ContractDesiredState::Running,
                    component_health: ContractHealth::Running,
                }],
            },
        }
    }

    fn report(&self) -> EngineStatusReport {
        EngineStatusReport::from_contract(self.status.clone())
    }
}

#[test]
fn engine_status_report_round_trips_as_datom() {
    let report = SchemaFixture::starting_engine().report();
    let encoded = report.textualize();
    let recovered = EngineStatusReport::actualize_text(&encoded)
        .unwrap_or_else(|fault| panic!("report restores from {encoded}: {fault:?}"));

    assert_eq!(recovered, report);
}

#[test]
fn signal_persona_status_projects_its_contract_enums() {
    let report = SchemaFixture::starting_engine().report();
    let component = report.components.first().unwrap();

    assert_eq!(report.generation, 3);
    assert_eq!(report.phase, "Starting");
    assert_eq!(component.kind, "System");
    assert_eq!(component.desired_state, "Running");
    assert_eq!(component.health, "Starting");
    assert_eq!(component.component.as_str(), "persona-system");
}

#[test]
fn signal_message_kind_projects_without_widening() {
    let report = SchemaFixture::message_engine().report();
    let component = report.components.first().unwrap();
    let encoded = report.textualize();

    assert_eq!(component.kind, "Message");
    assert!(encoded.contains("Message"));
    assert!(!encoded.contains("MessageProxy"));
}
