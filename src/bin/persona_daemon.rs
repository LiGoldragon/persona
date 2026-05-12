use std::process::ExitCode;

use persona::transport::PersonaDaemonCommand;

#[tokio::main]
async fn main() -> ExitCode {
    let command = match PersonaDaemonCommand::from_environment() {
        Ok(command) => command,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    match command.run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}
