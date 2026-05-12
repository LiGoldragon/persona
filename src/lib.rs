//! Persona - engine manager for the multi-harness AI system.
//!
//! The current crate carries the first engine-manager runtime stub: a Kameo
//! manager actor that accepts `signal-persona` management requests and renders
//! NOTA projections for the command-line surface.

pub mod engine;
pub mod engine_event;
pub mod error;
pub mod launch;
pub mod manager;
pub mod manager_store;
pub mod request;
pub mod schema;
pub mod state;
pub mod transport;

pub use error::{Error, Result};
