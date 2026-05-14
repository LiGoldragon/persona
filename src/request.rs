use std::ffi::OsString;
use std::path::PathBuf;

use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode, NotaEnum, NotaRecord};
use signal_persona as contract;

use crate::error::{Error, Result};
use crate::schema::{
    ComponentStatusMissingReport, ComponentStatusReport, EngineStatusReport,
    SupervisorActionAcceptedReport, SupervisorActionRejectedReport,
};

#[derive(NotaEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineStatusScope {
    WholeEngine,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct EngineStatusQuery {
    pub scope: EngineStatusScope,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusQuery {
    pub component: contract::ComponentName,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStartup {
    pub component: contract::ComponentName,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentShutdown {
    pub component: contract::ComponentName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonaRequest {
    EngineStatusQuery(EngineStatusQuery),
    ComponentStatusQuery(ComponentStatusQuery),
    ComponentStartup(ComponentStartup),
    ComponentShutdown(ComponentShutdown),
}

impl PersonaRequest {
    pub fn from_nota(text: &str) -> Result<Self> {
        let mut decoder = Decoder::new(text);
        let request = Self::decode(&mut decoder)?;
        if let Some(token) = decoder.peek_token()? {
            return Err(nota_codec::Error::UnexpectedToken {
                expected: "end of input",
                got: token,
            }
            .into());
        }
        Ok(request)
    }

    pub fn into_engine_request(self) -> contract::EngineRequest {
        match self {
            Self::EngineStatusQuery(request) => match request.scope {
                EngineStatusScope::WholeEngine => contract::EngineRequest::EngineStatusQuery(
                    contract::EngineStatusQuery::whole_engine(),
                ),
            },
            Self::ComponentStatusQuery(request) => {
                contract::EngineRequest::ComponentStatusQuery(contract::ComponentStatusQuery {
                    component: request.component,
                })
            }
            Self::ComponentStartup(request) => {
                contract::EngineRequest::ComponentStartup(contract::ComponentStartup {
                    component: request.component,
                })
            }
            Self::ComponentShutdown(request) => {
                contract::EngineRequest::ComponentShutdown(contract::ComponentShutdown {
                    component: request.component,
                })
            }
        }
    }
}

impl NotaEncode for PersonaRequest {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        match self {
            Self::EngineStatusQuery(request) => request.encode(encoder),
            Self::ComponentStatusQuery(request) => request.encode(encoder),
            Self::ComponentStartup(request) => request.encode(encoder),
            Self::ComponentShutdown(request) => request.encode(encoder),
        }
    }
}

impl NotaDecode for PersonaRequest {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        let head = decoder.peek_record_head()?;
        match head.as_str() {
            "EngineStatusQuery" => Ok(Self::EngineStatusQuery(EngineStatusQuery::decode(decoder)?)),
            "ComponentStatusQuery" => Ok(Self::ComponentStatusQuery(ComponentStatusQuery::decode(
                decoder,
            )?)),
            "ComponentStartup" => Ok(Self::ComponentStartup(ComponentStartup::decode(decoder)?)),
            "ComponentShutdown" => Ok(Self::ComponentShutdown(ComponentShutdown::decode(decoder)?)),
            other => Err(nota_codec::Error::UnknownKindForVerb {
                verb: "PersonaRequest",
                got: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonaOutput {
    EngineLaunchAccepted(contract::EngineLaunchAcceptance),
    EngineLaunchRejected(contract::EngineLaunchRejection),
    EngineCatalog(contract::EngineCatalog),
    EngineRetirementAccepted(contract::EngineRetirementAcceptance),
    EngineRetirementRejected(contract::EngineRetirementRejection),
    EngineStatusReport(EngineStatusReport),
    ComponentStatusReport(ComponentStatusReport),
    ComponentStatusMissingReport(ComponentStatusMissingReport),
    SupervisorActionAcceptedReport(SupervisorActionAcceptedReport),
    SupervisorActionRejectedReport(SupervisorActionRejectedReport),
}

impl PersonaOutput {
    pub fn from_engine_reply(reply: contract::EngineReply) -> Self {
        match reply {
            contract::EngineReply::EngineLaunchAccepted(acceptance) => {
                Self::EngineLaunchAccepted(acceptance)
            }
            contract::EngineReply::EngineLaunchRejected(rejection) => {
                Self::EngineLaunchRejected(rejection)
            }
            contract::EngineReply::EngineCatalog(catalog) => Self::EngineCatalog(catalog),
            contract::EngineReply::EngineRetirementAccepted(acceptance) => {
                Self::EngineRetirementAccepted(acceptance)
            }
            contract::EngineReply::EngineRetirementRejected(rejection) => {
                Self::EngineRetirementRejected(rejection)
            }
            contract::EngineReply::EngineStatus(status) => {
                Self::EngineStatusReport(EngineStatusReport::from_contract(status))
            }
            contract::EngineReply::ComponentStatus(status) => {
                Self::ComponentStatusReport(ComponentStatusReport { component: status })
            }
            contract::EngineReply::ComponentStatusMissing(missing) => {
                Self::ComponentStatusMissingReport(ComponentStatusMissingReport {
                    component: missing.component,
                })
            }
            contract::EngineReply::SupervisorActionAccepted(acceptance) => {
                Self::SupervisorActionAcceptedReport(SupervisorActionAcceptedReport {
                    component: acceptance.component,
                    desired_state: acceptance.desired_state,
                })
            }
            contract::EngineReply::SupervisorActionRejected(rejection) => {
                Self::SupervisorActionRejectedReport(SupervisorActionRejectedReport {
                    component: rejection.component,
                    reason: rejection.reason,
                })
            }
        }
    }

    pub fn to_nota(&self) -> Result<String> {
        let mut encoder = Encoder::new();
        self.encode(&mut encoder)?;
        Ok(encoder.into_string())
    }
}

impl NotaEncode for PersonaOutput {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        match self {
            Self::EngineLaunchAccepted(output) => output.encode(encoder),
            Self::EngineLaunchRejected(output) => output.encode(encoder),
            Self::EngineCatalog(output) => output.encode(encoder),
            Self::EngineRetirementAccepted(output) => output.encode(encoder),
            Self::EngineRetirementRejected(output) => output.encode(encoder),
            Self::EngineStatusReport(output) => output.encode(encoder),
            Self::ComponentStatusReport(output) => output.encode(encoder),
            Self::ComponentStatusMissingReport(output) => output.encode(encoder),
            Self::SupervisorActionAcceptedReport(output) => output.encode(encoder),
            Self::SupervisorActionRejectedReport(output) => output.encode(encoder),
        }
    }
}

impl NotaDecode for PersonaOutput {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        let head = decoder.peek_record_head()?;
        match head.as_str() {
            "EngineLaunchAcceptance" => Ok(Self::EngineLaunchAccepted(
                contract::EngineLaunchAcceptance::decode(decoder)?,
            )),
            "EngineLaunchRejection" => Ok(Self::EngineLaunchRejected(
                contract::EngineLaunchRejection::decode(decoder)?,
            )),
            "EngineCatalog" => Ok(Self::EngineCatalog(contract::EngineCatalog::decode(
                decoder,
            )?)),
            "EngineRetirementAcceptance" => Ok(Self::EngineRetirementAccepted(
                contract::EngineRetirementAcceptance::decode(decoder)?,
            )),
            "EngineRetirementRejection" => Ok(Self::EngineRetirementRejected(
                contract::EngineRetirementRejection::decode(decoder)?,
            )),
            "EngineStatusReport" => Ok(Self::EngineStatusReport(EngineStatusReport::decode(
                decoder,
            )?)),
            "ComponentStatusReport" => Ok(Self::ComponentStatusReport(
                ComponentStatusReport::decode(decoder)?,
            )),
            "ComponentStatusMissingReport" => Ok(Self::ComponentStatusMissingReport(
                ComponentStatusMissingReport::decode(decoder)?,
            )),
            "SupervisorActionAcceptedReport" => Ok(Self::SupervisorActionAcceptedReport(
                SupervisorActionAcceptedReport::decode(decoder)?,
            )),
            "SupervisorActionRejectedReport" => Ok(Self::SupervisorActionRejectedReport(
                SupervisorActionRejectedReport::decode(decoder)?,
            )),
            other => Err(nota_codec::Error::UnknownKindForVerb {
                verb: "PersonaOutput",
                got: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    arguments: Vec<OsString>,
}

impl CommandLine {
    pub fn from_env() -> Self {
        Self::from_arguments(std::env::args_os().skip(1))
    }

    pub fn from_arguments<I, S>(arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            arguments: arguments.into_iter().map(Into::into).collect(),
        }
    }

    pub fn decode_request(&self) -> Result<PersonaRequest> {
        match self.arguments.first() {
            Some(first) if CommandLineArgument::new(first).starts_inline_record() => {
                PersonaRequest::from_nota(&self.inline_nota_text()?)
            }
            Some(first) => {
                self.require_single_path_argument()?;
                RequestFile::from_path(PathBuf::from(first)).decode()
            }
            None => Ok(PersonaRequest::EngineStatusQuery(EngineStatusQuery {
                scope: EngineStatusScope::WholeEngine,
            })),
        }
    }

    fn inline_nota_text(&self) -> Result<String> {
        let mut parts = Vec::new();
        for argument in &self.arguments {
            let Some(text) = argument.to_str() else {
                return Err(Error::InvalidInlineNotaArgument {
                    got: format!("{argument:?}"),
                });
            };
            parts.push(text.to_string());
        }
        Ok(parts.join(" "))
    }

    fn require_single_path_argument(&self) -> Result<()> {
        if let Some(argument) = self.arguments.get(1) {
            return Err(Error::UnexpectedArgument {
                got: argument.to_string_lossy().to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFile {
    path: PathBuf,
}

impl RequestFile {
    pub fn from_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn decode(&self) -> Result<PersonaRequest> {
        let text = std::fs::read_to_string(&self.path)?;
        PersonaRequest::from_nota(&text)
    }
}

struct CommandLineArgument<'argument> {
    argument: &'argument OsString,
}

impl<'argument> CommandLineArgument<'argument> {
    fn new(argument: &'argument OsString) -> Self {
        Self { argument }
    }

    fn starts_inline_record(&self) -> bool {
        self.argument.to_string_lossy().starts_with('(')
    }
}
