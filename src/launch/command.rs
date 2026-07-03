use std::path::Path;

use nota::{NotaDecode, NotaEncode};

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ExecutablePath(String);

impl ExecutablePath {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    pub fn as_path(&self) -> &Path {
        Path::new(self.0.as_str())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct CommandArgument(String);

impl CommandArgument {
    pub fn new(argument: impl Into<String>) -> Self {
        Self(argument.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentVariableName(String);

impl EnvironmentVariableName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentVariableValue(String);

impl EnvironmentVariableValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentVariable {
    name: EnvironmentVariableName,
    value: EnvironmentVariableValue,
}

impl EnvironmentVariable {
    pub fn from_input(input: EnvironmentVariableInput) -> Self {
        Self {
            name: input.name,
            value: input.value,
        }
    }

    pub fn name(&self) -> &EnvironmentVariableName {
        &self.name
    }

    pub fn value(&self) -> &EnvironmentVariableValue {
        &self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentVariableInput {
    pub name: EnvironmentVariableName,
    pub value: EnvironmentVariableValue,
}

#[derive(NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq)]
pub struct ComponentCommand {
    executable_path: ExecutablePath,
    arguments: Vec<CommandArgument>,
    environment: Vec<EnvironmentVariable>,
}

impl ComponentCommand {
    pub fn executable(executable_path: ExecutablePath) -> Self {
        Self {
            executable_path,
            arguments: Vec::new(),
            environment: Vec::new(),
        }
    }

    pub fn from_input(input: ComponentCommandInput) -> Self {
        Self {
            executable_path: input.executable_path,
            arguments: input.arguments,
            environment: input.environment,
        }
    }

    pub fn executable_path(&self) -> &ExecutablePath {
        &self.executable_path
    }

    pub fn arguments(&self) -> &[CommandArgument] {
        self.arguments.as_slice()
    }

    pub fn environment(&self) -> &[EnvironmentVariable] {
        self.environment.as_slice()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentCommandInput {
    pub executable_path: ExecutablePath,
    pub arguments: Vec<CommandArgument>,
    pub environment: Vec<EnvironmentVariable>,
}
