use std::process::ExitCode;

use persona::actor::{HandlePersonaRequest, PersonaRuntime};
use persona::error::Error;
use persona::request::CommandLine;

#[tokio::main]
async fn main() -> ExitCode {
    let request = match CommandLine::from_env().decode_request() {
        Ok(request) => request,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    let runtime = PersonaRuntime::start().await;
    let output = match runtime
        .ask(HandlePersonaRequest::new(request))
        .await
        .map_err(|error| Error::actor("handle persona request", error))
        .and_then(|output| output.to_nota())
    {
        Ok(output) => output,
        Err(error) => {
            eprintln!("error: {error}");
            let _ = PersonaRuntime::stop(runtime).await;
            return ExitCode::from(2);
        }
    };
    if let Err(error) = PersonaRuntime::stop(runtime).await {
        eprintln!("error: {error}");
        return ExitCode::from(2);
    }

    println!("{output}");
    ExitCode::SUCCESS
}
