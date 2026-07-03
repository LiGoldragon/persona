use std::{env, path::PathBuf};

use schema_rust::{
    NexusDaemonShape, WorkingListenerTier,
    build::{GenerationDriver, GenerationPlan, ModuleEmission},
};

fn main() {
    SchemaBuild::from_environment().run();
}

struct SchemaBuild {
    crate_root: PathBuf,
}

impl SchemaBuild {
    fn from_environment() -> Self {
        Self {
            crate_root: PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir set")),
        }
    }

    fn run(&self) {
        println!("cargo:rerun-if-changed=schema/daemon.schema");
        println!("cargo:rerun-if-changed=src/schema/daemon.rs");

        let plan = GenerationPlan::new(&self.crate_root, "persona", "0.2.0").with_module(
            ModuleEmission::daemon_module("daemon", Self::daemon_shape()),
        );
        GenerationDriver::new(plan)
            .generate()
            .expect("generate persona schema artifacts")
            .write_or_check("PERSONA_UPDATE_SCHEMA_ARTIFACTS")
            .expect("checked-in persona schema artifacts are fresh");
    }

    /// Persona's ordinary manager socket is the peer-callable working listener,
    /// decoded by the component: persona speaks its own `meta-signal-persona`
    /// length-prefixed `Frame` wire (a relation contract, not a schema-derived
    /// `Input`/`Output` root), driving the in-process kameo `EngineManager`.
    /// The generated daemon owns argv, socket binding, async accept, request
    /// gating, lifecycle, and exit; the component owns only the per-connection
    /// frame decode/encode through `handle_working_connection`.
    fn daemon_shape() -> NexusDaemonShape {
        NexusDaemonShape::new("persona-daemon", WorkingListenerTier::component_decoded())
    }
}
