use meta_signal_persona::{
    ComponentDesiredState, ComponentHealth, ComponentKind, ComponentName, EngineGeneration,
    EnginePhase, EngineStatus, EngineStatusReport as ContractEngineStatusReport,
    EngineStatusScope as ContractEngineStatusScope, LifecycleComponentStatus, Query,
};
use meta_signal_persona::{
    Frame as PersonaFrame, FrameBody, Operation as EngineRequest, Reply as EngineReply,
};
use nota::NotaSource;
use persona::generated_contract::PayloadString;
use persona::request::{
    CommandLine, EngineStatusQuery, EngineStatusScope, PersonaOutput, PersonaRequest,
};
use persona::schema::{EngineStatusReport, LifecycleComponentStatusReport};
use persona::transport::PersonaFrameCodec;
use signal_frame::{
    ExchangeIdentifier, ExchangeLane, LaneSequence, NonEmpty, Request, SessionEpoch,
};

struct RequestFixture {
    arguments: [&'static str; 2],
}

impl RequestFixture {
    fn inline_component_status_query() -> Self {
        Self {
            arguments: ["(ComponentStatusQuery", "(persona-router))"],
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
        component: ComponentName::new("persona-system"),
    });
    let engine_request = request.into_engine_request();

    match engine_request {
        EngineRequest::Query(query) => match query.into_payload() {
            Query::ComponentStatus(component) => {
                assert_eq!(component.as_str(), "persona-system");
            }
            other => panic!("expected component status query, got {other:?}"),
        },
        other => panic!("expected signal component status query, got {other:?}"),
    }
}

#[test]
fn persona_frame_codec_rejects_multi_operation_request() {
    let request = Request::from_payloads(NonEmpty::from_head_and_tail(
        EngineRequest::Query(Query::EngineStatus(ContractEngineStatusScope::WholeEngine).into()),
        vec![EngineRequest::Query(
            Query::EngineStatus(ContractEngineStatusScope::WholeEngine).into(),
        )],
    ));
    let frame = PersonaFrame::new(FrameBody::Request {
        exchange: ExchangeIdentifier::new(
            SessionEpoch::new(1),
            ExchangeLane::Connector,
            LaneSequence::first(),
        ),
        request,
    });
    let error = PersonaFrameCodec::default()
        .request_from_frame(frame)
        .expect_err("multi-operation request is rejected");

    match error {
        persona::Error::UnexpectedSignalFrame { got } => {
            assert!(got.contains("currently accepts one operation"));
        }
        other => panic!("expected unexpected frame rejection, got {other:?}"),
    }
}

#[test]
fn engine_status_reply_renders_as_nota() {
    let reply = EngineReply::EngineStatus(
        EngineStatus::new(ContractEngineStatusReport {
            generation: EngineGeneration::new(2),
            phase: EnginePhase::Starting,
            components: vec![LifecycleComponentStatus {
                component_name: ComponentName::new("mind"),
                component_kind: ComponentKind::Mind,
                component_desired_state: ComponentDesiredState::Running,
                component_health: ComponentHealth::Starting,
            }],
        })
        .into(),
    );
    let output = PersonaOutput::from_engine_reply(reply).to_nota().unwrap();

    assert!(
        output.starts_with("(EngineStatusReport (2 Starting ["),
        "output: {output}"
    );
    assert!(output.contains("(mind Mind Running Starting)"));
}

#[test]
fn output_round_trips_through_nota() {
    let output = PersonaOutput::EngineStatusReport(EngineStatusReport {
        generation: 1,
        phase: "Starting".to_owned(),
        components: vec![LifecycleComponentStatusReport {
            component: ComponentName::new("persona-router"),
            kind: "Router".to_owned(),
            desired_state: "Running".to_owned(),
            health: "Starting".to_owned(),
        }],
    });
    let encoded = output.to_nota().unwrap();
    let recovered = NotaSource::new(&encoded).parse::<PersonaOutput>().unwrap();

    assert_eq!(recovered, output);
}
