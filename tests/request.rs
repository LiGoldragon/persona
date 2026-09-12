use meta_signal_persona::{
    EnginePhase, EngineStatusReport as ContractEngineStatusReport, MetaQuery,
    Query as EngineRequest, Response as EngineReply,
};
use persona::datom_text::{DatomActualizable, DatomTextualizable};
use persona::request::{
    CommandLine, ComponentStatusQuery, EngineStatusQuery, EngineStatusScope, PersonaOutput,
    PersonaRequest,
};
use persona::schema::{EngineStatusReport, LifecycleComponentStatusReport};
use signal_persona::{ComponentDesiredState, ComponentHealth, ComponentKind, ComponentStatus};

/// The inline argument a shell would hand Persona, built by the codec that
/// reads it back — never spelled by hand.
fn inline_arguments(request: &PersonaRequest) -> Vec<String> {
    request
        .textualize()
        .split(' ')
        .map(str::to_owned)
        .collect()
}

#[test]
fn empty_command_line_queries_engine_status() {
    let request = CommandLine::from_arguments(std::iter::empty::<&str>())
        .decode_request()
        .unwrap();

    assert_eq!(
        request,
        PersonaRequest::EngineStatusQuery(EngineStatusQuery {
            scope: EngineStatusScope::WholeEngine,
        })
    );
}

#[test]
fn inline_datom_request_decodes_after_shell_token_join() {
    let original = PersonaRequest::ComponentStatusQuery(ComponentStatusQuery {
        component: "persona-router".to_string(),
    });
    let decoded = CommandLine::from_arguments(inline_arguments(&original))
        .decode_request()
        .unwrap();

    assert_eq!(decoded, original);
}

#[test]
fn persona_request_lowers_to_signal_persona_engine_request() {
    let request = PersonaRequest::ComponentStatusQuery(ComponentStatusQuery {
        component: "persona-system".to_string(),
    });

    match request.into_engine_request() {
        EngineRequest::Query(MetaQuery::ComponentStatus(component)) => {
            assert_eq!(component.as_str(), "persona-system");
        }
        other => panic!("expected component status query, got {other:?}"),
    }
}

#[test]
fn engine_status_reply_renders_as_datom() {
    let reply = EngineReply::EngineStatus(ContractEngineStatusReport {
        engine_generation: 2,
        engine_phase: EnginePhase::Starting,
        component_status_vector: vec![ComponentStatus {
            component_name: "mind".to_string(),
            component_kind: ComponentKind::Mind,
            component_desired_state: ComponentDesiredState::Running,
            component_health: ComponentHealth::Starting,
        }],
    });
    let output = PersonaOutput::from_engine_reply(reply);
    let text = output.textualize();

    assert_eq!(PersonaOutput::actualize_text(&text).unwrap(), output);
    assert!(text.contains("Mind"), "output: {text}");
    assert!(text.contains("Starting"), "output: {text}");
}

#[test]
fn output_round_trips_through_datom() {
    let output = PersonaOutput::EngineStatusReport(EngineStatusReport {
        generation: 1,
        phase: "Starting".to_owned(),
        components: vec![LifecycleComponentStatusReport {
            component: "persona-router".to_string(),
            kind: "Router".to_owned(),
            desired_state: "Running".to_owned(),
            health: "Starting".to_owned(),
        }],
    });
    let encoded = output.textualize();
    let recovered = PersonaOutput::actualize_text(&encoded)
        .unwrap_or_else(|fault| panic!("output restores from {encoded}: {fault:?}"));

    assert_eq!(recovered, output);
}
