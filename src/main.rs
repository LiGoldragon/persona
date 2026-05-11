use std::process::ExitCode;

use persona::error::Error;
use persona::manager::{EngineManager, HandleEngineRequest};
use persona::request::{CommandLine, PersonaOutput};

#[tokio::main]
async fn main() -> ExitCode {
    let request = match CommandLine::from_env().decode_request() {
        Ok(request) => request,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    let engine_request = request.into_engine_request();

    let manager = EngineManager::start().await;
    let reply = match manager
        .ask(HandleEngineRequest::new(engine_request))
        .await
        .map_err(|error| Error::actor("handle engine request", error))
    {
        Ok(reply) => reply,
        Err(error) => {
            eprintln!("error: {error}");
            let _ = EngineManager::stop(manager).await;
            return ExitCode::from(2);
        }
    };
    let output = match PersonaOutput::from_engine_reply(reply).to_nota() {
        Ok(output) => output,
        Err(error) => {
            eprintln!("error: {error}");
            let _ = EngineManager::stop(manager).await;
            return ExitCode::from(2);
        }
    };
    if let Err(error) = EngineManager::stop(manager).await {
        eprintln!("error: {error}");
        return ExitCode::from(2);
    }

    println!("{output}");
    ExitCode::SUCCESS
}
