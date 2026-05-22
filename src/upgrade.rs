use signal_persona::{ComponentName, WirePath};
use signal_version_handover::{HandoverMarker, MarkerRequest, Operation as HandoverOperation};
use version_projection::{ComponentName as HandoverComponentName, ContractVersion};

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
pub struct Version(String);

impl Version {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    component: ComponentName,
    current_version: Version,
    next_version: Version,
    current_owner_socket_path: WirePath,
    current_upgrade_socket_path: WirePath,
    next_owner_socket_path: WirePath,
    next_upgrade_socket_path: WirePath,
}

impl Target {
    pub fn from_input(input: TargetInput) -> Self {
        Self {
            component: input.component,
            current_version: input.current_version,
            next_version: input.next_version,
            current_owner_socket_path: input.current_owner_socket_path,
            current_upgrade_socket_path: input.current_upgrade_socket_path,
            next_owner_socket_path: input.next_owner_socket_path,
            next_upgrade_socket_path: input.next_upgrade_socket_path,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn current_version(&self) -> &Version {
        &self.current_version
    }

    pub fn next_version(&self) -> &Version {
        &self.next_version
    }

    pub fn current_owner_socket_path(&self) -> &WirePath {
        &self.current_owner_socket_path
    }

    pub fn current_upgrade_socket_path(&self) -> &WirePath {
        &self.current_upgrade_socket_path
    }

    pub fn next_owner_socket_path(&self) -> &WirePath {
        &self.next_owner_socket_path
    }

    pub fn next_upgrade_socket_path(&self) -> &WirePath {
        &self.next_upgrade_socket_path
    }

    pub fn prepare(&self) -> Prepared {
        let request = MarkerRequest {
            component: HandoverComponentName::new(self.component.as_str()),
        };
        Prepared {
            target: self.clone(),
            first_handover_operation: HandoverOperation::AskHandoverMarker(request),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetInput {
    pub component: ComponentName,
    pub current_version: Version,
    pub next_version: Version,
    pub current_owner_socket_path: WirePath,
    pub current_upgrade_socket_path: WirePath,
    pub next_owner_socket_path: WirePath,
    pub next_upgrade_socket_path: WirePath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    target: Target,
    first_handover_operation: HandoverOperation,
}

impl Prepared {
    pub fn target(&self) -> &Target {
        &self.target
    }

    pub fn first_handover_operation(&self) -> &HandoverOperation {
        &self.first_handover_operation
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct PreparedEvent {
    component: ComponentName,
    current_version: Version,
    next_version: Version,
    current_owner_socket_path: WirePath,
    current_upgrade_socket_path: WirePath,
    next_owner_socket_path: WirePath,
    next_upgrade_socket_path: WirePath,
}

impl PreparedEvent {
    pub fn from_target(target: &Target) -> Self {
        Self {
            component: target.component.clone(),
            current_version: target.current_version.clone(),
            next_version: target.next_version.clone(),
            current_owner_socket_path: target.current_owner_socket_path.clone(),
            current_upgrade_socket_path: target.current_upgrade_socket_path.clone(),
            next_owner_socket_path: target.next_owner_socket_path.clone(),
            next_upgrade_socket_path: target.next_upgrade_socket_path.clone(),
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn current_version(&self) -> &Version {
        &self.current_version
    }

    pub fn next_version(&self) -> &Version {
        &self.next_version
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    __C::Error: rkyv::rancor::Source
)))]
pub struct ActiveVersionChanged {
    component: ComponentName,
    active_version: Version,
    schema_hash: ContractVersion,
    commit_sequence: u64,
}

impl ActiveVersionChanged {
    pub fn from_marker(target: &Target, marker: &HandoverMarker) -> Self {
        Self {
            component: target.component.clone(),
            active_version: target.next_version.clone(),
            schema_hash: marker.schema_hash,
            commit_sequence: marker.commit_sequence,
        }
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn active_version(&self) -> &Version {
        &self.active_version
    }

    pub fn schema_hash(&self) -> ContractVersion {
        self.schema_hash
    }

    pub fn commit_sequence(&self) -> u64 {
        self.commit_sequence
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ActiveVersion {
    component: ComponentName,
    active_version: Version,
    schema_hash: ContractVersion,
    commit_sequence: u64,
}

impl ActiveVersion {
    pub fn new(
        component: ComponentName,
        active_version: Version,
        schema_hash: ContractVersion,
        commit_sequence: u64,
    ) -> Self {
        Self {
            component,
            active_version,
            schema_hash,
            commit_sequence,
        }
    }

    pub fn from_change(change: &ActiveVersionChanged) -> Self {
        Self::new(
            change.component.clone(),
            change.active_version.clone(),
            change.schema_hash,
            change.commit_sequence,
        )
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn active_version(&self) -> &Version {
        &self.active_version
    }

    pub fn schema_hash(&self) -> ContractVersion {
        self.schema_hash
    }

    pub fn commit_sequence(&self) -> u64 {
        self.commit_sequence
    }
}
