use persona::schema::{
    CorePhase, EventIdentifier, PersonaStateSnapshot, StateCursorRecord, StateRevision,
    StateTransitionRecord, TransitionCommandKind, TransitionIdentifier,
};
use persona::state::PersonaState;

fn empty_snapshot() -> PersonaStateSnapshot {
    PersonaStateSnapshot {
        revision: StateRevision::new(0),
        phase: CorePhase::Configured,
        harnesses: Vec::new(),
        messages: Vec::new(),
        authorizations: Vec::new(),
        deliveries: Vec::new(),
        observations: Vec::new(),
        interactions: Vec::new(),
        cursors: vec![StateCursorRecord {
            source: "persona-event-log".to_string(),
            next_sequence: StateRevision::new(1),
        }],
        transitions: Vec::new(),
    }
}

#[test]
fn state_records_transition_and_advances_revision() {
    let mut state = PersonaState::from_snapshot(empty_snapshot());

    state.record_transition(StateTransitionRecord {
        identifier: TransitionIdentifier::new("transition-1"),
        command: TransitionCommandKind::DeclareHarness,
        subject: "operator".to_string(),
        before: StateRevision::new(0),
        after: StateRevision::new(1),
        event: EventIdentifier::new("event-1"),
    });

    assert_eq!(state.revision(), StateRevision::new(1));
    assert_eq!(state.snapshot().transitions.len(), 1);
}
