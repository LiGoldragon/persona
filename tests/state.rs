use meta_signal_persona::Reply;
use meta_signal_persona::{
    ActionRejectionReason, ComponentDesiredState, ComponentHealth, ComponentName,
    ComponentShutdown, ComponentStartup,
};
use persona::generated_contract::{EngineGenerationValue, PayloadString};
use persona::state::EngineState;

#[test]
fn default_catalog_names_engine_components() {
    let state = EngineState::default_catalog();
    let names: Vec<&str> = state
        .snapshot()
        .components
        .iter()
        .map(|component| component.component_name.as_str())
        .collect();

    assert_eq!(
        names,
        vec![
            "mind",
            "persona-router",
            "persona-system",
            "persona-harness",
            "persona-terminal",
            "persona-message",
            "persona-introspect",
            "persona-spirit",
        ]
    );
}

#[test]
fn component_shutdown_advances_generation_and_updates_status() {
    let mut state = EngineState::default_catalog();
    let reply = state.stop_component(ComponentShutdown::new(ComponentName::new(
        "persona-terminal",
    )));

    assert!(matches!(reply, Reply::ActionAccepted(_)));
    assert_eq!(state.snapshot().generation.clone().into_u64(), 1);

    let status = state.component_status(ComponentName::new("persona-terminal"));
    match status {
        Reply::ComponentStatus(component) => {
            let component = component.into_payload();
            assert_eq!(
                component.component_desired_state,
                ComponentDesiredState::Stopped
            );
            assert_eq!(component.component_health, ComponentHealth::Stopped);
        }
        other => panic!("expected component status, got {other:?}"),
    }
}

#[test]
fn missing_component_query_returns_typed_missing_reply() {
    let state = EngineState::default_catalog();
    let reply = state.component_status(ComponentName::new("persona-missing"));

    match reply {
        Reply::ComponentMissing(missing) => {
            assert_eq!(missing.into_payload().as_str(), "persona-missing");
        }
        other => panic!("expected missing component reply, got {other:?}"),
    }
}

#[test]
fn repeated_startup_returns_already_desired_rejection() {
    let mut state = EngineState::default_catalog();
    let reply = state.start_component(ComponentStartup::new(ComponentName::new("persona-router")));

    match reply {
        Reply::ActionRejected(rejection) => {
            let rejection = rejection.into_payload();
            assert_eq!(
                rejection.reason,
                ActionRejectionReason::ComponentAlreadyInDesiredState
            );
        }
        other => panic!("expected rejection, got {other:?}"),
    }
}
