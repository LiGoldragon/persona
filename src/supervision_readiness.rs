use std::path::{Path, PathBuf};
use std::time::Duration;

use kameo::actor::{Actor, ActorRef};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use signal_engine_management::{
    ComponentHealth, ComponentHealthReport, ComponentIdentity, ComponentKind, ComponentName,
    ComponentNotReady, ComponentReady, EngineManagementProtocolVersion,
    Frame as EngineManagementFrame, FrameBody, Operation as EngineManagementRequest, Presence,
    Query as EngineManagementQuery, Reply as EngineManagementReply,
};
use signal_frame::{
    ExchangeIdentifier, ExchangeLane, LaneSequence, NonEmpty, Reply, Request, SessionEpoch,
    SubReply,
};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::engine::{ComponentSpawnEnvelope, EngineComponent};

#[derive(Debug)]
pub struct ComponentSupervisionReadiness {
    attempt_count: u32,
    attempt_interval: Duration,
    codec: SupervisionFrameCodec,
}

impl ComponentSupervisionReadiness {
    pub const fn new(attempt_count: u32, attempt_interval: Duration) -> Self {
        Self {
            attempt_count,
            attempt_interval,
            codec: SupervisionFrameCodec::new(1024 * 1024),
        }
    }

    async fn verify(
        &self,
        expectation: ComponentSupervisionExpectation,
    ) -> Result<ComponentSupervisionReady, ComponentSupervisionReadinessFailure> {
        let mut remaining = self.attempt_count;
        while remaining > 0 {
            match self.probe(&expectation).await {
                Ok(ready) => return Ok(ready),
                Err(ComponentSupervisionReadinessFailure::Connect { source, .. })
                    if Self::is_retryable_connect_error(&source) =>
                {
                    remaining -= 1;
                    if remaining > 0 {
                        tokio::time::sleep(self.attempt_interval).await;
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Err(ComponentSupervisionReadinessFailure::NotReachable {
            component: expectation.component,
            path: expectation.path,
        })
    }

    async fn probe(
        &self,
        expectation: &ComponentSupervisionExpectation,
    ) -> Result<ComponentSupervisionReady, ComponentSupervisionReadinessFailure> {
        let mut stream = UnixStream::connect(expectation.path.as_path())
            .await
            .map_err(|source| ComponentSupervisionReadinessFailure::Connect {
                component: expectation.component,
                path: expectation.path.clone(),
                source,
            })?;

        let identity = self.request_identity(&mut stream, expectation).await?;
        expectation.verify_identity(&identity)?;

        let ready = self.request_readiness(&mut stream, expectation).await?;
        let health = self.request_health(&mut stream, expectation).await?;
        if health.health != ComponentHealth::Running {
            return Err(ComponentSupervisionReadinessFailure::Unhealthy {
                component: expectation.component,
                health: health.health,
            });
        }

        Ok(ComponentSupervisionReady {
            component: expectation.component,
            identity,
            ready,
            health,
        })
    }

    async fn request_identity(
        &self,
        stream: &mut UnixStream,
        expectation: &ComponentSupervisionExpectation,
    ) -> Result<ComponentIdentity, ComponentSupervisionReadinessFailure> {
        let request = EngineManagementRequest::Announce(Presence {
            expected_component: expectation.name.clone(),
            expected_kind: expectation.kind,
            engine_management_protocol_version: expectation.version,
        });
        self.codec.write_request(stream, request).await?;
        match self.codec.read_reply(stream).await? {
            EngineManagementReply::Identified(identity) => Ok(identity),
            other => Err(ComponentSupervisionReadinessFailure::UnexpectedReply {
                component: expectation.component,
                operation: "component hello",
                got: format!("{other:?}"),
            }),
        }
    }

    async fn request_readiness(
        &self,
        stream: &mut UnixStream,
        expectation: &ComponentSupervisionExpectation,
    ) -> Result<ComponentReady, ComponentSupervisionReadinessFailure> {
        let request = EngineManagementRequest::Query(EngineManagementQuery::ReadinessStatus(
            expectation.name.clone(),
        ));
        self.codec.write_request(stream, request).await?;
        match self.codec.read_reply(stream).await? {
            EngineManagementReply::Ready(ready) => Ok(ready),
            EngineManagementReply::NotReady(not_ready) => {
                Err(ComponentSupervisionReadinessFailure::NotReady {
                    component: expectation.component,
                    not_ready,
                })
            }
            other => Err(ComponentSupervisionReadinessFailure::UnexpectedReply {
                component: expectation.component,
                operation: "component readiness",
                got: format!("{other:?}"),
            }),
        }
    }

    async fn request_health(
        &self,
        stream: &mut UnixStream,
        expectation: &ComponentSupervisionExpectation,
    ) -> Result<ComponentHealthReport, ComponentSupervisionReadinessFailure> {
        let request = EngineManagementRequest::Query(EngineManagementQuery::HealthStatus(
            expectation.name.clone(),
        ));
        self.codec.write_request(stream, request).await?;
        match self.codec.read_reply(stream).await? {
            EngineManagementReply::HealthReport(health) => Ok(health),
            other => Err(ComponentSupervisionReadinessFailure::UnexpectedReply {
                component: expectation.component,
                operation: "component health",
                got: format!("{other:?}"),
            }),
        }
    }

    fn is_retryable_connect_error(error: &std::io::Error) -> bool {
        matches!(
            error.kind(),
            std::io::ErrorKind::NotFound
                | std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::WouldBlock
        )
    }
}

impl Default for ComponentSupervisionReadiness {
    fn default() -> Self {
        Self::new(200, Duration::from_millis(50))
    }
}

impl Actor for ComponentSupervisionReadiness {
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
pub struct VerifyComponentSupervision {
    expectation: ComponentSupervisionExpectation,
}

impl VerifyComponentSupervision {
    pub fn new(expectation: ComponentSupervisionExpectation) -> Self {
        Self { expectation }
    }
}

impl Message<VerifyComponentSupervision> for ComponentSupervisionReadiness {
    type Reply = Result<ComponentSupervisionReady, ComponentSupervisionReadinessFailure>;

    async fn handle(
        &mut self,
        message: VerifyComponentSupervision,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.verify(message.expectation).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSupervisionExpectation {
    component: EngineComponent,
    path: PathBuf,
    name: ComponentName,
    kind: ComponentKind,
    version: EngineManagementProtocolVersion,
}

impl ComponentSupervisionExpectation {
    pub fn new(
        component: EngineComponent,
        path: impl Into<PathBuf>,
        name: ComponentName,
        kind: ComponentKind,
        version: EngineManagementProtocolVersion,
    ) -> Self {
        Self {
            component,
            path: path.into(),
            name,
            kind,
            version,
        }
    }

    pub fn from_envelope(envelope: &ComponentSpawnEnvelope) -> Self {
        Self::new(
            envelope.component(),
            envelope.supervision_socket_path().to_path_buf(),
            envelope.component().component_name(),
            envelope.component().component_kind(),
            EngineManagementProtocolVersion::new(1),
        )
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn name(&self) -> &ComponentName {
        &self.name
    }

    pub fn kind(&self) -> ComponentKind {
        self.kind
    }

    pub fn version(&self) -> EngineManagementProtocolVersion {
        self.version
    }

    fn verify_identity(
        &self,
        identity: &ComponentIdentity,
    ) -> Result<(), ComponentSupervisionReadinessFailure> {
        if identity.name != self.name
            || identity.kind != self.kind
            || identity.engine_management_protocol_version != self.version
        {
            return Err(ComponentSupervisionReadinessFailure::IdentityMismatch {
                component: self.component,
                expected_name: self.name.clone(),
                actual_name: identity.name.clone(),
                expected_kind: self.kind,
                actual_kind: identity.kind,
                expected_version: self.version,
                actual_version: identity.engine_management_protocol_version,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSupervisionReady {
    component: EngineComponent,
    identity: ComponentIdentity,
    ready: ComponentReady,
    health: ComponentHealthReport,
}

impl ComponentSupervisionReady {
    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn identity(&self) -> &ComponentIdentity {
        &self.identity
    }

    pub fn ready(&self) -> &ComponentReady {
        &self.ready
    }

    pub fn health(&self) -> &ComponentHealthReport {
        &self.health
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupervisionFrameCodec {
    maximum_frame_bytes: usize,
}

impl SupervisionFrameCodec {
    pub const fn new(maximum_frame_bytes: usize) -> Self {
        Self {
            maximum_frame_bytes,
        }
    }

    pub async fn write_request(
        &self,
        stream: &mut UnixStream,
        request: EngineManagementRequest,
    ) -> Result<(), ComponentSupervisionReadinessFailure> {
        let frame = EngineManagementFrame::new(FrameBody::Request {
            exchange: self.initial_exchange(),
            request: Request::from_payload(request),
        });
        self.write_frame(stream, &frame).await
    }

    pub async fn write_reply(
        &self,
        stream: &mut UnixStream,
        reply: EngineManagementReply,
    ) -> Result<(), ComponentSupervisionReadinessFailure> {
        let frame = EngineManagementFrame::new(FrameBody::Reply {
            exchange: self.initial_exchange(),
            reply: Reply::committed(NonEmpty::single(SubReply::Ok(reply))),
        });
        self.write_frame(stream, &frame).await
    }

    pub async fn read_reply(
        &self,
        stream: &mut UnixStream,
    ) -> Result<EngineManagementReply, ComponentSupervisionReadinessFailure> {
        match self.read_frame(stream).await?.into_body() {
            FrameBody::Reply { reply, .. } => match reply {
                Reply::Accepted { per_operation, .. } => {
                    let mut operations = per_operation.into_vec();
                    if operations.len() != 1 {
                        return Err(ComponentSupervisionReadinessFailure::UnexpectedFrame {
                            got: format!(
                                "supervision readiness expects one reply operation, got {}",
                                operations.len()
                            ),
                        });
                    }
                    match operations.remove(0) {
                        SubReply::Ok(payload) => Ok(payload),
                        other => Err(ComponentSupervisionReadinessFailure::UnexpectedFrame {
                            got: format!("{other:?}"),
                        }),
                    }
                }
                Reply::Rejected { reason } => {
                    Err(ComponentSupervisionReadinessFailure::UnexpectedFrame {
                        got: format!("{reason:?}"),
                    })
                }
            },
            other => Err(ComponentSupervisionReadinessFailure::UnexpectedFrame {
                got: format!("{other:?}"),
            }),
        }
    }

    pub async fn read_request(
        &self,
        stream: &mut UnixStream,
    ) -> Result<EngineManagementRequest, ComponentSupervisionReadinessFailure> {
        match self.read_frame(stream).await?.into_body() {
            FrameBody::Request { request, .. } => {
                let mut operations = request.payloads.into_vec();
                if operations.len() != 1 {
                    return Err(ComponentSupervisionReadinessFailure::UnexpectedFrame {
                        got: format!(
                            "supervision readiness expects one request operation, got {}",
                            operations.len()
                        ),
                    });
                }
                Ok(operations.remove(0))
            }
            other => Err(ComponentSupervisionReadinessFailure::UnexpectedFrame {
                got: format!("{other:?}"),
            }),
        }
    }

    async fn write_frame(
        &self,
        stream: &mut UnixStream,
        frame: &EngineManagementFrame,
    ) -> Result<(), ComponentSupervisionReadinessFailure> {
        let bytes = frame.encode_length_prefixed()?;
        stream.write_all(&bytes).await?;
        stream.flush().await?;
        Ok(())
    }

    async fn read_frame(
        &self,
        stream: &mut UnixStream,
    ) -> Result<EngineManagementFrame, ComponentSupervisionReadinessFailure> {
        let mut prefix = [0_u8; 4];
        stream.read_exact(&mut prefix).await?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length > self.maximum_frame_bytes {
            return Err(ComponentSupervisionReadinessFailure::FrameTooLarge { bytes: length });
        }

        let mut bytes = Vec::with_capacity(4 + length);
        bytes.extend_from_slice(&prefix);
        bytes.resize(4 + length, 0);
        stream.read_exact(&mut bytes[4..]).await?;
        Ok(EngineManagementFrame::decode_length_prefixed(&bytes)?)
    }

    fn initial_exchange(&self) -> ExchangeIdentifier {
        ExchangeIdentifier::new(
            SessionEpoch::new(1),
            ExchangeLane::Connector,
            LaneSequence::first(),
        )
    }
}

#[derive(Debug, Error)]
pub enum ComponentSupervisionReadinessFailure {
    #[error("component {component:?} supervision socket did not become reachable: {path}")]
    NotReachable {
        component: EngineComponent,
        path: PathBuf,
    },
    #[error("connect to component {component:?} supervision socket {path}: {source}")]
    Connect {
        component: EngineComponent,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "component {component:?} supervision identity mismatch: expected {expected_name:?}/{expected_kind:?}/v{expected_version:?}, got {actual_name:?}/{actual_kind:?}/v{actual_version:?}"
    )]
    IdentityMismatch {
        component: EngineComponent,
        expected_name: ComponentName,
        actual_name: ComponentName,
        expected_kind: ComponentKind,
        actual_kind: ComponentKind,
        expected_version: EngineManagementProtocolVersion,
        actual_version: EngineManagementProtocolVersion,
    },
    #[error("component {component:?} is not ready: {not_ready:?}")]
    NotReady {
        component: EngineComponent,
        not_ready: ComponentNotReady,
    },
    #[error("component {component:?} health is {health:?}")]
    Unhealthy {
        component: EngineComponent,
        health: ComponentHealth,
    },
    #[error("unexpected component {component:?} supervision reply during {operation}: {got}")]
    UnexpectedReply {
        component: EngineComponent,
        operation: &'static str,
        got: String,
    },
    #[error("unexpected supervision frame: {got}")]
    UnexpectedFrame { got: String },
    #[error("supervision frame is too large: {bytes} bytes")]
    FrameTooLarge { bytes: usize },
    #[error("signal frame: {0}")]
    SignalFrame(#[from] signal_frame::FrameError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
