//! Compatibility re-exports for upgrade-owned version state types.
//!
//! Persona no longer owns the upgrade handover protocol. The runtime
//! driver, active-version event records, and quarantine records live in
//! the `upgrade` triad. This module keeps existing Persona store/schema
//! code on stable paths while the owning implementation has moved.

pub use upgrade::{
    ActiveVersion, ActiveVersionChangeSource, ActiveVersionChanged, PreparedEvent, SocketPath,
    Target, TargetInput, VersionLabel as Version, VersionQuarantined,
};
