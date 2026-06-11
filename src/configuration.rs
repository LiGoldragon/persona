//! Persona's binary daemon configuration.
//!
//! The schema-emitted daemon takes exactly one startup argument: a binary rkyv
//! configuration file (daemons never parse NOTA — the daemon-binary-only
//! override). `PersonaDaemonConfiguration` is that startup message: the manager
//! socket the working listener binds, the manager store the engine opens, and
//! the owner-only socket mode.

use std::path::Path;

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use triad_runtime::{BindingSurface, RequestConcurrencyLimit, SocketMode};

const OWNER_ONLY_SOCKET_MODE: u32 = 0o600;
const MAXIMUM_CONCURRENT_REQUESTS: usize = 64;

/// The binary rkyv startup message the persona daemon decodes from its single
/// argument. Paths are stored as their lossless UTF-8 byte string; the manager
/// socket is the working listener, the store path is the manager `.sema`.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct PersonaDaemonConfiguration {
    manager_socket_path: String,
    manager_store_path: String,
}

impl PersonaDaemonConfiguration {
    pub fn new(
        manager_socket_path: impl Into<String>,
        manager_store_path: impl Into<String>,
    ) -> Self {
        Self {
            manager_socket_path: manager_socket_path.into(),
            manager_store_path: manager_store_path.into(),
        }
    }

    pub fn manager_socket_path(&self) -> &str {
        self.manager_socket_path.as_str()
    }

    pub fn manager_store_path(&self) -> &str {
        self.manager_store_path.as_str()
    }

    /// Encode to the binary rkyv form the daemon accepts as its single startup
    /// argument.
    pub fn to_signal_bytes(&self) -> Result<Vec<u8>, ConfigurationError> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|bytes| bytes.to_vec())
            .map_err(|_| ConfigurationError::ArchiveEncode)
    }

    /// Decode from the binary rkyv startup bytes.
    pub fn from_signal_bytes(bytes: &[u8]) -> Result<Self, ConfigurationError> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
            .map_err(|_| ConfigurationError::ArchiveDecode)
    }

    /// Read and decode the binary rkyv configuration from the daemon's single
    /// startup-argument file path.
    pub fn from_signal_file(path: &Path) -> Result<Self, ConfigurationError> {
        let bytes = std::fs::read(path).map_err(ConfigurationError::Read)?;
        Self::from_signal_bytes(&bytes)
    }
}

impl BindingSurface for PersonaDaemonConfiguration {
    fn socket_path(&self) -> &Path {
        Path::new(self.manager_socket_path.as_str())
    }

    fn socket_mode(&self) -> Option<SocketMode> {
        Some(SocketMode::new(OWNER_ONLY_SOCKET_MODE))
    }

    fn request_concurrency_limit(&self) -> RequestConcurrencyLimit {
        RequestConcurrencyLimit::new(MAXIMUM_CONCURRENT_REQUESTS)
    }

    fn database_path(&self) -> &Path {
        Path::new(self.manager_store_path.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigurationError {
    #[error("read daemon configuration file: {0}")]
    Read(std::io::Error),

    #[error("daemon configuration rkyv encode failed")]
    ArchiveEncode,

    #[error("daemon configuration rkyv decode failed")]
    ArchiveDecode,
}
