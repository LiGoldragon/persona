use meta_signal_persona::Response;
use meta_signal_persona::ActionRejectionReason;
use signal_persona::{ComponentDesiredState, ComponentHealth};
use persona::state::EngineState;

#[test]
fn default_catalog_names_engine_components() {
    let state = EngineState::default_catalog();
    let names: Vec<&str> = state
        .snapshot()
        .component_status_vector
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
    let reply = state.stop_component("persona-terminal".to_string());

    assert!(matches!(reply, Response::ActionAccepted(_)));
    assert_eq!(state.snapshot().engine_generation as u64, 1);

    let status = state.component_status("persona-terminal".to_string());
    match status {
        Response::ComponentStatus(component) => {
            let component = component;
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
    let reply = state.component_status("persona-missing".to_string());

    match reply {
        Response::ComponentMissing(missing) => {
            assert_eq!(missing.as_str(), "persona-missing");
        }
        other => panic!("expected missing component reply, got {other:?}"),
    }
}

#[test]
fn repeated_startup_returns_already_desired_rejection() {
    let mut state = EngineState::default_catalog();
    let reply = state.start_component("persona-router".to_string());

    match reply {
        Response::ActionRejected(rejection) => {
            let rejection = rejection;
            assert_eq!(
                rejection.action_rejection_reason,
                ActionRejectionReason::ComponentAlreadyInDesiredState
            );
        }
        other => panic!("expected rejection, got {other:?}"),
    }
}
