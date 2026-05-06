//! Core state holder for the Persona reducer contract.
//!
//! This module intentionally stays small. The durable engine will own command
//! validation, redb tables, and rkyv archives; this holder gives tests and
//! callers one typed place to talk about the current snapshot and transition
//! ledger.

use crate::schema::{PersonaStateSnapshot, StateRevision, StateTransitionRecord};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaState {
    snapshot: PersonaStateSnapshot,
}

impl PersonaState {
    pub fn from_snapshot(snapshot: PersonaStateSnapshot) -> Self {
        Self { snapshot }
    }

    pub fn snapshot(&self) -> &PersonaStateSnapshot {
        &self.snapshot
    }

    pub fn into_snapshot(self) -> PersonaStateSnapshot {
        self.snapshot
    }

    pub fn revision(&self) -> StateRevision {
        self.snapshot.revision
    }

    pub fn record_transition(&mut self, transition: StateTransitionRecord) {
        self.snapshot.revision = transition.after;
        self.snapshot.transitions.push(transition);
    }
}
