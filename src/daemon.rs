//! Persona's daemon hooks — the only daemon code persona hand-writes.
//!
//! The uniform daemon skeleton (argv parsing, async task-backed listener
//! binding, the accept loop, request gating, peer credentials, lifecycle, and
//! the `ExitReport` entry) is emitted into `src/schema/daemon.rs` by
//! schema-rust's daemon emitter from the [`NexusDaemonShape`] in
//! `build.rs`. Persona's ordinary manager socket is a component-decoded working
//! listener: persona speaks its own `meta-signal-persona` length-prefixed
//! `Frame` wire (a relation contract, not a schema-derived `Input`/`Output`
//! root), so the component owns only the per-connection frame decode/encode in
//! [`PersonaDaemon::handle_working_connection`].
//!
//! The engine is the live kameo actor system — [`EngineManager`] over a
//! [`ManagerStore`], optionally fronted by an [`EngineSupervisor`] that spawns
//! and supervises the prototype topology. The generated runtime owns one
//! [`PersonaEngine`] and shares it `&` across connections; the manager actor's
//! mailbox serialises every request, so no component-internal lock is required.

use std::sync::Arc;

use tokio::runtime::Handle;
use triad_runtime::AcceptedConnection;

use signal_persona::EngineIdentifier;

use crate::configuration::{ConfigurationError, PersonaDaemonConfiguration};
use crate::error::{Error, Result};
use crate::launch::EngineLaunchConfiguration;
use crate::manager::{EngineManager, HandleEngineRequest};
use crate::manager_store::{ManagerStore, ManagerStoreLocation};
use crate::schema::daemon::ComponentDaemon;
use crate::supervisor::{EngineSupervisor, EngineSupervisorInput, StartPrototypeSupervision};
use crate::transport::{PersonaEndpoint, PersonaFrameCodec, PersonaLaunchPlan};
use crate::unit::{ManualUnitController, UnitController};

/// The type-level selector for persona's emitted daemon. It carries no runtime
/// data — it is the marker the emitted `DaemonCommand<PersonaDaemon>` and the
/// generated runtime dispatch on, selecting persona's `Configuration` /
/// `Engine` / `Error` types through the `ComponentDaemon` associated types.
#[derive(Debug)]
pub struct PersonaDaemon;

/// The live engine the generated runtime owns: the manager actor reference the
/// working connections drive, the frame codec, and the supervisor whose
/// lifetime keeps the supervised topology running for as long as the daemon
/// serves.
pub struct PersonaEngine {
    manager: kameo::actor::ActorRef<EngineManager>,
    codec: PersonaFrameCodec,
    _supervisor: Option<kameo::actor::ActorRef<EngineSupervisor>>,
}

impl PersonaEngine {
    /// Decode one request `Frame` off the accepted stream, drive it through the
    /// manager actor, and write the reply `Frame` back.
    async fn serve_connection(&self, mut connection: AcceptedConnection) -> Result<()> {
        let frame = self.codec.read_frame(connection.stream_mut()).await?;
        let received = self.codec.request_from_frame(frame)?;
        let exchange = received.exchange();
        let reply = self
            .manager
            .ask(HandleEngineRequest::new(received.into_request()))
            .await
            .map_err(|error| Error::actor("handle daemon engine request", error))?;
        let reply_frame = self.codec.reply_frame(exchange, reply);
        self.codec
            .write_frame(connection.stream_mut(), &reply_frame)
            .await
    }
}

impl PersonaDaemon {
    /// Open the manager store, spawn the manager actor over it, and start the
    /// prototype supervisor when the launch plan resolves a component catalog.
    ///
    /// `build_runtime` is a synchronous hook the generated daemon calls from
    /// inside its own multi-thread tokio runtime, so the async actor startup is
    /// driven through `block_in_place` + the current runtime handle — blocking
    /// this worker thread without nesting a second runtime.
    fn open_engine(configuration: &PersonaDaemonConfiguration) -> Result<PersonaEngine> {
        let store_location = ManagerStoreLocation::new(configuration.manager_store_path());
        let store = ManagerStore::start(store_location)?;
        let unit_controller: Arc<dyn UnitController> = Arc::new(ManualUnitController);
        let endpoint = PersonaEndpoint::from_path(configuration.manager_socket_path());
        let launch_plan = PersonaLaunchPlan::from_environment(&endpoint)?;

        let handle = Handle::current();
        tokio::task::block_in_place(move || {
            handle.block_on(async move {
                let manager = EngineManager::start_with_store_and_unit_controller(
                    EngineIdentifier::new("default"),
                    store.clone(),
                    unit_controller,
                )
                .await?;
                let supervisor = match launch_plan {
                    Some(launch_plan) => Some(Self::start_supervisor(launch_plan, store).await?),
                    None => None,
                };
                Ok(PersonaEngine {
                    manager,
                    codec: PersonaFrameCodec::default(),
                    _supervisor: supervisor,
                })
            })
        })
    }

    async fn start_supervisor(
        launch_plan: PersonaLaunchPlan,
        store: kameo::actor::ActorRef<ManagerStore>,
    ) -> Result<kameo::actor::ActorRef<EngineSupervisor>> {
        let supervisor = EngineSupervisor::start(EngineSupervisorInput {
            layout: launch_plan.layout(),
            command_catalog: launch_plan.command_catalog(),
            launch_configuration: EngineLaunchConfiguration::empty(),
            store: Some(store),
        });
        match supervisor.ask(StartPrototypeSupervision).await {
            Ok(_) => Ok(supervisor),
            Err(kameo::error::SendError::HandlerError(error)) => Err(Error::engine_supervisor(
                "start prototype supervision",
                error,
            )),
            Err(error) => Err(Error::actor(
                "start prototype supervision supervisor",
                error,
            )),
        }
    }
}

impl ComponentDaemon for PersonaDaemon {
    type Configuration = PersonaDaemonConfiguration;
    type ConfigurationError = ConfigurationError;
    type Engine = PersonaEngine;
    type Error = Error;

    const PROCESS_NAME: &'static str = "persona-daemon";

    fn load_configuration(
        path: &std::path::Path,
    ) -> std::result::Result<Self::Configuration, Self::ConfigurationError> {
        PersonaDaemonConfiguration::from_signal_file(path)
    }

    fn build_runtime(
        configuration: &Self::Configuration,
    ) -> std::result::Result<Self::Engine, Self::Error> {
        Self::open_engine(configuration)
    }

    async fn handle_working_connection(
        engine: &Self::Engine,
        connection: AcceptedConnection,
    ) -> std::result::Result<(), Self::Error> {
        engine.serve_connection(connection).await
    }
}
