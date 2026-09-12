use std::process::ExitCode;

use persona::request::{CommandLine, PersonaOutput};
use persona::transport::PersonaClient;

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

    let reply = match PersonaClient::from_environment()
        .submit(engine_request)
        .await
    {
        Ok(reply) => reply,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    let output = PersonaOutput::from_engine_reply(reply).textualize();

    println!("{output}");
    ExitCode::SUCCESS
}
