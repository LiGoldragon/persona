use std::{
    fs,
    path::{Path, PathBuf},
};

use persona::PersonaDaemonConfiguration;
use persona::datom_text::{DatomActualizable, DatomTextualizable};
use thiserror::Error;
use triad_runtime::{ArgumentError, ComponentArgument, ComponentCommand};

fn main() {
    if let Err(error) = ConfigurationWriterCommand::from_environment().run() {
        eprintln!("persona-write-configuration: {error}");
        std::process::exit(1);
    }
}

struct ConfigurationWriterCommand {
    command: ComponentCommand,
}

struct ConfigurationWriterInput {
    text: String,
}

#[derive(Debug, Clone, PartialEq, datom_codec::Datomizable, datom_codec::Compositional)]
struct ConfigurationWriteRequest {
    manager_socket_path: ConfigurationWriterPath,
    manager_store_path: ConfigurationWriterPath,
    output_path: ConfigurationWriterPath,
}

#[derive(Debug, Clone, PartialEq, datom_codec::Datomizable, datom_codec::Compositional)]
struct ConfigurationWriterPath(String);

#[derive(Debug, Clone, PartialEq, datom_codec::Datomizable, datom_codec::Compositional)]
struct ConfigurationWriteOutput {
    output_path: ConfigurationWriterPath,
}

impl ConfigurationWriterCommand {
    fn from_environment() -> Self {
        Self {
            command: ComponentCommand::from_environment(),
        }
    }

    fn run(&self) -> Result<(), ConfigurationWriterError> {
        let source = self.source()?;
        let request = source.parse_request()?;
        let output = request.write()?;
        println!("{}", output.textualize());
        Ok(())
    }

    fn source(&self) -> Result<ConfigurationWriterInput, ConfigurationWriterError> {
        match self.command.dotos_argument()? {
            ComponentArgument::InlineDotos(argument) => {
                Ok(ConfigurationWriterInput::new(argument.into_string()))
            }
            ComponentArgument::DotosFile(file) => {
                let path = file.into_path();
                fs::read_to_string(&path)
                    .map(ConfigurationWriterInput::new)
                    .map_err(|source| ConfigurationWriterError::ReadRequestFile { path, source })
            }
            ComponentArgument::SignalFile(file) => Err(ConfigurationWriterError::SignalInput {
                path: file.into_path(),
            }),
        }
    }
}

impl ConfigurationWriterInput {
    fn new(text: String) -> Self {
        Self { text }
    }

    fn parse_request(&self) -> Result<ConfigurationWriteRequest, datom_codec::Error> {
        ConfigurationWriteRequest::actualize_text(&self.text)
    }
}

impl ConfigurationWriteRequest {
    fn write(self) -> Result<ConfigurationWriteOutput, ConfigurationWriterError> {
        let output_path = self.output_path.clone();
        fs::write(
            output_path.as_path(),
            self.configuration().to_signal_bytes()?,
        )
        .map_err(|source| ConfigurationWriterError::WriteArchive {
            path: output_path.path_buf(),
            source,
        })?;
        Ok(ConfigurationWriteOutput { output_path })
    }

    fn configuration(&self) -> PersonaDaemonConfiguration {
        PersonaDaemonConfiguration::new(
            self.manager_socket_path.as_str(),
            self.manager_store_path.as_str(),
        )
    }
}

impl ConfigurationWriterPath {
    fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn as_path(&self) -> &Path {
        Path::new(self.0.as_str())
    }

    fn path_buf(&self) -> PathBuf {
        self.as_path().to_path_buf()
    }
}

#[derive(Debug, Error)]
enum ConfigurationWriterError {
    #[error("command argument error: {0}")]
    Argument(#[from] ArgumentError),

    #[error("read datom request file {path}: {source}")]
    ReadRequestFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("signal input is not accepted by this text-edge helper: {path}")]
    SignalInput { path: PathBuf },

    #[error("decode datom request: {0:?}")]
    Decode(datom_codec::Error),

    #[error("write daemon configuration archive {path}: {source}")]
    WriteArchive {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("daemon configuration archive error: {0}")]
    Configuration(#[from] persona::ConfigurationError),
}

impl From<datom_codec::Error> for ConfigurationWriterError {
    fn from(fault: datom_codec::Error) -> Self {
        Self::Decode(fault)
    }
}
