use std::sync::Arc;

use persona::launch::{
    CommandArgument, ComponentCommand, ComponentCommandInput, EnvironmentVariable,
    EnvironmentVariableInput, EnvironmentVariableName, EnvironmentVariableValue, ExecutablePath,
};
use persona::unit::{
    ComponentUnit, ComponentUnitCatalog, ComponentUnitDefinition, ComponentUnitDefinitionInput,
    ComponentUnitManager, ReadUnitStatus, RestartUnit, StartUnit, StopUnit, UnitAction,
    UnitController, UnitFuture, UnitReceipt, UnitRestartPolicy, UnitStatus, UnitStatusReport,
};
use persona::upgrade::Version;
use signal_persona::ComponentName;
use signal_persona_origin::EngineIdentifier;

#[derive(Debug, Clone, Default)]
struct RecordingController {
    actions: Arc<std::sync::Mutex<Vec<UnitAction>>>,
}

impl RecordingController {
    fn actions(&self) -> Vec<UnitAction> {
        self.actions
            .lock()
            .expect("recording controller lock")
            .clone()
    }

    fn record(&self, action: UnitAction) {
        self.actions
            .lock()
            .expect("recording controller lock")
            .push(action);
    }
}

impl UnitController for RecordingController {
    fn start<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move {
            self.record(UnitAction::Start);
            Ok(UnitReceipt::started(unit))
        })
    }

    fn stop<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move {
            self.record(UnitAction::Stop);
            Ok(UnitReceipt::stopped(unit))
        })
    }

    fn restart<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move {
            self.record(UnitAction::Restart);
            Ok(UnitReceipt::restarted(unit))
        })
    }

    fn status<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitStatusReport> {
        Box::pin(async move { Ok(UnitStatusReport::new(unit, UnitStatus::Active)) })
    }
}

fn spirit_unit() -> ComponentUnit {
    ComponentUnit::new(
        EngineIdentifier::new("default"),
        ComponentName::new("persona-spirit"),
        Version::new("v0.1.1"),
    )
}

fn spirit_command() -> ComponentCommand {
    ComponentCommand::from_input(ComponentCommandInput {
        executable_path: ExecutablePath::new("/nix/store/persona-spirit/bin/persona-spirit-daemon"),
        arguments: vec![CommandArgument::new("/run/persona/spirit-v0.1.1.nota")],
        environment: vec![EnvironmentVariable::from_input(EnvironmentVariableInput {
            name: EnvironmentVariableName::new("PERSONA_ENGINE_ID"),
            value: EnvironmentVariableValue::new("default"),
        })],
    })
}

#[test]
fn constraint_component_unit_name_is_component_version_template_instance() {
    let unit = spirit_unit();

    assert_eq!(
        unit.name().as_str(),
        "persona-component@persona-spirit:v0.1.1.service"
    );
    assert_eq!(unit.engine().as_str(), "default");
    assert_eq!(unit.component().as_str(), "persona-spirit");
    assert_eq!(unit.version().as_str(), "v0.1.1");
}

#[tokio::test]
async fn constraint_component_unit_manager_dispatches_start_stop_restart_status() {
    let controller = RecordingController::default();
    let manager = ComponentUnitManager::start_with_controller(Arc::new(controller.clone()));
    let unit = spirit_unit();

    let started = manager
        .ask(StartUnit::new(unit.clone()))
        .await
        .expect("unit start dispatched");
    let stopped = manager
        .ask(StopUnit::new(unit.clone()))
        .await
        .expect("unit stop dispatched");
    let restarted = manager
        .ask(RestartUnit::new(unit.clone()))
        .await
        .expect("unit restart dispatched");
    let status = manager
        .ask(ReadUnitStatus::new(unit.clone()))
        .await
        .expect("unit status dispatched");

    assert_eq!(started.action(), UnitAction::Start);
    assert_eq!(stopped.action(), UnitAction::Stop);
    assert_eq!(restarted.action(), UnitAction::Restart);
    assert_eq!(status.status(), &UnitStatus::Active);
    assert_eq!(
        controller.actions(),
        vec![UnitAction::Start, UnitAction::Stop, UnitAction::Restart]
    );

    manager.stop_gracefully().await.expect("unit manager stops");
    let _outcome = manager.wait_for_shutdown().await;
}

#[test]
fn constraint_transient_unit_definition_projects_exec_start_and_environment() {
    let unit = spirit_unit();
    let definition = ComponentUnitDefinition::from_input(ComponentUnitDefinitionInput {
        unit: unit.clone(),
        command: spirit_command(),
        restart: UnitRestartPolicy::Disabled,
    });
    let properties = definition.transient_properties();

    assert_eq!(
        properties.description(),
        "Persona component persona-spirit v0.1.1 for engine default"
    );
    assert_eq!(properties.service_type(), "simple");
    assert_eq!(properties.restart(), "no");
    assert_eq!(
        properties.exec_start().path(),
        "/nix/store/persona-spirit/bin/persona-spirit-daemon"
    );
    assert_eq!(
        properties.exec_start().arguments(),
        &[
            "/nix/store/persona-spirit/bin/persona-spirit-daemon".to_string(),
            "/run/persona/spirit-v0.1.1.nota".to_string()
        ]
    );
    assert!(properties.exec_start().unclean_exit_fails());
    assert_eq!(
        properties.environment(),
        &["PERSONA_ENGINE_ID=default".to_string()]
    );

    let catalog = ComponentUnitCatalog::from_definitions(vec![definition]);
    assert_eq!(
        catalog.definition_for(&unit).map(|entry| entry.unit()),
        Some(&unit)
    );
}
