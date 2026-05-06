use std::process::ExitCode;

use persona::request::CommandLine;

fn main() -> ExitCode {
    let output = match CommandLine::from_env()
        .decode_request()
        .map(|request| request.into_output())
        .and_then(|output| output.to_nota())
    {
        Ok(output) => output,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    println!("{output}");
    ExitCode::SUCCESS
}
