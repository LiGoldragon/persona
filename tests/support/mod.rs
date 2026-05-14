use std::path::{Path, PathBuf};
use std::process::Command;

pub fn component_socket_fixture(root: &Path) -> PathBuf {
    let source = root.join("component-socket-fixture.rs");
    let binary = root.join("component-socket-fixture");
    std::fs::write(&source, component_socket_fixture_source())
        .expect("component socket fixture source written");
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let status = Command::new(rustc)
        .arg("--edition=2021")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .status()
        .expect("rustc runs for component socket fixture");
    assert!(
        status.success(),
        "component socket fixture failed to compile"
    );
    binary
}

fn component_socket_fixture_source() -> &'static str {
    r#"
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::thread;
use std::time::Duration;

fn main() {
    let state_path = env::var("PERSONA_STATE_PATH").expect("state path");
    let state_dir = Path::new(&state_path).parent().expect("state path parent");
    fs::create_dir_all(state_dir).expect("state dir created");

    let socket_path = env::var("PERSONA_SOCKET_PATH").expect("socket path");
    let _ = fs::remove_file(&socket_path);
    let _listener = UnixListener::bind(&socket_path).expect("component socket bound");
    let mode_text = env::var("PERSONA_SOCKET_MODE").expect("socket mode");
    let mode = u32::from_str_radix(&mode_text, 8).expect("octal socket mode");
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(mode))
        .expect("socket mode applied");

    let component = env::var("PERSONA_COMPONENT").expect("component");
    let capture = state_dir.join(format!("{component}.env"));
    let text = format!(
        "engine={}\ncomponent={}\nprocess={}\nsocket={}\nspawn_envelope={}\nmanager_socket={}\nmode={}\npeer_count={}\n",
        env::var("PERSONA_ENGINE_ID").expect("engine"),
        component,
        std::process::id(),
        socket_path,
        env::var("PERSONA_SPAWN_ENVELOPE").expect("spawn envelope"),
        env::var("PERSONA_MANAGER_SOCKET").expect("manager socket"),
        mode_text,
        env::var("PERSONA_PEER_SOCKET_COUNT").expect("peer count"),
    );
    fs::write(capture, text).expect("component capture written");

    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
"#
}
