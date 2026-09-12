//! The version-state records Persona's manager store appends and projects.

use meta_signal_upgrade::{ForceReason, QuarantineReason, RollbackReason};
use signal_upgrade::{ComponentName, ContractVersion, HandoverMarkerData, StateSequence};

use super::handover::{SocketPath, Target, VersionLabel};

/// A handover has been prepared for a component: both versions, both endpoints.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct PreparedEvent {
    component: ComponentName,
    current_version: VersionLabel,
    next_version: VersionLabel,
    current_meta_socket_path: SocketPath,
    current_upgrade_socket_path: SocketPath,
    next_meta_socket_path: SocketPath,
    next_upgrade_socket_path: SocketPath,
}

impl PreparedEvent {
    pub fn from_target(target: &Target) -> Self {
        Self {
            component: target.component().clone(),
            current_version: target.current_version().clone(),
            next_version: target.next_version().clone(),
            current_meta_socket_path: target.current_meta_socket_path().clone(),
            current_upgrade_socket_path: target.current_upgrade_socket_path().clone(),
            next_meta_socket_path: target.next_meta_socket_path().clone(),
            next_upgrade_socket_path: target.next_upgrade_socket_path().clone(),
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn current_version(&self) -> &VersionLabel {
        &self.current_version
    }

    pub fn next_version(&self) -> &VersionLabel {
        &self.next_version
    }
}

/// Why an engine's active version changed.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub enum ActiveVersionChangeSource {
    /// The next version published a handover marker at this state sequence.
    HandoverMarker { state_sequence: StateSequence },
    /// An operator forced the flip.
    ForceFlip { reason: ForceReason },
    /// The version was rolled back.
    Rollback { reason: RollbackReason },
}

impl ActiveVersionChangeSource {
    pub fn state_sequence(&self) -> Option<StateSequence> {
        match self {
            Self::HandoverMarker { state_sequence } => Some(*state_sequence),
            Self::ForceFlip { .. } | Self::Rollback { .. } => None,
        }
    }
}

/// An engine's active version changed.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct ActiveVersionChanged {
    component: ComponentName,
    active_version: VersionLabel,
    schema_hash: ContractVersion,
    source: ActiveVersionChangeSource,
}

impl ActiveVersionChanged {
    pub fn new(
        component: ComponentName,
        active_version: VersionLabel,
        schema_hash: ContractVersion,
        source: ActiveVersionChangeSource,
    ) -> Self {
        Self {
            component,
            active_version,
            schema_hash,
            source,
        }
    }

    /// The change witnessed by the next version's own handover marker.
    pub fn from_marker(target: &Target, marker: &HandoverMarkerData) -> Self {
        Self::new(
            target.component().clone(),
            target.next_version().clone(),
            marker.schema_hash.clone(),
            ActiveVersionChangeSource::HandoverMarker {
                state_sequence: marker.state_sequence,
            },
        )
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn active_version(&self) -> &VersionLabel {
        &self.active_version
    }

    pub fn schema_hash(&self) -> ContractVersion {
        self.schema_hash.clone()
    }

    pub fn source(&self) -> &ActiveVersionChangeSource {
        &self.source
    }

    pub fn state_sequence(&self) -> Option<StateSequence> {
        self.source.state_sequence()
    }
}

/// A component version was quarantined and must not be selected.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct VersionQuarantined {
    component: ComponentName,
    version: VersionLabel,
    schema_hash: ContractVersion,
    reason: QuarantineReason,
}

impl VersionQuarantined {
    pub fn new(
        component: ComponentName,
        version: VersionLabel,
        schema_hash: ContractVersion,
        reason: QuarantineReason,
    ) -> Self {
        Self {
            component,
            version,
            schema_hash,
            reason,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn version(&self) -> &VersionLabel {
        &self.version
    }

    pub fn schema_hash(&self) -> ContractVersion {
        self.schema_hash.clone()
    }

    pub fn reason(&self) -> QuarantineReason {
        self.reason
    }
}

/// The active version an engine is currently on, as stored.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ActiveVersion {
    component: ComponentName,
    active_version: VersionLabel,
    schema_hash: ContractVersion,
    source: ActiveVersionChangeSource,
}

impl ActiveVersion {
    pub fn new(
        component: ComponentName,
        active_version: VersionLabel,
        schema_hash: ContractVersion,
        source: ActiveVersionChangeSource,
    ) -> Self {
        Self {
            component,
            active_version,
            schema_hash,
            source,
        }
    }

    pub fn from_change(change: &ActiveVersionChanged) -> Self {
        Self::new(
            change.component().clone(),
            change.active_version().clone(),
            change.schema_hash(),
            change.source().clone(),
        )
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn active_version(&self) -> &VersionLabel {
        &self.active_version
    }

    pub fn schema_hash(&self) -> ContractVersion {
        self.schema_hash.clone()
    }

    pub fn source(&self) -> &ActiveVersionChangeSource {
        &self.source
    }

    pub fn state_sequence(&self) -> Option<StateSequence> {
        self.source.state_sequence()
    }
}
