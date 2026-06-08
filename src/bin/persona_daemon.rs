use persona::PersonaDaemon;
use persona::schema::daemon::DaemonEntry;

fn main() -> std::process::ExitCode {
    <PersonaDaemon as DaemonEntry>::run_to_exit_code()
}
