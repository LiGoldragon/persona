//! The component and version identity a prepared handover targets.

use std::path::Path;

use signal_upgrade::ComponentName;

/// A component version's human label, as the upgrade contract spells it.
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
pub struct VersionLabel(String);

impl VersionLabel {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&meta_signal_upgrade::VersionLabel> for VersionLabel {
    fn from(value: &meta_signal_upgrade::VersionLabel) -> Self {
        Self::new(value.clone())
    }
}

/// A filesystem path a handover endpoint listens on.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SocketPath(String);

impl SocketPath {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn as_path(&self) -> &Path {
        Path::new(self.as_str())
    }
}

/// The component, the two versions, and the four sockets a handover moves between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    component: ComponentName,
    current_version: VersionLabel,
    next_version: VersionLabel,
    current_meta_socket_path: SocketPath,
    current_upgrade_socket_path: SocketPath,
    next_meta_socket_path: SocketPath,
    next_upgrade_socket_path: SocketPath,
}

impl Target {
    pub fn from_input(input: TargetInput) -> Self {
        Self {
            component: input.component,
            current_version: input.current_version,
            next_version: input.next_version,
            current_meta_socket_path: input.current_meta_socket_path,
            current_upgrade_socket_path: input.current_upgrade_socket_path,
            next_meta_socket_path: input.next_meta_socket_path,
            next_upgrade_socket_path: input.next_upgrade_socket_path,
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

    pub fn current_meta_socket_path(&self) -> &SocketPath {
        &self.current_meta_socket_path
    }

    pub fn current_upgrade_socket_path(&self) -> &SocketPath {
        &self.current_upgrade_socket_path
    }

    pub fn next_meta_socket_path(&self) -> &SocketPath {
        &self.next_meta_socket_path
    }

    pub fn next_upgrade_socket_path(&self) -> &SocketPath {
        &self.next_upgrade_socket_path
    }
}

/// The named form of [`Target`]'s fields, for construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetInput {
    pub component: ComponentName,
    pub current_version: VersionLabel,
    pub next_version: VersionLabel,
    pub current_meta_socket_path: SocketPath,
    pub current_upgrade_socket_path: SocketPath,
    pub next_meta_socket_path: SocketPath,
    pub next_upgrade_socket_path: SocketPath,
}
