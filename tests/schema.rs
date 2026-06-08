use meta_signal_persona::{
    ComponentDesiredState as ContractDesiredState, ComponentHealth as ContractHealth,
    ComponentKind as ContractKind, ComponentName, ComponentStatus, EngineGeneration,
    EnginePhase as ContractPhase, EngineStatus,
};
use persona::schema::{
    ComponentDesiredState, ComponentHealth, ComponentKind, EnginePhase, EngineStatusReport,
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

    fn message_engine() -> Self {
        Self {
            status: EngineStatus {
                generation: EngineGeneration::new(4),
                phase: ContractPhase::Running,
                components: vec![ComponentStatus {
                    name: ComponentName::new("persona-message"),
                    kind: ContractKind::Message,
                    desired_state: ContractDesiredState::Running,
                    health: ContractHealth::Running,
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
    let encoded = report.to_nota();
    let recovered = EngineStatusReport::from_nota(&encoded).unwrap();

    assert_eq!(recovered, report);
    assert!(
        encoded.starts_with("(3 Starting ["),
        "encoded report: {encoded}"
    );
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

#[test]
fn signal_message_kind_projects_to_nota() {
    let report = SchemaFixture::message_engine().report();
    let component = report.components.first().unwrap();
    let encoded = report.to_nota();

    assert_eq!(component.kind, ComponentKind::Message);
    assert!(encoded.contains("Message"));
    assert!(!encoded.contains("MessageProxy"));
}

#[test]
fn persona_meta_schema_cannot_restore_system_prompt_gate_operations() {
    let engine_event_source = include_str!("../src/engine_event.rs");
    let schema_source = include_str!("../src/schema/reports.rs");

    for forbidden in [
        "InputBuffer",
        "input-buffer",
        "prompt/input-buffer",
        "prompt-buffer",
    ] {
        assert!(
            !engine_event_source.contains(forbidden),
            "engine event schema must not own terminal prompt gate vocabulary: {forbidden}"
        );
        assert!(
            !schema_source.contains(forbidden),
            "NOTA schema projection must not own terminal prompt gate vocabulary: {forbidden}"
        );
    }
}
