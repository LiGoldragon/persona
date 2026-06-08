//! Persona - engine manager for the multi-harness AI system.
//!
//! The current crate carries the first engine-manager runtime stub: a Kameo
//! manager actor that accepts `meta-signal-persona` management requests and renders
//! NOTA projections for the command-line surface.

pub mod direct_process;
pub mod engine;
pub mod engine_event;
pub mod error;
pub mod launch;
pub mod manager;
pub mod manager_store;
pub mod readiness;
pub mod request;
pub mod schema;
pub mod state;
pub mod supervision_readiness;
pub mod supervisor;
pub mod transport;
pub mod unit;
pub mod upgrade;

pub use error::{Error, Result};
