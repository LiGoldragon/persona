use std::ffi::OsString;
use std::path::PathBuf;

use crate::datom_text::{DatomActualizable, DatomTextualizable};
use meta_signal_persona as contract;
use signal_persona::ComponentName;

use crate::error::Error;
use crate::schema::{
    ActionAcceptedReport, ActionRejectedReport, ComponentStatusMissingReport,
    ComponentStatusReport, EngineCatalogReport, EngineStatusReport, LaunchAcceptanceReport,
    LaunchRejectionReport, RetirementAcceptanceReport, RetirementRejectionReport,
};

#[derive(datom_codec::Datomizable, datom_codec::Compositional, Debug, Clone, Copy, PartialEq)]
pub enum EngineStatusScope {
    WholeEngine,
}

#[derive(datom_codec::Datomizable, datom_codec::Compositional, Debug, Clone, PartialEq)]
pub struct EngineStatusQuery {
    pub scope: EngineStatusScope,
}

#[derive(datom_codec::Datomizable, datom_codec::Compositional, Debug, Clone, PartialEq)]
pub struct ComponentStatusQuery {
    pub component: ComponentName,
}

#[derive(datom_codec::Datomizable, datom_codec::Compositional, Debug, Clone, PartialEq)]
pub struct ComponentStartup {
    pub component: ComponentName,
}

#[derive(datom_codec::Datomizable, datom_codec::Compositional, Debug, Clone, PartialEq)]
pub struct ComponentShutdown {
    pub component: ComponentName,
}

#[derive(datom_codec::Datomizable, datom_codec::Compositional, Debug, Clone, PartialEq)]
pub enum PersonaRequest {
    EngineStatusQuery(EngineStatusQuery),
    ComponentStatusQuery(ComponentStatusQuery),
    ComponentStartup(ComponentStartup),
    ComponentShutdown(ComponentShutdown),
}

impl PersonaRequest {
    pub fn actualize(text: &str) -> crate::Result<Self> {
        Ok(<Self as DatomActualizable>::actualize_text(text)?)
    }

    pub fn into_engine_request(self) -> contract::Query {
        match self {
            Self::EngineStatusQuery(request) => match request.scope {
                EngineStatusScope::WholeEngine => contract::Query::Query(
                    contract::MetaQuery::EngineStatus(contract::EngineStatusScope::WholeEngine),
                ),
            },
            Self::ComponentStatusQuery(request) => {
                contract::Query::Query(contract::MetaQuery::ComponentStatus(request.component))
            }
            Self::ComponentStartup(request) => contract::Query::Start(request.component),
            Self::ComponentShutdown(request) => contract::Query::Stop(request.component),
        }
    }
}

#[derive(datom_codec::Datomizable, datom_codec::Compositional, Debug, Clone, PartialEq)]
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
    pub fn from_engine_reply(reply: contract::Response) -> Self {
        match reply {
            contract::Response::Launched(acceptance) => {
                Self::LaunchAccepted(LaunchAcceptanceReport::from_contract(acceptance))
            }
            contract::Response::LaunchRejected(rejection) => {
                Self::LaunchRejected(LaunchRejectionReport::from_contract(rejection))
            }
            contract::Response::Catalog(catalog) => {
                Self::EngineCatalog(EngineCatalogReport::from_contract(catalog))
            }
            contract::Response::Retired(engine) => {
                Self::RetirementAccepted(RetirementAcceptanceReport { engine })
            }
            contract::Response::RetireRejected(rejection) => {
                Self::RetirementRejected(RetirementRejectionReport::from_contract(rejection))
            }
            contract::Response::EngineStatus(status) => {
                Self::EngineStatusReport(EngineStatusReport::from_contract(status))
            }
            contract::Response::ComponentStatus(status) => {
                Self::ComponentStatusReport(ComponentStatusReport {
                    component: ComponentStatusReport::from_contract(status).component,
                })
            }
            contract::Response::ComponentMissing(component) => {
                Self::ComponentStatusMissingReport(ComponentStatusMissingReport { component })
            }
            contract::Response::ActionAccepted(acceptance) => {
                Self::ActionAcceptedReport(ActionAcceptedReport {
                    component: acceptance.component_name,
                    desired_state: format!("{:?}", acceptance.component_desired_state),
                })
            }
            contract::Response::ActionRejected(rejection) => {
                Self::ActionRejectedReport(ActionRejectedReport {
                    component: rejection.component_name,
                    reason: format!("{:?}", rejection.action_rejection_reason),
                })
            }
        }
    }

    pub fn textualize(&self) -> String {
        DatomTextualizable::textualize(self)
    }
}

#[derive(Debug, Clone, PartialEq)]
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
            Some(first) if CommandLineArgument::new(first).is_inline_datom() => {
                PersonaRequest::actualize(&self.inline_datom_text()?)
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

    fn inline_datom_text(&self) -> crate::Result<String> {
        let mut parts = Vec::new();
        for argument in &self.arguments {
            let Some(text) = argument.to_str() else {
                return Err(Error::InvalidInlineDatomArgument {
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

#[derive(Debug, Clone, PartialEq)]
pub struct RequestFile {
    path: PathBuf,
}

impl RequestFile {
    pub fn from_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn decode(&self) -> crate::Result<PersonaRequest> {
        let text = std::fs::read_to_string(&self.path)?;
        PersonaRequest::actualize(&text)
    }
}

struct CommandLineArgument<'argument> {
    argument: &'argument OsString,
}

impl<'argument> CommandLineArgument<'argument> {
    fn new(argument: &'argument OsString) -> Self {
        Self { argument }
    }

    /// A request reaches Persona either as one inline datom value or as the
    /// path of a file holding one. A path is what names a place on disk: it
    /// is absolute, or explicitly relative. Everything else is the value
    /// itself — a datom head, a struct, a vector or a map.
    fn is_inline_datom(&self) -> bool {
        let text = self.argument.to_string_lossy();
        !(text.starts_with('/') || text.starts_with("./") || text.starts_with("../"))
    }
}
