//! Version state Persona owns.
//!
//! The upgrade handover protocol is driven elsewhere; what Persona keeps is
//! the state it stores and projects: the target of a prepared handover, and
//! the event records its manager store appends when an engine's active
//! version changes or a version is quarantined.
//!
//! Names and payloads come from the `signal-upgrade` and
//! `meta-signal-upgrade` contracts.

mod event;
mod handover;

pub use event::{
    ActiveVersion, ActiveVersionChangeSource, ActiveVersionChanged, PreparedEvent,
    VersionQuarantined,
};
pub use handover::{SocketPath, Target, TargetInput, VersionLabel, VersionLabel as Version};
