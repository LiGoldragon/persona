use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::PathBuf;
use std::time::Duration;

use kameo::actor::{Actor, ActorRef};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use thiserror::Error;

use crate::engine::{ComponentSpawnEnvelope, EngineComponent, SocketMode};

#[derive(Debug)]
pub struct ComponentSocketReadiness {
    attempt_count: u32,
    attempt_interval: Duration,
}

impl ComponentSocketReadiness {
    pub const fn new(attempt_count: u32, attempt_interval: Duration) -> Self {
        Self {
            attempt_count,
            attempt_interval,
        }
    }

    async fn verify(
        &self,
        expectation: ComponentSocketExpectation,
    ) -> Result<ComponentSocketReady, ComponentSocketReadinessFailure> {
        let mut remaining = self.attempt_count;
        while remaining > 0 {
            match Self::inspect(&expectation)? {
                Some(ready) => return Ok(ready),
                None => {
                    remaining -= 1;
                    if remaining > 0 {
                        tokio::time::sleep(self.attempt_interval).await;
                    }
                }
            }
        }
        Err(ComponentSocketReadinessFailure::NotBound {
            component: expectation.component,
            path: expectation.path,
        })
    }

    fn inspect(
        expectation: &ComponentSocketExpectation,
    ) -> Result<Option<ComponentSocketReady>, ComponentSocketReadinessFailure> {
        let metadata = match std::fs::symlink_metadata(expectation.path.as_path()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(ComponentSocketReadinessFailure::Io {
                    operation: "inspect component socket metadata",
                    source,
                });
            }
        };
        if !metadata.file_type().is_socket() {
            return Err(ComponentSocketReadinessFailure::NotSocket {
                component: expectation.component,
                path: expectation.path.clone(),
            });
        }
        let actual_mode = metadata.permissions().mode() & 0o777;
        let expected_mode = expectation.mode.as_octal();
        if actual_mode != expected_mode {
            return Err(ComponentSocketReadinessFailure::WrongMode {
                component: expectation.component,
                path: expectation.path.clone(),
                expected: expected_mode,
                actual: actual_mode,
            });
        }
        Ok(Some(ComponentSocketReady::from_expectation(expectation)))
    }
}

impl Default for ComponentSocketReadiness {
    fn default() -> Self {
        Self::new(200, Duration::from_millis(50))
    }
}

impl Actor for ComponentSocketReadiness {
    type Args = Self;
    type Error = Infallible;

    async fn on_start(
        readiness: Self::Args,
        _actor_reference: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(readiness)
    }
}

#[derive(Debug)]
pub struct VerifyComponentSocket {
    expectation: ComponentSocketExpectation,
}

impl VerifyComponentSocket {
    pub fn new(expectation: ComponentSocketExpectation) -> Self {
        Self { expectation }
    }
}

impl Message<VerifyComponentSocket> for ComponentSocketReadiness {
    type Reply = Result<ComponentSocketReady, ComponentSocketReadinessFailure>;

    async fn handle(
        &mut self,
        message: VerifyComponentSocket,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.verify(message.expectation).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSocketExpectation {
    component: EngineComponent,
    path: PathBuf,
    mode: SocketMode,
}

impl ComponentSocketExpectation {
    pub fn new(component: EngineComponent, path: impl Into<PathBuf>, mode: SocketMode) -> Self {
        Self {
            component,
            path: path.into(),
            mode,
        }
    }

    pub fn from_envelope(envelope: &ComponentSpawnEnvelope) -> Self {
        Self::new(
            envelope.component(),
            envelope.socket_path().to_path_buf(),
            envelope.socket_mode(),
        )
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }

    pub fn mode(&self) -> SocketMode {
        self.mode
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSocketReady {
    component: EngineComponent,
    path: PathBuf,
    mode: SocketMode,
}

impl ComponentSocketReady {
    fn from_expectation(expectation: &ComponentSocketExpectation) -> Self {
        Self {
            component: expectation.component,
            path: expectation.path.clone(),
            mode: expectation.mode,
        }
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }

    pub fn mode(&self) -> SocketMode {
        self.mode
    }
}

#[derive(Debug, Error)]
pub enum ComponentSocketReadinessFailure {
    #[error("component {component:?} did not bind socket {path}")]
    NotBound {
        component: EngineComponent,
        path: PathBuf,
    },
    #[error("component {component:?} path is not a socket: {path}")]
    NotSocket {
        component: EngineComponent,
        path: PathBuf,
    },
    #[error("component {component:?} socket {path} has mode {actual:o}, expected {expected:o}")]
    WrongMode {
        component: EngineComponent,
        path: PathBuf,
        expected: u32,
        actual: u32,
    },
    #[error("{operation}: {source}")]
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
}
