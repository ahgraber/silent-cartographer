//! Process-level tests for `hooks install`: writing the post-commit hook that keeps the index
//! current, refusing rather than overwriting an existing hook, the typed environment failure outside
//! a git worktree, and resolving the hook path through git rather than assuming `.git/hooks` — the
//! scenarios a linked worktree, a relocated `core.hooksPath`, and installation from a subdirectory
//! each exercise.
//!
//! Every git invocation *this file* makes to set up a throwaway repository is isolated from the
//! developer's own git config (`GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`/`GIT_CONFIG_NOSYSTEM`, plus an
//! explicit `-c user.*`/`commit.gpgsign=false` on every commit), exactly as `tests/impact.rs` does.
//! The `c10r` invocations under test drive the built binary directly
//! (`Command::new(env!("CARGO_BIN_EXE_c10r"))`), never the ambient `c10r` on `PATH`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A throwaway git repository for one test, with a root commit so `HEAD` always resolves.
struct TestRepo {
    dir: tempfile::TempDir,
}

impl TestRepo {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = TestRepo { dir };
        repo.git(&["init", "-q"]);
        repo.git(&[
            "-c",
            "user.email=hooks-test@example.com",
            "-c",
            "user.name=Hooks Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            "root",
        ]);
        repo
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// This repository's root, canonicalized — matching the absolute form
    /// [`silent_cartographer::git::GitRepo::post_commit_hook_path`] reports, so a string comparison
    /// against the binary's own output does not trip on a symlinked temp root (`/tmp` -> `/private/tmp`
    /// on macOS).
    fn canonical_path(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap()
    }

    /// Run `git <args>` in this repository, isolated from the developer's own config.
    fn git(&self, args: &[&str]) {
        self.git_in(self.path(), args);
    }

    /// Run `git <args>` in `dir`, isolated from the developer's own config — used to drive git from a
    /// linked worktree's own directory rather than this repository's primary one.
    fn git_in(&self, dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed in {dir:?}");
    }

    /// The output of `git <args>` run in `dir`, isolated from the developer's own config.
    fn git_output_in(&self, dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?} failed in {dir:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// A fresh invocation of the built binary, run in this repository's own directory. Deliberately
    /// does not stub git config: none of the operations `run_hooks_install` performs (`rev-parse`)
    /// needs a committer identity.
    fn c10r(&self) -> Command {
        self.c10r_in(self.path())
    }

    /// A fresh invocation of the built binary, run in `dir` — used to drive `hooks install` from a
    /// linked worktree.
    fn c10r_in(&self, dir: &Path) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_c10r"));
        cmd.current_dir(dir);
        cmd
    }

    /// The absolute path git resolves `hooks/post-commit` to when invoked from `dir`, anchored the
    /// way [`silent_cartographer::git::GitRepo::post_commit_hook_path`] anchors it: joined against
    /// the canonicalized invocation directory, so a relative answer lands where git meant it and an
    /// absolute one (a relocated `core.hooksPath`) replaces the base outright.
    fn resolved_hook_path(&self, dir: &Path) -> PathBuf {
        let raw = self.git_output_in(dir, &["rev-parse", "--git-path", "hooks/post-commit"]);
        dir.canonicalize().unwrap().join(raw)
    }
}

/// A directory holding a stub `c10r` that records the argv it was invoked with, for placing early on
/// `PATH` so an executed hook's `c10r build` is observable. A real `c10r build` needs a language
/// indexer that is not available in this test environment; what the hook must carry is its argument
/// vector, and the stub is what makes that observable.
#[cfg(unix)]
struct StubBinary {
    dir: tempfile::TempDir,
}

#[cfg(unix)]
impl StubBinary {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("argv");
        let stub = dir.path().join("c10r");
        // A tempdir path carries no single quote, so embedding it in a single-quoted shell word is
        // sound here.
        std::fs::write(
            &stub,
            format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", record.display()),
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        StubBinary { dir }
    }

    /// `PATH` with this stub's directory first, so the stub shadows any ambient `c10r`.
    fn prepended_path(&self) -> std::ffi::OsString {
        let mut value = std::ffi::OsString::from(self.dir.path());
        if let Some(existing) = std::env::var_os("PATH") {
            value.push(":");
            value.push(existing);
        }
        value
    }

    /// The argv the stub last recorded, one argument per element.
    fn recorded_argv(&self) -> Vec<String> {
        let recorded = std::fs::read_to_string(self.dir.path().join("argv")).expect("the stub recorded its argv");
        recorded.lines().map(str::to_string).collect()
    }
}

// _(Hook installed where none exists)_ — `hooks install` writes an executable post-commit hook that
// runs `c10r build`, reports the path it landed at (matching where git actually resolved it), and
// exits with the success code. Exercised under both the human rendering and `--json`.
#[test]
fn installs_an_executable_hook_that_refreshes_the_index() {
    let repo = TestRepo::new();
    let expected_path =
        PathBuf::from(repo.git_output_in(repo.path(), &["rev-parse", "--git-path", "hooks/post-commit"]));
    let expected_path = repo.canonical_path().join(expected_path);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(expected_path.exists(), "the hook was written to {expected_path:?}");

    let contents = std::fs::read_to_string(&expected_path).unwrap();
    assert!(
        contents.contains("c10r build"),
        "the hook invokes the index build: {contents}"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&expected_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "the hook is executable");
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(expected_path.display().to_string().as_str()),
        "the human report names the path written: {stdout}"
    );

    // A second, freshly discovered repository confirms the same under `--json`.
    let repo = TestRepo::new();
    let expected_path =
        PathBuf::from(repo.git_output_in(repo.path(), &["rev-parse", "--git-path", "hooks/post-commit"]));
    let expected_path = repo.canonical_path().join(expected_path);

    let out = repo.c10r().args(["--json", "hooks", "install"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json answer parses");
    assert_eq!(
        answer["path"],
        expected_path.display().to_string(),
        "the JSON answer reports the path written"
    );
}

// _(An existing hook is never overwritten)_ — a hook already present at the resolved path is left
// byte-for-byte intact, the refusal names the path, and the process exits with a failure code.
#[test]
fn refuses_to_overwrite_an_existing_hook() {
    let repo = TestRepo::new();
    let hook_path = PathBuf::from(repo.git_output_in(repo.path(), &["rev-parse", "--git-path", "hooks/post-commit"]));
    let hook_path = repo.canonical_path().join(hook_path);
    if let Some(parent) = hook_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let sentinel = "#!/bin/sh\n# a caller's own pre-existing hook — must survive untouched\necho sentinel-9f3c\n";
    std::fs::write(&hook_path, sentinel).unwrap();

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_ne!(
        out.status.code(),
        Some(0),
        "an existing hook is refused, not overwritten"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(hook_path.display().to_string().as_str()),
        "the refusal names the path it found: {stderr}"
    );

    let contents_after = std::fs::read_to_string(&hook_path).unwrap();
    assert_eq!(
        contents_after, sentinel,
        "the existing hook is left byte-for-byte intact"
    );
}

// _(Installing outside a git worktree is a typed environment failure)_ — a plain, non-repository
// directory exits with the environment/setup-failure code (5), the same code `impact` raises for the
// same condition.
#[test]
fn outside_a_git_worktree_is_the_environment_setup_failure_code() {
    let dir = tempfile::tempdir().unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_c10r"))
        .current_dir(dir.path())
        .args(["hooks", "install"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(5),
        "a non-worktree directory is the environment/setup-failure code: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("worktree"),
        "the diagnostic states the directory is not a git worktree: {stderr}"
    );
}

// _(The hook is installed where this worktree resolves its hooks)_ — a linked worktree keeps its
// hooks in the repository's common directory, not in a `.git/hooks` directory beside its own working
// files. Running `hooks install` from inside the linked worktree must land the hook at the path that
// worktree actually resolves `git rev-parse --git-path hooks/post-commit` to, not at an assumed
// `.git/hooks` location the linked worktree does not even have.
#[test]
fn linked_worktree_installs_the_hook_where_it_actually_resolves() {
    let repo = TestRepo::new();
    // A dedicated tempdir for the linked worktree's parent, so its fixed subdirectory name cannot
    // collide with another test's linked worktree sharing the same system temp root.
    let linked_parent = tempfile::tempdir().unwrap();
    let linked_dir = linked_parent.path().join("linked-worktree");
    repo.git(&["worktree", "add", "-q", linked_dir.to_str().unwrap(), "-b", "linked"]);

    // The premise the boundary method exists for: a linked worktree's `.git` is a *file* pointing at
    // the repository's common directory, not a directory. A hard-coded `.git/hooks/post-commit` join
    // could not even be created here, and the hooks git actually reads live in the common directory.
    assert!(
        linked_dir.join(".git").is_file(),
        "a linked worktree's .git is a file, so an assumed .git/hooks path could not be written"
    );
    assert!(
        !linked_dir.join(".git").join("hooks").exists(),
        "a linked worktree does not carry its own hooks directory"
    );

    let resolved = PathBuf::from(repo.git_output_in(&linked_dir, &["rev-parse", "--git-path", "hooks/post-commit"]));
    let expected_path = linked_dir.join(resolved);

    let out = repo.c10r_in(&linked_dir).args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        expected_path.exists(),
        "the hook landed where the linked worktree resolves its hooks: {expected_path:?}"
    );
    // The expected path is not under the linked worktree's own (nonexistent) `.git/hooks` — it is the
    // primary repository's shared hooks directory.
    assert!(
        !expected_path.starts_with(linked_dir.join(".git").join("hooks")),
        "the hook is not written to a `.git/hooks` directory beside the linked worktree: {expected_path:?}"
    );
    // It landed in the primary repository's own directory — the one place git will read it from for
    // commits made in either worktree.
    assert!(
        expected_path.starts_with(repo.canonical_path()),
        "the hook landed under the primary repository, not beside the linked worktree: {expected_path:?}"
    );
}

// _(Hook installed where none exists)_ — "refreshes the index" means *this* caller's index, under
// *this* caller's workspace identity, over *this* caller's workspace root. Git runs `post-commit`
// with the working directory at the worktree's top level, which is neither the caller's invocation
// directory nor — for a workspace below the repository root — the workspace at all, so all three
// have to be written into the script as resolved absolute values rather than left to defaults.
#[test]
fn the_hook_carries_the_callers_db_workspace_and_root() {
    // A non-default `--db` and an explicit `--workspace`, installed from the repository root.
    let repo = TestRepo::new();
    let hook_path = repo.resolved_hook_path(repo.path());
    let out = repo
        .c10r()
        .args([
            "--db",
            "custom/store.db",
            "--workspace",
            "chosen-identity",
            "hooks",
            "install",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let contents = std::fs::read_to_string(&hook_path).unwrap();
    let expected_root = repo.canonical_path();
    let expected_db = expected_root.join("custom/store.db");
    assert!(
        contents.contains(expected_root.display().to_string().as_str()),
        "the hook names the resolved absolute workspace root: {contents}"
    );
    assert!(
        contents.contains(expected_db.display().to_string().as_str()),
        "the hook names the caller's `--db`, resolved absolute: {contents}"
    );
    assert!(
        contents.contains("--workspace"),
        "the hook pins the workspace identity rather than letting `build` re-derive it: {contents}"
    );
    assert!(
        contents.contains("chosen-identity"),
        "the hook names the caller's workspace identity: {contents}"
    );

    // A workspace in a subdirectory of the repository, with the default `--db` and a derived
    // identity: the hook must index the subdirectory, not the worktree top level git will run it
    // from, and must namespace symbols under the subdirectory's own derived identity.
    let repo = TestRepo::new();
    let workspace = repo.path().join("nested-workspace");
    std::fs::create_dir(&workspace).unwrap();
    let hook_path = repo.resolved_hook_path(&workspace);
    let out = repo.c10r_in(&workspace).args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let contents = std::fs::read_to_string(&hook_path).unwrap();
    let expected_root = workspace.canonicalize().unwrap();
    let expected_db = expected_root.join(".c10r/index.db");
    assert!(
        contents.contains(expected_root.display().to_string().as_str()),
        "the hook names the workspace subdirectory, not the worktree top level: {contents}"
    );
    assert!(
        contents.contains(expected_db.display().to_string().as_str()),
        "the hook names the index under the workspace subdirectory: {contents}"
    );
    assert!(
        contents.contains("--workspace 'nested-workspace'"),
        "the hook pins the identity derived from the workspace root's own name: {contents}"
    );
    // The one thing that would still be wrong if the values were merely present: the worktree top
    // level must not be what `build` is pointed at.
    assert!(
        !contents.contains(&format!("c10r build '{}'", repo.canonical_path().display())),
        "the hook must not index the worktree top level for a workspace below it: {contents}"
    );
}

// _(Hook installed where none exists)_ — the written file is a hook git can actually run, and
// running it invokes `c10r build` with the arguments the script carries. Asserting the file's text
// alone leaves the shebang and the script's executability-as-a-program untested, so the hook is
// executed here the way git runs `post-commit`: as a program, with the working directory at the
// worktree's top level. A real `c10r build` needs a language indexer this environment does not have,
// so a stub `c10r` early on `PATH` records the argv instead.
#[cfg(unix)]
#[test]
fn the_installed_hook_runs_c10r_build_with_the_resolved_arguments() {
    let repo = TestRepo::new();
    let hook_path = repo.resolved_hook_path(repo.path());

    let out = repo
        .c10r()
        .args([
            "--db",
            "custom/store.db",
            "--workspace",
            "chosen-identity",
            "hooks",
            "install",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let contents = std::fs::read_to_string(&hook_path).unwrap();
    assert!(
        contents.starts_with("#!/bin/sh\n"),
        "the hook carries an interpreter line, without which git cannot execute it: {contents}"
    );

    let stub = StubBinary::new();
    let status = Command::new(&hook_path)
        // Git invokes `post-commit` with the working directory at the worktree's top level.
        .current_dir(repo.canonical_path())
        .env("PATH", stub.prepended_path())
        .status()
        .expect("the installed hook is executable as a program");
    assert!(status.success(), "the hook ran to completion: {status:?}");

    let argv = stub.recorded_argv();
    let expected_root = repo.canonical_path().display().to_string();
    let expected_db = repo.canonical_path().join("custom/store.db").display().to_string();
    assert_eq!(
        argv,
        vec![
            "build".to_string(),
            expected_root,
            "--db".to_string(),
            expected_db,
            "--workspace".to_string(),
            "chosen-identity".to_string(),
        ],
        "the hook invoked `c10r build` with the caller's root, --db, and --workspace"
    );
}

// _(The hook is installed where this worktree resolves its hooks)_ — the scenario's second arm: a
// repository whose `core.hooksPath` relocates hooks outside `.git` entirely. `git rev-parse
// --git-path hooks/post-commit` is the only authority on where that lands, so the expectation is
// taken from git itself rather than reconstructed.
#[test]
fn a_relocated_hooks_path_installs_the_hook_where_git_resolves_it() {
    let repo = TestRepo::new();
    let relocated = repo.canonical_path().join("ops").join("githooks");
    repo.git(&["config", "core.hooksPath", relocated.to_str().unwrap()]);

    let expected_path = repo.resolved_hook_path(repo.path());
    assert!(
        expected_path.starts_with(&relocated),
        "git resolves hooks to the relocated directory: {expected_path:?}"
    );
    assert!(
        !expected_path.starts_with(repo.canonical_path().join(".git")),
        "the relocated hook path is outside `.git`: {expected_path:?}"
    );

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        expected_path.exists(),
        "the hook landed at the relocated hook path: {expected_path:?}"
    );
    assert!(
        !repo.canonical_path().join(".git/hooks/post-commit").exists(),
        "nothing was written to the assumed `.git/hooks` location git would never read"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(expected_path.display().to_string().as_str()),
        "the report names the relocated path written: {stdout}"
    );
}

// _(The hook is installed where this worktree resolves its hooks)_ — installing from a subdirectory
// rather than the worktree top level. `git rev-parse --git-path` answers relative to the invocation
// directory, so a subdirectory invocation is where anchoring that answer against the wrong base
// would put the hook somewhere git never reads.
#[test]
fn installing_from_a_subdirectory_lands_where_git_resolves_hooks() {
    let repo = TestRepo::new();
    let subdir = repo.path().join("crates").join("inner");
    std::fs::create_dir_all(&subdir).unwrap();

    let expected_path = repo.resolved_hook_path(&subdir);
    let out = repo
        .c10r_in(&subdir)
        .args(["--json", "hooks", "install"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        expected_path.exists(),
        "the hook landed where git resolves hooks from the subdirectory: {expected_path:?}"
    );
    // Independently of git's own relativity: this repository keeps its hooks in the primary
    // `.git/hooks`, and that is the single file the hook must be.
    assert!(
        repo.canonical_path().join(".git/hooks/post-commit").exists(),
        "the hook is the repository's own post-commit hook, not one nested under the subdirectory"
    );
    assert!(
        !subdir.join(".git").exists(),
        "no `.git` directory was fabricated beside the subdirectory"
    );

    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json answer parses");
    assert_eq!(
        answer["path"],
        expected_path.display().to_string(),
        "the reported path is the absolute path git resolved, not a subdirectory-relative one"
    );
}

// _(An existing hook is never overwritten)_ — a dangling symlink at the hook path is something
// already present, and following it would write outside the hooks directory: neither refusing nor
// overwriting, but clobbering a third location. It is refused like any other occupant.
#[cfg(unix)]
#[test]
fn a_dangling_symlink_at_the_hook_path_is_refused() {
    let repo = TestRepo::new();
    let hook_path = repo.resolved_hook_path(repo.path());
    std::fs::create_dir_all(hook_path.parent().unwrap()).unwrap();
    let target = repo.canonical_path().join("absent-target");
    std::os::unix::fs::symlink(&target, &hook_path).unwrap();
    assert!(!target.exists(), "the symlink's target does not exist");

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_ne!(
        out.status.code(),
        Some(0),
        "a dangling symlink at the hook path is refused, not written through"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(hook_path.display().to_string().as_str()),
        "the refusal names the path it found: {stderr}"
    );
    assert!(!target.exists(), "nothing was written through the link to {target:?}");
    assert!(
        std::fs::symlink_metadata(&hook_path)
            .expect("the link itself is still there")
            .file_type()
            .is_symlink(),
        "the link itself is left intact"
    );
}
