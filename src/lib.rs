//! Persona - typed coordination for multi-harness AI systems.
//!
//! The current crate is a NOTA-facing schema scaffold. It defines the first
//! records Persona needs before the daemon and durable store land.

pub mod error;
pub mod request;
pub mod schema;
pub mod state;

pub use error::{Error, Result};
