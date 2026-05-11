use persona::schema::{
    ComponentDesiredState, ComponentHealth, ComponentKind, EnginePhase, EngineStatusReport,
};
use signal_persona::{
    ComponentDesiredState as ContractDesiredState, ComponentHealth as ContractHealth,
    ComponentKind as ContractKind, ComponentName, ComponentStatus, EngineGeneration,
    EnginePhase as ContractPhase, EngineStatus,
};

struct SchemaFixture {
    status: EngineStatus,
}

impl SchemaFixture {
    fn starting_engine() -> Self {
        Self {
            status: EngineStatus {
                generation: EngineGeneration::new(3),
                phase: ContractPhase::Starting,
                components: vec![ComponentStatus {
                    name: ComponentName::new("persona-system"),
                    kind: ContractKind::System,
                    desired_state: ContractDesiredState::Running,
                    health: ContractHealth::Starting,
                }],
            },
        }
    }

    fn report(&self) -> EngineStatusReport {
        EngineStatusReport::from_contract(self.status.clone())
    }
}

#[test]
fn engine_status_report_round_trips_as_nota() {
    let report = SchemaFixture::starting_engine().report();
    let encoded = report.to_nota().unwrap();
    let recovered = EngineStatusReport::from_nota(&encoded).unwrap();

    assert_eq!(recovered, report);
    assert!(encoded.starts_with("(EngineStatusReport 3 Starting ["));
}

#[test]
fn signal_persona_status_projects_to_nota_enums() {
    let report = SchemaFixture::starting_engine().report();
    let component = report.components.first().unwrap();

    assert_eq!(report.phase, EnginePhase::Starting);
    assert_eq!(component.kind, ComponentKind::System);
    assert_eq!(component.desired_state, ComponentDesiredState::Running);
    assert_eq!(component.health, ComponentHealth::Starting);
    assert_eq!(component.name.as_str(), "persona-system");
}
