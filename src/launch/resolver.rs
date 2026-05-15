use kameo::actor::{Actor, ActorRef};
use kameo::error::Infallible;
use kameo::message::{Context, Message};

use super::configuration::{
    CommandResolutionFailure, ComponentCommandCatalog, EngineLaunchConfiguration,
    ResolvedComponentCommand, ResolvedComponentCommandInput, ResolvedComponentCommands,
};

#[derive(Debug)]
pub struct ComponentCommandResolver {
    defaults: ComponentCommandCatalog,
    resolution_count: u64,
}

impl ComponentCommandResolver {
    pub fn new(defaults: ComponentCommandCatalog) -> Self {
        Self {
            defaults,
            resolution_count: 0,
        }
    }

    fn resolve(
        &mut self,
        configuration: EngineLaunchConfiguration,
    ) -> std::result::Result<ResolvedComponentCommands, CommandResolutionFailure> {
        self.resolution_count += 1;
        let mut entries = Vec::new();
        for component in self.defaults.required_components().iter().copied() {
            let command = match configuration.command_override_for(component)? {
                Some(command) => command,
                None => self
                    .defaults
                    .command_for(component)?
                    .ok_or(CommandResolutionFailure::MissingRequiredCommand { component })?,
            };
            entries.push(ResolvedComponentCommand::from_input(
                ResolvedComponentCommandInput { component, command },
            ));
        }
        Ok(ResolvedComponentCommands::from_entries(entries))
    }

    pub fn resolution_count(&self) -> u64 {
        self.resolution_count
    }
}

impl Actor for ComponentCommandResolver {
    type Args = Self;
    type Error = Infallible;

    async fn on_start(
        actor: Self::Args,
        _actor_reference: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(actor)
    }
}

#[derive(Debug)]
pub struct ResolveComponentCommands {
    configuration: EngineLaunchConfiguration,
}

impl ResolveComponentCommands {
    pub fn new(configuration: EngineLaunchConfiguration) -> Self {
        Self { configuration }
    }
}

impl Message<ResolveComponentCommands> for ComponentCommandResolver {
    type Reply = std::result::Result<ResolvedComponentCommands, CommandResolutionFailure>;

    async fn handle(
        &mut self,
        message: ResolveComponentCommands,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.resolve(message.configuration)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadCommandResolutionAttemptCount;

impl Message<ReadCommandResolutionAttemptCount> for ComponentCommandResolver {
    type Reply = u64;

    async fn handle(
        &mut self,
        _message: ReadCommandResolutionAttemptCount,
        _context: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.resolution_count()
    }
}
