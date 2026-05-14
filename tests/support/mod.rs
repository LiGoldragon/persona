use std::path::{Path, PathBuf};

pub fn component_socket_fixture(_root: &Path) -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_persona-component-fixture"))
}
