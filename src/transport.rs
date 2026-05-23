use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixStream as StandardUnixStream;

use kameo::actor::ActorRef;
use owner_signal_version_handover::{
    Frame as OwnerFrame, FrameBody as OwnerFrameBody, Operation as OwnerRequest,
    Reply as OwnerReply,
};
use signal_frame::{
    ExchangeIdentifier, ExchangeLane, LaneSequence, NonEmpty, Reply, Request, SessionEpoch,
    SubReply,
};
use signal_persona::engine::{Frame, FrameBody, Operation as EngineRequest, Reply as EngineReply};
use signal_persona_auth::EngineId;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use unix_ancillary::UnixStreamExt;

use crate::engine::{ComponentLayout, EngineComponent, EngineTopology, PersonaDaemonPaths};
use crate::error::{Error, Result};
use crate::launch::{ComponentCommandCatalog, EngineLaunchConfiguration};
use crate::manager::{EngineManager, HandleEngineRequest, HandleOwnerVersionHandover};
use crate::manager_store::{ManagerStore, ManagerStoreLocation, ReadActiveVersion};
use crate::supervisor::{EngineSupervisor, EngineSupervisorInput, StartPrototypeSupervision};
use crate::unit::{ManualUnitController, UnitController};
use crate::upgrade::Version;

fn unlink_existing_socket_path(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            std::fs::remove_file(path)?;
            Ok(())
        }
        Ok(_) => Err(Error::SocketPathOccupied {
            path: path.to_path_buf(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn prepare_socket_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaEndpoint {
    path: PathBuf,
}

impl PersonaEndpoint {
    pub fn from_environment() -> Self {
        match std::env::var_os("PERSONA_SOCKET") {
            Some(path) => Self::from_path(path),
            None => Self::from_path(PersonaDaemonPaths::production().manager_socket()),
        }
    }

    pub fn from_argument_or_environment(argument: Option<impl Into<PathBuf>>) -> Self {
        match argument {
            Some(path) => Self::from_path(path),
            None => Self::from_environment(),
        }
    }

    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn as_path(&self) -> &Path {
        self.path.as_path()
    }

    fn unlink_existing_socket(&self) -> Result<()> {
        match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_socket() => {
                std::fs::remove_file(&self.path)?;
                Ok(())
            }
            Ok(_) => Err(Error::SocketPathOccupied {
                path: self.path.clone(),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerEndpoint {
    path: PathBuf,
}

impl OwnerEndpoint {
    pub fn from_environment_or_manager(manager: &PersonaEndpoint) -> Self {
        match std::env::var_os("PERSONA_OWNER_SOCKET") {
            Some(path) => Self::from_path(path),
            None => Self::from_manager(manager),
        }
    }

    pub fn from_manager(manager: &PersonaEndpoint) -> Self {
        let manager_path = manager.as_path();
        let parent = manager_path.parent().unwrap_or_else(|| Path::new("."));
        let stem = manager_path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".sock"))
            .unwrap_or("persona");
        Self::from_path(parent.join(format!("{stem}-owner.sock")))
    }

    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn as_path(&self) -> &Path {
        self.path.as_path()
    }

    fn unlink_existing_socket(&self) -> Result<()> {
        match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_socket() => {
                std::fs::remove_file(&self.path)?;
                Ok(())
            }
            Ok(_) => Err(Error::SocketPathOccupied {
                path: self.path.clone(),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentHandoffEndpoint {
    component_name: String,
    public_socket_mode: crate::engine::SocketMode,
    public_socket_path: PathBuf,
    control_socket_path: PathBuf,
}

impl ComponentHandoffEndpoint {
    pub fn new(
        component: EngineComponent,
        public_socket_path: impl Into<PathBuf>,
        control_socket_path: impl Into<PathBuf>,
    ) -> Self {
        Self::for_component_name(
            component.as_component_name(),
            public_socket_path,
            control_socket_path,
            component.socket_mode(),
        )
    }

    pub fn for_component_name(
        component_name: impl Into<String>,
        public_socket_path: impl Into<PathBuf>,
        control_socket_path: impl Into<PathBuf>,
        public_socket_mode: crate::engine::SocketMode,
    ) -> Self {
        Self {
            component_name: component_name.into(),
            public_socket_mode,
            public_socket_path: public_socket_path.into(),
            control_socket_path: control_socket_path.into(),
        }
    }

    pub fn from_layout(layout: &ComponentLayout, control_root: impl AsRef<Path>) -> Self {
        Self::new(
            layout.component(),
            layout.domain_socket().path().to_path_buf(),
            control_root
                .as_ref()
                .join(format!("{}.sock", layout.instance_name().as_str())),
        )
    }

    pub fn component_name(&self) -> &str {
        self.component_name.as_str()
    }

    pub fn public_socket_mode(&self) -> crate::engine::SocketMode {
        self.public_socket_mode
    }

    pub fn public_socket_path(&self) -> &Path {
        self.public_socket_path.as_path()
    }

    pub fn control_socket_path(&self) -> &Path {
        self.control_socket_path.as_path()
    }
}

#[derive(Debug, Clone)]
pub struct ManagerStoreActiveVersionReader {
    engine: EngineId,
    component_name: signal_persona::ComponentName,
    store: ActorRef<ManagerStore>,
}

impl ManagerStoreActiveVersionReader {
    pub fn new(
        engine: EngineId,
        component: EngineComponent,
        store: ActorRef<ManagerStore>,
    ) -> Self {
        Self::for_component_name(engine, component.as_component_name(), store)
    }

    pub fn for_component_name(
        engine: EngineId,
        component_name: impl Into<String>,
        store: ActorRef<ManagerStore>,
    ) -> Self {
        Self {
            engine,
            component_name: signal_persona::ComponentName::new(component_name),
            store,
        }
    }

    pub async fn read_active_version(&self) -> Result<Version> {
        let active = self
            .store
            .ask(ReadActiveVersion::new(
                self.engine.clone(),
                self.component_name.clone(),
            ))
            .await
            .map_err(|error| Error::actor("read active component version", error))?;
        active
            .map(|version| version.active_version().clone())
            .ok_or_else(|| Error::ActiveVersionMissing {
                engine: self.engine.as_str().to_owned(),
                component: self.component_name.as_str().to_owned(),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ComponentHandoffRouteKey {
    component_name: String,
    version: Version,
}

impl ComponentHandoffRouteKey {
    fn new(component_name: impl Into<String>, version: Version) -> Self {
        Self {
            component_name: component_name.into(),
            version,
        }
    }
}

#[derive(Debug, Default)]
struct ComponentHandoffRegistry {
    receivers: HashMap<ComponentHandoffRouteKey, StandardUnixStream>,
}

impl ComponentHandoffRegistry {
    fn register(
        &mut self,
        component_name: impl Into<String>,
        version: Version,
        receiver: StandardUnixStream,
    ) {
        self.receivers.insert(
            ComponentHandoffRouteKey::new(component_name, version),
            receiver,
        );
    }

    fn receiver_for(&self, component_name: &str, version: &Version) -> Result<StandardUnixStream> {
        self.receivers
            .get(&ComponentHandoffRouteKey::new(
                component_name,
                version.clone(),
            ))
            .ok_or_else(|| Error::HandoffReceiverUnavailable {
                component: component_name.to_owned(),
                version: version.as_str().to_owned(),
            })?
            .try_clone()
            .map_err(Error::from)
    }
}

#[derive(Debug)]
pub struct ComponentHandoffRouter {
    endpoint: ComponentHandoffEndpoint,
    public_listener: UnixListener,
    control_listener: UnixListener,
    registry: ComponentHandoffRegistry,
}

impl ComponentHandoffRouter {
    pub fn bind(endpoint: ComponentHandoffEndpoint) -> Result<Self> {
        prepare_socket_parent(endpoint.public_socket_path())?;
        prepare_socket_parent(endpoint.control_socket_path())?;
        unlink_existing_socket_path(endpoint.public_socket_path())?;
        unlink_existing_socket_path(endpoint.control_socket_path())?;
        let public_listener = UnixListener::bind(endpoint.public_socket_path())?;
        let control_listener = UnixListener::bind(endpoint.control_socket_path())?;
        std::fs::set_permissions(
            endpoint.public_socket_path(),
            std::fs::Permissions::from_mode(endpoint.public_socket_mode().as_octal()),
        )?;
        std::fs::set_permissions(
            endpoint.control_socket_path(),
            std::fs::Permissions::from_mode(0o600),
        )?;
        Ok(Self {
            endpoint,
            public_listener,
            control_listener,
            registry: ComponentHandoffRegistry::default(),
        })
    }

    pub fn endpoint(&self) -> &ComponentHandoffEndpoint {
        &self.endpoint
    }

    pub async fn accept_receiver_for_version(&mut self, version: Version) -> Result<()> {
        let (stream, _address) = self.control_listener.accept().await?;
        let stream = stream.into_std()?;
        stream.set_nonblocking(false)?;
        self.registry
            .register(self.endpoint.component_name(), version, stream);
        Ok(())
    }

    pub async fn handoff_one(&mut self, active_version: &Version) -> Result<()> {
        let (stream, _address) = self.public_listener.accept().await?;
        let stream = stream.into_std()?;
        stream.set_nonblocking(false)?;
        let receiver = self
            .registry
            .receiver_for(self.endpoint.component_name(), active_version)?;
        receiver.send_fds(b"persona-public-client", &[&stream])?;
        Ok(())
    }

    pub async fn handoff_one_from_manager_store(
        &mut self,
        reader: &ManagerStoreActiveVersionReader,
    ) -> Result<()> {
        let active_version = reader.read_active_version().await?;
        self.handoff_one(&active_version).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersonaFrameCodec {
    maximum_frame_bytes: usize,
}

impl PersonaFrameCodec {
    pub const fn new(maximum_frame_bytes: usize) -> Self {
        Self {
            maximum_frame_bytes,
        }
    }

    pub async fn read_frame(&self, stream: &mut UnixStream) -> Result<Frame> {
        let mut prefix = [0_u8; 4];
        stream.read_exact(&mut prefix).await?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length > self.maximum_frame_bytes {
            return Err(Error::DaemonFrameTooLarge { bytes: length });
        }

        let mut bytes = Vec::with_capacity(4 + length);
        bytes.extend_from_slice(&prefix);
        bytes.resize(4 + length, 0);
        stream.read_exact(&mut bytes[4..]).await?;

        Ok(Frame::decode_length_prefixed(&bytes)?)
    }

    pub async fn write_frame(&self, stream: &mut UnixStream, frame: &Frame) -> Result<()> {
        let bytes = frame.encode_length_prefixed()?;
        stream.write_all(&bytes).await?;
        stream.flush().await?;
        Ok(())
    }

    pub fn request_frame(&self, request: EngineRequest) -> Frame {
        Frame::new(FrameBody::Request {
            exchange: self.initial_exchange(),
            request: Request::from_payload(request),
        })
    }

    pub fn reply_frame(&self, exchange: ExchangeIdentifier, reply: EngineReply) -> Frame {
        Frame::new(FrameBody::Reply {
            exchange,
            reply: Reply::committed(NonEmpty::single(SubReply::Ok(reply))),
        })
    }

    pub fn request_from_frame(&self, frame: Frame) -> Result<ReceivedEngineRequest> {
        match frame.into_body() {
            FrameBody::Request { exchange, request } => {
                let mut operations = request.payloads.into_vec();
                if operations.len() != 1 {
                    return Err(Error::UnexpectedSignalFrame {
                        got: format!(
                            "persona manager currently accepts one operation, got {}",
                            operations.len()
                        ),
                    });
                }
                let operation = operations.remove(0);
                Ok(ReceivedEngineRequest::new(exchange, operation))
            }
            other => Err(Error::UnexpectedSignalFrame {
                got: format!("{other:?}"),
            }),
        }
    }

    pub fn reply_from_frame(&self, frame: Frame) -> Result<EngineReply> {
        match frame.into_body() {
            FrameBody::Reply { reply, .. } => match reply {
                Reply::Accepted { per_operation, .. } => {
                    let mut operations = per_operation.into_vec();
                    if operations.len() != 1 {
                        return Err(Error::UnexpectedSignalFrame {
                            got: format!(
                                "persona client currently accepts one reply operation, got {}",
                                operations.len()
                            ),
                        });
                    }
                    match operations.remove(0) {
                        SubReply::Ok(payload) => Ok(payload),
                        other => Err(Error::UnexpectedSignalFrame {
                            got: format!("{other:?}"),
                        }),
                    }
                }
                Reply::Rejected { reason } => Err(Error::UnexpectedSignalFrame {
                    got: format!("{reason:?}"),
                }),
            },
            other => Err(Error::UnexpectedSignalFrame {
                got: format!("{other:?}"),
            }),
        }
    }

    fn initial_exchange(&self) -> ExchangeIdentifier {
        ExchangeIdentifier::new(
            SessionEpoch::new(1),
            ExchangeLane::Connector,
            LaneSequence::first(),
        )
    }
}

impl Default for PersonaFrameCodec {
    fn default() -> Self {
        Self::new(1024 * 1024)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerFrameCodec {
    maximum_frame_bytes: usize,
}

impl OwnerFrameCodec {
    pub const fn new(maximum_frame_bytes: usize) -> Self {
        Self {
            maximum_frame_bytes,
        }
    }

    pub async fn read_frame(&self, stream: &mut UnixStream) -> Result<OwnerFrame> {
        let mut prefix = [0_u8; 4];
        stream.read_exact(&mut prefix).await?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length > self.maximum_frame_bytes {
            return Err(Error::DaemonFrameTooLarge { bytes: length });
        }

        let mut bytes = Vec::with_capacity(4 + length);
        bytes.extend_from_slice(&prefix);
        bytes.resize(4 + length, 0);
        stream.read_exact(&mut bytes[4..]).await?;

        Ok(OwnerFrame::decode_length_prefixed(&bytes)?)
    }

    pub async fn write_frame(&self, stream: &mut UnixStream, frame: &OwnerFrame) -> Result<()> {
        let bytes = frame.encode_length_prefixed()?;
        stream.write_all(&bytes).await?;
        stream.flush().await?;
        Ok(())
    }

    pub fn request_frame(&self, request: OwnerRequest) -> OwnerFrame {
        OwnerFrame::new(OwnerFrameBody::Request {
            exchange: self.initial_exchange(),
            request: Request::from_payload(request),
        })
    }

    pub fn reply_frame(&self, exchange: ExchangeIdentifier, reply: OwnerReply) -> OwnerFrame {
        OwnerFrame::new(OwnerFrameBody::Reply {
            exchange,
            reply: Reply::committed(NonEmpty::single(SubReply::Ok(reply))),
        })
    }

    pub fn request_from_frame(&self, frame: OwnerFrame) -> Result<ReceivedOwnerRequest> {
        match frame.into_body() {
            OwnerFrameBody::Request { exchange, request } => {
                let mut operations = request.payloads.into_vec();
                if operations.len() != 1 {
                    return Err(Error::UnexpectedSignalFrame {
                        got: format!(
                            "persona owner endpoint currently accepts one operation, got {}",
                            operations.len()
                        ),
                    });
                }
                Ok(ReceivedOwnerRequest::new(exchange, operations.remove(0)))
            }
            other => Err(Error::UnexpectedSignalFrame {
                got: format!("{other:?}"),
            }),
        }
    }

    pub fn reply_from_frame(&self, frame: OwnerFrame) -> Result<OwnerReply> {
        match frame.into_body() {
            OwnerFrameBody::Reply { reply, .. } => match reply {
                Reply::Accepted { per_operation, .. } => {
                    let mut operations = per_operation.into_vec();
                    if operations.len() != 1 {
                        return Err(Error::UnexpectedSignalFrame {
                            got: format!(
                                "persona owner client currently accepts one reply operation, got {}",
                                operations.len()
                            ),
                        });
                    }
                    match operations.remove(0) {
                        SubReply::Ok(payload) => Ok(payload),
                        other => Err(Error::UnexpectedSignalFrame {
                            got: format!("{other:?}"),
                        }),
                    }
                }
                Reply::Rejected { reason } => Err(Error::UnexpectedSignalFrame {
                    got: format!("{reason:?}"),
                }),
            },
            other => Err(Error::UnexpectedSignalFrame {
                got: format!("{other:?}"),
            }),
        }
    }

    fn initial_exchange(&self) -> ExchangeIdentifier {
        ExchangeIdentifier::new(
            SessionEpoch::new(1),
            ExchangeLane::Connector,
            LaneSequence::first(),
        )
    }
}

impl Default for OwnerFrameCodec {
    fn default() -> Self {
        Self::new(1024 * 1024)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedEngineRequest {
    exchange: ExchangeIdentifier,
    request: EngineRequest,
}

impl ReceivedEngineRequest {
    pub fn new(exchange: ExchangeIdentifier, request: EngineRequest) -> Self {
        Self { exchange, request }
    }

    pub fn exchange(&self) -> ExchangeIdentifier {
        self.exchange
    }

    pub fn into_request(self) -> EngineRequest {
        self.request
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedOwnerRequest {
    exchange: ExchangeIdentifier,
    request: OwnerRequest,
}

impl ReceivedOwnerRequest {
    pub fn new(exchange: ExchangeIdentifier, request: OwnerRequest) -> Self {
        Self { exchange, request }
    }

    pub fn exchange(&self) -> ExchangeIdentifier {
        self.exchange
    }

    pub fn into_request(self) -> OwnerRequest {
        self.request
    }
}

#[derive(Debug, Clone)]
pub struct PersonaClient {
    endpoint: PersonaEndpoint,
    codec: PersonaFrameCodec,
}

impl PersonaClient {
    pub fn from_environment() -> Self {
        Self::new(PersonaEndpoint::from_environment())
    }

    pub fn new(endpoint: PersonaEndpoint) -> Self {
        Self {
            endpoint,
            codec: PersonaFrameCodec::default(),
        }
    }

    pub async fn submit(&self, request: EngineRequest) -> Result<EngineReply> {
        let mut stream = UnixStream::connect(self.endpoint.as_path()).await?;
        let frame = self.codec.request_frame(request);
        self.codec.write_frame(&mut stream, &frame).await?;
        let reply = self.codec.read_frame(&mut stream).await?;
        self.codec.reply_from_frame(reply)
    }
}

#[derive(Debug, Clone)]
pub struct OwnerClient {
    endpoint: OwnerEndpoint,
    codec: OwnerFrameCodec,
}

impl OwnerClient {
    pub fn new(endpoint: OwnerEndpoint) -> Self {
        Self {
            endpoint,
            codec: OwnerFrameCodec::default(),
        }
    }

    pub async fn submit(&self, request: OwnerRequest) -> Result<OwnerReply> {
        let mut stream = UnixStream::connect(self.endpoint.as_path()).await?;
        let frame = self.codec.request_frame(request);
        self.codec.write_frame(&mut stream, &frame).await?;
        let reply = self.codec.read_frame(&mut stream).await?;
        self.codec.reply_from_frame(reply)
    }
}

#[derive(Debug, Clone)]
pub struct PersonaDaemon {
    endpoint: PersonaEndpoint,
    owner_endpoint: OwnerEndpoint,
    manager_store: ManagerStoreLocation,
    launch_plan: Option<PersonaLaunchPlan>,
    unit_controller: Arc<dyn UnitController>,
    codec: PersonaFrameCodec,
    owner_codec: OwnerFrameCodec,
}

impl PersonaDaemon {
    pub fn new(endpoint: PersonaEndpoint) -> Self {
        let manager_store = ManagerStoreLocation::from_endpoint(endpoint.as_path())
            .unwrap_or_else(|_| ManagerStoreLocation::new("manager.redb"));
        Self::with_manager_store(endpoint, manager_store)
    }

    pub fn with_manager_store(
        endpoint: PersonaEndpoint,
        manager_store: ManagerStoreLocation,
    ) -> Self {
        let owner_endpoint = OwnerEndpoint::from_environment_or_manager(&endpoint);
        Self::with_manager_store_and_owner_endpoint(endpoint, owner_endpoint, manager_store)
    }

    pub fn with_manager_store_and_owner_endpoint(
        endpoint: PersonaEndpoint,
        owner_endpoint: OwnerEndpoint,
        manager_store: ManagerStoreLocation,
    ) -> Self {
        Self {
            endpoint,
            owner_endpoint,
            manager_store,
            launch_plan: None,
            unit_controller: Arc::new(ManualUnitController),
            codec: PersonaFrameCodec::default(),
            owner_codec: OwnerFrameCodec::default(),
        }
    }

    pub fn with_launch_plan(mut self, launch_plan: Option<PersonaLaunchPlan>) -> Self {
        self.launch_plan = launch_plan;
        self
    }

    pub fn with_unit_controller(mut self, unit_controller: Arc<dyn UnitController>) -> Self {
        self.unit_controller = unit_controller;
        self
    }

    pub async fn serve(self) -> Result<()> {
        self.endpoint.unlink_existing_socket()?;
        self.owner_endpoint.unlink_existing_socket()?;
        let listener = UnixListener::bind(self.endpoint.as_path())?;
        let owner_listener = UnixListener::bind(self.owner_endpoint.as_path())?;
        let store = ManagerStore::start(self.manager_store.clone())?;
        let manager = EngineManager::start_with_store_and_unit_controller(
            EngineId::new("default"),
            store.clone(),
            self.unit_controller.clone(),
        )
        .await?;
        let supervisor = self.start_supervisor(store).await?;
        let owner_task =
            Self::spawn_owner_server(owner_listener, manager.clone(), self.owner_codec);

        println!(
            "persona-daemon socket={}",
            self.endpoint.as_path().display()
        );

        let _owner_server_lifetime = owner_task;
        let _supervisor_lifetime = supervisor;
        loop {
            let (stream, _) = listener.accept().await?;
            if let Err(error) = self.handle_stream(stream, &manager).await {
                eprintln!("persona-daemon connection error: {error}");
            }
        }
    }

    async fn start_supervisor(
        &self,
        store: kameo::actor::ActorRef<ManagerStore>,
    ) -> Result<Option<kameo::actor::ActorRef<EngineSupervisor>>> {
        let Some(launch_plan) = &self.launch_plan else {
            return Ok(None);
        };
        let supervisor = EngineSupervisor::start(EngineSupervisorInput {
            layout: launch_plan.layout(),
            command_catalog: launch_plan.command_catalog.clone(),
            launch_configuration: EngineLaunchConfiguration::empty(),
            store: Some(store),
        });
        match supervisor.ask(StartPrototypeSupervision).await {
            Ok(_) => {}
            Err(kameo::error::SendError::HandlerError(error)) => {
                return Err(Error::engine_supervisor(
                    "start prototype supervision",
                    error,
                ));
            }
            Err(error) => {
                return Err(Error::actor(
                    "start prototype supervision supervisor",
                    error,
                ));
            }
        }
        Ok(Some(supervisor))
    }

    async fn handle_stream(
        &self,
        mut stream: UnixStream,
        manager: &ActorRef<EngineManager>,
    ) -> Result<()> {
        let frame = self.codec.read_frame(&mut stream).await?;
        let received = self.codec.request_from_frame(frame)?;
        let exchange = received.exchange();
        let reply = manager
            .ask(HandleEngineRequest::new(received.into_request()))
            .await
            .map_err(|error| Error::actor("handle daemon engine request", error))?;
        let frame = self.codec.reply_frame(exchange, reply);
        self.codec.write_frame(&mut stream, &frame).await
    }

    fn spawn_owner_server(
        listener: UnixListener,
        manager: ActorRef<EngineManager>,
        codec: OwnerFrameCodec,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let stream = listener.accept().await;
                let Ok((stream, _)) = stream else {
                    if let Err(error) = stream {
                        eprintln!("persona-daemon owner listener error: {error}");
                    }
                    break;
                };
                if let Err(error) = Self::handle_owner_stream(stream, &manager, codec).await {
                    eprintln!("persona-daemon owner connection error: {error}");
                }
            }
        })
    }

    async fn handle_owner_stream(
        mut stream: UnixStream,
        manager: &ActorRef<EngineManager>,
        codec: OwnerFrameCodec,
    ) -> Result<()> {
        let frame = codec.read_frame(&mut stream).await?;
        let received = codec.request_from_frame(frame)?;
        let exchange = received.exchange();
        let reply = manager
            .ask(HandleOwnerVersionHandover::new(received.into_request()))
            .await
            .map_err(|error| Error::actor("handle daemon owner version handover request", error))?;
        let frame = codec.reply_frame(exchange, reply);
        codec.write_frame(&mut stream, &frame).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaDaemonCommand {
    endpoint: PersonaEndpoint,
    manager_store: ManagerStoreLocation,
    launch_plan: Option<PersonaLaunchPlan>,
}

impl PersonaDaemonCommand {
    pub fn from_environment() -> Result<Self> {
        let endpoint = PersonaEndpoint::from_argument_or_environment(std::env::args_os().nth(1));
        let manager_store = ManagerStoreLocation::from_environment().unwrap_or_else(|| {
            ManagerStoreLocation::from_endpoint(endpoint.as_path())
                .unwrap_or_else(|_| ManagerStoreLocation::new("manager.redb"))
        });
        let launch_plan = PersonaLaunchPlan::from_environment(&endpoint)?;
        Ok(Self {
            endpoint,
            manager_store,
            launch_plan,
        })
    }

    pub async fn run(self) -> Result<()> {
        PersonaDaemon::with_manager_store(self.endpoint, self.manager_store)
            .with_launch_plan(self.launch_plan)
            .serve()
            .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaLaunchPlan {
    engine: EngineId,
    topology: EngineTopology,
    paths: PersonaDaemonPaths,
    manager_socket: PathBuf,
    command_catalog: ComponentCommandCatalog,
}

impl PersonaLaunchPlan {
    pub fn from_environment(endpoint: &PersonaEndpoint) -> Result<Option<Self>> {
        let topology = Self::topology_from_environment()?;
        let Some(command_catalog) =
            ComponentCommandCatalog::from_environment_for_topology(topology)?
        else {
            return Ok(None);
        };
        let engine = std::env::var("PERSONA_MANAGER_ENGINE_ID")
            .map(EngineId::new)
            .unwrap_or_else(|_| EngineId::new("default"));
        let paths = Self::paths_from_environment(endpoint)?;
        Ok(Some(Self {
            engine,
            topology,
            paths,
            manager_socket: endpoint.as_path().to_path_buf(),
            command_catalog,
        }))
    }

    pub fn from_input(input: PersonaLaunchPlanInput) -> Self {
        Self {
            engine: input.engine,
            topology: input.topology,
            paths: input.paths,
            manager_socket: input.manager_socket,
            command_catalog: input.command_catalog,
        }
    }

    fn topology_from_environment() -> Result<EngineTopology> {
        let Some(value) = std::env::var_os("PERSONA_ENGINE_TOPOLOGY") else {
            return Ok(EngineTopology::FullPrototype);
        };
        let text = value.to_string_lossy();
        EngineTopology::from_str(text.as_ref()).ok_or_else(|| Error::UnknownEngineTopology {
            got: text.into_owned(),
        })
    }

    fn paths_from_environment(endpoint: &PersonaEndpoint) -> Result<PersonaDaemonPaths> {
        let endpoint_parent =
            endpoint
                .as_path()
                .parent()
                .ok_or_else(|| Error::ManagerStorePathMissingParent {
                    path: endpoint.as_path().to_path_buf(),
                })?;
        let state_root = std::env::var_os("PERSONA_STATE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| endpoint_parent.join("state"));
        let run_root = std::env::var_os("PERSONA_RUN_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| endpoint_parent.join("run"));
        Ok(PersonaDaemonPaths::new(state_root, run_root))
    }

    pub fn layout(&self) -> crate::engine::EngineLayout {
        self.paths.engine_layout_with_manager_socket_and_topology(
            self.engine.clone(),
            self.manager_socket.clone(),
            self.topology,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaLaunchPlanInput {
    pub engine: EngineId,
    pub topology: EngineTopology,
    pub paths: PersonaDaemonPaths,
    pub manager_socket: PathBuf,
    pub command_catalog: ComponentCommandCatalog,
}
