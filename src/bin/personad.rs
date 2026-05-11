use std::process::ExitCode;

use persona::transport::PersonaDaemonCommand;

#[tokio::main]
async fn main() -> ExitCode {
    match PersonaDaemonCommand::from_environment().run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}
