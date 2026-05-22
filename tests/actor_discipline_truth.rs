//! Architectural-truth witnesses for persona's actor discipline.
//!
//! - Public actor nouns are data-bearing — `mem::size_of::<X>() > 0`.
//! - No shared `Arc<Mutex<_>>` / `Arc<RwLock<_>>` between actors
//!   (per `~/primary/skills/actor-systems.md` §"No shared locks").
//!
//! The scan covers `src/**` except `DirectProcessLauncher`'s
//! `StopHandoff` line in `src/direct_process.rs`. That handoff is
//! a single-actor coordination primitive — the launcher owns the
//! mutex and a watcher task takes a `oneshot::Sender` out of it
//! after the child exits. It is not shared lock state between
//! actors. The doc comment on the `StopHandoff` type explicitly
//! names the constraint it satisfies (no Arc-Mutex-as-ownership
//! between two actors); the witness preserves that documented
//! carve-out.

use std::fs;
use std::path::{Path, PathBuf};

use persona::direct_process::DirectProcessLauncher;
use persona::launch::ComponentCommandResolver;
use persona::manager::EngineManager;
use persona::manager_store::ManagerStore;
use persona::readiness::ComponentSocketReadiness;
use persona::supervision_readiness::ComponentSupervisionReadiness;
use persona::supervisor::EngineSupervisor;
use persona::unit::ComponentUnitManager;

#[test]
fn public_actor_nouns_carry_data() {
    assert!(std::mem::size_of::<EngineManager>() > 0);
    assert!(std::mem::size_of::<ManagerStore>() > 0);
    assert!(std::mem::size_of::<EngineSupervisor>() > 0);
    assert!(std::mem::size_of::<DirectProcessLauncher>() > 0);
    assert!(std::mem::size_of::<ComponentCommandResolver>() > 0);
    assert!(std::mem::size_of::<ComponentSocketReadiness>() > 0);
    assert!(std::mem::size_of::<ComponentSupervisionReadiness>() > 0);
    assert!(std::mem::size_of::<ComponentUnitManager>() > 0);
}

#[test]
fn actor_source_does_not_share_locks_between_actors() {
    let forbidden = [
        ("Arc<Mutex", "shared mutex state between actors"),
        ("Arc < Mutex", "shared mutex state between actors"),
        ("RwLock", "shared read-write lock state between actors"),
    ];

    let mut violations: Vec<String> = Vec::new();
    for path in production_source_files() {
        let text = fs::read_to_string(&path).expect("read source file");
        let is_direct_process_source =
            path.file_name().and_then(|name| name.to_str()) == Some("direct_process.rs");
        for (fragment, reason) in forbidden {
            for (index, line) in text.lines().enumerate() {
                if !line.contains(fragment) {
                    continue;
                }
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                // The `StopHandoff` line in `direct_process.rs`
                // is a single-actor coordination primitive — the
                // launcher owns the mutex; the watcher task
                // takes the oneshot out after the child exits.
                // The doc comment immediately above it names
                // the constraint.
                if is_direct_process_source && line.contains("type StopHandoff") {
                    continue;
                }
                violations.push(format!(
                    "{}:{}: {reason} ({line})",
                    path.display(),
                    index + 1,
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "shared-lock violations in actor source:\n{}",
        violations.join("\n"),
    );
}

fn production_source_files() -> Vec<PathBuf> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = crate_root.join("src");
    let mut output = Vec::new();
    collect_rust_files(&src, &mut output);
    output
}

fn collect_rust_files(directory: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
}
