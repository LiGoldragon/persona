use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

struct MetaTestingFixture {
    root: PathBuf,
}

impl MetaTestingFixture {
    fn new() -> Self {
        Self {
            root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        }
    }

    fn architecture(&self) -> String {
        self.read_configured_file("PERSONA_ARCHITECTURE_DOCUMENT_PATH", "ARCHITECTURE.md")
    }

    fn tests_document(&self) -> String {
        self.read_configured_file("PERSONA_TESTS_DOCUMENT_PATH", "TESTS.md")
    }

    fn flake(&self) -> String {
        self.read_configured_file("PERSONA_FLAKE_DOCUMENT_PATH", "flake.nix")
    }

    fn read_configured_file(&self, environment_variable: &str, relative: &str) -> String {
        let path = std::env::var_os(environment_variable)
            .map(PathBuf::from)
            .unwrap_or_else(|| self.root.join(relative));
        fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {relative} at {}: {error}", path.display()))
    }

    fn rust_test_files(&self) -> Vec<PathBuf> {
        let tests = self.root.join("tests");
        let mut files: Vec<_> = fs::read_dir(tests)
            .expect("read tests directory")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
            .collect();
        files.sort();
        files
    }
}

#[test]
fn constraint_architecture_named_nix_witnesses_exist_in_flake() {
    let fixture = MetaTestingFixture::new();
    let flake = fixture.flake();
    let documents = [fixture.architecture(), fixture.tests_document()];
    let referenced_checks = documents
        .iter()
        .flat_map(|document| NixCheckReference::collect(document))
        .collect::<BTreeSet<_>>();

    assert!(
        !referenced_checks.is_empty(),
        "architecture/test docs should name at least one Nix witness"
    );

    let missing = referenced_checks
        .iter()
        .filter(|check| !check.exists_in_flake(&flake))
        .map(NixCheckReference::as_str)
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "documented Nix witnesses missing from flake checks:\n{}",
        missing.join("\n")
    );
}

#[test]
fn constraint_test_documents_publish_valid_nix_review_surfaces() {
    let fixture = MetaTestingFixture::new();
    let documents = [
        ("ARCHITECTURE.md", fixture.architecture()),
        ("TESTS.md", fixture.tests_document()),
    ];
    let violations = documents
        .iter()
        .flat_map(|(name, document)| {
            document
                .lines()
                .enumerate()
                .filter(|(_index, line)| {
                    line.contains("cargo test --") || line.contains("nix flake check .#")
                })
                .map(|(index, line)| format!("{name}:{}: {line}", index + 1))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "Persona test docs must name valid Nix check builds, not bare cargo commands or invalid named flake-check commands:\n{}",
        violations.join("\n")
    );
}

#[test]
fn constraint_multi_thread_actor_tests_name_their_threaded_actor_need() {
    let fixture = MetaTestingFixture::new();
    let mut violations = Vec::new();

    for path in fixture.rust_test_files() {
        let text = fs::read_to_string(&path).expect("read test file");
        let file_uses_threaded_store_actor = text.contains("ManagerStore::start");
        for (index, line) in text.lines().enumerate() {
            if line.contains("#[tokio::test(flavor = \"multi_thread\"")
                && !file_uses_threaded_store_actor
            {
                violations.push(format!(
                    "{}:{}: use #[tokio::test] unless the file exercises ManagerStore::start, which uses Kameo spawn_in_thread on 0.20",
                    path.display(),
                    index + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Persona actor/process tests should default to Tokio's single-thread test runtime unless they exercise the threaded store actor path:\n{}",
        violations.join("\n")
    );
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct NixCheckReference {
    name: String,
}

impl NixCheckReference {
    fn collect(document: &str) -> Vec<Self> {
        let marker = "nix build .#checks.";
        let mut references = Vec::new();
        let mut remaining = document;
        while let Some(position) = remaining.find(marker) {
            let after_marker = &remaining[position + marker.len()..];
            let Some((_system, after_system)) = after_marker.split_once('.') else {
                remaining = after_marker;
                continue;
            };
            let name: String = after_system
                .chars()
                .take_while(|character| {
                    character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
                })
                .collect();
            if !name.is_empty() {
                references.push(Self { name });
            }
            remaining = after_system;
        }
        references
    }

    fn exists_in_flake(&self, flake: &str) -> bool {
        flake.contains(&format!("{} =", self.name))
    }

    fn as_str(&self) -> &str {
        &self.name
    }
}
