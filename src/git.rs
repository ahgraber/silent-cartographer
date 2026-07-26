//! The git boundary: this module owns *every* `git` invocation the tool makes. Git is an external
//! boundary, so every invocation is time-bounded through the shared bounded-subprocess helper and
//! every failure is typed; no other module shells out to git.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::exit::Failure;
use crate::semantic::probe::{PROBE_DEADLINE, ProbeCapture, ProbeOutcome, run_bounded, run_bounded_full};

/// Which change seeds an impact assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedMode {
    /// The working-tree change against `HEAD` — every uncommitted edit, staged or not.
    WorkingTree,
    /// The staged change against `HEAD`.
    Staged,
    /// A caller-supplied revision or revision range, interpreted by `git diff` itself.
    Revspec(String),
}

/// A typed failure at the git boundary.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// No usable `git` executable could be spawned.
    #[error("git could not be run ({0}); `impact` requires a working `git` to seed a diff")]
    Absent(String),
    /// The directory is not inside a git worktree.
    #[error("{0} is not inside a git worktree")]
    NotAWorktree(String),
    /// A `git` invocation did not complete within the bounded deadline.
    #[error("git {invocation} did not complete within the {secs}-second bounded deadline", secs = PROBE_DEADLINE.as_secs())]
    Timeout {
        /// The invocation that timed out, named for diagnostics.
        invocation: String,
    },
    /// A revision specification that is malformed or does not resolve in this repository.
    #[error("{0} is not a valid revision in this repository")]
    BadRevspec(String),
    /// A `git` invocation completed but reported failure.
    #[error("git {invocation} failed: {stderr}")]
    Failed {
        /// The invocation that failed, named for diagnostics.
        invocation: String,
        /// The invocation's captured stderr.
        stderr: String,
    },
}

impl GitError {
    /// Classify this boundary failure onto the process exit-code taxonomy.
    ///
    /// An absent `git`, a directory that is not a worktree, and a bounded timeout are unmet
    /// environment prerequisites — the "or setup step" clause of the indexer/setup-failure code — so
    /// they reuse that code rather than widening the closed taxonomy. A malformed or unresolvable
    /// revision specification is caller input, so it is a usage error, kept distinct so the two
    /// surface differently. Any other reported failure carries no more specific category.
    pub fn into_failure(self) -> anyhow::Error {
        let message = self.to_string();
        match self {
            GitError::Absent(_) | GitError::NotAWorktree(_) | GitError::Timeout { .. } => {
                Failure::IndexerSetup(message).into()
            }
            GitError::BadRevspec(_) => Failure::Usage(message).into(),
            GitError::Failed { .. } => anyhow::anyhow!(message),
        }
    }
}

/// The two endpoints of a range revspec, split exactly as git's own range syntax parses it: a
/// three-dot range (`A...B`, whose result is the merge base of its endpoints) is checked before a
/// two-dot range (`A..B`, whose result is its left endpoint) because a three-dot spec also contains
/// `..`. `None` when `spec` names a single revision rather than a range.
///
/// Factored out so [`GitRepo::base_revision`] and [`GitRepo::range_head`] split a range the same
/// way, rather than each duplicating the `...`-before-`..` rule.
enum RangeSplit<'a> {
    /// `A...B`: the merge base of `A` and `B`.
    ThreeDot(&'a str, &'a str),
    /// `A..B`: `A` itself.
    TwoDot(&'a str, &'a str),
}

fn split_range(spec: &str) -> Option<RangeSplit<'_>> {
    if let Some((left, right)) = spec.split_once("...") {
        Some(RangeSplit::ThreeDot(left, right))
    } else {
        spec.split_once("..")
            .map(|(left, right)| RangeSplit::TwoDot(left, right))
    }
}

/// Spawn a small metadata `git` invocation (`args`) in `dir`, bounded by `run_bounded`'s capture cap —
/// every metadata call site here reads a short, fixed-shape answer (a revision id, a boolean, a
/// prefix), never a payload that could exceed it. `label` names the invocation in the errors this
/// returns.
///
/// Only a spawn failure (→ [`GitError::Absent`]) and a bounded timeout (→ [`GitError::Timeout`]) are
/// translated here; a non-zero exit is returned as a completed capture for the caller to interpret,
/// since what a non-zero exit means (a bad revspec, an absent path, an operational failure) differs by
/// call site.
fn spawn_metadata(dir: &Path, args: &[&str], label: &str) -> Result<ProbeCapture, GitError> {
    let mut command = Command::new("git");
    command.args(args).current_dir(dir);
    match run_bounded(command, PROBE_DEADLINE) {
        Ok(ProbeOutcome::Completed(capture)) => Ok(capture),
        Ok(ProbeOutcome::TimedOut) => Err(GitError::Timeout {
            invocation: label.to_string(),
        }),
        Err(err) => Err(GitError::Absent(err.to_string())),
    }
}

/// As [`spawn_metadata`], but retains the full stdout/stderr via `run_bounded_full` rather than
/// capping them — a diff payload, a `git show` blob, and a repository's tracked-file listing are all
/// unbounded in size, and a capped capture would silently answer from a prefix of them.
fn spawn_payload(dir: &Path, args: &[&str], label: &str) -> Result<ProbeCapture, GitError> {
    let mut command = Command::new("git");
    command.args(args).current_dir(dir);
    match run_bounded_full(command, PROBE_DEADLINE) {
        Ok(ProbeOutcome::Completed(capture)) => Ok(capture),
        Ok(ProbeOutcome::TimedOut) => Err(GitError::Timeout {
            invocation: label.to_string(),
        }),
        Err(err) => Err(GitError::Absent(err.to_string())),
    }
}

/// The git worktree an impact assessment draws its diff from: the workspace root `git` is invoked in,
/// together with that root's path prefix relative to the repository root.
///
/// The prefix exists so the boundary can speak two path vocabularies at once: diffs are requested
/// `--relative`, so their paths line up with the workspace-relative document paths the index holds,
/// while `git show` needs a repository-root-relative path — the prefix converts between them.
#[derive(Debug)]
pub struct GitRepo {
    root: PathBuf,
    prefix: String,
}

impl GitRepo {
    /// Discover the git worktree that `root` is invoked from: one preflight `git rev-parse
    /// --is-inside-work-tree --show-prefix`, current-dir'd at `root`.
    ///
    /// The first stdout line must read exactly `true`; anything else (a non-zero exit, or a first line
    /// that isn't `true`) means `root` is not inside a git worktree. The second stdout line is the
    /// prefix, stored verbatim — it already ends with `/` when non-empty (git emits it that way), so
    /// this never appends or strips a separator.
    pub fn discover(root: &Path) -> Result<Self, GitError> {
        let label = "rev-parse --is-inside-work-tree --show-prefix";
        let capture = spawn_metadata(root, &["rev-parse", "--is-inside-work-tree", "--show-prefix"], label)?;
        if !capture.status.success() {
            return Err(GitError::NotAWorktree(root.display().to_string()));
        }
        let stdout = String::from_utf8_lossy(&capture.stdout);
        let mut lines = stdout.lines();
        if lines.next() != Some("true") {
            return Err(GitError::NotAWorktree(root.display().to_string()));
        }
        let prefix = lines.next().unwrap_or("").to_string();
        Ok(GitRepo {
            root: root.to_path_buf(),
            prefix,
        })
    }

    /// Resolve `endpoint` (a revision expression understood by `git rev-parse --verify`) to its full
    /// commit id, or `Ok(None)` when it does not resolve (a non-zero exit) — the distinct `BadRevspec`
    /// text a caller wants (naming the spec it typed, not a derived endpoint) is layered on by the
    /// caller rather than by this helper. A spawn failure or bounded timeout still propagates as `Err`.
    fn resolve_commit(&self, endpoint: &str) -> Result<Option<String>, GitError> {
        let arg = format!("{endpoint}^{{commit}}");
        let label = format!("rev-parse --verify --quiet {endpoint}");
        let capture = spawn_metadata(&self.root, &["rev-parse", "--verify", "--quiet", &arg], &label)?;
        if !capture.status.success() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&capture.stdout).trim().to_string()))
    }

    /// The merge base of two already-resolved commit ids, or `BadRevspec(original_spec)` when they
    /// share no common ancestor.
    fn merge_base(&self, a: &str, b: &str, original_spec: &str) -> Result<String, GitError> {
        let label = format!("merge-base {a} {b}");
        let capture = spawn_metadata(&self.root, &["merge-base", a, b], &label)?;
        if !capture.status.success() {
            return Err(GitError::BadRevspec(original_spec.to_string()));
        }
        Ok(String::from_utf8_lossy(&capture.stdout).trim().to_string())
    }

    /// The resolved pre-change revision for `mode`, as a full commit id.
    ///
    /// `WorkingTree` and `Staged` both resolve `HEAD`; an unborn `HEAD` (a repository with no commits)
    /// surfaces as `BadRevspec`, since there is no pre-change side to seed a diff from.
    ///
    /// A `Revspec` is split by [`split_range`]: a three-dot range's two endpoints' merge base is the
    /// result; a two-dot range's left endpoint is the base, after verifying the right endpoint also
    /// resolves; a spec naming neither is the base itself, matching `git diff <commit>`'s own meaning
    /// of "that revision against the working tree." Every endpoint is resolved with `git rev-parse
    /// --verify --quiet <endpoint>^{commit}`; a non-zero exit reports `BadRevspec` naming the spec the
    /// caller typed, not the derived endpoint.
    ///
    /// Each endpoint is checked for a leading `-` before it is resolved, so no part of a caller-supplied
    /// spec can reach `git` as an option — the guard is per-endpoint rather than on the whole spec
    /// because a range hides its endpoints from a whole-string check (`HEAD..--upload-pack=…`).
    pub fn base_revision(&self, mode: &SeedMode) -> Result<String, GitError> {
        match mode {
            SeedMode::WorkingTree | SeedMode::Staged => self
                .resolve_commit("HEAD")?
                .ok_or_else(|| GitError::BadRevspec("HEAD".to_string())),
            SeedMode::Revspec(spec) => match split_range(spec) {
                Some(RangeSplit::ThreeDot(left, right)) => {
                    let base_id = self.resolve_endpoint(left, spec)?;
                    let head_id = self.resolve_endpoint(right, spec)?;
                    self.merge_base(&base_id, &head_id, spec)
                }
                Some(RangeSplit::TwoDot(left, right)) => {
                    let base_id = self.resolve_endpoint(left, spec)?;
                    self.resolve_endpoint(right, spec)?;
                    Ok(base_id)
                }
                None => self.resolve_endpoint(spec, spec),
            },
        }
    }

    /// The resolved commit id of a `Revspec` range's head endpoint — the right-hand side of a
    /// two-dot or three-dot range — or `Ok(None)` for `WorkingTree`, `Staged`, or a bare
    /// single-revision spec, none of which name a range.
    ///
    /// Used to decide whether a range's head is what is currently checked out (the reconstructibility
    /// condition [`crate::query::impact`] requires before claiming `exact`), and to materialize the
    /// recovery recipe's second, head-side worktree.
    pub fn range_head(&self, mode: &SeedMode) -> Result<Option<String>, GitError> {
        let SeedMode::Revspec(spec) = mode else {
            return Ok(None);
        };
        match split_range(spec) {
            Some(RangeSplit::ThreeDot(_, right)) | Some(RangeSplit::TwoDot(_, right)) => {
                Ok(Some(self.resolve_endpoint(right, spec)?))
            }
            None => Ok(None),
        }
    }

    /// Resolve one endpoint of a caller-supplied revision spec to its commit id, reporting
    /// `BadRevspec(original_spec)` when the endpoint begins with `-` (it would reach `git` as an option,
    /// never as a revision) or does not resolve. An empty endpoint means `HEAD`, matching git's own
    /// range shorthand.
    fn resolve_endpoint(&self, endpoint: &str, original_spec: &str) -> Result<String, GitError> {
        let endpoint = if endpoint.is_empty() { "HEAD" } else { endpoint };
        if endpoint.starts_with('-') {
            return Err(GitError::BadRevspec(original_spec.to_string()));
        }
        self.resolve_commit(endpoint)?
            .ok_or_else(|| GitError::BadRevspec(original_spec.to_string()))
    }

    /// The unified patch for `mode`, captured in full and unnarrowed.
    ///
    /// Invocation: `git -c core.quotepath=false diff --no-color --no-ext-diff --no-textconv
    /// --no-prefix --find-renames --unified=0 --relative` followed by the mode arguments
    /// (`WorkingTree` → `HEAD`; `Staged` → `--cached HEAD`; `Revspec(s)` → `s`). Each flag is
    /// load-bearing:
    ///
    /// - `-c core.quotepath=false` keeps non-ASCII paths unquoted.
    /// - `--no-textconv` keeps the hunk line numbers describing the file's real bytes. A configured
    ///   textconv driver rewrites what `diff` sees but not what `show` returns, and the byte offsets
    ///   are measured against `show`'s output — so a driver that changed a file's line count would
    ///   silently land every seed on the wrong declaration while the answer still graded itself exact.
    /// - `--no-prefix` removes the `a/`…`b/` prefixes, which are ambiguous to strip when a path
    ///   contains a space.
    /// - `--unified=0` makes each hunk header name exactly the changed pre-change lines, with no
    ///   context lines widening the seed onto a neighbouring declaration.
    /// - `--find-renames` makes a renamed file's changed regions attach to its pre-change path.
    /// - `--relative` yields paths relative to the invocation directory, matching the
    ///   workspace-relative document paths the index holds, and confines the diff to the workspace
    ///   subtree.
    ///
    /// Path narrowing is not applied here: a pathspec is handed to the same invocation that performs
    /// rename detection, and filters a renamed file's pre-change path out of the diff queue before
    /// git has anything to pair the post-change path with — see
    /// [`narrow_to_paths`](crate::query::diff::narrow_to_paths), which applies narrowing to the
    /// parsed change set instead.
    ///
    /// A revision spec whose text would reach `git` as an option is refused here as well as in
    /// [`GitRepo::base_revision`], so the guard holds however the two are ordered.
    pub fn diff(&self, mode: &SeedMode) -> Result<String, GitError> {
        if let SeedMode::Revspec(spec) = mode
            && spec.split("..").any(|endpoint| endpoint.starts_with('-'))
        {
            return Err(GitError::BadRevspec(spec.clone()));
        }
        let mut args: Vec<String> = vec![
            "-c".to_string(),
            "core.quotepath=false".to_string(),
            "diff".to_string(),
            "--no-color".to_string(),
            "--no-ext-diff".to_string(),
            "--no-textconv".to_string(),
            "--no-prefix".to_string(),
            "--find-renames".to_string(),
            "--unified=0".to_string(),
            "--relative".to_string(),
        ];
        match mode {
            SeedMode::WorkingTree => args.push("HEAD".to_string()),
            SeedMode::Staged => {
                args.push("--cached".to_string());
                args.push("HEAD".to_string());
            }
            SeedMode::Revspec(spec) => args.push(spec.clone()),
        }

        let label = "diff".to_string();
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let capture = spawn_payload(&self.root, &arg_refs, &label)?;
        if !capture.status.success() {
            return Err(GitError::Failed {
                invocation: label,
                stderr: String::from_utf8_lossy(&capture.stderr).into_owned(),
            });
        }
        String::from_utf8(capture.stdout).map_err(|_| GitError::Failed {
            invocation: label,
            stderr: "the patch was not valid UTF-8".to_string(),
        })
    }

    /// The content of the workspace-relative `path` at `revision`, i.e. `git show
    /// <revision>:<prefix><path>`.
    ///
    /// `revision` must already be a resolved id from [`GitRepo::base_revision`], so no unverified user
    /// text reaches this argument. Returns `Ok(None)` when the path does not exist at that revision or
    /// its bytes are not valid UTF-8 — neither is a boundary failure: a path absent from the
    /// pre-change side simply has no pre-change content, and a non-UTF-8 blob is not a source the index
    /// holds. A timeout is still `GitError::Timeout`.
    ///
    /// A non-zero exit from `show` itself is ambiguous: it fires both for a path genuinely absent at
    /// `revision` and for an operational failure (a corrupt object, an unreadable file, a permission
    /// failure), and folding the latter into `Ok(None)` would dress a broken repository as a set of
    /// unresolvable regions on an otherwise successful assessment. The two are disambiguated with
    /// `git cat-file -e <revision>:<prefix><path>`, run only on `show`'s failure path: a zero exit
    /// there means the object exists, so `show`'s own failure was operational and surfaces as
    /// [`GitError::Failed`]; a non-zero exit means the path really is not present at that revision,
    /// which stays `Ok(None)`.
    pub fn show(&self, revision: &str, path: &str) -> Result<Option<String>, GitError> {
        let object = format!("{revision}:{}{path}", self.prefix);
        let label = format!("show {object}");
        let capture = spawn_payload(&self.root, &["show", &object], &label)?;
        if !capture.status.success() {
            let exists_label = format!("cat-file -e {object}");
            let exists = spawn_metadata(&self.root, &["cat-file", "-e", &object], &exists_label)?;
            if exists.status.success() {
                return Err(GitError::Failed {
                    invocation: label,
                    stderr: String::from_utf8_lossy(&capture.stderr).into_owned(),
                });
            }
            return Ok(None);
        }
        Ok(String::from_utf8(capture.stdout).ok())
    }

    /// Every tracked path under the invocation directory (`git ls-files -z`, split on NUL, empties
    /// dropped). Paths are relative to the invocation directory, so they share the workspace-relative
    /// vocabulary of the index and of the diff — the caller subtracts this set from the discovered
    /// source set to learn which discovered sources are untracked.
    pub fn tracked_files(&self) -> Result<Vec<String>, GitError> {
        let label = "ls-files -z".to_string();
        let capture = spawn_payload(&self.root, &["ls-files", "-z"], &label)?;
        if !capture.status.success() {
            return Err(GitError::Failed {
                invocation: label,
                stderr: String::from_utf8_lossy(&capture.stderr).into_owned(),
            });
        }
        let stdout = String::from_utf8_lossy(&capture.stdout);
        Ok(stdout
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect())
    }

    /// The absolute path this worktree resolves `hooks/post-commit` to.
    ///
    /// Resolved through git rather than assumed to be `.git/hooks`: a linked worktree keeps its hooks
    /// in the repository's common directory, and `core.hooksPath` can relocate them outright, so a
    /// hard-coded path would write somewhere git never reads.
    ///
    /// `git rev-parse --git-path hooks/post-commit` yields a path relative to the invocation
    /// directory when the repository is nearby, so the result is resolved against the invocation root
    /// to make it absolute regardless of that relativity.
    pub fn post_commit_hook_path(&self) -> Result<PathBuf, GitError> {
        let label = "rev-parse --git-path hooks/post-commit";
        let capture = spawn_metadata(&self.root, &["rev-parse", "--git-path", "hooks/post-commit"], label)?;
        if !capture.status.success() {
            return Err(GitError::Failed {
                invocation: label.to_string(),
                stderr: String::from_utf8_lossy(&capture.stderr).into_owned(),
            });
        }
        let raw = String::from_utf8_lossy(&capture.stdout).trim().to_string();
        // The invocation root itself may be relative (e.g. `.`, when the caller relies on the
        // process's own current directory rather than an absolute path) — canonicalized before
        // joining, so the result is absolute even when `raw` is not. An already-absolute `raw` (a
        // relocated `core.hooksPath`) still wins outright, since `Path::join` replaces rather than
        // appends when its argument is absolute.
        let base = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        Ok(base.join(raw))
    }

    /// The workspace root's path prefix within the repository, exactly as [`GitRepo::discover`]
    /// recorded it — ends with `/` when non-empty. The recovery recipe uses this to rebuild the
    /// workspace subtree inside a throwaway worktree rather than the repository root.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The paths differing between the index and the working tree (`git diff --name-only
    /// --relative`) — the edits a staged-mode diff does not describe. Paths are relative to the
    /// invocation directory, matching the diff's own vocabulary. Uses the full-capture spawn helper,
    /// since a path list is unbounded.
    pub fn unstaged_paths(&self) -> Result<Vec<String>, GitError> {
        self.diff_name_only(&["diff", "--name-only", "--relative"], "diff --name-only --relative")
    }

    /// The paths differing between `HEAD` and the working tree, staged or not (`git diff --name-only
    /// --relative HEAD`).
    pub fn uncommitted_paths(&self) -> Result<Vec<String>, GitError> {
        self.diff_name_only(
            &["diff", "--name-only", "--relative", "HEAD"],
            "diff --name-only --relative HEAD",
        )
    }

    /// Run a `git diff --name-only` invocation (`args`, labeled `label`) and split its output into
    /// one path per line.
    fn diff_name_only(&self, args: &[&str], label: &str) -> Result<Vec<String>, GitError> {
        let capture = spawn_payload(&self.root, args, label)?;
        if !capture.status.success() {
            return Err(GitError::Failed {
                invocation: label.to_string(),
                stderr: String::from_utf8_lossy(&capture.stderr).into_owned(),
            });
        }
        let stdout = String::from_utf8_lossy(&capture.stdout);
        Ok(stdout.lines().filter(|s| !s.is_empty()).map(str::to_string).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    /// Run `git <args>` in `dir`, isolated from the developer's own config: every ambient config
    /// source is pointed at `/dev/null` so the test outcome cannot depend on it.
    fn git(dir: &Path, args: &[&str]) {
        let status = StdCommand::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed in {dir:?}");
    }

    /// Initialize a repository at `dir` with an initial empty-tree commit, so `HEAD` always resolves.
    fn init_repo(dir: &Path) {
        git(dir, &["init", "-q"]);
        git(
            dir,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-q",
                "-m",
                "root",
            ],
        );
    }

    fn commit_all(dir: &Path, message: &str) {
        git(dir, &["add", "-A"]);
        git(
            dir,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                message,
            ],
        );
    }

    fn head_id(dir: &Path) -> String {
        let output = StdCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git runs");
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    // `discover` in a plain, non-repository directory reports `NotAWorktree`.
    #[test]
    fn discover_outside_a_repository_is_not_a_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let result = GitRepo::discover(dir.path());
        assert!(matches!(result, Err(GitError::NotAWorktree(_))), "{result:?}");
    }

    // `discover` at a repository root reports an empty prefix; discovering a subdirectory reports that
    // subdirectory's path, trailing-slash included, verbatim from git.
    #[test]
    fn discover_reports_the_prefix_relative_to_the_repository_root() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();

        let at_root = GitRepo::discover(dir.path()).expect("root is a worktree");
        assert_eq!(at_root.prefix, "");

        let at_sub = GitRepo::discover(&sub).expect("subdir is inside the worktree");
        assert_eq!(at_sub.prefix, "sub/");
    }

    // `WorkingTree` and `Staged` both resolve `HEAD` to the current commit id.
    #[test]
    fn base_revision_working_tree_and_staged_resolve_head() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let repo = GitRepo::discover(dir.path()).unwrap();
        let head = head_id(dir.path());

        assert_eq!(repo.base_revision(&SeedMode::WorkingTree).unwrap(), head);
        assert_eq!(repo.base_revision(&SeedMode::Staged).unwrap(), head);
    }

    // A `A..B` range resolves to `A`, its left endpoint, after verifying `B` also resolves.
    #[test]
    fn base_revision_two_dot_range_resolves_to_the_left_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let a = head_id(dir.path());
        std::fs::write(dir.path().join("file.txt"), "content\n").unwrap();
        commit_all(dir.path(), "second");
        let repo = GitRepo::discover(dir.path()).unwrap();

        let base = repo.base_revision(&SeedMode::Revspec(format!("{a}..HEAD"))).unwrap();
        assert_eq!(base, a);
    }

    // A `A...B` range resolves to the merge base of a two-branch history.
    #[test]
    fn base_revision_three_dot_range_resolves_to_the_merge_base() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let root = head_id(dir.path());

        git(dir.path(), &["checkout", "-q", "-b", "feature"]);
        std::fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
        commit_all(dir.path(), "feature commit");
        let feature = head_id(dir.path());

        git(dir.path(), &["checkout", "-q", "-"]);
        std::fs::write(dir.path().join("main.txt"), "main\n").unwrap();
        commit_all(dir.path(), "main commit");

        let repo = GitRepo::discover(dir.path()).unwrap();
        let base = repo
            .base_revision(&SeedMode::Revspec(format!("{root}...{feature}")))
            .unwrap();
        assert_eq!(base, root, "the merge base of root and feature is root itself");
    }

    // A bare revision resolves to itself, matching `git diff <commit>`'s own meaning.
    #[test]
    fn base_revision_bare_revision_resolves_to_itself() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let head = head_id(dir.path());
        let repo = GitRepo::discover(dir.path()).unwrap();

        assert_eq!(
            repo.base_revision(&SeedMode::Revspec("HEAD".to_string())).unwrap(),
            head
        );
    }

    // A garbage spec, and a spec beginning with `-`, are both rejected as `BadRevspec` rather than
    // reaching git as a flag or an unresolvable name.
    #[test]
    fn base_revision_rejects_garbage_and_leading_dash_specs() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let repo = GitRepo::discover(dir.path()).unwrap();

        assert!(matches!(
            repo.base_revision(&SeedMode::Revspec("not-a-revision-at-all".to_string())),
            Err(GitError::BadRevspec(_))
        ));
        assert!(matches!(
            repo.base_revision(&SeedMode::Revspec("--force".to_string())),
            Err(GitError::BadRevspec(_))
        ));
        // The guard is per-endpoint: a range hides a dash-prefixed endpoint from a whole-string check,
        // so both the resolution and the diff invocation must refuse it.
        let smuggled = SeedMode::Revspec("HEAD..--upload-pack=evil".to_string());
        assert!(matches!(repo.base_revision(&smuggled), Err(GitError::BadRevspec(_))));
        assert!(matches!(repo.diff(&smuggled), Err(GitError::BadRevspec(_))));
    }

    // A `WorkingTree` diff over an edited tracked file contains a `@@` hunk header naming that file.
    #[test]
    fn diff_working_tree_contains_a_hunk_for_the_edited_file() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("file.txt"), "one\n").unwrap();
        commit_all(dir.path(), "add file");
        std::fs::write(dir.path().join("file.txt"), "one\ntwo\n").unwrap();

        let repo = GitRepo::discover(dir.path()).unwrap();
        let patch = repo.diff(&SeedMode::WorkingTree).unwrap();
        assert!(patch.contains("file.txt"), "{patch}");
        assert!(patch.contains("@@"), "{patch}");
    }

    // `Staged` mode over a repository with one staged and one unstaged edit mentions only the staged
    // file.
    #[test]
    fn diff_staged_mentions_only_the_staged_file() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("staged.txt"), "one\n").unwrap();
        std::fs::write(dir.path().join("unstaged.txt"), "one\n").unwrap();
        commit_all(dir.path(), "add both files");

        std::fs::write(dir.path().join("staged.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.path().join("unstaged.txt"), "one\ntwo\n").unwrap();
        git(dir.path(), &["add", "staged.txt"]);

        let repo = GitRepo::discover(dir.path()).unwrap();
        let patch = repo.diff(&SeedMode::Staged).unwrap();
        assert!(patch.contains("staged.txt"), "{patch}");
        assert!(!patch.contains("unstaged.txt"), "{patch}");
    }

    // `show` returns the pre-change content of a modified file, and `None` for a path absent at that
    // revision — the disambiguating `cat-file -e` reports that path as genuinely missing rather than
    // an operational failure. The reverse arm (`show` fails on a path that `cat-file -e` confirms
    // exists, surfacing `GitError::Failed`) is exercised separately, below.
    #[test]
    fn show_returns_pre_change_content_and_none_for_an_absent_path() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("file.txt"), "before\n").unwrap();
        commit_all(dir.path(), "add file");
        let before = head_id(dir.path());
        std::fs::write(dir.path().join("file.txt"), "after\n").unwrap();
        commit_all(dir.path(), "edit file");

        let repo = GitRepo::discover(dir.path()).unwrap();
        let content = repo.show(&before, "file.txt").unwrap();
        assert_eq!(content, Some("before\n".to_string()));

        let absent = repo.show(&before, "does-not-exist.txt").unwrap();
        assert_eq!(absent, None);
    }

    /// Set when this test binary has been re-executed solely to run the operational-failure
    /// scenario below, so the scenario knows it is safe to narrow this process's own `PATH`.
    ///
    /// `GitRepo` resolves `git` from this process's ambient `PATH` (a `Command`-scoped `.env()`
    /// override does not affect executable resolution on Unix, only the spawned child's own
    /// environment), so exercising a stub `git` cannot be done by mutating this test binary's
    /// `PATH` in place — every other test in this binary that shells out to the real `git` could
    /// run concurrently and would observe the mutated `PATH` too. Instead, the scenario re-executes
    /// this same test binary, filtered to just this one test, with this marker set: the child
    /// process has no sibling tests to race, so narrowing its `PATH` is safe.
    const SHOW_OPERATIONAL_FAILURE_CHILD: &str = "C10R_TEST_SHOW_OPERATIONAL_FAILURE_CHILD";

    // `show`'s own non-zero exit is ambiguous between a path genuinely absent at that revision and
    // an operational failure (a corrupt object, an unreadable file, a permission failure); it
    // disambiguates with `git cat-file -e`. When `show` fails but `cat-file -e` confirms the object
    // exists, the result must be `GitError::Failed`, never `Ok(None)` — a broken repository must
    // never be dressed up as a set of merely-unresolvable regions. See
    // `SHOW_OPERATIONAL_FAILURE_CHILD` above for why this runs in a re-executed child process.
    #[cfg(unix)]
    #[test]
    fn show_reports_operational_failure_when_show_fails_but_the_object_exists() {
        if std::env::var(SHOW_OPERATIONAL_FAILURE_CHILD).is_ok() {
            run_show_operational_failure_scenario();
            return;
        }

        let exe = std::env::current_exe().expect("the test binary has a resolvable path");
        let output = StdCommand::new(exe)
            .arg("--exact")
            .arg("git::tests::show_reports_operational_failure_when_show_fails_but_the_object_exists")
            .env(SHOW_OPERATIONAL_FAILURE_CHILD, "1")
            .output()
            .expect("re-executing the test binary");
        assert!(
            output.status.success(),
            "the re-executed scenario failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Builds a repository with the real `git`, then narrows this process's own `PATH` to a stub
    /// `git` that fails `show` but succeeds `cat-file -e`, and asserts `show` reports
    /// `GitError::Failed` rather than `Ok(None)`. Only safe to call from the re-executed child
    /// process described at [`SHOW_OPERATIONAL_FAILURE_CHILD`].
    #[cfg(unix)]
    fn run_show_operational_failure_scenario() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("file.txt"), "before\n").unwrap();
        commit_all(dir.path(), "add file");
        let before = head_id(dir.path());
        let repo = GitRepo::discover(dir.path()).unwrap();

        let bin_dir = tempfile::tempdir().unwrap();
        write_show_operational_failure_git_stub(bin_dir.path());
        // SAFETY: this process was re-executed solely to run this one test (see
        // `SHOW_OPERATIONAL_FAILURE_CHILD`), so no concurrently running test observes this change.
        unsafe {
            std::env::set_var("PATH", bin_dir.path());
        }

        let result = repo.show(&before, "file.txt");
        assert!(
            matches!(result, Err(GitError::Failed { .. })),
            "an operational failure on show's own path must surface as GitError::Failed, not {result:?}"
        );
    }

    /// Writes a stub `git` (following the PATH-stub idiom in
    /// `tests/impact.rs::write_hanging_git_stub`) that fails `show` — as if the object were
    /// unreadable — but succeeds `cat-file -e` — as if the object exists — so the two outcomes
    /// `show` must disambiguate are reproduced deterministically, without a corrupted object store.
    #[cfg(unix)]
    fn write_show_operational_failure_git_stub(dir: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join("git");
        std::fs::write(
            &path,
            "#!/bin/sh\ncase \"$1\" in\n  show) echo 'fatal: stub operational failure' >&2; exit 128 ;;\n  cat-file) exit 0 ;;\n  *) exit 1 ;;\nesac\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
    }

    // A configured textconv driver does not reach the patch: the hunk line numbers must describe the
    // file's real bytes, because the byte offsets they are mapped to come from `show`, which applies
    // no such conversion. A driver that changed a file's line count would otherwise put every seed on
    // the wrong declaration with nothing in the answer to disclose it.
    #[test]
    fn diff_ignores_a_configured_textconv_driver() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("f.rs"), "line1\nline2\n").unwrap();
        std::fs::write(dir.path().join(".gitattributes"), "*.rs diff=mangle\n").unwrap();
        commit_all(dir.path(), "add file with a textconv attribute");
        git(
            dir.path(),
            &["config", "diff.mangle.textconv", "sed -e s/line/MANGLED/"],
        );
        std::fs::write(dir.path().join("f.rs"), "line1\nline2-edited\n").unwrap();

        let repo = GitRepo::discover(dir.path()).unwrap();
        let patch = repo.diff(&SeedMode::WorkingTree).unwrap();
        assert!(
            patch.contains("line2-edited"),
            "the patch carries the file's real text: {patch}"
        );
        assert!(
            !patch.contains("MANGLED"),
            "the textconv driver's output must not reach the patch: {patch}"
        );
    }

    // `tracked_files` lists a committed file and omits an untracked one.
    #[test]
    fn tracked_files_lists_committed_and_omits_untracked() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("tracked.txt"), "content\n").unwrap();
        commit_all(dir.path(), "add tracked");
        std::fs::write(dir.path().join("untracked.txt"), "content\n").unwrap();

        let repo = GitRepo::discover(dir.path()).unwrap();
        let files = repo.tracked_files().unwrap();
        assert!(files.contains(&"tracked.txt".to_string()), "{files:?}");
        assert!(!files.contains(&"untracked.txt".to_string()), "{files:?}");
    }

    // `unstaged_paths` lists a file with an unstaged edit but omits one whose edit is staged.
    #[test]
    fn unstaged_paths_lists_only_files_with_an_unstaged_edit() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("staged.txt"), "one\n").unwrap();
        std::fs::write(dir.path().join("unstaged.txt"), "one\n").unwrap();
        commit_all(dir.path(), "add both files");

        std::fs::write(dir.path().join("staged.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.path().join("unstaged.txt"), "one\ntwo\n").unwrap();
        git(dir.path(), &["add", "staged.txt"]);

        let repo = GitRepo::discover(dir.path()).unwrap();
        let paths = repo.unstaged_paths().unwrap();
        assert!(paths.contains(&"unstaged.txt".to_string()), "{paths:?}");
        assert!(!paths.contains(&"staged.txt".to_string()), "{paths:?}");
    }

    // `uncommitted_paths` lists a file whether its edit is staged or not — the union
    // `unstaged_paths` only reports half of.
    #[test]
    fn uncommitted_paths_lists_both_staged_and_unstaged_edits() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("staged.txt"), "one\n").unwrap();
        std::fs::write(dir.path().join("unstaged.txt"), "one\n").unwrap();
        commit_all(dir.path(), "add both files");

        std::fs::write(dir.path().join("staged.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.path().join("unstaged.txt"), "one\ntwo\n").unwrap();
        git(dir.path(), &["add", "staged.txt"]);

        let repo = GitRepo::discover(dir.path()).unwrap();
        let paths = repo.uncommitted_paths().unwrap();
        assert!(paths.contains(&"staged.txt".to_string()), "{paths:?}");
        assert!(paths.contains(&"unstaged.txt".to_string()), "{paths:?}");
    }

    // `range_head` resolves a two-dot and a three-dot range's right-hand endpoint, and reports
    // `None` for a bare single-revision spec.
    #[test]
    fn range_head_resolves_the_right_endpoint_and_none_for_a_bare_revision() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let root = head_id(dir.path());
        std::fs::write(dir.path().join("file.txt"), "content\n").unwrap();
        commit_all(dir.path(), "second");
        let head = head_id(dir.path());
        let repo = GitRepo::discover(dir.path()).unwrap();

        assert_eq!(
            repo.range_head(&SeedMode::Revspec(format!("{root}..HEAD"))).unwrap(),
            Some(head.clone())
        );
        assert_eq!(
            repo.range_head(&SeedMode::Revspec(format!("{root}...HEAD"))).unwrap(),
            Some(head)
        );
        assert_eq!(repo.range_head(&SeedMode::Revspec("HEAD".to_string())).unwrap(), None);
        assert_eq!(repo.range_head(&SeedMode::WorkingTree).unwrap(), None);
    }
}
