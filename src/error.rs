use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("nota decode: {0}")]
    NotaDecode(#[from] nota::NotaDecodeError),

    #[error("sema engine: {0}")]
    SemaEngine(#[from] sema_engine::Error),

    #[error("sema kernel: {0}")]
    SemaKernel(#[from] sema_engine::StorageKernelError),

    #[error("inline Nota argument must be UTF-8: {got:?}")]
    InvalidInlineNotaArgument { got: String },

    #[error("unexpected command-line argument: {got:?}")]
    UnexpectedArgument { got: String },

    #[error("no Persona request supplied and no default config file exists; searched {searched:?}")]
    NoRequestConfig { searched: Vec<PathBuf> },

    #[error("persona actor failed during {operation}: {detail}")]
    Actor {
        operation: &'static str,
        detail: String,
    },

    #[error("engine supervisor failed during {operation}: {detail}")]
    EngineSupervisor {
        operation: &'static str,
        detail: String,
    },

    #[error("signal frame: {0}")]
    SignalFrame(#[from] signal_frame::FrameError),

    #[error("daemon frame is too large: {bytes} bytes")]
    DaemonFrameTooLarge { bytes: usize },

    #[error("socket path is occupied by a non-socket file: {path}")]
    SocketPathOccupied { path: PathBuf },

    #[error("unexpected Signal frame: {got}")]
    UnexpectedSignalFrame { got: String },

    #[error("signal request failed structural checks: {reason}")]
    InvalidSignalRequest {
        reason: signal_frame::RequestRejectionReason,
    },

    #[error("manager store path is missing a parent directory: {path}")]
    ManagerStorePathMissingParent { path: PathBuf },

    #[error("manager store handle has been released after on_stop")]
    ManagerStoreClosed,

    #[error("unknown Persona engine topology: {got}")]
    UnknownEngineTopology { got: String },

    #[error("component command resolution: {0}")]
    CommandResolution(#[from] crate::launch::CommandResolutionFailure),

    #[error("component unit control: {0}")]
    ComponentUnit(#[from] crate::unit::UnitFailure),

    #[error("active version is missing: engine={engine}, component={component}")]
    ActiveVersionMissing { engine: String, component: String },

    #[error("component handoff receiver is unavailable: component={component}, version={version}")]
    HandoffReceiverUnavailable { component: String, version: String },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn actor(operation: &'static str, error: impl std::fmt::Debug) -> Self {
        Self::Actor {
            operation,
            detail: format!("{error:?}"),
        }
    }

    pub fn engine_supervisor(operation: &'static str, error: impl std::fmt::Debug) -> Self {
        Self::EngineSupervisor {
            operation,
            detail: format!("{error:?}"),
        }
    }
}
