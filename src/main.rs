use std::process::ExitCode;

use persona::actor::PersonaActorRuntime;
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

    let runtime = PersonaActorRuntime::start();
    let output = match runtime
        .handle(request)
        .await
        .and_then(|output| output.to_nota())
    {
        Ok(output) => output,
        Err(error) => {
            eprintln!("error: {error}");
            let _ = runtime.stop().await;
            return ExitCode::from(2);
        }
    };
    if let Err(error) = runtime.stop().await {
        eprintln!("error: {error}");
        return ExitCode::from(2);
    }

    println!("{output}");
    ExitCode::SUCCESS
}
