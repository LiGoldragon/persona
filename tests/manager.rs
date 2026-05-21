use persona::manager::{EngineManager, HandleEngineRequest, ManagerEvent, ReadTrace};
use signal_persona::engine::{Operation as EngineRequest, Reply as EngineReply};
use signal_persona::{
    ComponentDesiredState, ComponentHealth, ComponentName, ComponentShutdown, EngineStatusScope,
    Query,
};

#[tokio::test]
async fn constraint_engine_request_reply_is_created_by_kameo_manager_path() {
    let manager = EngineManager::start().await;

    let reply = manager
        .ask(HandleEngineRequest::new(EngineRequest::Query(
            Query::EngineStatus(EngineStatusScope::WholeEngine),
        )))
        .await
        .expect("request handled by actor");

    assert!(matches!(reply, EngineReply::EngineStatus(_)));

    let trace = manager
        .ask(ReadTrace::expecting_at_least(3))
        .await
        .expect("trace read through actor");
    assert_eq!(
        trace,
        vec![
            ManagerEvent::Started,
            ManagerEvent::EngineRequestAccepted,
            ManagerEvent::EngineReplyCreated,
            ManagerEvent::TraceRead,
        ]
    );

    EngineManager::stop(manager)
        .await
        .expect("actor stops cleanly");
}

#[tokio::test]
async fn constraint_engine_manager_keeps_component_state_between_messages() {
    let manager = EngineManager::start().await;

    let shutdown = ComponentShutdown {
        component: ComponentName::new("persona-terminal"),
    };
    let acceptance = manager
        .ask(HandleEngineRequest::new(EngineRequest::Stop(shutdown)))
        .await
        .expect("shutdown handled by actor");

    assert!(matches!(acceptance, EngineReply::ActionAccepted(_)));

    let status = manager
        .ask(HandleEngineRequest::new(EngineRequest::Query(
            Query::ComponentStatus(ComponentName::new("persona-terminal")),
        )))
        .await
        .expect("status handled by actor");

    match status {
        EngineReply::ComponentStatus(component) => {
            assert_eq!(component.desired_state, ComponentDesiredState::Stopped);
            assert_eq!(component.health, ComponentHealth::Stopped);
        }
        other => panic!("expected terminal component status, got {other:?}"),
    }

    EngineManager::stop(manager)
        .await
        .expect("actor stops cleanly");
}

#[test]
fn constraint_engine_manager_is_not_a_zst_actor() {
    assert!(std::mem::size_of::<EngineManager>() > 0);
}
