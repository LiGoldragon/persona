use persona::request::{
    CommandLine, EngineStatusQuery, EngineStatusScope, PersonaOutput, PersonaRequest,
};
use persona::schema::{ComponentStatusRecord, EngineStatusReport, TextComponentName};
use signal_persona::{
    ComponentDesiredState, ComponentHealth, ComponentKind, ComponentName, ComponentStatus,
    EngineGeneration, EnginePhase, EngineReply, EngineStatus,
};

struct RequestFixture {
    arguments: [&'static str; 2],
}

impl RequestFixture {
    fn inline_component_status_query() -> Self {
        Self {
            arguments: ["(ComponentStatusQuery", "persona-router)"],
        }
    }

    fn command_line(&self) -> CommandLine {
        CommandLine::from_arguments(self.arguments)
    }
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
fn inline_nota_request_decodes_after_shell_token_join() {
    let request = RequestFixture::inline_component_status_query()
        .command_line()
        .decode_request()
        .unwrap();

    match request {
        PersonaRequest::ComponentStatusQuery(query) => {
            assert_eq!(query.component.as_str(), "persona-router");
        }
        other => panic!("expected ComponentStatusQuery, got {other:?}"),
    }
}

#[test]
fn persona_request_lowers_to_signal_persona_engine_request() {
    let request = PersonaRequest::ComponentStatusQuery(persona::request::ComponentStatusQuery {
        component: TextComponentName::new("persona-system"),
    });
    let engine_request = request.into_engine_request();

    match engine_request {
        signal_persona::EngineRequest::ComponentStatusQuery(query) => {
            assert_eq!(query.component.as_str(), "persona-system");
        }
        other => panic!("expected signal component status query, got {other:?}"),
    }
}

#[test]
fn engine_status_reply_renders_as_nota() {
    let reply = EngineReply::EngineStatus(EngineStatus {
        generation: EngineGeneration::new(2),
        phase: EnginePhase::Starting,
        components: vec![ComponentStatus {
            name: ComponentName::new("persona-mind"),
            kind: ComponentKind::Mind,
            desired_state: ComponentDesiredState::Running,
            health: ComponentHealth::Starting,
        }],
    });
    let output = PersonaOutput::from_engine_reply(reply).to_nota().unwrap();

    assert!(output.starts_with("(EngineStatusReport 2 Starting ["));
    assert!(output.contains("(ComponentStatusRecord persona-mind Mind Running Starting)"));
}

#[test]
fn output_round_trips_through_nota() {
    let output = PersonaOutput::EngineStatusReport(EngineStatusReport {
        generation: 1,
        phase: persona::schema::EnginePhase::Starting,
        components: vec![ComponentStatusRecord {
            name: TextComponentName::new("persona-router"),
            kind: persona::schema::ComponentKind::Router,
            desired_state: persona::schema::ComponentDesiredState::Running,
            health: persona::schema::ComponentHealth::Starting,
        }],
    });
    let encoded = output.to_nota().unwrap();
    let mut decoder = nota_codec::Decoder::new(&encoded);
    let recovered = <PersonaOutput as nota_codec::NotaDecode>::decode(&mut decoder).unwrap();

    assert_eq!(recovered, output);
}
