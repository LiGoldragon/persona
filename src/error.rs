use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("nota: {0}")]
    Nota(#[from] nota_codec::Error),

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

    #[error("signal frame: {0}")]
    SignalFrame(#[from] signal_core::FrameError),

    #[error("daemon frame is too large: {bytes} bytes")]
    DaemonFrameTooLarge { bytes: usize },

    #[error("socket path is occupied by a non-socket file: {path}")]
    SocketPathOccupied { path: PathBuf },

    #[error("persona daemon request is missing authentication proof")]
    MissingAuthProof,

    #[error("unexpected Signal frame: {got}")]
    UnexpectedSignalFrame { got: String },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn actor(operation: &'static str, error: impl std::fmt::Debug) -> Self {
        Self::Actor {
            operation,
            detail: format!("{error:?}"),
        }
    }
}
