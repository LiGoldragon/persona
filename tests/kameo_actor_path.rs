use persona::actor::{HandlePersonaRequest, PersonaRuntime, ReadTrace, RuntimeEvent};
use persona::request::{DescribeSchema, PersonaOutput, PersonaRequest};

#[tokio::test]
async fn constraint_persona_request_output_is_created_by_kameo_actor_path() {
    let runtime = PersonaRuntime::start().await;

    let output = runtime
        .ask(HandlePersonaRequest::new(PersonaRequest::DescribeSchema(
            DescribeSchema {},
        )))
        .await
        .expect("request handled by actor");

    assert!(matches!(output, PersonaOutput::SchemaExample(_)));

    let trace = runtime
        .ask(ReadTrace::expecting_at_least(3))
        .await
        .expect("trace read through actor");
    assert_eq!(
        trace,
        vec![
            RuntimeEvent::Started,
            RuntimeEvent::RequestAccepted,
            RuntimeEvent::OutputCreated,
            RuntimeEvent::TraceRead,
        ]
    );

    PersonaRuntime::stop(runtime)
        .await
        .expect("actor stops cleanly");
}

#[tokio::test]
async fn constraint_persona_runtime_keeps_state_between_messages() {
    let runtime = PersonaRuntime::start().await;

    let _first = runtime
        .ask(HandlePersonaRequest::new(PersonaRequest::DescribeSchema(
            DescribeSchema {},
        )))
        .await
        .expect("first request handled by actor");
    let _second = runtime
        .ask(HandlePersonaRequest::new(PersonaRequest::DescribeSchema(
            DescribeSchema {},
        )))
        .await
        .expect("second request handled by actor");

    let trace = runtime
        .ask(ReadTrace::expecting_at_least(5))
        .await
        .expect("trace read through actor");
    let accepted_count = trace
        .iter()
        .filter(|event| **event == RuntimeEvent::RequestAccepted)
        .count();
    let output_count = trace
        .iter()
        .filter(|event| **event == RuntimeEvent::OutputCreated)
        .count();

    assert_eq!(accepted_count, 2);
    assert_eq!(output_count, 2);

    PersonaRuntime::stop(runtime)
        .await
        .expect("actor stops cleanly");
}
