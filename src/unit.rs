use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use signal_persona::ComponentName;
use signal_persona_auth::EngineId;
use thiserror::Error;
use tokio::process::Command;

use crate::upgrade::Version;

pub type UnitStartFuture<'a> =
    Pin<Box<dyn Future<Output = Result<UnitReceipt, UnitFailure>> + Send + 'a>>;

pub trait UnitController: std::fmt::Debug + Send + Sync + 'static {
    fn start_unit<'a>(&'a self, unit: ComponentUnit) -> UnitStartFuture<'a>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentUnit {
    engine: EngineId,
    component: ComponentName,
    version: Version,
    name: UnitName,
}

impl ComponentUnit {
    pub fn new(engine: EngineId, component: ComponentName, version: Version) -> Self {
        let name = UnitName::for_component(&engine, &component, &version);
        Self {
            engine,
            component,
            version,
            name,
        }
    }

    pub fn engine(&self) -> &EngineId {
        &self.engine
    }

    pub fn component(&self) -> &ComponentName {
        &self.component
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn name(&self) -> &UnitName {
        &self.name
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitName(String);

impl UnitName {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn for_component(engine: &EngineId, component: &ComponentName, version: &Version) -> Self {
        Self::new(format!(
            "persona-component@{}-{}-{}.service",
            engine.as_str(),
            component.as_str(),
            version.as_str()
        ))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for UnitName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitReceipt {
    unit: ComponentUnit,
}

impl UnitReceipt {
    pub fn started(unit: ComponentUnit) -> Self {
        Self { unit }
    }

    pub fn unit(&self) -> &ComponentUnit {
        &self.unit
    }
}

#[derive(Debug, Error)]
pub enum UnitFailure {
    #[error("start systemd unit {unit}: {source}")]
    Start {
        unit: UnitName,
        source: std::io::Error,
    },

    #[error("systemd rejected start for {unit}: status={status:?}, stderr={stderr}")]
    StartRejected {
        unit: UnitName,
        status: Option<i32>,
        stderr: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ManualUnitController;

impl UnitController for ManualUnitController {
    fn start_unit<'a>(&'a self, unit: ComponentUnit) -> UnitStartFuture<'a> {
        Box::pin(async move { Ok(UnitReceipt::started(unit)) })
    }
}

#[derive(Debug, Clone)]
pub struct SystemdUnitController {
    systemctl: PathBuf,
}

impl SystemdUnitController {
    pub fn new(systemctl: impl Into<PathBuf>) -> Self {
        Self {
            systemctl: systemctl.into(),
        }
    }
}

impl Default for SystemdUnitController {
    fn default() -> Self {
        Self::new("systemctl")
    }
}

impl UnitController for SystemdUnitController {
    fn start_unit<'a>(&'a self, unit: ComponentUnit) -> UnitStartFuture<'a> {
        let systemctl = self.systemctl.clone();
        Box::pin(async move {
            let output = Command::new(&systemctl)
                .arg("start")
                .arg(unit.name().as_str())
                .output()
                .await
                .map_err(|source| UnitFailure::Start {
                    unit: unit.name().clone(),
                    source,
                })?;
            if output.status.success() {
                Ok(UnitReceipt::started(unit))
            } else {
                Err(UnitFailure::StartRejected {
                    unit: unit.name().clone(),
                    status: output.status.code(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                })
            }
        })
    }
}
