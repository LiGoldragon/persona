use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use kameo::actor::{Actor, ActorRef, Spawn};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use signal_persona::ComponentName;
use signal_persona_auth::EngineId;
use thiserror::Error;
use tokio::process::Command;
use zbus::zvariant::OwnedObjectPath;

use crate::upgrade::Version;

pub type UnitFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, UnitFailure>> + Send + 'a>>;

pub trait UnitController: std::fmt::Debug + Send + Sync + 'static {
    fn start<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt>;

    fn stop<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt>;

    fn restart<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt>;

    fn status<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitStatusReport>;
}

#[derive(Debug)]
pub struct ComponentUnitManager {
    controller: Arc<dyn UnitController>,
}

impl ComponentUnitManager {
    pub fn new(controller: Arc<dyn UnitController>) -> Self {
        Self { controller }
    }

    pub fn start_with_controller(controller: Arc<dyn UnitController>) -> ActorRef<Self> {
        Self::spawn(Self::new(controller))
    }
}

impl Actor for ComponentUnitManager {
    type Args = Self;
    type Error = Infallible;

    async fn on_start(
        actor: Self::Args,
        _actor_reference: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(actor)
    }
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
        let name = UnitName::for_component(&component, &version);
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

    pub fn for_component(component: &ComponentName, version: &Version) -> Self {
        Self::new(format!(
            "persona-component@{}:{}.service",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitAction {
    Start,
    Stop,
    Restart,
}

impl UnitAction {
    fn systemctl_verb(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

impl std::fmt::Display for UnitAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.systemctl_verb())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitReceipt {
    unit: ComponentUnit,
    action: UnitAction,
}

impl UnitReceipt {
    pub fn started(unit: ComponentUnit) -> Self {
        Self {
            unit,
            action: UnitAction::Start,
        }
    }

    pub fn stopped(unit: ComponentUnit) -> Self {
        Self {
            unit,
            action: UnitAction::Stop,
        }
    }

    pub fn restarted(unit: ComponentUnit) -> Self {
        Self {
            unit,
            action: UnitAction::Restart,
        }
    }

    pub fn from_action(unit: ComponentUnit, action: UnitAction) -> Self {
        match action {
            UnitAction::Start => Self::started(unit),
            UnitAction::Stop => Self::stopped(unit),
            UnitAction::Restart => Self::restarted(unit),
        }
    }

    pub fn unit(&self) -> &ComponentUnit {
        &self.unit
    }

    pub fn action(&self) -> UnitAction {
        self.action
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitStatus {
    Active,
    Inactive,
    Failed,
    Unknown(String),
}

impl UnitStatus {
    fn from_systemctl_output(output: &[u8]) -> Self {
        match String::from_utf8_lossy(output).trim() {
            "active" => Self::Active,
            "inactive" => Self::Inactive,
            "failed" => Self::Failed,
            other => Self::Unknown(other.to_string()),
        }
    }

    fn from_systemd_active_state(state: String) -> Self {
        match state.as_str() {
            "active" | "activating" | "reloading" | "refreshing" => Self::Active,
            "inactive" | "deactivating" => Self::Inactive,
            "failed" => Self::Failed,
            _ => Self::Unknown(state),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitStatusReport {
    unit: ComponentUnit,
    status: UnitStatus,
}

impl UnitStatusReport {
    pub fn new(unit: ComponentUnit, status: UnitStatus) -> Self {
        Self { unit, status }
    }

    pub fn unit(&self) -> &ComponentUnit {
        &self.unit
    }

    pub fn status(&self) -> &UnitStatus {
        &self.status
    }
}

#[derive(Debug, Error)]
pub enum UnitFailure {
    #[error("{action} systemd unit {unit}: {source}")]
    Command {
        action: UnitAction,
        unit: UnitName,
        source: std::io::Error,
    },

    #[error("systemd rejected {action} for {unit}: status={status:?}, stderr={stderr}")]
    CommandRejected {
        action: UnitAction,
        unit: UnitName,
        status: Option<i32>,
        stderr: String,
    },

    #[error("{action} systemd D-Bus unit {unit}: {source}")]
    Bus {
        action: UnitAction,
        unit: UnitName,
        source: zbus::Error,
    },

    #[error("read systemd D-Bus status for {unit}: {source}")]
    Status { unit: UnitName, source: zbus::Error },

    #[error("read systemctl status for {unit}: {source}")]
    StatusCommand {
        unit: UnitName,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ManualUnitController;

impl UnitController for ManualUnitController {
    fn start<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move { Ok(UnitReceipt::started(unit)) })
    }

    fn stop<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move { Ok(UnitReceipt::stopped(unit)) })
    }

    fn restart<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move { Ok(UnitReceipt::restarted(unit)) })
    }

    fn status<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitStatusReport> {
        Box::pin(async move { Ok(UnitStatusReport::new(unit, UnitStatus::Active)) })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemdBus {
    System,
    User,
}

#[derive(Debug, Clone)]
pub struct SystemdUnitController {
    bus: SystemdBus,
}

impl SystemdUnitController {
    pub fn system() -> Self {
        Self {
            bus: SystemdBus::System,
        }
    }

    pub fn user() -> Self {
        Self {
            bus: SystemdBus::User,
        }
    }

    async fn connection(&self) -> std::result::Result<zbus::Connection, zbus::Error> {
        match self.bus {
            SystemdBus::System => zbus::Connection::system().await,
            SystemdBus::User => zbus::Connection::session().await,
        }
    }

    async fn call_unit_action(
        &self,
        unit: ComponentUnit,
        action: UnitAction,
    ) -> Result<UnitReceipt, UnitFailure> {
        let connection = self.connection().await.map_err(|source| UnitFailure::Bus {
            action,
            unit: unit.name().clone(),
            source,
        })?;
        let proxy = zbus::Proxy::new(
            &connection,
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
        )
        .await
        .map_err(|source| UnitFailure::Bus {
            action,
            unit: unit.name().clone(),
            source,
        })?;
        let _job: OwnedObjectPath = proxy
            .call(
                systemd_manager_method(action),
                &(unit.name().as_str(), "replace"),
            )
            .await
            .map_err(|source| UnitFailure::Bus {
                action,
                unit: unit.name().clone(),
                source,
            })?;
        Ok(UnitReceipt::from_action(unit, action))
    }
}

impl Default for SystemdUnitController {
    fn default() -> Self {
        Self::system()
    }
}

impl UnitController for SystemdUnitController {
    fn start<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move { self.call_unit_action(unit, UnitAction::Start).await })
    }

    fn stop<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move { self.call_unit_action(unit, UnitAction::Stop).await })
    }

    fn restart<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move { self.call_unit_action(unit, UnitAction::Restart).await })
    }

    fn status<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitStatusReport> {
        Box::pin(async move {
            let connection = self
                .connection()
                .await
                .map_err(|source| UnitFailure::Status {
                    unit: unit.name().clone(),
                    source,
                })?;
            let manager = zbus::Proxy::new(
                &connection,
                "org.freedesktop.systemd1",
                "/org/freedesktop/systemd1",
                "org.freedesktop.systemd1.Manager",
            )
            .await
            .map_err(|source| UnitFailure::Status {
                unit: unit.name().clone(),
                source,
            })?;
            let path: OwnedObjectPath = manager
                .call("GetUnit", &(unit.name().as_str()))
                .await
                .map_err(|source| UnitFailure::Status {
                    unit: unit.name().clone(),
                    source,
                })?;
            let unit_proxy = zbus::Proxy::new(
                &connection,
                "org.freedesktop.systemd1",
                path,
                "org.freedesktop.systemd1.Unit",
            )
            .await
            .map_err(|source| UnitFailure::Status {
                unit: unit.name().clone(),
                source,
            })?;
            let active_state: String =
                unit_proxy
                    .get_property("ActiveState")
                    .await
                    .map_err(|source| UnitFailure::Status {
                        unit: unit.name().clone(),
                        source,
                    })?;
            Ok(UnitStatusReport::new(
                unit,
                UnitStatus::from_systemd_active_state(active_state),
            ))
        })
    }
}

#[derive(Debug, Clone)]
pub struct SystemctlUnitController {
    systemctl: PathBuf,
}

impl SystemctlUnitController {
    pub fn new(systemctl: impl Into<PathBuf>) -> Self {
        Self {
            systemctl: systemctl.into(),
        }
    }

    async fn run_unit_action(
        systemctl: PathBuf,
        unit: ComponentUnit,
        action: UnitAction,
    ) -> Result<UnitReceipt, UnitFailure> {
        let output = Command::new(&systemctl)
            .arg(action.systemctl_verb())
            .arg(unit.name().as_str())
            .output()
            .await
            .map_err(|source| UnitFailure::Command {
                action,
                unit: unit.name().clone(),
                source,
            })?;
        if output.status.success() {
            Ok(UnitReceipt::from_action(unit, action))
        } else {
            Err(UnitFailure::CommandRejected {
                action,
                unit: unit.name().clone(),
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }
}

impl Default for SystemctlUnitController {
    fn default() -> Self {
        Self::new("systemctl")
    }
}

impl UnitController for SystemctlUnitController {
    fn start<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        let systemctl = self.systemctl.clone();
        Box::pin(async move { Self::run_unit_action(systemctl, unit, UnitAction::Start).await })
    }

    fn stop<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        let systemctl = self.systemctl.clone();
        Box::pin(async move { Self::run_unit_action(systemctl, unit, UnitAction::Stop).await })
    }

    fn restart<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        let systemctl = self.systemctl.clone();
        Box::pin(async move { Self::run_unit_action(systemctl, unit, UnitAction::Restart).await })
    }

    fn status<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitStatusReport> {
        let systemctl = self.systemctl.clone();
        Box::pin(async move {
            let output = Command::new(&systemctl)
                .arg("is-active")
                .arg(unit.name().as_str())
                .output()
                .await
                .map_err(|source| UnitFailure::StatusCommand {
                    unit: unit.name().clone(),
                    source,
                })?;
            Ok(UnitStatusReport::new(
                unit,
                UnitStatus::from_systemctl_output(&output.stdout),
            ))
        })
    }
}

#[derive(Debug, Clone)]
pub struct StartUnit {
    unit: ComponentUnit,
}

impl StartUnit {
    pub fn new(unit: ComponentUnit) -> Self {
        Self { unit }
    }
}

impl Message<StartUnit> for ComponentUnitManager {
    type Reply = Result<UnitReceipt, UnitFailure>;

    async fn handle(
        &mut self,
        message: StartUnit,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.controller.start(message.unit).await
    }
}

#[derive(Debug, Clone)]
pub struct StopUnit {
    unit: ComponentUnit,
}

impl StopUnit {
    pub fn new(unit: ComponentUnit) -> Self {
        Self { unit }
    }
}

impl Message<StopUnit> for ComponentUnitManager {
    type Reply = Result<UnitReceipt, UnitFailure>;

    async fn handle(
        &mut self,
        message: StopUnit,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.controller.stop(message.unit).await
    }
}

#[derive(Debug, Clone)]
pub struct RestartUnit {
    unit: ComponentUnit,
}

impl RestartUnit {
    pub fn new(unit: ComponentUnit) -> Self {
        Self { unit }
    }
}

impl Message<RestartUnit> for ComponentUnitManager {
    type Reply = Result<UnitReceipt, UnitFailure>;

    async fn handle(
        &mut self,
        message: RestartUnit,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.controller.restart(message.unit).await
    }
}

#[derive(Debug, Clone)]
pub struct ReadUnitStatus {
    unit: ComponentUnit,
}

impl ReadUnitStatus {
    pub fn new(unit: ComponentUnit) -> Self {
        Self { unit }
    }
}

impl Message<ReadUnitStatus> for ComponentUnitManager {
    type Reply = Result<UnitStatusReport, UnitFailure>;

    async fn handle(
        &mut self,
        message: ReadUnitStatus,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.controller.status(message.unit).await
    }
}

fn systemd_manager_method(action: UnitAction) -> &'static str {
    match action {
        UnitAction::Start => "StartUnit",
        UnitAction::Stop => "StopUnit",
        UnitAction::Restart => "RestartUnit",
    }
}
