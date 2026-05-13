use persona::state::EngineState;
use signal_persona::{
    ComponentDesiredState, ComponentHealth, ComponentName, ComponentShutdown, ComponentStartup,
    ComponentStatusQuery, EngineReply, SupervisorActionRejectionReason,
};

#[test]
fn default_catalog_names_engine_components() {
    let state = EngineState::default_catalog();
    let names: Vec<&str> = state
        .snapshot()
        .components
        .iter()
        .map(|component| component.name.as_str())
        .collect();

    assert_eq!(
        names,
        vec![
            "persona-mind",
            "persona-router",
            "persona-system",
            "persona-harness",
            "persona-terminal",
            "persona-message",
            "persona-introspect",
        ]
    );
}

#[test]
fn component_shutdown_advances_generation_and_updates_status() {
    let mut state = EngineState::default_catalog();
    let reply = state.stop_component(ComponentShutdown {
        component: ComponentName::new("persona-terminal"),
    });

    assert!(matches!(reply, EngineReply::SupervisorActionAccepted(_)));
    assert_eq!(state.snapshot().generation.into_u64(), 1);

    let status = state.component_status(ComponentStatusQuery {
        component: ComponentName::new("persona-terminal"),
    });
    match status {
        EngineReply::ComponentStatus(component) => {
            assert_eq!(component.desired_state, ComponentDesiredState::Stopped);
            assert_eq!(component.health, ComponentHealth::Stopped);
        }
        other => panic!("expected component status, got {other:?}"),
    }
}

#[test]
fn missing_component_query_returns_typed_missing_reply() {
    let state = EngineState::default_catalog();
    let reply = state.component_status(ComponentStatusQuery {
        component: ComponentName::new("persona-missing"),
    });

    match reply {
        EngineReply::ComponentStatusMissing(missing) => {
            assert_eq!(missing.component.as_str(), "persona-missing");
        }
        other => panic!("expected missing component reply, got {other:?}"),
    }
}

#[test]
fn repeated_startup_returns_already_desired_rejection() {
    let mut state = EngineState::default_catalog();
    let reply = state.start_component(ComponentStartup {
        component: ComponentName::new("persona-router"),
    });

    match reply {
        EngineReply::SupervisorActionRejected(rejection) => {
            assert_eq!(
                rejection.reason,
                SupervisorActionRejectionReason::ComponentAlreadyInDesiredState
            );
        }
        other => panic!("expected rejection, got {other:?}"),
    }
}
