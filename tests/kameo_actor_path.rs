use persona::actor::{PersonaActorEvent, PersonaActorRuntime};
use persona::request::{DescribeSchema, PersonaOutput, PersonaRequest};

#[tokio::test]
async fn constraint_persona_request_output_is_created_by_kameo_actor_path() {
    let runtime = PersonaActorRuntime::start();

    let output = runtime
        .handle(PersonaRequest::DescribeSchema(DescribeSchema {}))
        .await
        .expect("request handled by actor");

    assert!(matches!(output, PersonaOutput::SchemaExample(_)));

    let trace = runtime.trace().await.expect("trace read through actor");
    assert_eq!(
        trace,
        vec![
            PersonaActorEvent::Started,
            PersonaActorEvent::RequestAccepted,
            PersonaActorEvent::OutputCreated,
            PersonaActorEvent::TraceRead,
        ]
    );

    runtime.stop().await.expect("actor stops cleanly");
}

#[tokio::test]
async fn constraint_persona_request_actor_keeps_state_between_messages() {
    let runtime = PersonaActorRuntime::start();

    let _first = runtime
        .handle(PersonaRequest::DescribeSchema(DescribeSchema {}))
        .await
        .expect("first request handled by actor");
    let _second = runtime
        .handle(PersonaRequest::DescribeSchema(DescribeSchema {}))
        .await
        .expect("second request handled by actor");

    let trace = runtime.trace().await.expect("trace read through actor");
    let accepted_count = trace
        .iter()
        .filter(|event| **event == PersonaActorEvent::RequestAccepted)
        .count();
    let output_count = trace
        .iter()
        .filter(|event| **event == PersonaActorEvent::OutputCreated)
        .count();

    assert_eq!(accepted_count, 2);
    assert_eq!(output_count, 2);

    runtime.stop().await.expect("actor stops cleanly");
}
