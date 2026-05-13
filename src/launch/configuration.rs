use nota_codec::NotaRecord;
use thiserror::Error;

use crate::engine::EngineComponent;

use super::command::{ComponentCommand, ExecutablePath};

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentCommandEntry {
    component: EngineComponent,
    command: ComponentCommand,
}

impl ComponentCommandEntry {
    pub fn from_input(input: ComponentCommandEntryInput) -> Self {
        Self {
            component: input.component,
            command: input.command,
        }
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn command(&self) -> &ComponentCommand {
        &self.command
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentCommandEntryInput {
    pub component: EngineComponent,
    pub command: ComponentCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentCommandCatalog {
    entries: Vec<ComponentCommandEntry>,
}

impl ComponentCommandCatalog {
    pub fn from_entries(entries: Vec<ComponentCommandEntry>) -> Self {
        Self { entries }
    }

    pub fn from_repeated_executable(executable_path: impl Into<String>) -> Self {
        let executable_path = executable_path.into();
        Self::from_entries(
            EngineComponent::prototype_supervised_components()
                .into_iter()
                .map(|component| {
                    ComponentCommandEntry::from_input(ComponentCommandEntryInput {
                        component,
                        command: ComponentCommand::executable(ExecutablePath::new(
                            executable_path.clone(),
                        )),
                    })
                })
                .collect(),
        )
    }

    pub fn from_environment() -> std::result::Result<Option<Self>, CommandResolutionFailure> {
        if let Some(executable_path) = std::env::var_os("PERSONA_PROTOTYPE_STACK_EXECUTABLE") {
            return Ok(Some(Self::from_repeated_executable(
                executable_path.to_string_lossy().into_owned(),
            )));
        }

        let mut entries = Vec::new();
        let mut saw_environment = false;
        for component in EngineComponent::prototype_supervised_components() {
            match std::env::var_os(component.executable_environment_variable()) {
                Some(path) => {
                    saw_environment = true;
                    entries.push(ComponentCommandEntry::from_input(
                        ComponentCommandEntryInput {
                            component,
                            command: ComponentCommand::executable(ExecutablePath::new(
                                path.to_string_lossy().into_owned(),
                            )),
                        },
                    ));
                }
                None => {}
            }
        }

        if !saw_environment {
            return Ok(None);
        }

        let catalog = Self::from_entries(entries);
        for component in EngineComponent::prototype_supervised_components() {
            if catalog.command_for(component)?.is_none() {
                return Err(CommandResolutionFailure::MissingRequiredCommand { component });
            }
        }
        Ok(Some(catalog))
    }

    pub fn entries(&self) -> &[ComponentCommandEntry] {
        self.entries.as_slice()
    }

    pub fn command_for(
        &self,
        component: EngineComponent,
    ) -> std::result::Result<Option<ComponentCommand>, CommandResolutionFailure> {
        let mut matches = self
            .entries
            .iter()
            .filter(|entry| entry.component == component);
        let Some(first) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err(CommandResolutionFailure::DuplicateDefaultCommand { component });
        }
        Ok(Some(first.command.clone()))
    }
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct EngineLaunchConfiguration {
    overrides: Vec<ComponentCommandOverride>,
}

impl EngineLaunchConfiguration {
    pub fn empty() -> Self {
        Self {
            overrides: Vec::new(),
        }
    }

    pub fn from_overrides(overrides: Vec<ComponentCommandOverride>) -> Self {
        Self { overrides }
    }

    pub fn overrides(&self) -> &[ComponentCommandOverride] {
        self.overrides.as_slice()
    }

    pub fn command_override_for(
        &self,
        component: EngineComponent,
    ) -> std::result::Result<Option<ComponentCommand>, CommandResolutionFailure> {
        let mut matches = self
            .overrides
            .iter()
            .filter(|entry| entry.component == component);
        let Some(first) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err(CommandResolutionFailure::DuplicateOverrideCommand { component });
        }
        Ok(Some(first.command.clone()))
    }
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ComponentCommandOverride {
    component: EngineComponent,
    command: ComponentCommand,
}

impl ComponentCommandOverride {
    pub fn from_input(input: ComponentCommandOverrideInput) -> Self {
        Self {
            component: input.component,
            command: input.command,
        }
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn command(&self) -> &ComponentCommand {
        &self.command
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentCommandOverrideInput {
    pub component: EngineComponent,
    pub command: ComponentCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedComponentCommands {
    entries: Vec<ResolvedComponentCommand>,
}

impl ResolvedComponentCommands {
    pub fn from_entries(entries: Vec<ResolvedComponentCommand>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[ResolvedComponentCommand] {
        self.entries.as_slice()
    }

    pub fn command_for(&self, component: EngineComponent) -> Option<&ComponentCommand> {
        self.entries
            .iter()
            .find(|entry| entry.component == component)
            .map(|entry| &entry.command)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedComponentCommand {
    component: EngineComponent,
    command: ComponentCommand,
}

impl ResolvedComponentCommand {
    pub fn from_input(input: ResolvedComponentCommandInput) -> Self {
        Self {
            component: input.component,
            command: input.command,
        }
    }

    pub fn component(&self) -> EngineComponent {
        self.component
    }

    pub fn command(&self) -> &ComponentCommand {
        &self.command
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedComponentCommandInput {
    pub component: EngineComponent,
    pub command: ComponentCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandResolutionFailure {
    #[error("missing command for required component {component:?}")]
    MissingRequiredCommand { component: EngineComponent },

    #[error("duplicate default command for component {component:?}")]
    DuplicateDefaultCommand { component: EngineComponent },

    #[error("duplicate override command for component {component:?}")]
    DuplicateOverrideCommand { component: EngineComponent },
}
