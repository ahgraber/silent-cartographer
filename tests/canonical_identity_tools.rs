//! Canonical identity over copies of the conformance projects, built through `run_build` with the real
//! indexers. Each test skips with a printed `SKIP` when its tool is absent.

mod support;

use std::collections::BTreeSet;
use std::path::Path;

use silent_cartographer::commands::{BuildOutcome, PythonExecutables, run_build};
use silent_cartographer::graph::store::GraphStore;
use silent_cartographer::graph::syntax::Language;
use silent_cartographer::identity::CanonicalId;
use silent_cartographer::semantic::python_adapter::PythonIndexer;
use silent_cartographer::semantic::rust_adapter::RustAdapter;

/// Whether `rust-analyzer` is installed; prints a `SKIP` line when it is not.
fn rust_analyzer_present() -> bool {
    let present = RustAdapter::new("rust-analyzer").is_ok();
    if !present {
        eprintln!("SKIP: rust-analyzer not installed");
    }
    present
}

/// A copy of the Rust conformance project in a fresh directory.
fn rust_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let fixture = support::rust_fixture_root();
    std::fs::copy(fixture.join("Cargo.toml"), dir.path().join("Cargo.toml")).unwrap();
    for rel in support::RUST_FIXTURE_FILES {
        let to = dir.path().join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixture.join(rel), to).unwrap();
    }
    dir
}

/// Write `text` to `rel` under `root`, creating its directory.
fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Build the Rust project at `root` into `root/index.db` through the real build path.
fn build_rust(root: &Path) -> BuildOutcome {
    run_build(
        &root.join("index.db"),
        Some("test-ws"),
        root,
        "rust-analyzer",
        &PythonExecutables::default(),
        None,
        None,
        Some(Language::Rust),
        false,
        false,
        &Default::default(),
        &|_| false,
        &silent_cartographer::graph::store::free_bytes,
    )
    .expect("the build succeeds")
}

/// Every in-workspace identity the store at `root/index.db` holds.
fn identities(root: &Path) -> BTreeSet<String> {
    let conn =
        rusqlite::Connection::open_with_flags(root.join("index.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut stmt = conn
        .prepare("SELECT canonical_id FROM symbols WHERE class = 'in_workspace'")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// Whether the store at `root/index.db` resolves `id` to a symbol.
fn resolves(root: &Path, id: &str) -> bool {
    let store = GraphStore::open(&root.join("index.db")).unwrap();
    store.symbol(&CanonicalId::from_raw(id)).unwrap().is_some()
}

// _(A renamed binary rebuilds)_ — a build through the real path records the target inventory and
// the manifest digest; an unchanged rebuild reports the index current; renaming the binary in
// `Cargo.toml` alone then drives a rebuild whose binary declarations carry `+bin.cli`.
// _(Rebuilding an unchanged workspace analyzes nothing)_ is the unchanged arm, on the real
// inventory. Python's unchanged arm is `an_unchanged_store_reads_current_on_rebuild_for_each_indexer` in
// `tests/operational.rs`.
#[test]
fn renamed_target_rebuilds() {
    if !rust_analyzer_present() {
        return;
    }
    let project = rust_project();
    let root = project.path();
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let declare = |name: &str| format!("{manifest}\n[[bin]]\nname = \"{name}\"\npath = \"src/bin/tool.rs\"\n");
    write(root, "Cargo.toml", &declare("tool"));

    assert!(build_rust(root).rebuilt, "the first build analyzes");
    assert!(
        identities(root).contains("test-ws::libbin+bin.tool::util::Config"),
        "the binary is owned as `tool`"
    );
    assert!(
        !build_rust(root).rebuilt,
        "an unchanged manifest and inventory read as current"
    );

    write(root, "Cargo.toml", &declare("cli"));
    assert!(build_rust(root).rebuilt, "a manifest-only rename rebuilds");
    let after = identities(root);
    assert!(after.contains("test-ws::libbin+bin.cli::util::Config"), "{after:?}");
    assert!(
        after.iter().all(|id| !id.contains("+bin.tool")),
        "no identity keeps the old target name: {after:?}"
    );
}

// _(Rust inline module moved to its own file)_ — `mod util { … }` becomes `mod util;` with the body in
// `src/util.rs`; every identity is unchanged.
#[test]
fn inline_module_moved_to_file_keeps_identity() {
    if !rust_analyzer_present() {
        return;
    }
    let project = rust_project();
    let root = project.path();
    build_rust(root);
    let before = identities(root);
    assert!(before.contains("test-ws::libbin::util::load"));

    write(root, "src/lib.rs", "pub mod util;\n");
    write(root, "src/util.rs", "pub struct Config;\npub fn load() {}\n");
    assert!(build_rust(root).rebuilt);

    assert_eq!(identities(root), before);
}

// _(Rust module file renamed to a directory module)_ — `src/util.rs` becomes `src/util/mod.rs`; every
// identity is unchanged.
#[test]
fn module_file_moved_to_directory_module_keeps_identity() {
    if !rust_analyzer_present() {
        return;
    }
    let project = rust_project();
    let root = project.path();
    write(root, "src/lib.rs", "pub mod util;\n");
    write(root, "src/util.rs", "pub struct Config;\npub fn load() {}\n");
    build_rust(root);
    let before = identities(root);
    assert!(before.contains("test-ws::libbin::util::load"));

    std::fs::remove_file(root.join("src/util.rs")).unwrap();
    write(root, "src/util/mod.rs", "pub struct Config;\npub fn load() {}\n");
    assert!(build_rust(root).rebuilt);

    assert_eq!(identities(root), before);
}

// _(A module rename changes identity)_ — `util` becomes `helpers` in the library: its declarations end
// in `helpers`, and the prior identities resolve to no symbol.
#[test]
fn module_rename_changes_identity() {
    if !rust_analyzer_present() {
        return;
    }
    let project = rust_project();
    let root = project.path();
    build_rust(root);
    assert!(resolves(root, "test-ws::libbin::util::load"));

    write(
        root,
        "src/lib.rs",
        "pub mod helpers {\n    pub struct Config;\n    pub fn load() {}\n}\n",
    );
    assert!(build_rust(root).rebuilt);

    let after = identities(root);
    for renamed in ["helpers", "helpers::Config", "helpers::load"] {
        assert!(
            after.contains(&format!("test-ws::libbin::{renamed}")),
            "{renamed}: {after:?}"
        );
    }
    for prior in ["util", "util::Config", "util::load"] {
        assert!(!resolves(root, &format!("test-ws::libbin::{prior}")), "{prior}");
    }
}

// _(Rust `super` path)_ — `super::load()` inside `a::b` is a reference to `a::load`.
#[test]
fn super_path_references_the_parent_module_function() {
    if !rust_analyzer_present() {
        return;
    }
    let project = rust_project();
    let root = project.path();
    write(
        root,
        "src/lib.rs",
        "pub mod a {\n    pub fn load() {}\n    mod b {\n        fn f() {\n            super::load();\n        }\n    }\n}\n",
    );
    build_rust(root);

    let store = GraphStore::open(&root.join("index.db")).unwrap();
    let references = store
        .references_of(&CanonicalId::from_raw("test-ws::libbin::a::load"))
        .unwrap();
    assert!(
        references
            .iter()
            .any(|row| row.document_path == "src/lib.rs" && row.line_column.is_some_and(|span| span.start.line == 5)),
        "the `super::load()` call references a::load: {references:?}"
    );
}

// _(Rust inline modules nest without a directory)_ — `mod a { mod b { pub struct S; } }` in `src/lib.rs`
// gives `S` the segments `a`, `b`, `S`.
#[test]
fn inline_modules_nest_without_directory() {
    if !rust_analyzer_present() {
        return;
    }
    let project = rust_project();
    let root = project.path();
    let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    write(
        root,
        "src/lib.rs",
        &format!("{lib}pub mod a {{\n    pub mod b {{\n        pub struct S;\n    }}\n}}\n"),
    );
    build_rust(root);

    let ids = identities(root);
    for nested in ["a", "a::b", "a::b::S"] {
        assert!(ids.contains(&format!("test-ws::libbin::{nested}")), "{nested}: {ids:?}");
    }
}

// _(Rust module loaded through a path attribute keeps its declared name)_ — the module `model` loaded
// from `src/generated/schema.rs` gives `Record` the segments `model`, `Record`.
#[test]
fn path_attribute_module_keeps_declared_name() {
    if !rust_analyzer_present() {
        return;
    }
    let project = rust_project();
    let root = project.path();
    let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    write(
        root,
        "src/lib.rs",
        &format!("{lib}#[path = \"generated/schema.rs\"]\npub mod model;\n"),
    );
    write(root, "src/generated/schema.rs", "pub struct Record;\n");
    let outcome = build_rust(root);

    let ids = identities(root);
    assert!(ids.contains("test-ws::libbin::model::Record"), "{ids:?}");
    assert!(
        ids.iter().all(|id| !id.contains("generated") && !id.contains("schema")),
        "no segment is named after the file: {ids:?}"
    );
    assert!(
        outcome.undetermined_units.documents.is_empty(),
        "the path-attribute file is placed in the library: {:?}",
        outcome.undetermined_units
    );
}

/// A Python project with the package `acme` and its module `tools` under `layout_root`
/// (`src` for the source-root layout, empty for the flat one), with a virtual environment.
/// `None` when `uv` or `scip-python` is absent.
fn python_project(source_root: &str) -> Option<(tempfile::TempDir, std::path::PathBuf)> {
    if silent_cartographer::semantic::python_adapter::PythonAdapter::discover_version(
        PythonIndexer::ScipPython,
        PythonIndexer::ScipPython.executable(),
    )
    .is_err()
    {
        eprintln!("SKIP: scip-python not installed");
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    let venv = dir.path().join(".venv");
    // `python3 -m venv` needs ensurepip, which not every host has; `uv venv` does not.
    let made = std::process::Command::new("uv").arg("venv").arg(&venv).output();
    if !matches!(made, Ok(ref out) if out.status.success()) {
        eprintln!("SKIP: `uv venv` could not create the environment");
        return None;
    }
    let package = Path::new(source_root).join("acme");
    write(dir.path(), &package.join("__init__.py").to_string_lossy(), "");
    write(
        dir.path(),
        &package.join("tools.py").to_string_lossy(),
        "def load():\n    return 1\n\n\nclass Tool:\n    def run(self):\n        return load()\n",
    );
    Some((dir, venv))
}

/// Write the project configuration: `src` is an import root when `extra_paths` says so.
fn write_python_config(root: &Path, extra_paths: &str) {
    write(
        root,
        "pyproject.toml",
        &format!("[project]\nname = \"acme-proj\"\nversion = \"0.1.0\"\n{extra_paths}"),
    );
}

fn build_python(root: &Path, venv: &Path) -> BuildOutcome {
    run_build(
        &root.join("index.db"),
        Some("test-ws"),
        root,
        "rust-analyzer",
        &PythonExecutables::default(),
        Some(PythonIndexer::ScipPython),
        Some(venv),
        Some(Language::Python),
        false,
        false,
        &Default::default(),
        &|_| false,
        &silent_cartographer::graph::store::free_bytes,
    )
    .expect("the build succeeds")
}

// _(Python package moved between source roots)_ — `src/acme/tools.py` moves to `acme/tools.py` and the
// configuration drops the `src` import root so the import name stays `acme.tools`; every identity is
// unchanged. Needs `scip-python` and `uv`.
#[test]
fn python_package_moved_between_source_roots_keeps_identity() {
    let Some((project, venv)) = python_project("src") else {
        return;
    };
    let root = project.path();
    write_python_config(root, "\n[tool.pyright]\nextraPaths = [\"src\"]\n");
    build_python(root, &venv);
    let before = identities(root);
    assert!(before.contains("test-ws::test-ws::acme::tools::load"), "{before:?}");

    for rel in ["acme/__init__.py", "acme/tools.py"] {
        let to = root.join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::rename(root.join("src").join(rel), to).unwrap();
    }
    write_python_config(root, "");
    assert!(build_python(root, &venv).rebuilt);

    assert_eq!(identities(root), before);
}

/// A package `libbin` whose library loads `util` and `caller` from files while its binary `tool`
/// has its root at `tool_root` holding `tool_source`, a program with its own `util` module.
fn lib_and_bin_project(tool_root: &str, tool_source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"libbin\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    );
    write(root, "src/lib.rs", "pub mod util;\npub mod caller;\n");
    write(root, "src/util.rs", "pub struct Config;\npub fn load() {}\n");
    write(root, "src/caller.rs", "pub fn call() {\n    crate::util::load();\n}\n");
    write(root, tool_root, tool_source);
    dir
}

/// Every in-workspace identity of the store at `root/index.db` that mentions `needle`.
fn identities_with(root: &Path, needle: &str) -> BTreeSet<String> {
    identities(root).into_iter().filter(|id| id.contains(needle)).collect()
}

/// Whether the store holds a reference to `target` located in `document`.
fn referenced_from(root: &Path, target: &str, document: &str) -> bool {
    let store = GraphStore::open(&root.join("index.db")).unwrap();
    store
        .references_of(&CanonicalId::from_raw(target))
        .unwrap()
        .iter()
        .any(|row| row.document_path == document)
}

/// Assert the library and the binary each own a `util::Config` and `util::load`, and that no unit was
/// left undetermined.
fn assert_util_split_by_unit(root: &Path, outcome: &BuildOutcome) {
    assert!(
        outcome.undetermined_units.documents.is_empty(),
        "{:?}",
        outcome.undetermined_units
    );
    let ids = identities_with(root, "util");
    for expected in [
        "test-ws::libbin::util::load",
        "test-ws::libbin+bin.tool::util::load",
        "test-ws::libbin::util::Config",
        "test-ws::libbin+bin.tool::util::Config",
    ] {
        assert!(ids.contains(expected), "{expected}: {ids:?}");
    }
}

// _(Rust binary declarations are marked by kind and name)_, _(Rust library declarations are unmarked)_ —
// a library file module and an inline binary module share a descriptor: neither is disclosed as
// undetermined, the identities split by unit, the sibling file's call reaches the library's function, and an
// unchanged rebuild is current.
#[test]
fn library_file_module_and_binary_inline_module_keep_their_units() {
    if !rust_analyzer_present() {
        return;
    }
    let project = lib_and_bin_project(
        "src/bin/tool.rs",
        "mod util {\n    pub struct Config;\n    pub fn load() {}\n}\nfn main() {\n    util::load();\n}\n",
    );
    let root = project.path();
    let first = build_rust(root);
    assert_util_split_by_unit(root, &first);
    assert!(
        referenced_from(root, "test-ws::libbin::util::load", "src/caller.rs"),
        "the sibling file's call is attributed to the library's function"
    );

    assert!(!build_rust(root).rebuilt, "an unchanged workspace is current");
}

// _(Rust binary declarations are marked by kind and name)_ — the binary loads its `util` from a file
// through a path attribute: its declarations carry `+bin.tool` and the library's stay unmarked.
#[test]
fn binary_file_module_through_path_attribute_keeps_its_unit() {
    if !rust_analyzer_present() {
        return;
    }
    let project = lib_and_bin_project(
        "src/bin/tool.rs",
        "#[path = \"../tool_util.rs\"]\nmod util;\nfn main() {\n    util::load();\n}\n",
    );
    let root = project.path();
    write(root, "src/tool_util.rs", "pub struct Config;\npub fn load() {}\n");
    let first = build_rust(root);
    assert_util_split_by_unit(root, &first);
    assert!(referenced_from(root, "test-ws::libbin::util::load", "src/caller.rs"));
}

// _(Rust binary declarations are marked by kind and name)_ — the binary loads its `util` from the
// sibling file `src/bin/tool/util.rs`, with no path attribute.
#[test]
fn binary_sibling_file_module_keeps_its_unit() {
    if !rust_analyzer_present() {
        return;
    }
    let project = lib_and_bin_project("src/bin/tool/main.rs", "mod util;\nfn main() {\n    util::load();\n}\n");
    let root = project.path();
    write(root, "src/bin/tool/util.rs", "pub struct Config;\npub fn load() {}\n");
    let first = build_rust(root);
    assert_util_split_by_unit(root, &first);
}
