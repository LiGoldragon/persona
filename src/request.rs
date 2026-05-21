use std::ffi::OsString;
use std::path::PathBuf;

use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode, NotaEnum, NotaRecord};
use signal_persona as contract;

use crate::error::{Error, Result};
use crate::schema::{
    ActionAcceptedReport, ActionRejectedReport, ComponentStatusMissingReport,
    ComponentStatusReport, EngineStatusReport, RetirementAcceptanceReport,
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

    pub fn into_engine_request(self) -> contract::engine::Operation {
        match self {
            Self::EngineStatusQuery(request) => match request.scope {
                EngineStatusScope::WholeEngine => contract::engine::Operation::Query(
                    contract::Query::EngineStatus(contract::EngineStatusScope::WholeEngine),
                ),
            },
            Self::ComponentStatusQuery(request) => contract::engine::Operation::Query(
                contract::Query::ComponentStatus(request.component),
            ),
            Self::ComponentStartup(request) => {
                contract::engine::Operation::Start(contract::ComponentStartup {
                    component: request.component,
                })
            }
            Self::ComponentShutdown(request) => {
                contract::engine::Operation::Stop(contract::ComponentShutdown {
                    component: request.component,
                })
            }
        }
    }
}

impl NotaEncode for PersonaRequest {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        match self {
            Self::EngineStatusQuery(request) => {
                encoder.start_record("EngineStatusQuery")?;
                request.scope.encode(encoder)?;
                encoder.end_record()
            }
            Self::ComponentStatusQuery(request) => {
                encoder.start_record("ComponentStatusQuery")?;
                request.component.encode(encoder)?;
                encoder.end_record()
            }
            Self::ComponentStartup(request) => {
                encoder.start_record("ComponentStartup")?;
                request.component.encode(encoder)?;
                encoder.end_record()
            }
            Self::ComponentShutdown(request) => {
                encoder.start_record("ComponentShutdown")?;
                request.component.encode(encoder)?;
                encoder.end_record()
            }
        }
    }
}

impl NotaDecode for PersonaRequest {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        let head = decoder.peek_record_head()?;
        match head.as_str() {
            "EngineStatusQuery" => {
                decoder.expect_record_head("EngineStatusQuery")?;
                let scope = EngineStatusScope::decode(decoder)?;
                decoder.expect_record_end()?;
                Ok(Self::EngineStatusQuery(EngineStatusQuery { scope }))
            }
            "ComponentStatusQuery" => {
                decoder.expect_record_head("ComponentStatusQuery")?;
                let component = contract::ComponentName::decode(decoder)?;
                decoder.expect_record_end()?;
                Ok(Self::ComponentStatusQuery(ComponentStatusQuery {
                    component,
                }))
            }
            "ComponentStartup" => {
                decoder.expect_record_head("ComponentStartup")?;
                let component = contract::ComponentName::decode(decoder)?;
                decoder.expect_record_end()?;
                Ok(Self::ComponentStartup(ComponentStartup { component }))
            }
            "ComponentShutdown" => {
                decoder.expect_record_head("ComponentShutdown")?;
                let component = contract::ComponentName::decode(decoder)?;
                decoder.expect_record_end()?;
                Ok(Self::ComponentShutdown(ComponentShutdown { component }))
            }
            other => Err(nota_codec::Error::UnknownVariant {
                enum_name: "PersonaRequest",
                got: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonaOutput {
    LaunchAccepted(contract::LaunchAcceptance),
    LaunchRejected(contract::LaunchRejection),
    EngineCatalog(contract::EngineCatalog),
    RetirementAccepted(RetirementAcceptanceReport),
    RetirementRejected(contract::RetirementRejection),
    EngineStatusReport(EngineStatusReport),
    ComponentStatusReport(ComponentStatusReport),
    ComponentStatusMissingReport(ComponentStatusMissingReport),
    ActionAcceptedReport(ActionAcceptedReport),
    ActionRejectedReport(ActionRejectedReport),
    ObserverSubscriptionOpened(contract::engine::ObserverSubscriptionOpened),
}

impl PersonaOutput {
    pub fn from_engine_reply(reply: contract::engine::Reply) -> Self {
        match reply {
            contract::engine::Reply::Launched(acceptance) => Self::LaunchAccepted(acceptance),
            contract::engine::Reply::LaunchRejected(rejection) => Self::LaunchRejected(rejection),
            contract::engine::Reply::Catalog(catalog) => Self::EngineCatalog(catalog),
            contract::engine::Reply::Retired(engine) => {
                Self::RetirementAccepted(RetirementAcceptanceReport { engine })
            }
            contract::engine::Reply::RetireRejected(rejection) => {
                Self::RetirementRejected(rejection)
            }
            contract::engine::Reply::EngineStatus(status) => {
                Self::EngineStatusReport(EngineStatusReport::from_contract(status))
            }
            contract::engine::Reply::ComponentStatus(status) => {
                Self::ComponentStatusReport(ComponentStatusReport { component: status })
            }
            contract::engine::Reply::ComponentMissing(component) => {
                Self::ComponentStatusMissingReport(ComponentStatusMissingReport { component })
            }
            contract::engine::Reply::ActionAccepted(acceptance) => {
                Self::ActionAcceptedReport(ActionAcceptedReport {
                    component: acceptance.component,
                    desired_state: acceptance.desired_state,
                })
            }
            contract::engine::Reply::ActionRejected(rejection) => {
                Self::ActionRejectedReport(ActionRejectedReport {
                    component: rejection.component,
                    reason: rejection.reason,
                })
            }
            contract::engine::Reply::ObserverSubscriptionOpened(opened) => {
                Self::ObserverSubscriptionOpened(opened)
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
            Self::LaunchAccepted(output) => output.encode(encoder),
            Self::LaunchRejected(output) => output.encode(encoder),
            Self::EngineCatalog(output) => output.encode(encoder),
            Self::RetirementAccepted(output) => output.encode(encoder),
            Self::RetirementRejected(output) => output.encode(encoder),
            Self::EngineStatusReport(output) => output.encode(encoder),
            Self::ComponentStatusReport(output) => output.encode(encoder),
            Self::ComponentStatusMissingReport(output) => output.encode(encoder),
            Self::ActionAcceptedReport(output) => output.encode(encoder),
            Self::ActionRejectedReport(output) => output.encode(encoder),
            Self::ObserverSubscriptionOpened(output) => output.encode(encoder),
        }
    }
}

impl NotaDecode for PersonaOutput {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        let head = decoder.peek_record_head()?;
        match head.as_str() {
            "LaunchAcceptance" => Ok(Self::LaunchAccepted(contract::LaunchAcceptance::decode(
                decoder,
            )?)),
            "LaunchRejection" => Ok(Self::LaunchRejected(contract::LaunchRejection::decode(
                decoder,
            )?)),
            "EngineCatalog" => Ok(Self::EngineCatalog(contract::EngineCatalog::decode(
                decoder,
            )?)),
            "RetirementAcceptanceReport" => Ok(Self::RetirementAccepted(
                RetirementAcceptanceReport::decode(decoder)?,
            )),
            "RetirementRejection" => Ok(Self::RetirementRejected(
                contract::RetirementRejection::decode(decoder)?,
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
            "ActionAcceptedReport" => Ok(Self::ActionAcceptedReport(ActionAcceptedReport::decode(
                decoder,
            )?)),
            "ActionRejectedReport" => Ok(Self::ActionRejectedReport(ActionRejectedReport::decode(
                decoder,
            )?)),
            "ObserverSubscriptionOpened" => Ok(Self::ObserverSubscriptionOpened(
                contract::engine::ObserverSubscriptionOpened::decode(decoder)?,
            )),
            other => Err(nota_codec::Error::UnknownVariant {
                enum_name: "PersonaOutput",
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
