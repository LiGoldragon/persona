use std::path::{Path, PathBuf};

use std::os::unix::fs::FileTypeExt;

use kameo::actor::ActorRef;
use signal_core::{AuthProof, FrameBody, LocalOperatorProof, Reply, Request};
use signal_persona::{EngineReply, EngineRequest, Frame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::error::{Error, Result};
use crate::manager::{EngineManager, HandleEngineRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaEndpoint {
    path: PathBuf,
}

impl PersonaEndpoint {
    pub fn from_environment() -> Self {
        match std::env::var_os("PERSONA_SOCKET") {
            Some(path) => Self::from_path(path),
            None => Self::from_path("/tmp/persona.sock"),
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
pub struct PersonaCaller {
    name: String,
}

impl PersonaCaller {
    pub fn from_environment() -> Self {
        match std::env::var("PERSONA_OPERATOR") {
            Ok(name) => Self::new(name),
            Err(_) => Self::new("operator"),
        }
    }

    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn as_str(&self) -> &str {
        self.name.as_str()
    }

    pub fn auth_proof(&self) -> AuthProof {
        AuthProof::LocalOperator(LocalOperatorProof::new(self.as_str()))
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

    pub fn request_frame(&self, caller: &PersonaCaller, request: EngineRequest) -> Frame {
        Frame::new(FrameBody::Request(Request::assert(request))).with_auth(caller.auth_proof())
    }

    pub fn reply_frame(&self, reply: EngineReply) -> Frame {
        Frame::new(FrameBody::Reply(Reply::operation(reply)))
    }

    pub fn request_from_frame(&self, frame: Frame) -> Result<EngineRequest> {
        if frame.auth().is_none() {
            return Err(Error::MissingAuthProof);
        }
        match frame.into_body() {
            FrameBody::Request(Request::Operation { payload, .. }) => Ok(payload),
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
    caller: PersonaCaller,
    codec: PersonaFrameCodec,
}

impl PersonaClient {
    pub fn from_environment() -> Self {
        Self::new(
            PersonaEndpoint::from_environment(),
            PersonaCaller::from_environment(),
        )
    }

    pub fn new(endpoint: PersonaEndpoint, caller: PersonaCaller) -> Self {
        Self {
            endpoint,
            caller,
            codec: PersonaFrameCodec::default(),
        }
    }

    pub async fn submit(&self, request: EngineRequest) -> Result<EngineReply> {
        let mut stream = UnixStream::connect(self.endpoint.as_path()).await?;
        let frame = self.codec.request_frame(&self.caller, request);
        self.codec.write_frame(&mut stream, &frame).await?;
        let reply = self.codec.read_frame(&mut stream).await?;
        self.codec.reply_from_frame(reply)
    }
}

#[derive(Debug, Clone)]
pub struct PersonaDaemon {
    endpoint: PersonaEndpoint,
    codec: PersonaFrameCodec,
}

impl PersonaDaemon {
    pub fn new(endpoint: PersonaEndpoint) -> Self {
        Self {
            endpoint,
            codec: PersonaFrameCodec::default(),
        }
    }

    pub async fn serve(self) -> Result<()> {
        self.endpoint.unlink_existing_socket()?;
        let listener = UnixListener::bind(self.endpoint.as_path())?;
        let manager = EngineManager::start().await;

        println!("personad socket={}", self.endpoint.as_path().display());

        loop {
            let (stream, _) = listener.accept().await?;
            if let Err(error) = self.handle_stream(stream, &manager).await {
                eprintln!("personad connection error: {error}");
            }
        }
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
}

impl PersonaDaemonCommand {
    pub fn from_environment() -> Self {
        let endpoint = PersonaEndpoint::from_argument_or_environment(std::env::args_os().nth(1));
        Self { endpoint }
    }

    pub async fn run(self) -> Result<()> {
        PersonaDaemon::new(self.endpoint).serve().await
    }
}
