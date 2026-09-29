//! The generated hook file: the four git events c10r's refresh hooks handle, the script `render`
//! produces for one of them, and `parse`'s inverse read of that script back into the workspace lines
//! it carries. Also the recognizer that tells a c10r-owned hook slot apart from a supported hook
//! manager's shim, and the rules that decide whether a hook invocation should trigger a rebuild.

use std::io;
use std::path::{Path, PathBuf};

use clap::ValueEnum;

use crate::query::impact::shell_quote;

/// A git hook event that changes the checked-out commit, and that c10r's refresh hooks handle.
///
/// The value names clap derives (kebab-case of the variant name) are the git hook names themselves,
/// so a `HookEvent` parses directly from the name of the hook git invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HookEvent {
    /// Fires after a commit is created, including an amended commit.
    PostCommit,
    /// Fires after a rebase or another operation rewrites commits.
    PostRewrite,
    /// Fires after `git checkout` or `git switch` changes what is checked out, or restores files.
    PostCheckout,
    /// Fires after a merge completes, including a fast-forward or a squash merge.
    PostMerge,
}

impl HookEvent {
    /// Every refresh event, in the order the generated hook files are considered.
    pub const ALL: [HookEvent; 4] = [
        HookEvent::PostCommit,
        HookEvent::PostRewrite,
        HookEvent::PostCheckout,
        HookEvent::PostMerge,
    ];

    /// The git hook name this event corresponds to — the file name git invokes it under.
    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::PostCommit => "post-commit",
            HookEvent::PostRewrite => "post-rewrite",
            HookEvent::PostCheckout => "post-checkout",
            HookEvent::PostMerge => "post-merge",
        }
    }
}

/// One workspace's line in a rendered hook file: the root, index store, and workspace identity that
/// line's `c10r hooks run` invocation carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLine {
    /// The workspace's root directory, absolute.
    pub root: PathBuf,
    /// The index store path, absolute.
    pub db: PathBuf,
    /// The workspace identity.
    pub workspace: String,
}

/// The line that identifies a hook file as c10r's own, and that a hand-edited or foreign file will
/// not carry. `parse` treats its absence as "not a refresh hook" rather than attempting to read further.
const MARKER: &str = "# c10r refresh hook — managed by `c10r hooks install` and `c10r hooks uninstall`";

/// The suffix `render`, `read_event_files`, and the `Modified` fix text each append to an event's name
/// for the path a kept hook moves to and restores from.
const PRIOR_SUFFIX: &str = ".c10r-prior";

/// Render the hook file for `event`, carrying one `c10r hooks run` line per entry in `workspaces`.
///
/// The kept hook, `<event>.c10r-prior`, runs first when it is a regular file and executable, with the
/// script's own arguments and standard input; the script exits with the kept hook's status regardless
/// of what the refresh lines that follow it do, so a refresh can never change the outcome of the git
/// operation that ran it. `-f` guards `-x`, so a directory that happens to sit at that path (never
/// written by this module) is not mistaken for an executable hook.
///
/// Every value uses the `--flag=value` form, so a workspace identity or path that starts with `-` is
/// read as that flag's value rather than as another flag.
pub fn render(event: HookEvent, workspaces: &[WorkspaceLine]) -> String {
    let name = event.as_str();
    let mut script = format!(
        "#!/bin/sh\n{MARKER}\nstatus=0\nprior=\"$(dirname \"$0\")/{name}{PRIOR_SUFFIX}\"\nif [ -f \"$prior\" ] && [ \
         -x \"$prior\" ]; then \"$prior\" \"$@\"; status=$?; fi\n"
    );
    for line in workspaces {
        script.push_str(&format!(
            "c10r hooks run {name} --root={} --db={} --workspace={} -- \"$@\"\n",
            shell_quote(&line.root.display().to_string()),
            shell_quote(&line.db.display().to_string()),
            shell_quote(&line.workspace),
        ));
    }
    script.push_str("exit $status\n");
    script
}

/// Un-quote one shell-quoted word ([`shell_quote`]'s output) from the front of `s`, returning the
/// decoded value and the remainder of `s` after the closing quote.
///
/// Inverts `shell_quote` character by character: an embedded quote is encoded as the four-byte
/// sequence `'\''` (close, backslash-escaped literal quote, reopen), so a `'` found mid-word is only
/// the closing quote when it is *not* followed by that sequence.
fn unquote(s: &str) -> Option<(String, &str)> {
    let s = s.strip_prefix('\'')?;
    let mut out = String::new();
    let mut rest = s;
    loop {
        let mut chars = rest.char_indices();
        match chars.next() {
            None => return None,
            Some((_, '\'')) => {
                if let Some(escaped) = rest.strip_prefix("'\\''") {
                    out.push('\'');
                    rest = escaped;
                } else {
                    return Some((out, &rest[1..]));
                }
            }
            Some((_, c)) => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
}

/// Parse a rendered hook file back into its workspace lines, or `None` when the file does not carry
/// c10r's marker line — it was not written by `render`, or something else has rewritten it.
///
/// Lines that are not `c10r hooks run` invocations are ignored, so the shared preamble and the
/// trailing `exit $status` need not be matched here. A `c10r hooks run` line that does not match the
/// fixed layout `render` produces is likewise skipped rather than failing the whole parse.
pub fn parse(text: &str) -> Option<Vec<WorkspaceLine>> {
    let mut lines = text.lines();
    lines.next()?;
    if lines.next()? != MARKER {
        return None;
    }
    let workspaces = lines.filter_map(parse_workspace_line).collect();
    Some(workspaces)
}

/// Parse one `c10r hooks run <event> --root='<root>' --db='<db>' --workspace='<ws>' -- "$@"` line,
/// or `None` when `line` is not such an invocation.
fn parse_workspace_line(line: &str) -> Option<WorkspaceLine> {
    let rest = line.strip_prefix("c10r hooks run ")?;
    let (_event, rest) = rest.split_once(' ')?;
    let rest = rest.strip_prefix("--root=")?;
    let (root, rest) = unquote(rest)?;
    let rest = rest.strip_prefix(" --db=")?;
    let (db, rest) = unquote(rest)?;
    let rest = rest.strip_prefix(" --workspace=")?;
    let (workspace, _rest) = unquote(rest)?;
    Some(WorkspaceLine {
        root: PathBuf::from(root),
        db: PathBuf::from(db),
        workspace,
    })
}

/// The captured prek shim fixtures this build recognizes, one per event, each keyed to the prek
/// version whose `prek install` wrote it.
const PREK_0_5_3_POST_COMMIT: &str = include_str!("../tests/fixtures/prek-shims/0.5.3/post-commit");
const PREK_0_5_3_POST_REWRITE: &str = include_str!("../tests/fixtures/prek-shims/0.5.3/post-rewrite");
const PREK_0_5_3_POST_CHECKOUT: &str = include_str!("../tests/fixtures/prek-shims/0.5.3/post-checkout");
const PREK_0_5_3_POST_MERGE: &str = include_str!("../tests/fixtures/prek-shims/0.5.3/post-merge");

/// The captured shim forms for `event`, across every prek version this build recognizes.
fn prek_shim_forms(event: HookEvent) -> &'static [&'static str] {
    match event {
        HookEvent::PostCommit => &[PREK_0_5_3_POST_COMMIT],
        HookEvent::PostRewrite => &[PREK_0_5_3_POST_REWRITE],
        HookEvent::PostCheckout => &[PREK_0_5_3_POST_CHECKOUT],
        HookEvent::PostMerge => &[PREK_0_5_3_POST_MERGE],
    }
}

/// Whether `text` is a prek shim recognized for `event`: a whole-file match against a captured form
/// for that event, where the `# ID:` line and the `PREK="..."` path line are the only lines allowed
/// to differ in value.
///
/// Installing at `<event>.legacy` is correct only when the file at the event's path runs that file.
/// A whole-file match establishes that; file presence or header lines alone do not.
pub fn is_recognized_prek_shim(text: &str, event: HookEvent) -> bool {
    prek_shim_forms(event).iter().any(|form| matches_shim_form(text, form))
}

/// Whether `text` matches captured shim `form` line-for-line, allowing only the `# ID:` line and the
/// `PREK="..."` line to differ in value. A different line count, an inserted or removed line, or any
/// other line's text differing all fail the match.
fn matches_shim_form(text: &str, form: &str) -> bool {
    let mut text_lines = text.lines();
    let mut form_lines = form.lines();
    loop {
        match (text_lines.next(), form_lines.next()) {
            (Some(t), Some(f)) => {
                let matches = if f.starts_with("# ID: ") {
                    t.starts_with("# ID: ") && t.len() > "# ID: ".len()
                } else if f.starts_with("PREK=\"") && f.ends_with('"') {
                    t.starts_with("PREK=\"") && t.ends_with('"')
                } else {
                    t == f
                };
                if !matches {
                    return false;
                }
            }
            (None, None) => return true,
            _ => return false,
        }
    }
}

/// Whether a rebase is in progress in a worktree, and how many entries remain in its to-do list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebaseState {
    /// No rebase is in progress.
    None,
    /// A rebase is in progress, with `todo_entries` commits left to replay.
    InProgress {
        /// The number of `git-rebase-todo` lines that are neither blank nor a `#` comment.
        todo_entries: usize,
    },
}

/// Whether `hooks run <event> -- <git_args>` should build, given the git arguments the event fired
/// with and the worktree's current rebase state.
///
/// `post-rewrite amend` is excluded because `post-commit` already built for the amend; a squash merge
/// is excluded because it leaves the checked-out commit unchanged, and the commit that follows it
/// builds. With the apply backend, a rebase's `post-checkout` looks like an ordinary branch switch
/// and builds, and `post-rewrite rebase` then builds again — an accepted two-build bound for that
/// backend.
pub fn should_refresh(event: HookEvent, git_args: &[String], rebase: &RebaseState) -> bool {
    match event {
        HookEvent::PostCommit => matches!(rebase, RebaseState::None),
        HookEvent::PostRewrite => git_args.first().map(String::as_str) == Some("rebase"),
        HookEvent::PostCheckout => {
            git_args.get(2).map(String::as_str) == Some("1")
                && matches!(rebase, RebaseState::None | RebaseState::InProgress { todo_entries: 0 })
        }
        HookEvent::PostMerge => git_args.first().map(String::as_str) == Some("0"),
    }
}

/// One event's currently readable state in the hooks directory: the text at `E` and at `E.legacy`,
/// read once and shared by every check `install_plan` and `uninstall_plan` run against this event.
struct EventFiles {
    event: HookEvent,
    e_path: PathBuf,
    legacy_path: PathBuf,
    /// The path a kept hook always moves to and restores from, independent of which of `e_path` or
    /// `legacy_path` c10r's own file occupies.
    prior_path: PathBuf,
    e_text: Option<String>,
    legacy_text: Option<String>,
}

fn read_event_files(hooks_dir: &Path, event: HookEvent) -> EventFiles {
    let name = event.as_str();
    EventFiles {
        event,
        e_text: std::fs::read_to_string(hooks_dir.join(name)).ok(),
        legacy_text: std::fs::read_to_string(hooks_dir.join(format!("{name}.legacy"))).ok(),
        e_path: hooks_dir.join(name),
        legacy_path: hooks_dir.join(format!("{name}.legacy")),
        prior_path: hooks_dir.join(format!("{name}{PRIOR_SUFFIX}")),
    }
}

/// Where this event's refresh hook currently lives, and its parsed workspace lines, when one exists.
/// `E` is checked before `E.legacy`, so a refresh hook readable at both is only ever found at `E`.
fn located_c10r_file(files: &EventFiles) -> Option<(PathBuf, Vec<WorkspaceLine>)> {
    files
        .e_text
        .as_deref()
        .and_then(parse)
        .map(|ws| (files.e_path.clone(), ws))
        .or_else(|| {
            files
                .legacy_text
                .as_deref()
                .and_then(parse)
                .map(|ws| (files.legacy_path.clone(), ws))
        })
}

/// Whether two workspace-line lists name the same set, order aside.
fn same_workspace_set(a: &[WorkspaceLine], b: &[WorkspaceLine]) -> bool {
    a.len() == b.len() && a.iter().all(|line| b.contains(line))
}

/// `set` with every entry equal to `line` removed.
fn set_without(set: &[WorkspaceLine], line: &WorkspaceLine) -> Vec<WorkspaceLine> {
    set.iter().filter(|w| *w != line).cloned().collect()
}

/// What occupies a path installation might write to, without following a symbolic link past its own
/// target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotOccupant {
    /// Neither a file nor a symbolic link is there.
    Absent,
    /// A symbolic link is there, but its target does not exist.
    Dangling,
    /// A file is there, or a symbolic link to one.
    Present,
}

fn classify(path: &Path) -> SlotOccupant {
    match std::fs::symlink_metadata(path) {
        Err(_) => SlotOccupant::Absent,
        Ok(meta) if meta.file_type().is_symlink() => {
            if std::fs::metadata(path).is_ok() {
                SlotOccupant::Present
            } else {
                SlotOccupant::Dangling
            }
        }
        Ok(_) => SlotOccupant::Present,
    }
}

/// Whether the hook kept at `a` and the file now occupying a slot at `b` are the same hook: two
/// symbolic links naming the same target, or two regular files with the same bytes. Any other pairing
/// — including a link compared against a file — is different. Neither path is dereferenced past its
/// own `symlink_metadata`/`read_link`, so a link is never compared against the file it points to.
pub fn kept_hooks_identical(a: &Path, b: &Path) -> io::Result<bool> {
    let meta_a = std::fs::symlink_metadata(a)?;
    let meta_b = std::fs::symlink_metadata(b)?;
    Ok(
        match (meta_a.file_type().is_symlink(), meta_b.file_type().is_symlink()) {
            (true, true) => std::fs::read_link(a)? == std::fs::read_link(b)?,
            (false, false) => std::fs::read(a)? == std::fs::read(b)?,
            _ => false,
        },
    )
}

/// Where installation would place event `E`'s refresh hook, decided without regard to what already
/// occupies that path.
enum HookSlot {
    /// A refresh hook already exists here; installation edits it in place.
    Existing(PathBuf),
    /// No refresh hook exists yet; installation would place a new one here.
    Fresh(PathBuf),
}

/// Resolve one event's hook slot: a refresh hook already at `E` or `E.legacy` is edited in place;
/// otherwise, a new refresh hook's slot is `E.legacy` when the file at `E` is a recognized prek shim, and
/// `E` otherwise.
fn resolve_slot(files: &EventFiles, location: Option<&(PathBuf, Vec<WorkspaceLine>)>) -> HookSlot {
    if let Some((path, _)) = location {
        return HookSlot::Existing(path.clone());
    }
    if files
        .e_text
        .as_deref()
        .is_some_and(|text| is_recognized_prek_shim(text, files.event))
    {
        HookSlot::Fresh(files.legacy_path.clone())
    } else {
        HookSlot::Fresh(files.e_path.clone())
    }
}

/// Why `install_plan` refused to plan anything, carrying the paths a caller needs to resolve it.
#[derive(Debug)]
pub enum InstallRefusal {
    /// Present refresh hooks do not all name the same workspace set: each entry is one file's path and
    /// the workspaces it names.
    Disagreement(Vec<(PathBuf, Vec<WorkspaceLine>)>),
    /// A symbolic link with no existing target occupies the path installation would write to.
    DanglingSymlink(PathBuf),
    /// A hook kept by an earlier install remains at this path without a refresh hook for its event, and
    /// differs from the hook installation would now keep in its place.
    PriorCollision(PathBuf),
    /// The workspace root, index path, or workspace identity named by `&'static str` carries a line
    /// break, which the hook file's one-line-per-workspace format cannot represent.
    LineBreak(&'static str),
}

/// The message naming a hook kept from an earlier install that collides with the file now occupying
/// its slot, shared by [`InstallRefusal::PriorCollision`]'s display and `not_installed_fix`'s fix for
/// the same condition.
fn prior_collision_message(path: &Path) -> String {
    format!(
        "a hook kept from an earlier install remains at {}; remove it or rename it before installing",
        path.display()
    )
}

impl std::fmt::Display for InstallRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallRefusal::Disagreement(files) => {
                write!(f, "hook files disagree on the installed workspaces:")?;
                for (path, workspaces) in files {
                    let names: Vec<&str> = workspaces.iter().map(|line| line.workspace.as_str()).collect();
                    write!(f, "\n  {} names [{}]", path.display(), names.join(", "))?;
                }
                Ok(())
            }
            InstallRefusal::DanglingSymlink(path) => write!(
                f,
                "{} is a dangling symbolic link; refusing to write through it",
                path.display()
            ),
            InstallRefusal::PriorCollision(path) => write!(f, "{}", prior_collision_message(path)),
            InstallRefusal::LineBreak(field) => {
                write!(f, "{field} contains a line break, which a hook file cannot carry")
            }
        }
    }
}

/// Whether `install_plan` must refuse `line` before touching the hooks directory: its root, index
/// path, or workspace identity carries a line break, which would split across the hook file's
/// one-line-per-workspace format and silently corrupt the workspace line `parse` reads back.
fn line_break_refusal(line: &WorkspaceLine) -> Option<InstallRefusal> {
    let carries_line_break = |s: &str| s.contains('\n') || s.contains('\r');
    if carries_line_break(&line.root.display().to_string()) {
        return Some(InstallRefusal::LineBreak("the workspace root"));
    }
    if carries_line_break(&line.db.display().to_string()) {
        return Some(InstallRefusal::LineBreak("the index path"));
    }
    if carries_line_break(&line.workspace) {
        return Some(InstallRefusal::LineBreak("the workspace identity"));
    }
    None
}

impl std::error::Error for InstallRefusal {}

/// A refresh hook installation writes into its slot.
#[derive(Debug)]
pub struct HookWrite {
    /// The path the refresh hook is written to, and its rendered content.
    pub path: PathBuf,
    pub content: String,
    /// A hook moved to keep it running before the write: the path it moved from, and
    /// `<event>.c10r-prior`.
    pub kept: Option<(PathBuf, PathBuf)>,
}

/// One event's part of an install plan: what `apply_plan` will do at that event's hooks-directory
/// paths, and what a plan report shows for it.
#[derive(Debug)]
pub struct EventInstallPlan {
    pub event: HookEvent,
    /// The refresh hook installation writes — `None` only when a refresh hook already there carries the
    /// target workspace set unchanged.
    pub write: Option<HookWrite>,
    /// Paths this plan deletes.
    pub deleted: Vec<PathBuf>,
    /// Paths this plan leaves exactly as found.
    pub unchanged: Vec<PathBuf>,
}

/// A plan `apply_plan` can carry out without reading the hooks directory again, produced by
/// `install_plan`.
#[derive(Debug)]
pub struct InstallPlan {
    /// One entry per [`HookEvent::ALL`], in that order.
    pub events: Vec<EventInstallPlan>,
}

/// Plan `install`'s effect on `hooks_dir` for `line`, without writing anything.
///
/// Refuses before reading the hooks directory at all when `line`'s root, index path, or workspace
/// identity carries a line break. Otherwise reads all four events: when every present refresh hook names
/// the same workspace set, plans updating or creating each event's file with that set plus `line`
/// appended; a refresh hook whose content is already correct is planned unchanged rather than rewritten.
/// Present refresh hooks that differ only by `line` — what an installation that failed partway leaves —
/// are planned toward the set they share plus `line`.
/// Also refuses, naming what a caller must resolve, when present refresh hooks disagree otherwise, when the path
/// installation would write to is a dangling symbolic link, or when a hook kept by an earlier install
/// would collide with a different hook now in its place.
pub fn install_plan(hooks_dir: &Path, line: &WorkspaceLine, overwrite: bool) -> Result<InstallPlan, InstallRefusal> {
    if let Some(refusal) = line_break_refusal(line) {
        return Err(refusal);
    }
    let files: Vec<EventFiles> = HookEvent::ALL
        .iter()
        .map(|&event| read_event_files(hooks_dir, event))
        .collect();
    let located: Vec<Option<(PathBuf, Vec<WorkspaceLine>)>> = files.iter().map(located_c10r_file).collect();

    let present: Vec<(PathBuf, Vec<WorkspaceLine>)> = located.iter().filter_map(|l| l.clone()).collect();
    let mut target: Vec<WorkspaceLine> = present.first().map(|(_, ws)| ws.clone()).unwrap_or_default();
    if let Some((_, first)) = present.first()
        && present.iter().any(|(_, ws)| !same_workspace_set(first, ws))
    {
        target = set_without(first, line);
        if present
            .iter()
            .any(|(_, ws)| !same_workspace_set(&target, &set_without(ws, line)))
        {
            return Err(InstallRefusal::Disagreement(present));
        }
    }
    if !target.contains(line) {
        target.push(line.clone());
    }

    let events = files
        .iter()
        .zip(located.iter())
        .map(|(event_files, location)| plan_one_event(event_files, location.as_ref(), &target, overwrite))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(InstallPlan { events })
}

fn plan_one_event(
    files: &EventFiles,
    location: Option<&(PathBuf, Vec<WorkspaceLine>)>,
    target: &[WorkspaceLine],
    overwrite: bool,
) -> Result<EventInstallPlan, InstallRefusal> {
    let mut deleted = Vec::new();
    match resolve_slot(files, location) {
        HookSlot::Existing(path) => {
            let existing_text = if path == files.e_path {
                files.e_text.as_deref()
            } else {
                files.legacy_text.as_deref()
            };
            let new_content = render(files.event, target);
            let (write, unchanged) = if existing_text == Some(new_content.as_str()) {
                (None, vec![path.clone()])
            } else {
                (
                    Some(HookWrite {
                        path: path.clone(),
                        content: new_content,
                        kept: None,
                    }),
                    Vec::new(),
                )
            };
            if overwrite && std::fs::symlink_metadata(&files.prior_path).is_ok() {
                deleted.push(files.prior_path.clone());
            }
            Ok(EventInstallPlan {
                event: files.event,
                write,
                deleted,
                unchanged,
            })
        }
        HookSlot::Fresh(slot) => {
            let slot_occupant = classify(&slot);
            if slot_occupant == SlotOccupant::Dangling {
                return Err(InstallRefusal::DanglingSymlink(slot));
            }
            let prior_exists = std::fs::symlink_metadata(&files.prior_path).is_ok();

            let mut kept = None;
            let mut unchanged = Vec::new();
            if overwrite {
                if slot_occupant == SlotOccupant::Present {
                    deleted.push(slot.clone());
                }
                if prior_exists {
                    deleted.push(files.prior_path.clone());
                }
            } else {
                if prior_exists
                    && slot_occupant == SlotOccupant::Present
                    && !kept_hooks_identical(&files.prior_path, &slot).unwrap_or(false)
                {
                    return Err(InstallRefusal::PriorCollision(files.prior_path.clone()));
                }
                if slot_occupant == SlotOccupant::Present {
                    kept = Some((slot.clone(), files.prior_path.clone()));
                } else if prior_exists {
                    // Another tool removed c10r's file, but the hook it originally kept is still here
                    // with nothing occupying the slot to collide with it; leave it to run again
                    // alongside the file the write below restores.
                    unchanged.push(files.prior_path.clone());
                }
            }

            Ok(EventInstallPlan {
                event: files.event,
                write: Some(HookWrite {
                    path: slot,
                    content: render(files.event, target),
                    kept,
                }),
                deleted,
                unchanged,
            })
        }
    }
}

/// Stage `content` for a later hook write: create a uniquely named temporary file in `hooks_dir`,
/// write `content` to it, and make it executable — without touching any other path in `hooks_dir`.
///
/// This is the fallible step in writing a hook file, done first and in full before anything already
/// in `hooks_dir` is moved or deleted, so a failure here — a filesystem error partway through the
/// write, say — leaves every existing file exactly as found. The caller commits the staged file into
/// place with `std::fs::rename`, or removes it if a later step in the same event fails first.
fn stage_hook_write(hooks_dir: &Path, content: &str) -> io::Result<PathBuf> {
    use std::io::Write;

    let mut attempt: u64 = 0;
    let tmp_path = loop {
        let candidate = hooks_dir.join(format!(".c10r-hook-{}-{attempt}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                file.write_all(content.as_bytes())?;
                break candidate;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
            Err(e) => return Err(e),
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(tmp_path)
}

/// Carry out `plan`, creating `hooks_dir` first if it does not exist.
///
/// Every event's refresh hook is staged to a temporary path first — the fallible step — so a staging
/// failure removes the temporary files and changes nothing else. Then each event in turn runs its
/// deletions, the kept-hook move (`std::fs::rename`, which preserves a symbolic link as a link), and
/// finally renames its staged file into its slot. A failure at any of those steps removes the
/// remaining temporary files, and a failed final rename moves the kept hook back to the path it was
/// found at, so a kept hook is never left disconnected from its event.
pub fn apply_plan(hooks_dir: &Path, plan: &InstallPlan) -> io::Result<()> {
    apply_plan_staging(hooks_dir, plan, stage_hook_write)
}

fn apply_plan_staging(
    hooks_dir: &Path,
    plan: &InstallPlan,
    mut stage: impl FnMut(&Path, &str) -> io::Result<PathBuf>,
) -> io::Result<()> {
    std::fs::create_dir_all(hooks_dir)?;
    let mut staged: Vec<Option<PathBuf>> = Vec::new();
    let mut result = Ok(());
    for event_plan in &plan.events {
        match event_plan.write.as_ref().map(|write| stage(hooks_dir, &write.content)) {
            Some(Err(e)) => {
                result = Err(e);
                break;
            }
            Some(Ok(path)) => staged.push(Some(path)),
            None => staged.push(None),
        }
    }
    if result.is_ok() {
        for (event_plan, staged_path) in plan.events.iter().zip(&staged) {
            result = apply_one_event_install(event_plan, staged_path.as_deref());
            if result.is_err() {
                break;
            }
        }
    }
    if result.is_err() {
        for path in staged.iter().flatten() {
            let _ = std::fs::remove_file(path);
        }
    }
    result
}

fn apply_one_event_install(event_plan: &EventInstallPlan, staged: Option<&Path>) -> io::Result<()> {
    for path in &event_plan.deleted {
        std::fs::remove_file(path)?;
    }
    let Some(write) = &event_plan.write else {
        return Ok(());
    };
    if let Some((from, to)) = &write.kept {
        std::fs::rename(from, to)?;
    }
    if let Some(staged) = staged
        && let Err(e) = std::fs::rename(staged, &write.path)
    {
        if let Some((from, to)) = &write.kept {
            let _ = std::fs::rename(to, from);
        }
        return Err(e);
    }
    Ok(())
}

/// One event's part of an uninstall plan: what `apply_uninstall_plan` will do at that event's c10r
/// file, if any names the removed workspace.
#[derive(Debug)]
pub struct EventUninstallPlan {
    pub event: HookEvent,
    pub action: Option<UninstallAction>,
}

/// What `apply_uninstall_plan` does for one event whose refresh hook names the removed workspace.
#[derive(Debug)]
pub enum UninstallAction {
    /// Other workspaces remain in the refresh hook at `path`; rewrite it without the removed line.
    Rewrite { path: PathBuf, content: String },
    /// The removed line was the file's last workspace: delete the refresh hook at `path`, and, if a hook
    /// was kept from an earlier install, move it back into `path`.
    Remove { path: PathBuf, restore: Option<PathBuf> },
}

/// A plan `apply_uninstall_plan` can carry out without reading the hooks directory again, produced by
/// `uninstall_plan`.
#[derive(Debug)]
pub struct UninstallPlan {
    /// One entry per [`HookEvent::ALL`], in that order.
    pub events: Vec<EventUninstallPlan>,
}

/// Plan `uninstall`'s effect on `hooks_dir` for `line`: for each event whose refresh hook names `line`,
/// remove it from that file — whether or not the hooks directory's files agree with each other. An
/// event whose refresh hook does not name `line`, including one `parse` does not recognize as a refresh
/// hook at all, is left untouched.
pub fn uninstall_plan(hooks_dir: &Path, line: &WorkspaceLine) -> UninstallPlan {
    let events = HookEvent::ALL
        .iter()
        .map(|&event| {
            let files = read_event_files(hooks_dir, event);
            let action = located_c10r_file(&files).and_then(|(path, workspaces)| {
                if !workspaces.contains(line) {
                    return None;
                }
                let remaining: Vec<WorkspaceLine> = workspaces.into_iter().filter(|w| w != line).collect();
                Some(if remaining.is_empty() {
                    let restore = std::fs::symlink_metadata(&files.prior_path)
                        .is_ok()
                        .then_some(files.prior_path.clone());
                    UninstallAction::Remove { path, restore }
                } else {
                    UninstallAction::Rewrite {
                        content: render(event, &remaining),
                        path,
                    }
                })
            });
            EventUninstallPlan { event, action }
        })
        .collect();
    UninstallPlan { events }
}

/// Carry out `plan`.
///
/// A rewrite stages its new content before touching `path`, the same order `apply_plan` uses. A
/// removal with a kept hook to restore renames that hook directly over `path` — an atomic replace —
/// rather than deleting `path` first, so a failure restoring it can never leave the event with no file
/// at all.
pub fn apply_uninstall_plan(hooks_dir: &Path, plan: &UninstallPlan) -> io::Result<()> {
    for event_plan in &plan.events {
        match &event_plan.action {
            None => {}
            Some(UninstallAction::Rewrite { path, content }) => {
                let staged = stage_hook_write(hooks_dir, content)?;
                std::fs::rename(staged, path)?;
            }
            Some(UninstallAction::Remove {
                path,
                restore: Some(prior_path),
            }) => {
                std::fs::rename(prior_path, path)?;
            }
            Some(UninstallAction::Remove { path, restore: None }) => std::fs::remove_file(path)?,
        }
    }
    Ok(())
}

/// One event's refresh-hook state, as reported by [`hook_status`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookState {
    /// A c10r refresh hook runs for this event, refreshing `workspaces`, byte-for-byte as `install`
    /// would write it: its refresh hook is executable, and is at the event's own path, or at the legacy
    /// path while the event's own path is an executable recognized hook-manager shim that runs it.
    Installed { path: PathBuf, workspaces: Vec<String> },
    /// The file git runs for this event — same reachability as `Installed` — names workspaces, but its
    /// content differs from what `install` would write for them, most commonly a command added by
    /// hand. An edit can stop the refresh lines from running, so `workspaces_named` are not reported
    /// as refreshed.
    Modified {
        path: PathBuf,
        workspaces_named: Vec<String>,
        /// States that installation and removal replace this file, and where a caller's own commands
        /// belong instead.
        note: String,
    },
    /// A refresh hook is at the legacy path, but the file at the event's own path is not one this build
    /// recognizes as reaching it — including no file at all. Whether the legacy file ever runs cannot
    /// be confirmed, so `workspaces_named` are not reported as refreshed.
    Unverified {
        path: PathBuf,
        workspaces_named: Vec<String>,
    },
    /// No refresh hook runs for this event: no refresh hook exists at either path, or one does but git
    /// will not run it (not executable, or reached only through a shim or legacy file that is not
    /// itself executable). `fix` is the action that resolves it.
    NotInstalled { fix: String },
}

/// One event's status, as reported by [`hook_status`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventHookStatus {
    pub event: HookEvent,
    pub state: HookState,
}

/// The refresh-hook status of every event in a hooks directory, as `doctor` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookStatusReport {
    /// One entry per [`HookEvent::ALL`], in that order.
    pub events: Vec<EventHookStatus>,
    /// Whether the events' present refresh hooks do not all name the same workspace set — the same
    /// check [`install_plan`] refuses on.
    pub disagreement: bool,
}

/// Report each event's refresh-hook status in `hooks_dir`, reading it but changing nothing.
///
/// See [`HookState`] for what distinguishes `installed`, `modified`, `unverified`, and
/// `not_installed`.
pub fn hook_status(hooks_dir: &Path) -> HookStatusReport {
    let files: Vec<EventFiles> = HookEvent::ALL
        .iter()
        .map(|&event| read_event_files(hooks_dir, event))
        .collect();
    let located: Vec<Option<(PathBuf, Vec<WorkspaceLine>)>> = files.iter().map(located_c10r_file).collect();

    let present: Vec<&Vec<WorkspaceLine>> = located.iter().filter_map(|l| l.as_ref().map(|(_, ws)| ws)).collect();
    let disagreement = present
        .first()
        .is_some_and(|first| present.iter().any(|ws| !same_workspace_set(first, ws)));

    // A `NotInstalled` event's suggested install command reuses the first workspace line any other
    // event's c10r file already names, in event order, so it carries the root, db, and workspace
    // already in effect rather than guessing at them.
    let install_line = located.iter().flatten().flat_map(|(_, ws)| ws.iter()).next();
    let install_command = suggest_install_command(install_line);

    let events = files
        .iter()
        .zip(located.iter())
        .map(|(event_files, location)| EventHookStatus {
            event: event_files.event,
            state: event_hook_state(event_files, location.as_ref(), &install_command),
        })
        .collect();
    HookStatusReport { events, disagreement }
}

/// The install command a `NotInstalled` event's fix suggests: `line`'s root, db, and workspace when
/// some other event's refresh hook already names one, so the suggestion matches what is already
/// installed; otherwise the plain install command, run from the workspace root.
fn suggest_install_command(line: Option<&WorkspaceLine>) -> String {
    match line {
        Some(line) => format!(
            "cd {} && c10r hooks install --db={} --workspace={}",
            shell_quote(&line.root.display().to_string()),
            shell_quote(&line.db.display().to_string()),
            shell_quote(&line.workspace),
        ),
        None => "c10r hooks install".to_string(),
    }
}

/// One event's state for [`hook_status`].
fn event_hook_state(
    files: &EventFiles,
    location: Option<&(PathBuf, Vec<WorkspaceLine>)>,
    install_command: &str,
) -> HookState {
    let Some((path, workspaces)) = location else {
        return HookState::NotInstalled {
            fix: not_installed_fix(files, install_command),
        };
    };
    let workspace_names: Vec<String> = workspaces.iter().map(|w| w.workspace.clone()).collect();
    if *path == files.e_path {
        if !is_executable(&files.e_path) {
            return HookState::NotInstalled {
                fix: not_executable_fix(&files.e_path),
            };
        }
        return located_state(files, path, workspaces, workspace_names);
    }
    let under_recognized_shim = files
        .e_text
        .as_deref()
        .is_some_and(|text| is_recognized_prek_shim(text, files.event));
    if !under_recognized_shim {
        return HookState::Unverified {
            path: path.clone(),
            workspaces_named: workspace_names,
        };
    }
    if !is_executable(&files.e_path) {
        return HookState::NotInstalled {
            fix: not_executable_fix(&files.e_path),
        };
    }
    if !is_executable(&files.legacy_path) {
        return HookState::NotInstalled {
            fix: not_executable_fix(&files.legacy_path),
        };
    }
    located_state(files, path, workspaces, workspace_names)
}

/// Whether git would run the file at `path`: present, a regular file (following a symbolic link), and
/// — on a platform with the concept — carrying the executable bit. A platform with no executable-bit
/// concept reads any regular file as runnable, matching [`stage_hook_write`]'s own unix-only `chmod`.
fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// `Installed` when the file git runs for this event at `path` carries exactly what `install` would
/// write for `workspaces`; `Modified` when it carries something else — a hand edit, most commonly —
/// naming `workspace_names` without asserting they are refreshed.
fn located_state(
    files: &EventFiles,
    path: &Path,
    workspaces: &[WorkspaceLine],
    workspace_names: Vec<String>,
) -> HookState {
    let current_text = if path == files.e_path {
        files.e_text.as_deref()
    } else {
        files.legacy_text.as_deref()
    };
    if current_text == Some(render(files.event, workspaces).as_str()) {
        return HookState::Installed {
            path: path.to_path_buf(),
            workspaces: workspace_names,
        };
    }
    HookState::Modified {
        path: path.to_path_buf(),
        workspaces_named: workspace_names,
        note: format!(
            "installation and removal replace this file; put commands of your own in {}{PRIOR_SUFFIX}",
            files.event.as_str()
        ),
    }
}

/// The fix `hook_status` reports for an event whose refresh hook exists but is not one git will run: the
/// path git resolves for the event, or the shim or legacy file above it, is not executable.
fn not_executable_fix(path: &Path) -> String {
    format!(
        "{} exists but is not executable; git will not run it — make it executable",
        path.display()
    )
}

/// The fix `hook_status` reports for an event with no refresh hook at either path: the path of a kept
/// hook and the need to remove or rename it, when one remains and differs from the file now occupying
/// the slot installation would use — the same collision [`install_plan`] refuses on — or the install
/// command otherwise.
fn not_installed_fix(files: &EventFiles, install_command: &str) -> String {
    let HookSlot::Fresh(slot) = resolve_slot(files, None) else {
        unreachable!(
            "location is None at every not_installed_fix call site, so resolve_slot never finds an existing c10r file"
        )
    };
    let prior_exists = std::fs::symlink_metadata(&files.prior_path).is_ok();
    let slot_present = classify(&slot) == SlotOccupant::Present;
    if prior_exists && slot_present && !kept_hooks_identical(&files.prior_path, &slot).unwrap_or(false) {
        prior_collision_message(&files.prior_path)
    } else {
        install_command.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // A `render`ed file, `parse`d back, yields the same workspace lines it was given — including
    // values a shell word cannot carry unquoted, since those are exactly the values `shell_quote` and
    // its inverse exist for.
    #[test]
    fn render_then_parse_round_trips_workspace_lines_with_spaces_and_quotes() {
        let workspaces = vec![
            WorkspaceLine {
                root: PathBuf::from("/home/dev/my project"),
                db: PathBuf::from("/home/dev/my project/.c10r/index.db"),
                workspace: "my project".to_string(),
            },
            WorkspaceLine {
                root: PathBuf::from("/home/dev/o'brien's repo"),
                db: PathBuf::from("/home/dev/o'brien's repo/.c10r/index.db"),
                workspace: "o'brien's repo".to_string(),
            },
        ];
        let script = render(HookEvent::PostCommit, &workspaces);
        assert_eq!(parse(&script), Some(workspaces));
    }

    // A file without the marker line — never rendered by this module, or rewritten by something else
    // — is not a c10r file at all, so `parse` reports that rather than guessing at partial content.
    #[test]
    fn parse_returns_none_without_the_marker_line() {
        assert_eq!(parse("#!/bin/sh\necho not-a-c10r-hook\n"), None);
    }

    // The rendered script chains through the kept hook before it refreshes anything, and the script's
    // own exit status is the kept hook's — never the refresh's — so a refresh can never change the
    // outcome of the git operation that ran it.
    #[cfg(unix)]
    #[test]
    fn rendered_script_runs_the_kept_hook_before_refreshing_and_exits_with_its_status() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");

        let bin_dir = dir.path().join("bin");
        std::fs::create_dir(&bin_dir).unwrap();
        let stub_c10r = bin_dir.join("c10r");
        std::fs::write(
            &stub_c10r,
            format!(
                "#!/bin/sh\necho \"c10r $*\" >> {}\n",
                shell_quote(&log.display().to_string())
            ),
        )
        .unwrap();
        make_executable(&stub_c10r);

        let prior = dir.path().join("post-checkout.c10r-prior");
        std::fs::write(
            &prior,
            format!(
                "#!/bin/sh\necho prior >> {}\nexit 3\n",
                shell_quote(&log.display().to_string())
            ),
        )
        .unwrap();
        make_executable(&prior);

        let line = WorkspaceLine {
            root: PathBuf::from("/repo"),
            db: PathBuf::from("/repo/.c10r/index.db"),
            workspace: "repo".to_string(),
        };
        let hook_path = dir.path().join("post-checkout");
        std::fs::write(&hook_path, render(HookEvent::PostCheckout, &[line])).unwrap();
        make_executable(&hook_path);

        let mut path_value = std::ffi::OsString::from(bin_dir.as_os_str());
        path_value.push(":");
        path_value.push(std::env::var_os("PATH").unwrap_or_default());

        let status = std::process::Command::new("sh")
            .arg(&hook_path)
            .args(["prev", "new", "1"])
            .env("PATH", path_value)
            .status()
            .unwrap();

        assert_eq!(status.code(), Some(3), "the script exits with the kept hook's status");
        let recorded = std::fs::read_to_string(&log).unwrap();
        let recorded_lines: Vec<&str> = recorded.lines().collect();
        assert_eq!(
            recorded_lines.first(),
            Some(&"prior"),
            "the kept hook runs first: {recorded}"
        );
        assert!(
            recorded_lines.get(1).is_some_and(|l| l.starts_with("c10r ")),
            "the refresh line runs after the kept hook: {recorded}"
        );
    }

    /// The four captured fixtures, paired with the event each was captured for.
    fn fixtures() -> [(HookEvent, &'static str); 4] {
        [
            (HookEvent::PostCommit, PREK_0_5_3_POST_COMMIT),
            (HookEvent::PostRewrite, PREK_0_5_3_POST_REWRITE),
            (HookEvent::PostCheckout, PREK_0_5_3_POST_CHECKOUT),
            (HookEvent::PostMerge, PREK_0_5_3_POST_MERGE),
        ]
    }

    // Each captured fixture is recognized for the event it was captured for, and for no other event —
    // the `--hook-type=<event>` line it delegates through is exact-matched, not just present.
    #[test]
    fn each_fixture_is_recognized_only_for_its_own_event() {
        for (event, text) in fixtures() {
            assert!(
                is_recognized_prek_shim(text, event),
                "{:?} recognizes its own fixture",
                event
            );
            for (other_event, _) in fixtures() {
                if other_event != event {
                    assert!(
                        !is_recognized_prek_shim(text, other_event),
                        "{:?}'s fixture is not recognized as {:?}",
                        event,
                        other_event
                    );
                }
            }
        }
    }

    // A fixture with a different `# ID:` value and a different `PREK=` path is still recognized —
    // those are exactly the two lines the recognizer allows to vary.
    #[test]
    fn a_fixture_with_a_different_id_and_prek_path_is_recognized() {
        let text = PREK_0_5_3_POST_COMMIT
            .replace(
                "# ID: 182c10f181da4464a3eec51b83331688",
                "# ID: ffffffffffffffffffffffffffffffff",
            )
            .replace(
                "PREK=\"/Users/mithras/.local/bin/prek\"",
                "PREK=\"/opt/homebrew/bin/prek\"",
            );
        assert!(is_recognized_prek_shim(&text, HookEvent::PostCommit));
    }

    // A file with only prek's header lines — no delegation to `<event>.legacy` at all — is not a
    // supported hook manager's shim: recognizing it would place c10r at a path prek's own shim never
    // runs.
    #[test]
    fn a_file_with_only_the_prek_header_is_not_recognized() {
        let header = "#!/bin/sh\n# File generated by prek: https://github.com/j178/prek\n# ID: 182c10f181da4464a3eec51b83331688\n";
        assert!(!is_recognized_prek_shim(header, HookEvent::PostCommit));
    }

    // A fixture missing the `exec … hook-impl` delegation line no longer runs `<event>.legacy` at
    // all, so it must not be recognized.
    #[test]
    fn a_fixture_missing_the_delegation_line_is_not_recognized() {
        let text: String = PREK_0_5_3_POST_COMMIT
            .lines()
            .filter(|line| !line.starts_with("exec "))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!is_recognized_prek_shim(&text, HookEvent::PostCommit));
    }

    // A fixture with one extra line is not a whole-file match, whatever the extra line says.
    #[test]
    fn a_fixture_with_an_extra_line_is_not_recognized() {
        let text = format!("{PREK_0_5_3_POST_COMMIT}# an extra line\n");
        assert!(!is_recognized_prek_shim(&text, HookEvent::PostCommit));
    }

    // post-commit: builds outside a rebase, not during one — a rebase's own commits must not each
    // trigger a build; the rebase's completion (post-rewrite or the checkout that lands it) does.
    #[test]
    fn post_commit_builds_only_outside_a_rebase() {
        assert!(should_refresh(HookEvent::PostCommit, &[], &RebaseState::None));
        assert!(!should_refresh(
            HookEvent::PostCommit,
            &[],
            &RebaseState::InProgress { todo_entries: 3 }
        ));
    }

    // post-rewrite: builds when git names the operation `rebase`, not for an `amend` — `post-commit`
    // already built for the amend, so building again would be a second build for the same change.
    #[test]
    fn post_rewrite_builds_only_for_rebase_not_amend() {
        assert!(should_refresh(
            HookEvent::PostRewrite,
            &["rebase".to_string()],
            &RebaseState::None
        ));
        assert!(!should_refresh(
            HookEvent::PostRewrite,
            &["amend".to_string()],
            &RebaseState::None
        ));
    }

    // post-checkout: the third argument is the branch-checkout flag; a flag of `0` (a file checkout,
    // not a commit change) never builds regardless of rebase state.
    #[test]
    fn post_checkout_flag_zero_never_builds() {
        let args = vec!["a".to_string(), "b".to_string(), "0".to_string()];
        assert!(!should_refresh(HookEvent::PostCheckout, &args, &RebaseState::None));
    }

    // post-checkout with the flag set and no rebase in progress: an ordinary branch switch, and it
    // builds.
    #[test]
    fn post_checkout_flag_one_outside_a_rebase_builds() {
        let args = vec!["a".to_string(), "b".to_string(), "1".to_string()];
        assert!(should_refresh(HookEvent::PostCheckout, &args, &RebaseState::None));
    }

    // post-checkout with the flag set during a rebase that still has commits to replay: this
    // checkout is not the rebase's last one, so it must not build yet.
    #[test]
    fn post_checkout_flag_one_mid_rebase_with_entries_does_not_build() {
        let args = vec!["a".to_string(), "b".to_string(), "1".to_string()];
        assert!(!should_refresh(
            HookEvent::PostCheckout,
            &args,
            &RebaseState::InProgress { todo_entries: 2 }
        ));
    }

    // post-checkout with the flag set during a rebase with nothing left to replay: this is the
    // rebase's landing checkout, so it builds.
    #[test]
    fn post_checkout_flag_one_mid_rebase_with_no_entries_builds() {
        let args = vec!["a".to_string(), "b".to_string(), "1".to_string()];
        assert!(should_refresh(
            HookEvent::PostCheckout,
            &args,
            &RebaseState::InProgress { todo_entries: 0 }
        ));
    }

    // post-merge: the first argument is the squash flag. `0` is an ordinary merge that moved HEAD, so
    // it builds; `1` is a squash merge, which leaves HEAD unchanged, so it does not.
    #[test]
    fn post_merge_builds_on_ordinary_merge_not_squash() {
        assert!(should_refresh(
            HookEvent::PostMerge,
            &["0".to_string()],
            &RebaseState::None
        ));
        assert!(!should_refresh(
            HookEvent::PostMerge,
            &["1".to_string()],
            &RebaseState::None
        ));
    }

    fn workspace_line(name: &str) -> WorkspaceLine {
        WorkspaceLine {
            root: PathBuf::from(format!("/repo/{name}")),
            db: PathBuf::from(format!("/repo/{name}/.c10r/index.db")),
            workspace: name.to_string(),
        }
    }

    // An empty hooks directory gets a fresh, executable c10r file at every event's own path — no
    // existing hook means every slot resolves to `E` and nothing is kept.
    #[test]
    fn install_into_empty_directory_writes_four_executable_c10r_files() {
        let dir = tempfile::tempdir().unwrap();
        let line = workspace_line("alpha");
        let plan = install_plan(dir.path(), &line, false).unwrap();
        apply_plan(dir.path(), &plan).unwrap();

        for event in HookEvent::ALL {
            let path = dir.path().join(event.as_str());
            let text = std::fs::read_to_string(&path).unwrap();
            assert_eq!(parse(&text), Some(vec![line.clone()]));
            assert!(!dir.path().join(format!("{}.legacy", event.as_str())).exists());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o755, "c10r's own file is executable");
            }
        }
    }

    // A foreign file at `E` is not a recognized shim, so the slot is `E` itself: the foreign file is
    // kept at `E.c10r-prior` and c10r's file takes its place.
    #[test]
    fn foreign_file_at_e_is_kept_and_c10r_file_written_at_e() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post-commit");
        std::fs::write(&path, "#!/bin/sh\necho foreign\n").unwrap();

        let line = workspace_line("alpha");
        let plan = install_plan(dir.path(), &line, false).unwrap();
        apply_plan(dir.path(), &plan).unwrap();

        let prior = dir.path().join("post-commit.c10r-prior");
        assert_eq!(std::fs::read_to_string(&prior).unwrap(), "#!/bin/sh\necho foreign\n");
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()), Some(vec![line]));
    }

    // A recognized prek shim at `E` means the slot is `E.legacy`, since that is the path prek's own
    // shim actually runs; `E` is left exactly as found.
    #[test]
    fn recognized_prek_shim_at_e_places_c10r_at_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post-commit");
        std::fs::write(&path, PREK_0_5_3_POST_COMMIT).unwrap();

        let line = workspace_line("alpha");
        let plan = install_plan(dir.path(), &line, false).unwrap();
        apply_plan(dir.path(), &plan).unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            PREK_0_5_3_POST_COMMIT,
            "the shim at E is untouched"
        );
        let legacy = dir.path().join("post-commit.legacy");
        assert_eq!(parse(&std::fs::read_to_string(&legacy).unwrap()), Some(vec![line]));
    }

    // The slot is still `E.legacy` under a recognized shim, so a foreign file already at `E.legacy` is
    // the one that gets kept — not the shim, which install never touches.
    #[test]
    fn prek_shim_at_e_with_foreign_legacy_keeps_the_foreign_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post-commit");
        std::fs::write(&path, PREK_0_5_3_POST_COMMIT).unwrap();
        let legacy = dir.path().join("post-commit.legacy");
        std::fs::write(&legacy, "#!/bin/sh\necho foreign-legacy\n").unwrap();

        let line = workspace_line("alpha");
        let plan = install_plan(dir.path(), &line, false).unwrap();
        apply_plan(dir.path(), &plan).unwrap();

        let prior = dir.path().join("post-commit.c10r-prior");
        assert_eq!(
            std::fs::read_to_string(&prior).unwrap(),
            "#!/bin/sh\necho foreign-legacy\n"
        );
        assert_eq!(parse(&std::fs::read_to_string(&legacy).unwrap()), Some(vec![line]));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            PREK_0_5_3_POST_COMMIT,
            "the shim at E is untouched"
        );
    }

    // An unrecognized file at `E` is chained there directly, whatever else sits at `E.legacy` — that
    // file belongs to neither c10r nor a recognized hook manager, so it is left alone.
    #[test]
    fn unrecognized_file_at_e_with_separate_legacy_chains_at_e() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post-commit");
        std::fs::write(&path, "#!/bin/sh\necho unrecognized\n").unwrap();
        let legacy = dir.path().join("post-commit.legacy");
        std::fs::write(&legacy, "#!/bin/sh\necho separate\n").unwrap();

        let line = workspace_line("alpha");
        let plan = install_plan(dir.path(), &line, false).unwrap();
        apply_plan(dir.path(), &plan).unwrap();

        let prior = dir.path().join("post-commit.c10r-prior");
        assert_eq!(
            std::fs::read_to_string(&prior).unwrap(),
            "#!/bin/sh\necho unrecognized\n"
        );
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()), Some(vec![line]));
        assert_eq!(
            std::fs::read_to_string(&legacy).unwrap(),
            "#!/bin/sh\necho separate\n",
            "the legacy file is untouched"
        );
    }

    // c10r files already in place — one at `E`, one at `E.legacy` for a different event — are updated
    // in place rather than moved, and a reinstall of the same workspace is a byte-for-byte no-op.
    #[test]
    fn updating_existing_c10r_files_appends_a_workspace_and_reinstalling_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let first = workspace_line("alpha");

        let commit_path = dir.path().join("post-commit");
        std::fs::write(
            &commit_path,
            render(HookEvent::PostCommit, std::slice::from_ref(&first)),
        )
        .unwrap();
        let checkout_legacy = dir.path().join("post-checkout.legacy");
        std::fs::write(
            &checkout_legacy,
            render(HookEvent::PostCheckout, std::slice::from_ref(&first)),
        )
        .unwrap();

        let second = workspace_line("beta");
        let plan = install_plan(dir.path(), &second, false).unwrap();
        apply_plan(dir.path(), &plan).unwrap();

        assert_eq!(
            parse(&std::fs::read_to_string(&commit_path).unwrap()),
            Some(vec![first.clone(), second.clone()])
        );
        assert_eq!(
            parse(&std::fs::read_to_string(&checkout_legacy).unwrap()),
            Some(vec![first.clone(), second.clone()])
        );
        assert!(
            !dir.path().join("post-checkout").exists(),
            "post-checkout's c10r file stays at its legacy location"
        );

        let before = std::fs::read_to_string(&commit_path).unwrap();
        let reinstall = install_plan(dir.path(), &second, false).unwrap();
        for event_plan in &reinstall.events {
            assert!(
                event_plan.write.is_none(),
                "{:?} plans no write on reinstall",
                event_plan.event
            );
        }
        apply_plan(dir.path(), &reinstall).unwrap();
        assert_eq!(std::fs::read_to_string(&commit_path).unwrap(), before);
    }

    // A kept hook identical to the file now at the slot is replaced silently — the collision check
    // exists to protect a *different* kept hook, not to forbid reinstalling over an identical one.
    #[test]
    fn identical_kept_hook_allows_install_to_proceed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post-commit");
        let content = "#!/bin/sh\necho same\n";
        std::fs::write(&path, content).unwrap();
        let prior = dir.path().join("post-commit.c10r-prior");
        std::fs::write(&prior, content).unwrap();

        let line = workspace_line("alpha");
        let plan = install_plan(dir.path(), &line, false).unwrap();
        apply_plan(dir.path(), &plan).unwrap();

        assert_eq!(std::fs::read_to_string(&prior).unwrap(), content);
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()), Some(vec![line]));
    }

    // A kept hook that differs from the file now at the slot stops installation before anything
    // changes — silently replacing it could discard a hook that belongs to the user.
    #[test]
    fn different_kept_hook_refuses_installation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post-commit");
        std::fs::write(&path, "#!/bin/sh\necho now\n").unwrap();
        let prior = dir.path().join("post-commit.c10r-prior");
        std::fs::write(&prior, "#!/bin/sh\necho before\n").unwrap();

        let line = workspace_line("alpha");
        let err = install_plan(dir.path(), &line, false).unwrap_err();
        assert!(matches!(err, InstallRefusal::PriorCollision(ref p) if p == &prior));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "#!/bin/sh\necho now\n");
        assert_eq!(std::fs::read_to_string(&prior).unwrap(), "#!/bin/sh\necho before\n");
    }

    // A dangling symbolic link at the slot is refused rather than written through — following it would
    // land the write somewhere outside the hooks directory entirely.
    #[cfg(unix)]
    #[test]
    fn dangling_symlink_at_the_slot_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post-commit");
        std::os::unix::fs::symlink(dir.path().join("absent-target"), &path).unwrap();

        let line = workspace_line("alpha");
        let err = install_plan(dir.path(), &line, false).unwrap_err();
        assert!(matches!(err, InstallRefusal::DanglingSymlink(ref p) if p == &path));
        assert!(
            err.to_string().contains(&path.display().to_string()),
            "the refusal names the path: {err}"
        );
    }

    // Two symbolic links naming the same target move together as a link — the kept hook is not
    // dereferenced and rewritten as a copy of what it points to.
    #[cfg(unix)]
    #[test]
    fn symlinks_with_the_same_target_move_as_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real-hook");
        std::fs::write(&target, "#!/bin/sh\necho real\n").unwrap();

        let path = dir.path().join("post-commit");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let prior = dir.path().join("post-commit.c10r-prior");
        std::os::unix::fs::symlink(&target, &prior).unwrap();

        let line = workspace_line("alpha");
        let plan = install_plan(dir.path(), &line, false).unwrap();
        apply_plan(dir.path(), &plan).unwrap();

        let prior_meta = std::fs::symlink_metadata(&prior).unwrap();
        assert!(prior_meta.file_type().is_symlink(), "the kept hook is still a link");
        assert_eq!(std::fs::read_link(&prior).unwrap(), target);
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()), Some(vec![line]));
    }

    // Two symbolic links with different targets are different hooks even when the files they point to
    // happen to hold the same bytes — the comparison is by target, never by dereferencing.
    #[cfg(unix)]
    #[test]
    fn symlinks_with_different_targets_refuse_even_with_identical_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let target_a = dir.path().join("hook-a");
        let target_b = dir.path().join("hook-b");
        std::fs::write(&target_a, "#!/bin/sh\necho same\n").unwrap();
        std::fs::write(&target_b, "#!/bin/sh\necho same\n").unwrap();

        let path = dir.path().join("post-commit");
        std::os::unix::fs::symlink(&target_a, &path).unwrap();
        let prior = dir.path().join("post-commit.c10r-prior");
        std::os::unix::fs::symlink(&target_b, &prior).unwrap();

        let before = snapshot(dir.path());

        let line = workspace_line("alpha");
        let err = install_plan(dir.path(), &line, false).unwrap_err();
        assert!(matches!(err, InstallRefusal::PriorCollision(ref p) if p == &prior));
        assert!(
            err.to_string().contains("remove it or rename it before installing"),
            "{err}"
        );
        assert_eq!(snapshot(dir.path()), before, "no file changed");
    }

    // A link at the slot and a regular file with the link target's bytes at the kept-hook path are
    // different hooks: the link is never compared by dereferencing it.
    #[cfg(unix)]
    #[test]
    fn a_symlink_and_a_regular_file_with_the_same_bytes_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("hook-a");
        std::fs::write(&target, "#!/bin/sh\necho same\n").unwrap();
        let path = dir.path().join("post-commit");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let prior = dir.path().join("post-commit.c10r-prior");
        std::fs::write(&prior, "#!/bin/sh\necho same\n").unwrap();
        let before = snapshot(dir.path());

        let err = install_plan(dir.path(), &workspace_line("alpha"), false).unwrap_err();
        assert!(matches!(err, InstallRefusal::PriorCollision(ref p) if p == &prior));
        assert_eq!(snapshot(dir.path()), before, "no file changed");
    }

    // `--overwrite` discards whatever was at the slot and whatever c10r had kept — nothing survives to
    // be kept or restored later.
    #[test]
    fn overwrite_deletes_the_slot_file_and_the_prior_keeping_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post-commit");
        std::fs::write(&path, "#!/bin/sh\necho current\n").unwrap();
        let prior = dir.path().join("post-commit.c10r-prior");
        std::fs::write(&prior, "#!/bin/sh\necho old\n").unwrap();

        let line = workspace_line("alpha");
        let plan = install_plan(dir.path(), &line, true).unwrap();
        apply_plan(dir.path(), &plan).unwrap();

        assert!(!prior.exists(), "the prior hook is deleted, not kept");
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()), Some(vec![line]));
    }

    // A write failure must never leave a kept hook disconnected: c10r's new content is staged to a
    // temporary file before the existing hook at the slot is moved anywhere, so a failure staging it —
    // here, a hooks directory that denies the write — leaves the existing hook exactly where it was,
    // never partway moved to `.c10r-prior`.
    #[cfg(unix)]
    #[test]
    fn a_failed_write_leaves_the_kept_hook_at_its_original_path() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post-commit");
        std::fs::write(&path, "#!/bin/sh\necho foreign\n").unwrap();

        let line = workspace_line("alpha");
        let plan = install_plan(dir.path(), &line, false).unwrap();

        let original_mode = std::fs::metadata(dir.path()).unwrap().permissions().mode();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = apply_plan(dir.path(), &plan);
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(original_mode)).unwrap();

        assert!(result.is_err(), "the write failure propagates");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "#!/bin/sh\necho foreign\n",
            "the kept hook is untouched: staging the new content failed before it was moved"
        );
        assert!(
            !dir.path().join("post-commit.c10r-prior").exists(),
            "the kept hook was never moved"
        );
    }

    // Every entry directly under `dir` by name: a link by its target, never by the file it points to,
    // so a link replaced by a copy of its target reads as a change.
    fn snapshot(dir: &Path) -> std::collections::BTreeMap<String, (bool, Vec<u8>)> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                let is_link = std::fs::symlink_metadata(&path).unwrap().file_type().is_symlink();
                let content = if is_link {
                    std::fs::read_link(&path)
                        .unwrap()
                        .as_os_str()
                        .as_encoded_bytes()
                        .to_vec()
                } else {
                    std::fs::read(&path).unwrap()
                };
                (name, (is_link, content))
            })
            .collect()
    }

    fn dir_listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    // A failure of the final rename into the slot, after the kept hook moved, moves the kept hook back.
    #[test]
    fn a_failed_final_rename_moves_the_kept_hook_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post-commit");
        std::fs::write(&path, "#!/bin/sh\necho foreign\n").unwrap();

        let mut plan = install_plan(dir.path(), &workspace_line("alpha"), false).unwrap();
        let write = plan.events[0].write.as_mut().unwrap();
        write.path = dir.path().join("missing-directory").join("post-commit");
        let before = dir_listing(dir.path());

        assert!(apply_plan(dir.path(), &plan).is_err(), "the rename failure propagates");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "#!/bin/sh\necho foreign\n");
        assert_eq!(
            dir_listing(dir.path()),
            before,
            "no prior file or temporary file remains"
        );
    }

    // A staging failure on a later event changes no file, including the events staged before it.
    #[test]
    fn a_staging_failure_on_the_second_event_changes_no_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("post-commit"), "#!/bin/sh\necho foreign\n").unwrap();
        let plan = install_plan(dir.path(), &workspace_line("alpha"), false).unwrap();
        let before = dir_listing(dir.path());

        let mut calls = 0;
        let result = apply_plan_staging(dir.path(), &plan, |hooks_dir, content| {
            calls += 1;
            if calls == 2 {
                return Err(io::Error::other("injected staging failure"));
            }
            stage_hook_write(hooks_dir, content)
        });

        assert!(result.is_err());
        assert_eq!(dir_listing(dir.path()), before);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("post-commit")).unwrap(),
            "#!/bin/sh\necho foreign\n"
        );
    }

    // An installation that reached only some events leaves files that differ only by its workspace, so
    // running it again completes every file.
    #[test]
    fn a_retry_completes_an_installation_that_failed_partway() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = workspace_line("alpha");
        let beta = workspace_line("beta");
        apply_plan(dir.path(), &install_plan(dir.path(), &alpha, false).unwrap()).unwrap();
        apply_plan(dir.path(), &install_plan(dir.path(), &beta, false).unwrap()).unwrap();
        for event in [HookEvent::PostCheckout, HookEvent::PostMerge] {
            std::fs::write(
                dir.path().join(event.as_str()),
                render(event, std::slice::from_ref(&alpha)),
            )
            .unwrap();
        }

        apply_plan(dir.path(), &install_plan(dir.path(), &beta, false).unwrap()).unwrap();

        for event in HookEvent::ALL {
            assert_eq!(
                std::fs::read_to_string(dir.path().join(event.as_str())).unwrap(),
                render(event, &[alpha.clone(), beta.clone()]),
                "{event:?} names both workspaces"
            );
        }
    }

    // Files that differ by a workspace other than the one being installed are still refused.
    #[test]
    fn files_that_differ_by_another_workspace_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = workspace_line("alpha");
        let beta = workspace_line("beta");
        let gamma = workspace_line("gamma");
        let with_beta = [alpha.clone(), beta.clone()];
        let with_gamma = [alpha, gamma];
        std::fs::write(
            dir.path().join("post-commit"),
            render(HookEvent::PostCommit, &with_beta),
        )
        .unwrap();
        std::fs::write(dir.path().join("post-merge"), render(HookEvent::PostMerge, &with_gamma)).unwrap();

        assert!(matches!(
            install_plan(dir.path(), &beta, false),
            Err(InstallRefusal::Disagreement(_))
        ));
    }

    // c10r files that disagree on the installed workspace set refuse before anything changes, and the
    // refusal names every file that disagrees.
    #[test]
    fn disagreeing_c10r_files_refuse_naming_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = workspace_line("alpha");
        let beta = workspace_line("beta");

        let commit_path = dir.path().join("post-commit");
        std::fs::write(
            &commit_path,
            render(HookEvent::PostCommit, std::slice::from_ref(&alpha)),
        )
        .unwrap();
        let merge_path = dir.path().join("post-merge");
        std::fs::write(&merge_path, render(HookEvent::PostMerge, std::slice::from_ref(&beta))).unwrap();

        let err = install_plan(dir.path(), &alpha, false).unwrap_err();
        let message = err.to_string();
        assert!(message.contains(&commit_path.display().to_string()), "{message}");
        assert!(message.contains(&merge_path.display().to_string()), "{message}");
        assert_eq!(
            std::fs::read_to_string(&commit_path).unwrap(),
            render(HookEvent::PostCommit, &[alpha])
        );
        assert_eq!(
            std::fs::read_to_string(&merge_path).unwrap(),
            render(HookEvent::PostMerge, &[beta])
        );
    }

    // A workspace root carrying a line break is refused before the hooks directory is touched at all —
    // the hook file's one-line-per-workspace format cannot represent it.
    #[test]
    fn a_workspace_root_with_a_line_break_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut line = workspace_line("alpha");
        line.root = PathBuf::from("/repo/al\npha");

        let err = install_plan(dir.path(), &line, false).unwrap_err();
        assert!(
            matches!(err, InstallRefusal::LineBreak(_)),
            "the refusal names a line break: {err}"
        );
        assert!(err.to_string().contains("line break"), "{err}");
        assert!(!dir.path().join("post-commit").exists(), "no hook file was written");
    }

    // Removing one of two workspaces leaves the other in every c10r file.
    #[test]
    fn uninstall_removes_one_of_two_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = workspace_line("alpha");
        let beta = workspace_line("beta");
        for event in HookEvent::ALL {
            let path = dir.path().join(event.as_str());
            std::fs::write(&path, render(event, &[alpha.clone(), beta.clone()])).unwrap();
        }

        let plan = uninstall_plan(dir.path(), &alpha);
        apply_uninstall_plan(dir.path(), &plan).unwrap();

        for event in HookEvent::ALL {
            let path = dir.path().join(event.as_str());
            assert_eq!(
                parse(&std::fs::read_to_string(&path).unwrap()),
                Some(vec![beta.clone()])
            );
        }
    }

    // Removing the last workspace from a c10r file at `E` restores the kept hook to `E`, byte-for-byte.
    #[test]
    fn uninstall_last_workspace_restores_kept_hook_at_e() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = workspace_line("alpha");
        let path = dir.path().join("post-commit");
        std::fs::write(&path, render(HookEvent::PostCommit, std::slice::from_ref(&alpha))).unwrap();
        let prior = dir.path().join("post-commit.c10r-prior");
        let kept_content = "#!/bin/sh\necho kept\n";
        std::fs::write(&prior, kept_content).unwrap();

        let plan = uninstall_plan(dir.path(), &alpha);
        apply_uninstall_plan(dir.path(), &plan).unwrap();

        assert!(!prior.exists());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            kept_content,
            "the kept hook is restored byte-for-byte"
        );
    }

    // Removing the last workspace from a c10r file at `E.legacy` restores the kept hook there, not
    // at `E` — the slot it occupied is the one it returns to.
    #[test]
    fn uninstall_last_workspace_restores_kept_hook_at_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = workspace_line("alpha");
        let legacy = dir.path().join("post-commit.legacy");
        std::fs::write(&legacy, render(HookEvent::PostCommit, std::slice::from_ref(&alpha))).unwrap();
        let prior = dir.path().join("post-commit.c10r-prior");
        let kept_content = "#!/bin/sh\necho kept\n";
        std::fs::write(&prior, kept_content).unwrap();

        let plan = uninstall_plan(dir.path(), &alpha);
        apply_uninstall_plan(dir.path(), &plan).unwrap();

        assert!(!prior.exists());
        assert_eq!(
            std::fs::read_to_string(&legacy).unwrap(),
            kept_content,
            "the kept hook is restored at the legacy slot it occupied"
        );
    }

    // With no kept hook to restore, removing the last workspace simply leaves no c10r file behind.
    #[test]
    fn uninstall_last_workspace_with_no_kept_hook_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = workspace_line("alpha");
        let path = dir.path().join("post-commit");
        std::fs::write(&path, render(HookEvent::PostCommit, std::slice::from_ref(&alpha))).unwrap();

        let plan = uninstall_plan(dir.path(), &alpha);
        apply_uninstall_plan(dir.path(), &plan).unwrap();

        assert!(!path.exists());
    }

    // Uninstall works event by event, so it removes a workspace from every file that names it even
    // when the hooks directory's files disagree with each other.
    #[test]
    fn uninstall_works_across_disagreeing_files() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = workspace_line("alpha");
        let beta = workspace_line("beta");
        let commit_path = dir.path().join("post-commit");
        std::fs::write(
            &commit_path,
            render(HookEvent::PostCommit, std::slice::from_ref(&alpha)),
        )
        .unwrap();
        let merge_path = dir.path().join("post-merge");
        std::fs::write(
            &merge_path,
            render(HookEvent::PostMerge, &[alpha.clone(), beta.clone()]),
        )
        .unwrap();

        let plan = uninstall_plan(dir.path(), &alpha);
        apply_uninstall_plan(dir.path(), &plan).unwrap();

        assert!(
            !commit_path.exists(),
            "post-commit had only alpha, so it is removed entirely"
        );
        assert_eq!(parse(&std::fs::read_to_string(&merge_path).unwrap()), Some(vec![beta]));
    }

    // Removing a workspace that is not named by any c10r file leaves every file byte-for-byte
    // unchanged.
    #[test]
    fn uninstall_of_a_workspace_not_installed_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = workspace_line("alpha");
        let beta = workspace_line("beta");
        for event in HookEvent::ALL {
            let path = dir.path().join(event.as_str());
            std::fs::write(&path, render(event, std::slice::from_ref(&alpha))).unwrap();
        }

        let plan = uninstall_plan(dir.path(), &beta);
        apply_uninstall_plan(dir.path(), &plan).unwrap();

        for event in HookEvent::ALL {
            let path = dir.path().join(event.as_str());
            assert_eq!(
                parse(&std::fs::read_to_string(&path).unwrap()),
                Some(vec![alpha.clone()])
            );
        }
    }
}
