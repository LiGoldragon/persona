use std::path::{Path, PathBuf};

use std::os::unix::fs::FileTypeExt;

use kameo::actor::ActorRef;
use signal_core::{FrameBody, Reply, Request};
use signal_persona::{EngineReply, EngineRequest, Frame};
use signal_persona_auth::EngineId;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::engine::PersonaDaemonPaths;
use crate::error::{Error, Result};
use crate::launch::{ComponentCommandCatalog, EngineLaunchConfiguration};
use crate::manager::{EngineManager, HandleEngineRequest};
use crate::manager_store::{ManagerStore, ManagerStoreLocation};
use crate::supervisor::{EngineSupervisor, EngineSupervisorInput, StartPrototypeSupervision};

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
        Frame::new(FrameBody::Request(Request::from_payload(request)))
    }

    pub fn reply_frame(&self, reply: EngineReply) -> Frame {
        Frame::new(FrameBody::Reply(Reply::operation(reply)))
    }

    pub fn request_from_frame(&self, frame: Frame) -> Result<EngineRequest> {
        match frame.into_body() {
            FrameBody::Request(request) => {
                request
                    .into_payload_checked()
                    .map_err(|error| Error::UnexpectedSignalFrame {
                        got: error.to_string(),
                    })
            }
            other => Err(Error::UnexpectedSignalFrame {
                got: format!("{other:?}"),
            }),
        }
    }

    pub fn reply_from_frame(&self, frame: Frame) -> Result<EngineReply> {
        match frame.into_body() {
            FrameBody::Reply(Reply::Operation(reply)) => Ok(reply),
            other => Err(Error::UnexpectedSignalFrame {
                got: format!("{other:?}"),
            }),
        }
    }
}

impl Default for PersonaFrameCodec {
    fn default() -> Self {
        Self::new(1024 * 1024)
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
pub struct PersonaDaemon {
    endpoint: PersonaEndpoint,
    manager_store: ManagerStoreLocation,
    launch_plan: Option<PersonaLaunchPlan>,
    codec: PersonaFrameCodec,
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
        Self {
            endpoint,
            manager_store,
            launch_plan: None,
            codec: PersonaFrameCodec::default(),
        }
    }

    pub fn with_launch_plan(mut self, launch_plan: Option<PersonaLaunchPlan>) -> Self {
        self.launch_plan = launch_plan;
        self
    }

    pub async fn serve(self) -> Result<()> {
        self.endpoint.unlink_existing_socket()?;
        let listener = UnixListener::bind(self.endpoint.as_path())?;
        let store = ManagerStore::start(self.manager_store.clone())?;
        let manager =
            EngineManager::start_with_store(EngineId::new("default"), store.clone()).await?;
        let supervisor = self.start_supervisor(store).await?;

        println!(
            "persona-daemon socket={}",
            self.endpoint.as_path().display()
        );

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
        let request = self.codec.request_from_frame(frame)?;
        let reply = manager
            .ask(HandleEngineRequest::new(request))
            .await
            .map_err(|error| Error::actor("handle daemon engine request", error))?;
        let frame = self.codec.reply_frame(reply);
        self.codec.write_frame(&mut stream, &frame).await
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
    paths: PersonaDaemonPaths,
    manager_socket: PathBuf,
    command_catalog: ComponentCommandCatalog,
}

impl PersonaLaunchPlan {
    pub fn from_environment(endpoint: &PersonaEndpoint) -> Result<Option<Self>> {
        let Some(command_catalog) = ComponentCommandCatalog::from_environment()? else {
            return Ok(None);
        };
        let engine = std::env::var("PERSONA_MANAGER_ENGINE_ID")
            .map(EngineId::new)
            .unwrap_or_else(|_| EngineId::new("default"));
        let paths = Self::paths_from_environment(endpoint)?;
        Ok(Some(Self {
            engine,
            paths,
            manager_socket: endpoint.as_path().to_path_buf(),
            command_catalog,
        }))
    }

    pub fn from_input(input: PersonaLaunchPlanInput) -> Self {
        Self {
            engine: input.engine,
            paths: input.paths,
            manager_socket: input.manager_socket,
            command_catalog: input.command_catalog,
        }
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
        self.paths
            .engine_layout_with_manager_socket(self.engine.clone(), self.manager_socket.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaLaunchPlanInput {
    pub engine: EngineId,
    pub paths: PersonaDaemonPaths,
    pub manager_socket: PathBuf,
    pub command_catalog: ComponentCommandCatalog,
}
