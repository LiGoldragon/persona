use std::ffi::OsString;
use std::path::PathBuf;

use meta_signal_persona as contract;
use nota::{NotaDecode, NotaEncode, NotaSource};

use crate::error::Error;
use crate::schema::{
    ActionAcceptedReport, ActionRejectedReport, ComponentStatusMissingReport,
    ComponentStatusReport, EngineCatalogReport, EngineStatusReport, LaunchAcceptanceReport,
    LaunchRejectionReport, RetirementAcceptanceReport, RetirementRejectionReport,
};

#[derive(NotaEncode, NotaDecode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineStatusScope {
    WholeEngine,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct EngineStatusQuery {
    pub scope: EngineStatusScope,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusQuery {
    pub component: contract::ComponentName,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ComponentStartup {
    pub component: contract::ComponentName,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ComponentShutdown {
    pub component: contract::ComponentName,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub enum PersonaRequest {
    EngineStatusQuery(EngineStatusQuery),
    ComponentStatusQuery(ComponentStatusQuery),
    ComponentStartup(ComponentStartup),
    ComponentShutdown(ComponentShutdown),
}

impl PersonaRequest {
    pub fn from_nota(text: &str) -> crate::Result<Self> {
        Ok(NotaSource::new(text).parse::<Self>()?)
    }

    pub fn into_engine_request(self) -> contract::Operation {
        match self {
            Self::EngineStatusQuery(request) => match request.scope {
                EngineStatusScope::WholeEngine => contract::Operation::Query(
                    contract::MetaQuery::EngineStatus(contract::EngineStatusScope::WholeEngine)
                        .into(),
                ),
            },
            Self::ComponentStatusQuery(request) => contract::Operation::Query(
                contract::MetaQuery::ComponentStatus(request.component).into(),
            ),
            Self::ComponentStartup(request) => contract::Operation::Start(
                contract::ComponentStartup::new(request.component).into(),
            ),
            Self::ComponentShutdown(request) => contract::Operation::Stop(
                contract::ComponentShutdown::new(request.component).into(),
            ),
        }
    }
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub enum PersonaOutput {
    LaunchAccepted(LaunchAcceptanceReport),
    LaunchRejected(LaunchRejectionReport),
    EngineCatalog(EngineCatalogReport),
    RetirementAccepted(RetirementAcceptanceReport),
    RetirementRejected(RetirementRejectionReport),
    EngineStatusReport(EngineStatusReport),
    ComponentStatusReport(ComponentStatusReport),
    ComponentStatusMissingReport(ComponentStatusMissingReport),
    ActionAcceptedReport(ActionAcceptedReport),
    ActionRejectedReport(ActionRejectedReport),
}

impl PersonaOutput {
    pub fn from_engine_reply(reply: contract::Reply) -> Self {
        match reply {
            contract::Reply::Launched(acceptance) => Self::LaunchAccepted(
                LaunchAcceptanceReport::from_contract(acceptance.into_payload()),
            ),
            contract::Reply::LaunchRejected(rejection) => Self::LaunchRejected(
                LaunchRejectionReport::from_contract(rejection.into_payload()),
            ),
            contract::Reply::Catalog(catalog) => {
                Self::EngineCatalog(EngineCatalogReport::from_contract(catalog.into_payload()))
            }
            contract::Reply::Retired(engine) => {
                Self::RetirementAccepted(RetirementAcceptanceReport {
                    engine: engine.into_payload(),
                })
            }
            contract::Reply::RetireRejected(rejection) => Self::RetirementRejected(
                RetirementRejectionReport::from_contract(rejection.into_payload()),
            ),
            contract::Reply::EngineStatus(status) => {
                Self::EngineStatusReport(EngineStatusReport::from_contract(status.into_payload()))
            }
            contract::Reply::ComponentStatus(status) => {
                Self::ComponentStatusReport(ComponentStatusReport {
                    component: ComponentStatusReport::from_contract(status.into_payload())
                        .component,
                })
            }
            contract::Reply::ComponentMissing(component) => {
                Self::ComponentStatusMissingReport(ComponentStatusMissingReport {
                    component: component.into_payload(),
                })
            }
            contract::Reply::ActionAccepted(acceptance) => {
                let acceptance = acceptance.into_payload();
                Self::ActionAcceptedReport(ActionAcceptedReport {
                    component: acceptance.component,
                    desired_state: format!("{:?}", acceptance.desired_state),
                })
            }
            contract::Reply::ActionRejected(rejection) => {
                let rejection = rejection.into_payload();
                Self::ActionRejectedReport(ActionRejectedReport {
                    component: rejection.component,
                    reason: format!("{:?}", rejection.reason),
                })
            }
        }
    }

    pub fn to_nota(&self) -> crate::Result<String> {
        Ok(NotaEncode::to_nota(self))
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

    pub fn decode_request(&self) -> crate::Result<PersonaRequest> {
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

    fn inline_nota_text(&self) -> crate::Result<String> {
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

    fn require_single_path_argument(&self) -> crate::Result<()> {
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

    pub fn decode(&self) -> crate::Result<PersonaRequest> {
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
