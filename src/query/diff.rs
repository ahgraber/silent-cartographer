//! A pure unified-diff parser: no I/O, no subprocesses. A `git diff` is line-oriented while the index
//! holds byte spans, so this module carries a patch down to `(pre-change path, pre-change byte range)`
//! pairs the seed resolution can look up.
//!
//! Everything is expressed on the change's *pre-change* side: path and offsets always come from the
//! same side of the diff, so a renamed file's changed regions resolve where an index built at the base
//! actually holds them.

use std::path::{Path, PathBuf};

/// What a change did to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// The change introduced this file; it has no pre-change side.
    Added,
    /// The change removed this file; it has no post-change side.
    Deleted,
    /// The change gave this file a different path, with or without also changing its content.
    Renamed,
    /// The change edited this file's content in place, or changed only its mode.
    Modified,
}

/// A 1-based line range on a change's pre-change side.
///
/// `count == 0` marks an insertion point rather than a removed span: the change added lines after
/// pre-change line `start` without removing any, so there is no pre-change text to cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    /// The 1-based pre-change line the range starts at (or, for an insertion point, the pre-change
    /// line the insertion follows; `0` when the insertion precedes the first line).
    pub start: u32,
    /// How many pre-change lines the range covers; `0` for an insertion point.
    pub count: u32,
}

/// One file's entry in a parsed patch, stated on the pre-change side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// What the change did to this file.
    pub kind: ChangeKind,
    /// The path the file had before the change; `None` for a file the change added.
    pub pre_path: Option<String>,
    /// The path the file has after the change; `None` for a file the change deleted. Retained for
    /// path narrowing and display only — resolution never uses it.
    pub post_path: Option<String>,
    /// The pre-change line ranges the change touched, in patch order.
    pub pre_ranges: Vec<LineRange>,
}

/// Parse a unified patch — as produced with `--no-prefix --unified=0` — into per-file pre-change
/// entries.
///
/// Parsing rules:
///
/// - A line beginning with `diff --git ` starts a new file section; the previous section is flushed.
/// - Header lines (`---`, `+++`, `rename from`, `rename to`) are recognized only *before* the
///   section's first `@@` hunk header. This matters: with `--unified=0` a removed line whose own text
///   starts with `-- ` renders as `--- …`, which would otherwise be mistaken for a file header.
/// - `--- /dev/null` leaves `pre_path` as `None` (a file the change added); any other `--- <path>`
///   sets it. `+++ /dev/null` leaves `post_path` as `None` (a file the change deleted); any other
///   `+++ <path>` sets it.
/// - `rename from <path>` / `rename to <path>` also supply the two paths — they are the only source
///   for a pure rename, whose section carries no `---`/`+++` lines and no hunks at all. They never
///   overwrite a path a `---`/`+++` line already established.
/// - `@@ -<start>[,<count>] +… @@` yields one [`LineRange`]; an omitted `,<count>` means `1`.
/// - A section that establishes no path at all — a binary-file difference, a mode-only change — is
///   dropped entirely: it contributes no pre-change range, and degrading to "no seed from this
///   section" is the intended behavior, never an error.
/// - Path text: a value beginning with `"` is C-style quoted (git still quotes a path containing `"`,
///   `\`, or a control character); it is unquoted, handling `\\`, `\"`, and `\<three octal digits>`
///   escapes, and a malformed quoted value is treated as no path. Otherwise the value runs to the
///   first TAB, if any — git terminates the name with a TAB in a `---`/`+++` line when the name
///   contains a space.
/// - `kind` is derived at flush: no `pre_path` → `Added`; no `post_path` → `Deleted`; both present and
///   different → `Renamed`; otherwise `Modified` (which includes a mode-only-plus-content edit and a
///   section whose only change was a mode bit alongside a real path).
pub fn parse_patch(patch: &str) -> Vec<FileChange> {
    let mut result = Vec::new();
    let mut current: Option<Section> = None;

    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            if let Some(section) = current.take()
                && let Some(change) = section.flush()
            {
                result.push(change);
            }
            current = Some(Section::default());
            continue;
        }
        if let Some(section) = current.as_mut() {
            section.consume(line);
        }
    }
    if let Some(section) = current.take()
        && let Some(change) = section.flush()
    {
        result.push(change);
    }

    result
}

/// Keep only the changes touching one of `paths`, matching either side of the change.
///
/// Narrowing is applied to the parsed change set rather than handed to `git diff` as a pathspec:
/// a pathspec is applied before rename detection, so narrowing to a renamed file's post-change
/// path leaves git with nothing to pair it against and it degrades to an addition with no
/// pre-change side — silently seeding nothing for exactly the file the caller asked about.
///
/// A path matches when it equals the change's path or is a directory prefix of it, on either side.
/// Matching is by path component, so `src/net` selects `src/net/client.rs` but never `src/network.rs`.
/// An empty `paths` keeps everything.
pub fn narrow_to_paths(changes: Vec<FileChange>, paths: &[PathBuf]) -> Vec<FileChange> {
    if paths.is_empty() {
        return changes;
    }
    let prefixes: Vec<String> = paths.iter().map(|p| normalize_prefix(p)).collect();
    changes
        .into_iter()
        .filter(|change| {
            [change.pre_path.as_deref(), change.post_path.as_deref()]
                .into_iter()
                .flatten()
                .any(|path| prefixes.iter().any(|prefix| path_matches_prefix(path, prefix)))
        })
        .collect()
}

/// Normalize a caller-supplied narrowing path to the `/`-separated, trailing-slash-stripped form
/// [`path_matches_prefix`] compares against.
fn normalize_prefix(path: &Path) -> String {
    let raw = path.to_string_lossy().into_owned();
    raw.strip_suffix('/').map(str::to_string).unwrap_or(raw)
}

/// Whether `path` equals `prefix` or has `prefix` as a directory-boundary prefix — never a bare
/// string prefix, which would let `src/net` wrongly select `src/network.rs`.
fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    path == prefix || path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

/// The in-progress state of one file's section while `parse_patch` walks the patch.
#[derive(Default)]
struct Section {
    pre_path: Option<String>,
    post_path: Option<String>,
    ranges: Vec<LineRange>,
    /// Whether the first `@@` hunk header has been seen — header lines are recognized only before it.
    in_hunks: bool,
}

impl Section {
    /// Feed one patch line (already stripped of its trailing newline) into this section.
    fn consume(&mut self, line: &str) {
        if let Some(range) = parse_hunk_header(line) {
            self.in_hunks = true;
            self.ranges.push(range);
            return;
        }
        if self.in_hunks {
            // A content line (or a false-positive header lookalike produced by one) — ignored.
            return;
        }
        if let Some(rest) = line.strip_prefix("--- ") {
            self.pre_path = parse_path_value(rest);
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            self.post_path = parse_path_value(rest);
        } else if let Some(rest) = line.strip_prefix("rename from ")
            && self.pre_path.is_none()
        {
            self.pre_path = parse_path_value(rest);
        } else if let Some(rest) = line.strip_prefix("rename to ")
            && self.post_path.is_none()
        {
            self.post_path = parse_path_value(rest);
        }
        // Any other header line (`index …`, `old mode …`, `new mode …`, `similarity index …`, a
        // binary-file notice) carries no path or range information this module needs.
    }

    /// Finish this section, deriving its [`ChangeKind`] from which paths were established. Returns
    /// `None` when neither side of the section named a path (a binary-file difference or a mode-only
    /// change), so the section contributes no seed.
    fn flush(self) -> Option<FileChange> {
        if self.pre_path.is_none() && self.post_path.is_none() {
            return None;
        }
        let kind = match (&self.pre_path, &self.post_path) {
            (None, _) => ChangeKind::Added,
            (_, None) => ChangeKind::Deleted,
            (Some(pre), Some(post)) if pre != post => ChangeKind::Renamed,
            _ => ChangeKind::Modified,
        };
        Some(FileChange {
            kind,
            pre_path: self.pre_path,
            post_path: self.post_path,
            pre_ranges: self.ranges,
        })
    }
}

/// Parse a `@@ -<start>[,<count>] +… @@` hunk header line into its pre-change [`LineRange`], or `None`
/// when `line` is not a hunk header.
fn parse_hunk_header(line: &str) -> Option<LineRange> {
    let rest = line.strip_prefix("@@ -")?;
    let minus_end = rest.find(' ')?;
    let minus = &rest[..minus_end];
    let (start_str, count_str) = match minus.split_once(',') {
        Some((start, count)) => (start, Some(count)),
        None => (minus, None),
    };
    let start: u32 = start_str.parse().ok()?;
    let count: u32 = match count_str {
        Some(count) => count.parse().ok()?,
        None => 1,
    };
    Some(LineRange { start, count })
}

/// Parse the path text following a `---`/`+++ `/`rename from `/`rename to ` prefix. `/dev/null` yields
/// `None` (no path on that side); a `"`-prefixed value is C-style quoted and is unquoted, with a
/// malformed quote treated as no path; otherwise the value runs to the first TAB, if any.
fn parse_path_value(value: &str) -> Option<String> {
    if value == "/dev/null" {
        return None;
    }
    if let Some(rest) = value.strip_prefix('"') {
        return unquote_c_style(rest);
    }
    Some(match value.find('\t') {
        Some(tab) => value[..tab].to_string(),
        None => value.to_string(),
    })
}

/// Unquote a C-style quoted path, `rest` being the text right after the opening `"`.
///
/// Covers exactly the escapes git's own C-style quoting emits: `\\` and `\"`, the named control
/// escapes `\a \b \f \n \r \t \v`, and `\<three octal digits>` for every other byte it must escape.
/// Returns `None` when the value has no closing `"` or carries an escape outside that set — the
/// section then establishes no path on that side and contributes no seed, which is the intended
/// degradation rather than a fabricated path.
fn unquote_c_style(rest: &str) -> Option<String> {
    let bytes = rest.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    loop {
        let b = *bytes.get(i)?;
        match b {
            b'"' => return String::from_utf8(out).ok(),
            b'\\' => {
                i += 1;
                match *bytes.get(i)? {
                    b'0'..=b'7' => {
                        let octal = rest.get(i..i + 3)?;
                        out.push(u8::from_str_radix(octal, 8).ok()?);
                        i += 3;
                    }
                    named => {
                        out.push(unescape_named(named)?);
                        i += 1;
                    }
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
}

/// The byte a single-character C-style escape denotes, or `None` for an escape outside the set git
/// emits.
fn unescape_named(escape: u8) -> Option<u8> {
    match escape {
        b'\\' => Some(b'\\'),
        b'"' => Some(b'"'),
        b'a' => Some(0x07),
        b'b' => Some(0x08),
        b'f' => Some(0x0c),
        b'n' => Some(b'\n'),
        b'r' => Some(b'\r'),
        b't' => Some(b'\t'),
        b'v' => Some(0x0b),
        _ => None,
    }
}

/// A line→byte offset table over one document's text.
pub struct LineIndex {
    /// The byte offset each 1-based line starts at: `offsets[0]` is line 1's start, `offsets[1]` line
    /// 2's, and so on.
    offsets: Vec<usize>,
    /// The document's total byte length, the clamp value for an out-of-range line.
    len: usize,
}

impl LineIndex {
    /// Build the table: the byte offset each 1-based line starts at.
    ///
    /// Lines are delimited by `\n` alone, so a `\r` stays part of the line it terminates and a CRLF
    /// document's offsets are exact; offsets are byte offsets throughout, so multibyte content needs
    /// no special handling.
    pub fn new(text: &str) -> Self {
        let mut offsets = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                offsets.push(i + 1);
            }
        }
        LineIndex {
            offsets,
            len: text.len(),
        }
    }

    /// The byte offset 1-based `line` starts at, clamped to the end of the text for a line past its
    /// end (and for line 0, which no hunk names as a range start).
    pub fn line_start(&self, line: u32) -> usize {
        if line == 0 {
            return self.len;
        }
        self.offsets.get((line - 1) as usize).copied().unwrap_or(self.len)
    }

    /// The 1-based line containing byte offset `byte`, clamped to the last line for an offset at or
    /// past the end of the text.
    pub fn line_of(&self, byte: usize) -> u32 {
        let byte = byte.min(self.len);
        let line = match self.offsets.binary_search(&byte) {
            Ok(idx) => idx + 1,
            // `offsets[idx - 1] <= byte < offsets[idx]`, so `idx` (1-based) is the containing line.
            Err(idx) => idx,
        };
        line.clamp(1, self.offsets.len()) as u32
    }

    /// The byte range a pre-change line range covers, as `(start, end)` half-open byte offsets.
    ///
    /// A removed span (`count > 0`) runs from the start of its first line to the start of the line
    /// after its last, so it covers exactly the removed text. An insertion point (`count == 0`)
    /// covers no pre-change text at all, so it is widened to the single byte at the start of the
    /// anchor line — the line the insertion follows — which is where the declaration the caller
    /// edited actually lives; an insertion before the first line anchors at the start of the
    /// document. The widening exists because a zero-width range overlaps no span.
    pub fn byte_range(&self, range: LineRange) -> (usize, usize) {
        if range.count == 0 {
            let anchor = if range.start == 0 {
                0
            } else {
                self.line_start(range.start)
            }
            .min(self.len);
            let end = (anchor + 1).min(self.len);
            return (anchor, end);
        }
        let start = self.line_start(range.start).min(self.len);
        let end = self
            .line_start(range.start.saturating_add(range.count))
            .min(self.len)
            .max(start);
        (start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A rename-plus-edit section parses to `Renamed`, with the ranges attached to the pre-change path
    // and the post-change path retained for display.
    #[test]
    fn rename_with_edit_parses_to_renamed_with_ranges_on_the_pre_path() {
        let patch = r#"diff --git old_name.rs new_name.rs
similarity index 55%
rename from old_name.rs
rename to new_name.rs
index 1111111..2222222 100644
--- old_name.rs
+++ new_name.rs
@@ -10,2 +10,3 @@
-old line
-old line2
+new line
+new line2
+extra
"#;
        let changes = parse_patch(patch);
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(change.kind, ChangeKind::Renamed);
        assert_eq!(change.pre_path.as_deref(), Some("old_name.rs"));
        assert_eq!(change.post_path.as_deref(), Some("new_name.rs"));
        assert_eq!(change.pre_ranges, vec![LineRange { start: 10, count: 2 }]);
    }

    // A binary-file difference and a mode-only change each establish no path, so both yield no entry.
    #[test]
    fn binary_and_mode_only_sections_yield_no_entry() {
        let patch = r#"diff --git image.png image.png
index 1111111..2222222 100644
Binary files image.png and image.png differ
diff --git script.sh script.sh
old mode 100644
new mode 100755
"#;
        let changes = parse_patch(patch);
        assert!(changes.is_empty(), "{changes:?}");
    }

    // A deleted-file section yields `Deleted` with its removed ranges.
    #[test]
    fn deleted_file_yields_deleted_with_removed_ranges() {
        let patch = r#"diff --git deleted.rs deleted.rs
deleted file mode 100644
index 1111111..0000000
--- deleted.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-line one
-line two
"#;
        let changes = parse_patch(patch);
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(change.kind, ChangeKind::Deleted);
        assert_eq!(change.pre_path.as_deref(), Some("deleted.rs"));
        assert_eq!(change.post_path, None);
        assert_eq!(change.pre_ranges, vec![LineRange { start: 1, count: 2 }]);
    }

    // An added-file section yields `Added` with `pre_path: None`. Its `@@ -0,0 +1,n @@` header
    // contributes a zero-count range at pre-change line 0, which the seed resolution has nothing to
    // resolve against (there is no pre-change path at all) — this is the implemented behavior, not an
    // error the parser raises.
    #[test]
    fn added_file_yields_added_with_no_pre_path() {
        let patch = r#"diff --git added.rs added.rs
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ added.rs
@@ -0,0 +1,3 @@
+line one
+line two
+line three
"#;
        let changes = parse_patch(patch);
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(change.kind, ChangeKind::Added);
        assert_eq!(change.pre_path, None);
        assert_eq!(change.post_path.as_deref(), Some("added.rs"));
        assert_eq!(change.pre_ranges, vec![LineRange { start: 0, count: 0 }]);
    }

    // A pure rename (similarity 100%, no hunks) yields `Renamed` with both paths and no ranges.
    #[test]
    fn pure_rename_yields_renamed_with_no_ranges() {
        let patch = r#"diff --git old_name.rs new_name.rs
similarity index 100%
rename from old_name.rs
rename to new_name.rs
"#;
        let changes = parse_patch(patch);
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(change.kind, ChangeKind::Renamed);
        assert_eq!(change.pre_path.as_deref(), Some("old_name.rs"));
        assert_eq!(change.post_path.as_deref(), Some("new_name.rs"));
        assert!(change.pre_ranges.is_empty());
    }

    // A hunk header with an omitted count parses as count 1; a `-a,0` header parses as an insertion
    // point (count 0).
    #[test]
    fn hunk_header_omitted_count_is_one_and_zero_count_is_an_insertion_point() {
        let patch = r#"diff --git file.rs file.rs
--- file.rs
+++ file.rs
@@ -5 +5,2 @@
-old
+new
+new2
@@ -12,0 +14,2 @@
+added1
+added2
"#;
        let changes = parse_patch(patch);
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0].pre_ranges,
            vec![LineRange { start: 5, count: 1 }, LineRange { start: 12, count: 0 }]
        );
    }

    // A removed line whose own text begins with `-- ` renders (with `--unified=0`) as a line starting
    // `--- `, which must not be mistaken for the file's `---` header — header recognition stops at the
    // section's first `@@` line, so the file paths survive intact.
    #[test]
    fn removed_line_starting_with_double_dash_does_not_corrupt_file_paths() {
        let patch = r#"diff --git tricky.rs tricky.rs
--- tricky.rs
+++ tricky.rs
@@ -1,2 +1,1 @@
-- trap line
-kept line
+kept line
"#;
        let changes = parse_patch(patch);
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(change.pre_path.as_deref(), Some("tricky.rs"));
        assert_eq!(change.post_path.as_deref(), Some("tricky.rs"));
        assert_eq!(change.kind, ChangeKind::Modified);
    }

    // A path containing a space is TAB-terminated in the `---`/`+++` line; a path containing a `"` is
    // C-style quoted. Both parse to the exact path.
    #[test]
    fn tab_terminated_and_c_quoted_paths_parse_exactly() {
        let patch = "diff --git path with space.txt path with space.txt\n\
                      --- path with space.txt\t\n\
                      +++ path with space.txt\t\n\
                      @@ -1 +1 @@\n\
                      -old\n\
                      +new\n\
                      diff --git quoted.txt quoted.txt\n\
                      --- \"say \\\"hi\\\".txt\"\n\
                      +++ \"say \\\"hi\\\".txt\"\n\
                      @@ -1 +1 @@\n\
                      -old\n\
                      +new\n";
        let changes = parse_patch(patch);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].pre_path.as_deref(), Some("path with space.txt"));
        assert_eq!(changes[1].pre_path.as_deref(), Some(r#"say "hi".txt"#));
    }

    // A C-quoted path carrying git's named control escapes and an octal escape unquotes to the exact
    // bytes, so a pathological name still resolves at its real path rather than degrading to no seed.
    #[test]
    fn c_quoted_named_and_octal_escapes_unquote_exactly() {
        let patch = "diff --git x x\n--- \"tab\\there\\303\\251.rs\"\n+++ \"tab\\there\\303\\251.rs\"\n@@ -1 +1 @@\n-old\n+new\n";
        let changes = parse_patch(patch);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].pre_path.as_deref(), Some("tab\there\u{e9}.rs"));
    }

    // A renamed file's change is kept when narrowing to its post-change directory and when narrowing
    // to its pre-change directory; an unrelated change is dropped; a directory-component prefix never
    // selects a sibling whose name merely shares the same characters (`src/net` must not select
    // `src/network.rs`); an empty `paths` keeps everything.
    #[test]
    fn narrow_to_paths_matches_either_side_by_path_component() {
        let renamed = FileChange {
            kind: ChangeKind::Renamed,
            pre_path: Some("old/net.rs".to_string()),
            post_path: Some("new/net.rs".to_string()),
            pre_ranges: vec![],
        };
        let unrelated = FileChange {
            kind: ChangeKind::Modified,
            pre_path: Some("other/thing.rs".to_string()),
            post_path: Some("other/thing.rs".to_string()),
            pre_ranges: vec![],
        };
        let sibling = FileChange {
            kind: ChangeKind::Modified,
            pre_path: Some("src/network.rs".to_string()),
            post_path: Some("src/network.rs".to_string()),
            pre_ranges: vec![],
        };
        let changes = vec![renamed.clone(), unrelated, sibling.clone()];

        let kept = narrow_to_paths(changes.clone(), &[PathBuf::from("new")]);
        assert_eq!(
            kept,
            vec![renamed.clone()],
            "narrowing to the post-change directory keeps the rename"
        );

        let kept = narrow_to_paths(changes.clone(), &[PathBuf::from("old")]);
        assert_eq!(
            kept,
            vec![renamed],
            "narrowing to the pre-change directory keeps the rename too"
        );

        let kept = narrow_to_paths(changes.clone(), &[PathBuf::from("src/net")]);
        assert!(!kept.contains(&sibling), "src/net must not select src/network.rs");

        let kept = narrow_to_paths(changes.clone(), &[]);
        assert_eq!(kept, changes, "an empty paths list keeps everything");
    }

    // `line_of` finds the 1-based line containing a byte offset — exactly at a line's start and
    // mid-line — and clamps an offset at or past the end of the text to the last line.
    #[test]
    fn line_of_finds_the_containing_line_and_clamps_at_the_end() {
        let text = "one\ntwo\nthree\n";
        let index = LineIndex::new(text);
        assert_eq!(index.line_of(0), 1);
        assert_eq!(index.line_of(1), 1, "a mid-line offset stays on its own line");
        assert_eq!(index.line_of(4), 2, "the exact start of a line belongs to that line");
        assert_eq!(
            index.line_of(text.len()),
            4,
            "the end of the text clamps to the trailing empty line"
        );
        assert_eq!(
            index.line_of(text.len() + 100),
            4,
            "an offset past the end clamps the same way"
        );
    }

    // `LineIndex` maps line starts exactly over multibyte (UTF-8) content and over CRLF line endings.
    #[test]
    fn line_index_maps_line_starts_over_multibyte_and_crlf_content() {
        let text = "café\n😀 emoji\n汉字";
        let index = LineIndex::new(text);
        assert_eq!(index.line_start(1), 0);
        assert_eq!(index.line_start(2), "café\n".len());
        assert_eq!(index.line_start(3), "café\n😀 emoji\n".len());

        let crlf = "one\r\ntwo\r\nthree";
        let index = LineIndex::new(crlf);
        assert_eq!(index.line_start(1), 0);
        assert_eq!(index.line_start(2), "one\r\n".len());
        assert_eq!(index.line_start(3), "one\r\ntwo\r\n".len());
    }

    // `byte_range` covers exactly the removed text for a multi-line removal, widens an insertion point
    // to one byte at the anchor line, and stays in bounds for a line past the end of the document.
    #[test]
    fn byte_range_covers_removed_text_widens_insertions_and_stays_in_bounds() {
        let text = "one\ntwo\nthree\nfour\n";
        let index = LineIndex::new(text);

        // A multi-line removal covers exactly the removed lines.
        let (start, end) = index.byte_range(LineRange { start: 2, count: 2 });
        assert_eq!(&text[start..end], "two\nthree\n");

        // An insertion point after line 2 widens to the single byte at line 2's start.
        let (start, end) = index.byte_range(LineRange { start: 2, count: 0 });
        assert_eq!(end - start, 1);
        assert_eq!(start, index.line_start(2));

        // An insertion before the first line anchors at the start of the document.
        let (start, end) = index.byte_range(LineRange { start: 0, count: 0 });
        assert_eq!((start, end), (0, 1));

        // A line past the end of the document stays in bounds rather than panicking or inverting.
        let (start, end) = index.byte_range(LineRange { start: 100, count: 3 });
        assert_eq!((start, end), (text.len(), text.len()));

        // An empty document yields (0, 0) for any range.
        let empty = LineIndex::new("");
        assert_eq!(empty.byte_range(LineRange { start: 1, count: 1 }), (0, 0));
    }
}
