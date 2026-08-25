//! The diff-seeded impact assessment: `impact` takes a parsed patch plus the pre-change content of
//! each touched file, resolves each changed pre-change byte range to the declarations it touched,
//! unions those seeds' `dependents`, and grades the answer against the change's pre-change side — so
//! a diff-seeded answer is never presented as exact when the index no longer matches the change being
//! asked about.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::git::SeedMode;
use crate::graph::store::{DEPENDENTS_HORIZON, SymbolRow};
use crate::identity::CanonicalId;

use super::diff::{FileChange, LineIndex};
use super::output::{Answer, Location, SymbolView};
use super::{DependentsReport, OrderMode, QueryEngine, QueryError};

/// Whether the index the answer was drawn from matches the change's pre-change state, and — when it
/// does not — the recovery procedure that obligation carries.
///
/// The label and the recipe are one value because they are one decision:
/// `.specs/specs/code-navigation/spec.md:489` requires every approximate answer to carry a runnable
/// recovery procedure, and an exact answer has none to carry. Held as two fields, either could
/// appear without the other.
///
/// Flattened into the report, so `exactness` and `recovery` remain sibling keys.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "exactness", rename_all = "snake_case")]
pub enum Exactness {
    /// The index's recorded content hash matches the change's pre-change side exactly, and that
    /// pre-change side is one this workspace can actually reconstruct — a hash match produced by a
    /// lossy substitution does not count.
    Exact,
    /// The index was built from a source state that differs from the change's pre-change side, or
    /// that pre-change side could not be reconstructed from this workspace in the first place, so
    /// the impact answer may not reflect the change actually being asked about.
    Approximate {
        /// The runnable procedure that produces an exact answer.
        recovery: RecoveryRecipe,
    },
}

impl Exactness {
    /// Whether the answer is approximate — the label alone, for a caller that needs the grade
    /// without the recipe.
    pub fn is_approximate(&self) -> bool {
        matches!(self, Exactness::Approximate { .. })
    }
}

/// Whether the change's regions resolved to indexed declarations, and — when they did not — which
/// kind of nothing that is.
///
/// The two empty cases mean different things to a caller and must not collapse into one: "your
/// change touches nothing the graph tracks" is a definite none, while "no region resolved and at
/// least one sat in a document this index never saw" is a resolution gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedOutcome {
    /// At least one changed region resolved to an indexed declaration.
    Seeded,
    /// Every changed region sat inside a document the index holds, but none overlapped a
    /// declaration narrower than the file itself — a definite none.
    NoIndexedSymbolTouched,
    /// No region resolved to a seed, and at least one sat in a document the index never saw at
    /// all — a resolution gap, not a definite none.
    NoneResolvable,
}

/// One seed declaration a changed region resolved to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SeedView {
    /// The seed's identity and name.
    pub symbol: SymbolView,
    /// The seed's own definition location, absent for an external symbol with no source here.
    pub location: Option<Location>,
}

/// A changed pre-change region in a document the index does not hold, disclosed rather than dropped.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UnmappableRegion {
    /// The document the region sits in.
    pub document_path: String,
    /// The region's start byte offset on the pre-change side.
    pub span_start: usize,
    /// The region's end byte offset on the pre-change side.
    pub span_end: usize,
}

/// The runnable recovery procedure an approximate answer carries: the shell steps that produce an
/// exact answer, with the resolved base revision, index path, and workspace identity already
/// substituted.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RecoveryRecipe {
    /// The pre-change revision the recipe rebuilds a worktree at.
    pub base_revision: String,
    /// The throwaway index path the recipe builds and re-queries against — never the index this
    /// answer was drawn from, so following the recipe cannot replace the caller's live index with a
    /// historical one.
    pub db: String,
    /// The workspace identity the rebuilt index must be pinned to.
    pub workspace: String,
    /// The steps, in order, as shell command lines. A line beginning with `#` is guidance and a
    /// shell no-op, so the whole block stays copy-pasteable.
    pub steps: Vec<String>,
}

/// A diff-seeded impact answer: which change seeded it, what that change touched, how far the reach
/// extends, and whether the index it was drawn from actually matches the change.
///
/// An impact answer is always `Outcome::Found` carrying exactly one report, including when the
/// change touched no indexed declaration: the definite-none is carried by `seed_outcome` plus an
/// empty `seeds`/`dependents`, because the freshness requirement obliges *every* impact answer to
/// carry its exact/approximate label, and a payload-free `Outcome::Empty` has nowhere to put it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ImpactReport {
    /// The seed mode label (`working_tree`, `staged`, or `range`).
    pub seed_mode: &'static str,
    /// The resolved pre-change revision the diff was taken against.
    pub base_revision: String,
    /// Whether the index matches the change's pre-change state, with the recovery procedure an
    /// approximate answer carries.
    #[serde(flatten)]
    pub exactness: Exactness,
    /// Whether the change's regions resolved to indexed declarations.
    pub seed_outcome: SeedOutcome,
    /// The seed declarations the change's regions resolved to, ordered by canonical identity.
    pub seeds: Vec<SeedView>,
    /// The changed regions that sat in a document the index does not hold, present only when any
    /// such region was produced.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unmappable: Vec<UnmappableRegion>,
    /// The snapshot the reported dependents were drawn from. Always the current index: there is one
    /// index, so a range-seeded answer's seeds resolve against the range's pre-change side while its
    /// dependents are whatever the index holds now — a distinction a caller reading a range answer
    /// would otherwise have to infer.
    pub dependents_snapshot: &'static str,
    /// The union of the seeds' `dependents` reach.
    pub dependents: DependentsReport,
}

/// Everything an impact assessment needs from outside the query layer, gathered by the command
/// handler so this module performs no discovery, no subprocess, and no filesystem access of its own.
pub struct ImpactRequest<'a> {
    /// The seed mode the caller chose, for the answer's mode label and the recovery re-run line.
    pub mode: &'a SeedMode,
    /// The path narrowing the caller applied, for the recovery re-run line.
    pub paths: &'a [PathBuf],
    /// The pre-change revision the boundary resolved for that mode.
    pub base_revision: &'a str,
    /// The patch's per-file entries, already parsed onto the pre-change side.
    pub changes: &'a [FileChange],
    /// The pre-change content of each touched file, keyed by its pre-change path — the text the
    /// pre-change byte offsets index into.
    pub pre_contents: &'a BTreeMap<String, String>,
    /// The content hash of the workspace's discovered sources with this change reverted: the
    /// change's pre-change side as `build` would have seen it.
    pub pre_change_hash: &'a str,
    /// The content hash the index recorded at build time.
    pub index_hash: &'a str,
    /// The index path and workspace identity a recovery recipe must name.
    pub db: &'a str,
    /// The workspace identity a recovery recipe must name.
    pub workspace: &'a str,
    /// The workspace root's path prefix within the repository (trailing `/` included when
    /// non-empty), so the recovery recipe rebuilds the workspace subtree — `<worktree>/<prefix>` —
    /// rather than the repository root.
    pub prefix: &'a str,
    /// Whether the change's pre-change side can actually be reconstructed from this workspace.
    ///
    /// A hash match alone does not certify exactness: the pre-change side is rebuilt by substituting
    /// each changed file's base content wholesale, and that substitution is lossy whenever a file
    /// carries edits the selected diff does not describe (a staged file with further unstaged edits,
    /// or a revision range whose head is not what is checked out) — the substitution can then land
    /// on a state the index happens to match by hash while still not describing the workspace.
    /// `Exactness::Exact` requires both `pre_change_hash == index_hash` and this.
    pub reconstructible: bool,
    /// The discovered sources git does not track — exactly "discovered but untracked" — that the
    /// recovery recipe's copy step must materialize into the throwaway worktree, since a bare `git
    /// worktree add` populates only tracked files.
    pub untracked_sources: &'a [String],
    /// The resolved head revision of a revision-range seed, so the recovery recipe can re-run its
    /// query from a worktree at the range's head — the only tree from which reverting the range's
    /// diff reconstructs the base. `None` for `WorkingTree`, `Staged`, and a bare single-revision
    /// spec, none of which name a range.
    pub range_head: Option<&'a str>,
    /// The depth bound the detailed dependents run to.
    pub depth: u32,
    /// The order selector for the detailed dependent rows.
    pub order: OrderMode,
}

impl QueryEngine<'_> {
    /// `impact`: the reverse-reachability impact of a change, seeded from its diff rather than from a
    /// symbol the caller names.
    pub fn impact(&self, request: &ImpactRequest<'_>) -> Result<Answer<ImpactReport>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;

        let (seeds, unmappable) = self.resolve_seeds(request)?;

        let seed_views: Vec<SeedView> = seeds
            .values()
            .map(|row| SeedView {
                symbol: super::symbol_view(row),
                location: super::location_of(row),
            })
            .collect();

        let dependents = self.union_dependents(seeds.keys(), request.depth, request.order)?;

        let exactness = if request.pre_change_hash == request.index_hash && request.reconstructible {
            Exactness::Exact
        } else {
            Exactness::Approximate {
                recovery: recovery_recipe(request),
            }
        };

        let seed_outcome = if !seed_views.is_empty() {
            SeedOutcome::Seeded
        } else if !unmappable.is_empty() {
            SeedOutcome::NoneResolvable
        } else {
            SeedOutcome::NoIndexedSymbolTouched
        };

        let report = ImpactReport {
            seed_mode: mode_label(request.mode),
            base_revision: request.base_revision.to_string(),
            exactness,
            seed_outcome,
            seeds: seed_views,
            unmappable,
            dependents_snapshot: "current_index",
            dependents,
        };
        Ok(Answer::found(vec![report], provenance, freshness))
    }

    /// Resolve every changed pre-change region in `request` to the seed declarations it touched,
    /// applying the seed rule (overlap, minimality under containment, whole-file-span exclusion) to
    /// each region and deduplicating survivors by canonical identity across hunks and files.
    ///
    /// A region in a document the index does not hold, or a file whose pre-change content the
    /// boundary could not read, is disclosed as an [`UnmappableRegion`] rather than silently dropped.
    fn resolve_seeds(
        &self,
        request: &ImpactRequest<'_>,
    ) -> Result<(BTreeMap<CanonicalId, SymbolRow>, Vec<UnmappableRegion>), QueryError> {
        let mut seeds: BTreeMap<CanonicalId, SymbolRow> = BTreeMap::new();
        let mut unmappable: Vec<UnmappableRegion> = Vec::new();

        for change in request.changes {
            // A file the change added has no pre-change side, so it contributes nothing — a newly
            // added declaration has no pre-existing dependents.
            let Some(pre_path) = &change.pre_path else { continue };

            let Some(pre_text) = request.pre_contents.get(pre_path) else {
                // The boundary could not read this file's pre-change side at all: one disclosed
                // region for the whole file, since there is no content to derive per-range bounds
                // from.
                unmappable.push(UnmappableRegion {
                    document_path: pre_path.clone(),
                    span_start: 0,
                    span_end: 0,
                });
                continue;
            };

            let holds = self.store.holds_document(pre_path)?;
            // One `LineIndex` per file, reused across every hunk it carries.
            let line_index = LineIndex::new(pre_text);

            for range in &change.pre_ranges {
                let (start, end) = line_index.byte_range(*range);
                if !holds {
                    unmappable.push(UnmappableRegion {
                        document_path: pre_path.clone(),
                        span_start: start,
                        span_end: end,
                    });
                    continue;
                }
                // A range resolving to no symbol in a document the index does hold is a benign
                // no-declaration region — contributes nothing, and is not unmappable.
                let overlapping = self.store.symbols_overlapping_span(pre_path, start, end)?;
                let minimal = minimal_under_containment(overlapping);
                let minimal = if range.count == 0 {
                    drop_insertion_anchored_declaration_ends(minimal, &line_index, start)
                } else {
                    minimal
                };
                for row in drop_whole_file_span(minimal, pre_text) {
                    seeds.entry(row.canonical_id.clone()).or_insert(row);
                }
            }
        }

        unmappable.sort_by(|a, b| {
            a.document_path
                .cmp(&b.document_path)
                .then_with(|| a.span_start.cmp(&b.span_start))
                .then_with(|| a.span_end.cmp(&b.span_end))
        });
        Ok((seeds, unmappable))
    }

    /// The union of `dependents` reached from every seed: one combined walk over the whole seed set
    /// (see [`GraphStore::dependents_of_seeds`](crate::graph::store::GraphStore::dependents_of_seeds)),
    /// so each dependent arrives exactly once at its shortest distance from any seed, under the same
    /// order selector a single-seed answer applies — then split at `depth`
    /// exactly as [`QueryEngine::dependents`] splits a single-seed walk.
    fn union_dependents<'a>(
        &self,
        seed_ids: impl Iterator<Item = &'a CanonicalId>,
        depth: u32,
        order: OrderMode,
    ) -> Result<DependentsReport, QueryError> {
        let seeds: Vec<CanonicalId> = seed_ids.cloned().collect();
        let mut rows = self.store.dependents_of_seeds(&seeds, DEPENDENTS_HORIZON)?;
        self.order_dependent_rows(&mut rows, order)?;

        // No detail level: a multi-seed walk answers with location-only rows.
        self.dependents_report(&rows, depth, None, None)
    }
}

/// Keep only the candidates minimal under span containment: drop any candidate whose span strictly
/// contains another candidate's span (`a.start <= b.start && b.end <= a.end && (a.start, a.end) !=
/// (b.start, b.end)`). Equal spans — same-descriptor twins — never eliminate each other, so both
/// survive.
///
/// Definition spans nest, so plain overlap would attach every edit in a file to every declaration
/// enclosing it; keeping only the minimal spans attaches a hunk to the narrowest declaration it
/// actually falls inside — a hunk inside a method seeds the method, not its enclosing type.
fn minimal_under_containment(rows: Vec<SymbolRow>) -> Vec<SymbolRow> {
    let spans: Vec<(usize, usize)> = rows
        .iter()
        .map(|r| r.span.expect("query selects span_start IS NOT NULL"))
        .collect();
    let mut keep = vec![true; rows.len()];
    for i in 0..rows.len() {
        for j in 0..rows.len() {
            if i == j {
                continue;
            }
            let (a_start, a_end) = spans[i];
            let (b_start, b_end) = spans[j];
            if a_start <= b_start && b_end <= a_end && (a_start, a_end) != (b_start, b_end) {
                keep[i] = false;
                break;
            }
        }
    }
    rows.into_iter()
        .zip(keep)
        .filter_map(|(row, k)| k.then_some(row))
        .collect()
}

/// For a pure insertion (anchored rather than covering removed text), drop any candidate whose span's
/// final line starts at or before `anchor` — the byte offset the insertion's anchor line begins at.
///
/// An insertion has no pre-change text of its own, so the anchor (the start of the pre-change line it
/// follows) is the only evidence of where it landed. That evidence is sound for an interior line of a
/// declaration's body, but not when the anchor happens to fall on the declaration's own final line: a
/// declaration inserted immediately after another one's closing brace anchors there too, and would
/// otherwise report every dependent of a neighbour the caller never touched. Excluding a candidate
/// whose final line starts at or before the anchor separates the two cases exactly: an interior
/// insertion still seeds, while an insertion that only abuts a declaration's end does not.
fn drop_insertion_anchored_declaration_ends(
    rows: Vec<SymbolRow>,
    line_index: &LineIndex,
    anchor: usize,
) -> Vec<SymbolRow> {
    rows.into_iter()
        .filter(|r| {
            let (_, end) = r.span.expect("query selects span_start IS NOT NULL");
            let final_line = line_index.line_of(end.saturating_sub(1));
            let final_line_start = line_index.line_start(final_line);
            anchor < final_line_start
        })
        .collect()
}

/// Drop any candidate whose span reaches the end of the pre-change document apart from trailing
/// whitespace — a file module's span, which would otherwise attach every edit in the file to it and
/// make the typed-empty answer unreachable. An inline `mod` block's span is the block rather than the
/// file, so it is unaffected unless the block is itself the whole file.
///
/// The tail is compared after trimming rather than by exact length because a declaration's parsed
/// span stops at its last token: whether a file module's span takes in the file's final newline is a
/// property of the grammar, not of the change being assessed, and a one-byte difference must not
/// decide whether a comment-only edit reports the file's entire dependent set. A span reaching past
/// the pre-change text (an index built from different content) also counts as covering it.
fn drop_whole_file_span(rows: Vec<SymbolRow>, pre_text: &str) -> Vec<SymbolRow> {
    rows.into_iter()
        .filter(|r| {
            let (start, end) = r.span.expect("query selects span_start IS NOT NULL");
            let covers_document = start == 0 && pre_text.get(end..).is_none_or(|tail| tail.trim().is_empty());
            !covers_document
        })
        .collect()
}

/// The mode label an answer reports.
fn mode_label(mode: &SeedMode) -> &'static str {
    match mode {
        SeedMode::WorkingTree => "working_tree",
        SeedMode::Staged => "staged",
        SeedMode::Revspec(_) => "range",
    }
}

/// Build the recovery recipe for an approximate answer: the shell steps that build a throwaway
/// index at `request.base_revision` and re-run the same query against it — never against
/// `request.db`, the index this answer was drawn from, since a query must not hand the caller a
/// procedure whose side effect destroys the state it just read (design decision
/// `RecoveryNeverTouchesTheQueriedIndex`).
///
/// For a revision-range seed (`request.range_head.is_some()`), a second worktree is materialized at
/// the range's head and the re-query runs from it in a subshell: reverting the range's diff from
/// anywhere but the head yields a hybrid no index can match, so only a re-run from the head's tree
/// reconstructs the base and lets the procedure actually converge (design decision
/// `RangeRecoveryQueriesFromTheRangeHead`). Untracked files belong to neither commit a range
/// straddles, so the copy step is skipped for a range.
///
/// The recipe is emitted, never executed — `impact` is a query and must not mutate anything.
fn recovery_recipe(request: &ImpactRequest<'_>) -> RecoveryRecipe {
    // The throwaway paths are named through a shell variable rather than baked in, so the recipe
    // honors `TMPDIR` where a per-user temporary directory is the convention — and where a bare
    // `/tmp` may not be writable at all — while staying one paste-able block. The name is
    // deterministic rather than random, so the recipe is reproducible.
    let mut steps = Vec::new();
    steps.push(format!(
        "C10R_TMP=\"${{TMPDIR:-/tmp}}/c10r-exact-{}\"",
        &request.base_revision[..request.base_revision.len().min(8)]
    ));
    // Shell words that expand to the throwaway worktree and its index. The index sits beside the
    // worktree and is never `request.db`: the range re-run step below changes directory, so a
    // relative `--db` would break, and building into the caller's live index would replace it with
    // one built at a historical revision.
    let tmp = "\"$C10R_TMP\"";
    let tmp_db = "\"$C10R_TMP.db\"";
    let tmp_head = request.range_head.map(|_| "\"$C10R_TMP_HEAD\"");
    if let Some(head) = request.range_head {
        steps.push(format!(
            "C10R_TMP_HEAD=\"${{TMPDIR:-/tmp}}/c10r-exact-{}-head\"",
            &head[..head.len().min(8)]
        ));
    }

    steps.push(format!("git worktree add {tmp} {}", shell_quote(request.base_revision)));
    if let (Some(head), Some(tmp_head)) = (request.range_head, tmp_head) {
        steps.push(format!("git worktree add {tmp_head} {}", shell_quote(head)));
    }

    if tmp_head.is_none() && !request.untracked_sources.is_empty() {
        // Each untracked source is a shell-quoted word in the loop's list, never spliced in by
        // textual substitution, so a name carrying a space or a quote cannot break the command; `--`
        // guards both `dirname` and `cp` against an option-looking name. The destination expands
        // from the variable set above, inside the double quotes that already protect it.
        let names: Vec<String> = request.untracked_sources.iter().map(|f| shell_quote(f)).collect();
        steps.push(format!(
            "for f in {}; do mkdir -p \"$C10R_TMP/$(dirname -- \"$f\")\" && cp -- \"$f\" \"$C10R_TMP/$f\"; done",
            names.join(" ")
        ));
    }

    steps.push(
        "# --workspace is load-bearing: build derives the workspace identity from the root directory name, so a \
         worktree in a differently-named directory would namespace symbols wrongly and every seed lookup would miss."
            .to_string(),
    );
    // The workspace's own directory inside the worktree, not the repository root: a workspace below
    // the root would otherwise be rebuilt from the wrong tree.
    let build_target = with_prefix(tmp, request.prefix);
    steps.push(format!(
        "c10r build {build_target} --db {tmp_db} --workspace {}",
        shell_quote(request.workspace)
    ));

    let mode_args = match request.mode {
        SeedMode::WorkingTree => None,
        SeedMode::Staged => Some("--staged".to_string()),
        SeedMode::Revspec(spec) => Some(shell_quote(spec)),
    };
    let mut rerun = "c10r impact".to_string();
    if let Some(args) = mode_args {
        rerun.push(' ');
        rerun.push_str(&args);
    }
    // The re-run reproduces every answer-shaping parameter of the original question — the order
    // selector included, or a caller who asked for `unranked` would get back an "exact" answer
    // whose first bounded page is ordered by the ranked default they did not ask for.
    rerun.push_str(&format!(
        " --depth {} --order {} --db {tmp_db}",
        request.depth,
        request.order.label()
    ));
    if !request.paths.is_empty() {
        rerun.push_str(" -- ");
        let quoted: Vec<String> = request
            .paths
            .iter()
            .map(|p| shell_quote(&p.to_string_lossy()))
            .collect();
        rerun.push_str(&quoted.join(" "));
    }

    if let Some(tmp_head) = tmp_head {
        steps.push(
            "# reverting the range's diff only reconstructs the base from a worktree at the range's \
             head; re-running from the original workspace would recompute the same unreconstructable hybrid"
                .to_string(),
        );
        // The workspace's own directory inside the head worktree, for the same reason the build step
        // targets it inside the base worktree: the re-run's discovered set must be the workspace's
        // sources, not the repository root's, or its pre-change hash could never match an index built
        // over the workspace subtree and the procedure would never converge.
        steps.push(format!("( cd {} && {rerun} )", with_prefix(tmp_head, request.prefix)));
    } else {
        steps.push(
            "# re-run from the original workspace directory, not the worktree — the in-flight edits live there"
                .to_string(),
        );
        steps.push(rerun);
    }

    // `--force` because the copy step deliberately leaves untracked files in the worktree, and a
    // plain `git worktree remove` refuses to delete a worktree carrying any. Discarding these is the
    // whole intent: the worktree is a throwaway this recipe created moments earlier at a temporary
    // path, holding nothing the caller did not already have.
    steps.push(format!("git worktree remove --force {tmp}"));
    if let Some(tmp_head) = tmp_head {
        steps.push(format!("git worktree remove --force {tmp_head}"));
    }
    steps.push(format!("rm -f {tmp_db}"));

    RecoveryRecipe {
        base_revision: request.base_revision.to_string(),
        db: tmp_db.to_string(),
        workspace: request.workspace.to_string(),
        steps,
    }
}

/// The workspace's own directory inside a throwaway worktree, as a shell word: `worktree` alone when
/// the workspace *is* the repository root, and the worktree with the workspace's repository-relative
/// `prefix` appended otherwise.
///
/// The prefix is appended as its own quoted word beside the expanded variable, so adjacent-quote
/// concatenation keeps it literal.
fn with_prefix(worktree: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        worktree.to_string()
    } else {
        format!("{worktree}{}", shell_quote(&format!("/{prefix}")))
    }
}

/// Quote `value` as a single shell word: wrap it in single quotes, escaping an embedded single quote
/// as `'\''`, so a path or revision containing a space or a quote cannot break the emitted line.
pub(crate) fn shell_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::join::JoinAccounting;
    use crate::graph::store::{EdgeKind, GraphStore, IndexMetadata, PersistedClass};
    use crate::identity::WorkspaceId;
    use crate::query::HorizonDisclosure;
    use crate::query::diff::{ChangeKind, LineRange};
    use crate::query::output::Outcome;
    use crate::semantic::model::AnalyzerProvenance;

    /// An in-workspace symbol row with a definition span, minimal in every other field.
    fn symbol_at(id: &str, document: &str, start: usize, end: usize) -> SymbolRow {
        SymbolRow {
            canonical_id: CanonicalId::from_raw(id.to_string()),
            display_name: id.rsplit("::").next().unwrap_or(id).to_string(),
            kind: "function".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: Some(document.to_string()),
            span: Some((start, end)),
            span_text: None,
            signature_text: None,
            interface_text: None,
            duplicated: false,
            test_rule: None,
        }
    }

    fn write_metadata(store: &GraphStore, content_hash: &str) {
        store
            .write_metadata(&IndexMetadata {
                workspace_id: WorkspaceId::new("ws"),
                workspace_root: Some("/ws".to_string()),
                provenance: AnalyzerProvenance {
                    analyzer_name: "test".to_string(),
                    analyzer_version: "0".to_string(),
                },
                content_hash: content_hash.to_string(),
                accounting: JoinAccounting::default(),
                environment: None,
                chunk_params: Default::default(),
            })
            .unwrap();
    }

    fn engine<'a>(store: &'a GraphStore, content_hash: &str) -> QueryEngine<'a> {
        QueryEngine::new(
            store,
            AnalyzerProvenance {
                analyzer_name: "test".to_string(),
                analyzer_version: "0".to_string(),
            },
            content_hash.to_string(),
            None,
        )
    }

    fn modified_change(pre_path: &str, ranges: Vec<LineRange>) -> FileChange {
        FileChange {
            kind: ChangeKind::Modified,
            pre_path: Some(pre_path.to_string()),
            post_path: Some(pre_path.to_string()),
            pre_ranges: ranges,
        }
    }

    fn request<'a>(
        changes: &'a [FileChange],
        pre_contents: &'a BTreeMap<String, String>,
        pre_change_hash: &'a str,
        index_hash: &'a str,
    ) -> ImpactRequest<'a> {
        ImpactRequest {
            mode: &SeedMode::WorkingTree,
            paths: &[],
            base_revision: "abcdef0123456789",
            changes,
            pre_contents,
            pre_change_hash,
            index_hash,
            db: "index.db",
            workspace: "ws",
            prefix: "",
            reconstructible: true,
            untracked_sources: &[],
            range_head: None,
            depth: 1,
            order: OrderMode::Unranked,
        }
    }

    // A file's whole text, a method inside a type inside a file module: three nested spans, plus a
    // second sibling method, laid out so byte offsets are easy to reason about.
    struct Fixture {
        store: GraphStore,
        text: String,
    }

    fn build_fixture() -> Fixture {
        let store = GraphStore::open_in_memory().unwrap();
        let text = "mod m {\nimpl T {\nfn one() {\n  body1\n}\nfn two() {\n  body2\n}\n}\n}\n".to_string();
        // module spans the whole file.
        store
            .insert_symbol(&symbol_at("ws::doc::m", "doc.rs", 0, text.len()))
            .unwrap();
        let type_start = text.find("impl T").unwrap();
        let type_end = text.rfind("}\n}\n").unwrap() + "}\n".len(); // ends after the type's closing brace
        store
            .insert_symbol(&symbol_at("ws::doc::m::T", "doc.rs", type_start, type_end))
            .unwrap();
        let one_start = text.find("fn one").unwrap();
        let one_end = text.find("fn two").unwrap();
        store
            .insert_symbol(&symbol_at("ws::doc::m::T::one", "doc.rs", one_start, one_end))
            .unwrap();
        let two_start = text.find("fn two").unwrap();
        let two_end = type_end - "}\n".len();
        store
            .insert_symbol(&symbol_at("ws::doc::m::T::two", "doc.rs", two_start, two_end))
            .unwrap();
        Fixture { store, text }
    }

    // The seed rule: a hunk inside a method seeds the method, not its enclosing type nor the
    // file-spanning module.
    #[test]
    fn hunk_inside_a_method_seeds_only_the_method() {
        let fixture = build_fixture();
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);

        // A hunk touching the `body1` line inside `one`.
        let body1_line = fixture.text.lines().position(|l| l.contains("body1")).unwrap() as u32 + 1;
        let change = modified_change(
            "doc.rs",
            vec![LineRange {
                start: body1_line,
                count: 1,
            }],
        );
        let changes = vec![change];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("doc.rs".to_string(), fixture.text.clone());
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        let report = &results[0];
        assert_eq!(report.seeds.len(), 1, "{:?}", report.seeds);
        assert_eq!(report.seeds[0].symbol.canonical_id.as_str(), "ws::doc::m::T::one");
    }

    // A hunk straddling two methods seeds both.
    #[test]
    fn hunk_straddling_two_methods_seeds_both() {
        let fixture = build_fixture();
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);

        let one_line = fixture.text.lines().position(|l| l.contains("fn one")).unwrap() as u32 + 1;
        let two_line = fixture.text.lines().position(|l| l.contains("body2")).unwrap() as u32 + 1;
        let count = two_line - one_line + 1;
        let change = modified_change("doc.rs", vec![LineRange { start: one_line, count }]);
        let changes = vec![change];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("doc.rs".to_string(), fixture.text.clone());
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        let mut ids: Vec<&str> = results[0]
            .seeds
            .iter()
            .map(|s| s.symbol.canonical_id.as_str())
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["ws::doc::m::T::one", "ws::doc::m::T::two"]);
    }

    // A hunk in the gap between declarations — overlapping only the file-spanning module — seeds
    // nothing and produces no unmappable region.
    #[test]
    fn hunk_in_the_gap_seeds_nothing_and_is_not_unmappable() {
        let fixture = build_fixture();
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);

        // Line 1 is `mod m {`, outside the type and both methods, inside only the module.
        let change = modified_change("doc.rs", vec![LineRange { start: 1, count: 1 }]);
        let changes = vec![change];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("doc.rs".to_string(), fixture.text.clone());
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        assert!(results[0].seeds.is_empty());
        assert!(results[0].unmappable.is_empty());
        assert_eq!(results[0].seed_outcome, SeedOutcome::NoIndexedSymbolTouched);
    }

    /// Two adjacent, non-nested declarations (`one` then `two`), with or without a blank line between
    /// them, so an insertion anchor can be placed on an interior line, on the first declaration's own
    /// final line, or in the gap.
    struct AdjacentFixture {
        store: GraphStore,
        text: String,
    }

    fn build_adjacent_fixture(blank_line_between: bool) -> AdjacentFixture {
        let store = GraphStore::open_in_memory().unwrap();
        let text = if blank_line_between {
            "fn one() {\n    body1\n}\n\nfn two() {\n    body2\n}\n".to_string()
        } else {
            "fn one() {\n    body1\n}\nfn two() {\n    body2\n}\n".to_string()
        };
        let one_start = text.find("fn one").unwrap();
        let two_start = text.find("fn two").unwrap();
        // `one`'s span ends right after its own closing brace line — at `two`'s start when the two
        // are adjacent, or at the start of the blank line when one separates them.
        let one_end = if blank_line_between {
            text.find("}\n\n").unwrap() + "}\n".len()
        } else {
            two_start
        };
        store
            .insert_symbol(&symbol_at("ws::doc::one", "doc.rs", one_start, one_end))
            .unwrap();
        store
            .insert_symbol(&symbol_at("ws::doc::two", "doc.rs", two_start, text.len()))
            .unwrap();
        AdjacentFixture { store, text }
    }

    // An insertion anchored on an interior line of a declaration's body still seeds that declaration:
    // the anchor is real evidence the caller's change landed inside it.
    #[test]
    fn insertion_inside_a_method_body_still_seeds_it() {
        let fixture = build_adjacent_fixture(false);
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);

        let body1_line = fixture.text.lines().position(|l| l.contains("body1")).unwrap() as u32 + 1;
        let change = modified_change(
            "doc.rs",
            vec![LineRange {
                start: body1_line,
                count: 0,
            }],
        );
        let changes = vec![change];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("doc.rs".to_string(), fixture.text.clone());
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        let ids: Vec<&str> = results[0]
            .seeds
            .iter()
            .map(|s| s.symbol.canonical_id.as_str())
            .collect();
        assert_eq!(ids, vec!["ws::doc::one"], "{:?}", results[0]);
    }

    // A declaration inserted immediately after another one's closing brace — no blank line between —
    // seeds neither: the anchor sits on the preceding declaration's own final line, which is the
    // neighbour's evidence, not the caller's.
    #[test]
    fn insertion_immediately_after_a_declaration_seeds_neither() {
        let fixture = build_adjacent_fixture(false);
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);

        // Line 3 is `one`'s closing brace; the insertion follows it.
        let close_line = fixture.text.lines().position(|l| l == "}").unwrap() as u32 + 1;
        let change = modified_change(
            "doc.rs",
            vec![LineRange {
                start: close_line,
                count: 0,
            }],
        );
        let changes = vec![change];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("doc.rs".to_string(), fixture.text.clone());
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        assert!(results[0].seeds.is_empty(), "{:?}", results[0]);
    }

    // The same insertion point, but with a blank line separating the two declarations, still seeds
    // neither — source formatting must not decide whether a neighbour's dependents get reported.
    #[test]
    fn insertion_after_a_declaration_with_a_blank_line_between_seeds_neither() {
        let fixture = build_adjacent_fixture(true);
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);

        // The blank line is line 4; the insertion follows it.
        let blank_line = fixture.text.lines().position(|l| l.is_empty()).unwrap() as u32 + 1;
        let change = modified_change(
            "doc.rs",
            vec![LineRange {
                start: blank_line,
                count: 0,
            }],
        );
        let changes = vec![change];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("doc.rs".to_string(), fixture.text.clone());
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        assert!(results[0].seeds.is_empty(), "{:?}", results[0]);
    }

    // A removal spanning a whole declaration is unaffected by the insertion-anchor exclusion: it is a
    // real edit to that declaration's own text, not a bare anchor, so it still seeds it.
    #[test]
    fn removal_spanning_a_declaration_is_unaffected() {
        let fixture = build_adjacent_fixture(false);
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);

        // Lines 1-3 are exactly `one`'s span.
        let change = modified_change("doc.rs", vec![LineRange { start: 1, count: 3 }]);
        let changes = vec![change];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("doc.rs".to_string(), fixture.text.clone());
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        let ids: Vec<&str> = results[0]
            .seeds
            .iter()
            .map(|s| s.symbol.canonical_id.as_str())
            .collect();
        assert_eq!(ids, vec!["ws::doc::one"], "{:?}", results[0]);
    }

    // A declaration touched by two hunks appears once.
    #[test]
    fn declaration_touched_by_two_hunks_appears_once() {
        let fixture = build_fixture();
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);

        let body1_line = fixture.text.lines().position(|l| l.contains("body1")).unwrap() as u32 + 1;
        let one_line = fixture.text.lines().position(|l| l.contains("fn one")).unwrap() as u32 + 1;
        let change = modified_change(
            "doc.rs",
            vec![
                LineRange {
                    start: one_line,
                    count: 1,
                },
                LineRange {
                    start: body1_line,
                    count: 1,
                },
            ],
        );
        let changes = vec![change];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("doc.rs".to_string(), fixture.text.clone());
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        assert_eq!(results[0].seeds.len(), 1);
    }

    // Two symbols with identical spans both survive the minimality filter: a same-descriptor twin
    // pair, neither of which strictly contains the other, so neither is dropped. The twins' shared
    // span is narrower than the file, so the whole-file-span exclusion never enters into it — this
    // test isolates the minimality rule alone.
    #[test]
    fn identical_spans_both_survive_minimality() {
        let store = GraphStore::open_in_memory().unwrap();
        let doc = "aaaa fn twin() { body } bbbb\n".to_string();
        let start = doc.find("fn twin").unwrap();
        let end = doc.find('}').unwrap() + 1;
        store
            .insert_symbol(&symbol_at("ws::doc::twin#0", "doc.rs", start, end))
            .unwrap();
        store
            .insert_symbol(&symbol_at("ws::doc::twin#1", "doc.rs", start, end))
            .unwrap();
        let hash = "h";
        write_metadata(&store, hash);
        let engine = engine(&store, hash);

        let change = modified_change("doc.rs", vec![LineRange { start: 1, count: 1 }]);
        let changes = vec![change];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("doc.rs".to_string(), doc);
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        let mut ids: Vec<&str> = results[0]
            .seeds
            .iter()
            .map(|s| s.symbol.canonical_id.as_str())
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["ws::doc::twin#0", "ws::doc::twin#1"]);
    }

    // A range in a document the index does not hold records an unmappable region; a range in a held
    // document that hits no declaration does not.
    #[test]
    fn unmappable_region_recorded_only_for_an_unheld_document() {
        let fixture = build_fixture();
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);

        let unheld_text = "line one\nline two\n".to_string();
        let change = modified_change("other.rs", vec![LineRange { start: 1, count: 1 }]);
        let changes = vec![change];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("other.rs".to_string(), unheld_text);
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        assert_eq!(results[0].unmappable.len(), 1);
        assert_eq!(results[0].seed_outcome, SeedOutcome::NoneResolvable);
    }

    // A `pre_path` the boundary could not read at all (no entry in `pre_contents`) records one
    // whole-file unmappable region at `(0, 0)`, since there is no content to derive per-range byte
    // offsets from — distinct from the held-document and unheld-document cases above, which both
    // have content to index into.
    #[test]
    fn unreadable_pre_change_content_records_a_whole_file_unmappable_region() {
        let fixture = build_fixture();
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);

        let change = modified_change("doc.rs", vec![LineRange { start: 1, count: 1 }]);
        let changes = vec![change];
        // `pre_contents` carries no entry for "doc.rs" at all, even though the store holds it.
        let pre_contents = BTreeMap::new();
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        assert_eq!(results[0].unmappable.len(), 1);
        assert_eq!(results[0].unmappable[0].document_path, "doc.rs");
        assert_eq!(results[0].unmappable[0].span_start, 0);
        assert_eq!(results[0].unmappable[0].span_end, 0);
        assert_eq!(results[0].seed_outcome, SeedOutcome::NoneResolvable);
    }

    // A file the change added contributes no seed.
    #[test]
    fn added_file_contributes_no_seed() {
        let fixture = build_fixture();
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);

        let change = FileChange {
            kind: ChangeKind::Added,
            pre_path: None,
            post_path: Some("new.rs".to_string()),
            pre_ranges: vec![LineRange { start: 0, count: 0 }],
        };
        let changes = vec![change];
        let pre_contents = BTreeMap::new();
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        assert!(results[0].seeds.is_empty());
        assert!(results[0].unmappable.is_empty());
        assert_eq!(results[0].seed_outcome, SeedOutcome::NoIndexedSymbolTouched);
    }

    // The dependents union: a dependent reachable from two seeds appears once at its shortest
    // distance; the horizon disclosure is `BeyondBound` when a merged row exceeds the depth bound
    // and `EndsWithinBound` when none does.
    #[test]
    fn dependents_union_dedupes_and_discloses_the_bound() {
        let store = GraphStore::open_in_memory().unwrap();
        let seed_a = CanonicalId::from_raw("ws::a");
        let seed_b = CanonicalId::from_raw("ws::b");
        let dep = CanonicalId::from_raw("ws::dep");
        let far = CanonicalId::from_raw("ws::far");
        for row in [
            symbol_at("ws::a", "doc.rs", 0, 5),
            symbol_at("ws::b", "doc.rs", 10, 15),
            symbol_at("ws::dep", "doc.rs", 20, 25),
            symbol_at("ws::far", "doc.rs", 30, 35),
        ] {
            store.insert_symbol(&row).unwrap();
        }
        // dep depends on both a and b (reachable from both seeds at depth 1).
        store.insert_edge(EdgeKind::Uses, &dep, &seed_a).unwrap();
        store.insert_edge(EdgeKind::Uses, &dep, &seed_b).unwrap();
        // far depends on dep, at depth 2 from either seed.
        store.insert_edge(EdgeKind::Uses, &far, &dep).unwrap();

        let hash = "h";
        write_metadata(&store, hash);
        let engine = engine(&store, hash);

        let report = engine
            .union_dependents([&seed_a, &seed_b].into_iter(), 1, OrderMode::Unranked)
            .unwrap();
        assert_eq!(report.detail.len(), 1, "{:?}", report.detail);
        assert_eq!(report.detail[0].symbol.canonical_id, dep);
        assert_eq!(report.detail[0].distance, 1);
        assert_eq!(report.beyond_bound.len(), 1);
        assert_eq!(report.disclosure, HorizonDisclosure::BeyondBound);

        // With a depth bound covering both hops, reach ends within bound.
        let report = engine
            .union_dependents([&seed_a, &seed_b].into_iter(), 2, OrderMode::Unranked)
            .unwrap();
        assert_eq!(report.disclosure, HorizonDisclosure::EndsWithinBound);
        assert_eq!(report.detail.len(), 2);
    }

    // _(Ranked ordering of dependents: impact answers order each layer by importance)_ — the
    // multi-seed union is its own write-site for the ranked ordering: with two seeds whose
    // same-distance dependents differ in codebase-wide importance, the ranked union orders the
    // layer most-important-first while the unranked union keeps identity order.
    #[test]
    fn ranked_union_orders_each_layer_by_importance() {
        let store = GraphStore::open_in_memory().unwrap();
        let seed_a = CanonicalId::from_raw("ws::sa");
        let seed_b = CanonicalId::from_raw("ws::sb");
        for (name, span) in [
            ("ws::sa", 0),
            ("ws::sb", 10),
            ("ws::a_leaf", 20),
            ("ws::z_hub", 30),
            ("ws::u1", 40),
            ("ws::u2", 50),
            ("ws::u3", 60),
        ] {
            store.insert_symbol(&symbol_at(name, "doc.rs", span, span + 5)).unwrap();
        }
        // Both dependents sit at distance 1 (one per seed) under the same edge kind; only their
        // codebase-wide importance differs — three further symbols use `z_hub`.
        let a_leaf = CanonicalId::from_raw("ws::a_leaf");
        let z_hub = CanonicalId::from_raw("ws::z_hub");
        store.insert_edge(EdgeKind::Uses, &a_leaf, &seed_a).unwrap();
        store.insert_edge(EdgeKind::Uses, &z_hub, &seed_b).unwrap();
        for user in ["ws::u1", "ws::u2", "ws::u3"] {
            store
                .insert_edge(EdgeKind::Uses, &CanonicalId::from_raw(user), &z_hub)
                .unwrap();
        }

        let hash = "h";
        write_metadata(&store, hash);
        let engine = engine(&store, hash);

        let ids_under = |order: OrderMode| -> Vec<String> {
            engine
                .union_dependents([&seed_a, &seed_b].into_iter(), 1, order)
                .unwrap()
                .detail
                .iter()
                .map(|d| d.symbol.canonical_id.as_str().to_string())
                .collect()
        };
        assert_eq!(
            ids_under(OrderMode::Ranked),
            vec!["ws::z_hub", "ws::a_leaf"],
            "the ranked layer is most-important-first"
        );
        assert_eq!(
            ids_under(OrderMode::Unranked),
            vec!["ws::a_leaf", "ws::z_hub"],
            "the unranked layer keeps identity order"
        );
    }

    // Multi-seed detailed rows keep the single-seed order — distance, then the fixed kind order,
    // then identity — the ordering continuation tokens bind to. The identities are chosen so a plain
    // identity sort would disagree with the kind order.
    #[test]
    fn dependents_union_orders_rows_like_a_single_seed_answer() {
        let store = GraphStore::open_in_memory().unwrap();
        let seed_1 = CanonicalId::from_raw("ws::s1");
        let seed_2 = CanonicalId::from_raw("ws::s2");
        for (name, span) in [
            ("ws::s1", 0),
            ("ws::s2", 10),
            ("ws::zz", 20),
            ("ws::aa", 30),
            ("ws::mm", 40),
            ("ws::bb", 50),
        ] {
            store.insert_symbol(&symbol_at(name, "doc.rs", span, span + 5)).unwrap();
        }
        store
            .insert_edge(EdgeKind::Uses, &CanonicalId::from_raw("ws::zz"), &seed_1)
            .unwrap();
        store
            .insert_edge(EdgeKind::Imports, &CanonicalId::from_raw("ws::aa"), &seed_2)
            .unwrap();
        store
            .insert_edge(EdgeKind::TypeHierarchy, &CanonicalId::from_raw("ws::mm"), &seed_1)
            .unwrap();
        store
            .insert_edge(
                EdgeKind::Uses,
                &CanonicalId::from_raw("ws::bb"),
                &CanonicalId::from_raw("ws::zz"),
            )
            .unwrap();

        let hash = "h";
        write_metadata(&store, hash);
        let engine = engine(&store, hash);

        let report = engine
            .union_dependents([&seed_1, &seed_2].into_iter(), 2, OrderMode::Unranked)
            .unwrap();
        let ordered: Vec<(&str, u32, &str)> = report
            .detail
            .iter()
            .map(|d| (d.symbol.canonical_id.as_str(), d.distance, d.kind.tag()))
            .collect();
        assert_eq!(
            ordered,
            vec![
                ("ws::zz", 1, "uses"),
                ("ws::aa", 1, "imports"),
                ("ws::mm", 1, "type_hierarchy"),
                ("ws::bb", 2, "uses"),
            ],
            "distance, then kind order, then identity"
        );
    }

    // A multi-seed union whose reach extends to the internal horizon discloses the cut: a dependent
    // at the horizon distance means deeper reach may exist unexplored, and that outranks the
    // beyond-bound disclosure the other seed's shallow reach would produce.
    #[test]
    fn dependents_union_discloses_a_horizon_cut() {
        let store = GraphStore::open_in_memory().unwrap();
        let seed_1 = CanonicalId::from_raw("ws::s1");
        let seed_2 = CanonicalId::from_raw("ws::s2");
        store.insert_symbol(&symbol_at("ws::s1", "doc.rs", 0, 5)).unwrap();
        store.insert_symbol(&symbol_at("ws::s2", "doc.rs", 10, 15)).unwrap();
        store.insert_symbol(&symbol_at("ws::near", "doc.rs", 20, 25)).unwrap();
        store
            .insert_edge(EdgeKind::Uses, &CanonicalId::from_raw("ws::near"), &seed_2)
            .unwrap();
        // A chain hanging off seed 1 exactly `DEPENDENTS_HORIZON` links deep: its last member is
        // reached at the horizon distance, so the walk may have stopped short of deeper reach.
        let mut below = seed_1.clone();
        for i in 1..=DEPENDENTS_HORIZON {
            let id = format!("ws::c{i}");
            store
                .insert_symbol(&symbol_at(
                    &id,
                    "doc.rs",
                    (30 + 10 * i) as usize,
                    (35 + 10 * i) as usize,
                ))
                .unwrap();
            let link = CanonicalId::from_raw(id);
            store.insert_edge(EdgeKind::Uses, &link, &below).unwrap();
            below = link;
        }

        let hash = "h";
        write_metadata(&store, hash);
        let engine = engine(&store, hash);

        let report = engine
            .union_dependents([&seed_1, &seed_2].into_iter(), 1, OrderMode::Unranked)
            .unwrap();
        assert_eq!(report.disclosure, HorizonDisclosure::CutAtHorizon);
        // The detailed rows still hold both seeds' distance-one reach.
        let names: Vec<&str> = report.detail.iter().map(|d| d.symbol.canonical_id.as_str()).collect();
        assert_eq!(names, vec!["ws::c1", "ws::near"], "{names:?}");
    }

    // `Exactness` is `Exact` when the two hashes match and `Approximate` otherwise, and `recovery` is
    // present exactly on the approximate answer.
    #[test]
    fn exactness_and_recovery_track_the_hash_comparison() {
        let fixture = build_fixture();
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);
        let changes: Vec<FileChange> = vec![];
        let pre_contents = BTreeMap::new();

        let exact_req = request(&changes, &pre_contents, hash, hash);
        let answer = engine.impact(&exact_req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        // An exact answer carries the label and no recipe; the type admits no other pairing, so the
        // label alone is the whole assertion.
        assert_eq!(results[0].exactness, Exactness::Exact);

        let approx_req = request(&changes, &pre_contents, "different", hash);
        let answer = engine.impact(&approx_req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        assert!(
            results[0].exactness.is_approximate(),
            "an approximate answer carries the label and its recipe together"
        );
    }

    // `Exactness::Exact` requires reconstructibility too: a hash match produced by a lossy
    // pre-change substitution (`reconstructible: false` stands in for that condition) still grades
    // approximate — a hash match alone does not certify that this workspace can reconstruct the
    // pre-change side the hash was computed against.
    #[test]
    fn exactness_requires_reconstructibility_even_when_the_hash_matches() {
        let fixture = build_fixture();
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);
        let changes: Vec<FileChange> = vec![];
        let pre_contents = BTreeMap::new();

        let mut req = request(&changes, &pre_contents, hash, hash);
        req.reconstructible = false;
        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        assert!(
            results[0].exactness.is_approximate(),
            "a hash match alone does not certify reconstructibility"
        );
    }

    // _(Dependents order selector: the recovery re-run reproduces the selected order)_ — the
    // recipe's re-run line carries the order the caller chose, so following an unranked answer's
    // recipe cannot silently come back under the ranked default; the ranked default is carried
    // explicitly too.
    #[test]
    fn recipe_rerun_carries_the_selected_order() {
        let changes: Vec<FileChange> = vec![];
        let pre_contents = BTreeMap::new();
        for (order, flag) in [
            (OrderMode::Unranked, "--order unranked"),
            (OrderMode::Ranked, "--order ranked"),
        ] {
            let req = ImpactRequest {
                order,
                ..request(&changes, &pre_contents, "different", "h")
            };
            let recipe = recovery_recipe(&req);
            let rerun = recipe
                .steps
                .iter()
                .find(|s| s.contains("c10r impact"))
                .expect("the recipe carries a re-run step");
            assert!(rerun.contains(flag), "the re-run reproduces the order: {rerun}");
        }
    }

    // The recipe carries the substituted base revision and workspace identity, builds into and
    // re-queries a throwaway index rather than the queried one, includes the untracked-copy step
    // exactly when untracked sources are present, and shell-quotes a path containing a space.
    #[test]
    fn recipe_substitutes_values_and_quotes_a_space() {
        let changes: Vec<FileChange> = vec![];
        let pre_contents = BTreeMap::new();
        let untracked = vec!["has space/file.rs".to_string()];
        let req = ImpactRequest {
            mode: &SeedMode::WorkingTree,
            paths: &[PathBuf::from("has space/file.rs")],
            base_revision: "0123456789abcdef",
            changes: &changes,
            pre_contents: &pre_contents,
            pre_change_hash: "different",
            index_hash: "h",
            db: "path with space/index.db",
            workspace: "ws name",
            prefix: "",
            reconstructible: true,
            untracked_sources: &untracked,
            range_head: None,
            depth: 2,
            order: OrderMode::Unranked,
        };
        let recipe = recovery_recipe(&req);
        assert_eq!(recipe.base_revision, "0123456789abcdef");
        assert_eq!(recipe.workspace, "ws name");
        // The throwaway paths are named through a shell variable, so the base-revision derivation
        // lives in the step that sets it rather than in the path text itself.
        assert!(
            recipe.steps[0].contains("c10r-exact-01234567") && recipe.steps[0].contains("TMPDIR"),
            "the throwaway path is derived from the base revision and honors TMPDIR: {recipe:?}"
        );
        assert_ne!(
            recipe.db, "path with space/index.db",
            "the recipe never names the queried index path: {recipe:?}"
        );
        assert!(recipe.steps.iter().any(|s| s.starts_with("for f in")));
        assert!(recipe.steps.iter().any(|s| s.contains("'has space/file.rs'")));

        let req_no_untracked = ImpactRequest {
            untracked_sources: &[],
            ..ImpactRequest {
                mode: &SeedMode::WorkingTree,
                paths: &[],
                base_revision: "0123456789abcdef",
                changes: &changes,
                pre_contents: &pre_contents,
                pre_change_hash: "different",
                index_hash: "h",
                db: "index.db",
                workspace: "ws",
                prefix: "",
                reconstructible: true,
                untracked_sources: &untracked,
                range_head: None,
                depth: 1,
                order: OrderMode::Unranked,
            }
        };
        let recipe = recovery_recipe(&req_no_untracked);
        assert!(!recipe.steps.iter().any(|s| s.starts_with("for f in")));
    }

    // The recipe never names the queried index in its build step, and its cleanup removes the
    // throwaway index it created itself — following the recipe must never leave the caller's live
    // index replaced with a historical one.
    #[test]
    fn recipe_never_builds_into_the_queried_index_and_cleans_up_its_own() {
        let changes: Vec<FileChange> = vec![];
        let pre_contents = BTreeMap::new();
        let req = ImpactRequest {
            mode: &SeedMode::WorkingTree,
            paths: &[],
            base_revision: "0123456789abcdef",
            changes: &changes,
            pre_contents: &pre_contents,
            pre_change_hash: "different",
            index_hash: "h",
            db: ".c10r/index.db",
            workspace: "ws",
            prefix: "",
            reconstructible: true,
            untracked_sources: &[],
            range_head: None,
            depth: 1,
            order: OrderMode::Unranked,
        };
        let recipe = recovery_recipe(&req);
        let build_step = recipe
            .steps
            .iter()
            .find(|s| s.starts_with("c10r build"))
            .expect("a build step");
        assert!(
            !build_step.contains(".c10r/index.db"),
            "the build step must not name the queried index: {build_step}"
        );
        assert!(
            recipe
                .steps
                .iter()
                .any(|s| s.starts_with("rm -f") && s.contains(&recipe.db)),
            "cleanup removes the throwaway index the recipe created: {:?}",
            recipe.steps
        );
    }

    // The build step targets the workspace's prefix inside the throwaway worktree, not the
    // repository root, when the workspace sits below the repository root.
    #[test]
    fn build_step_targets_the_workspace_prefix_inside_the_worktree() {
        let changes: Vec<FileChange> = vec![];
        let pre_contents = BTreeMap::new();
        let req = ImpactRequest {
            mode: &SeedMode::WorkingTree,
            paths: &[],
            base_revision: "0123456789abcdef",
            changes: &changes,
            pre_contents: &pre_contents,
            pre_change_hash: "different",
            index_hash: "h",
            db: "index.db",
            workspace: "ws",
            prefix: "sub/dir/",
            reconstructible: true,
            untracked_sources: &[],
            range_head: None,
            depth: 1,
            order: OrderMode::Unranked,
        };
        let recipe = recovery_recipe(&req);
        let build_step = recipe
            .steps
            .iter()
            .find(|s| s.starts_with("c10r build"))
            .expect("a build step");
        assert!(
            build_step.contains("sub/dir/'"),
            "the build target carries the workspace prefix, trailing slash unstripped: {build_step}"
        );
    }

    // A range recipe's re-run enters the workspace's own directory inside the head worktree, not the
    // head worktree's root. The build step already targets the workspace subtree inside the base
    // worktree, so a re-run from the repository root would discover a different source set than the
    // index was built over — the hash could never match and the procedure would never converge, which
    // is precisely the exactness the recipe promises.
    #[test]
    fn range_rerun_enters_the_workspace_prefix_inside_the_head_worktree() {
        let changes: Vec<FileChange> = vec![];
        let pre_contents = BTreeMap::new();
        let req = ImpactRequest {
            mode: &SeedMode::Revspec("aaaa..bbbb".to_string()),
            paths: &[],
            base_revision: "0123456789abcdef",
            changes: &changes,
            pre_contents: &pre_contents,
            pre_change_hash: "different",
            index_hash: "h",
            db: "index.db",
            workspace: "ws",
            prefix: "sub/dir/",
            reconstructible: false,
            untracked_sources: &[],
            range_head: Some("fedcba9876543210"),
            depth: 1,
            order: OrderMode::Unranked,
        };
        let recipe = recovery_recipe(&req);
        let rerun = recipe
            .steps
            .iter()
            .find(|s| s.contains("c10r impact"))
            .expect("a re-run step");
        assert!(
            rerun.contains("\"$C10R_TMP_HEAD\"'/sub/dir/'"),
            "the range re-run enters the workspace prefix inside the head worktree: {rerun}"
        );

        // The same recipe with the workspace at the repository root enters the worktree itself, with
        // no stray prefix word — the prefix is appended only when there is one.
        let root_req = ImpactRequest { prefix: "", ..req };
        let root_recipe = recovery_recipe(&root_req);
        let root_rerun = root_recipe
            .steps
            .iter()
            .find(|s| s.contains("c10r impact"))
            .expect("a re-run step");
        assert!(
            root_rerun.contains("( cd \"$C10R_TMP_HEAD\" &&"),
            "a root workspace's re-run enters the head worktree itself: {root_rerun}"
        );
    }

    // The copy step is emitted exactly when the untracked list is non-empty, shell-quotes a name
    // containing a space, and guards both `dirname` and `cp` with `--` so an option-looking name
    // cannot break the command it is spliced into.
    #[test]
    fn copy_step_is_emitted_exactly_for_a_nonempty_untracked_list_and_guards_with_double_dash() {
        let changes: Vec<FileChange> = vec![];
        let pre_contents = BTreeMap::new();
        let untracked = vec!["has space/file.rs".to_string()];
        let req_with = ImpactRequest {
            mode: &SeedMode::WorkingTree,
            paths: &[],
            base_revision: "0123456789abcdef",
            changes: &changes,
            pre_contents: &pre_contents,
            pre_change_hash: "different",
            index_hash: "h",
            db: "index.db",
            workspace: "ws",
            prefix: "",
            reconstructible: true,
            untracked_sources: &untracked,
            range_head: None,
            depth: 1,
            order: OrderMode::Unranked,
        };
        let recipe = recovery_recipe(&req_with);
        let copy_step = recipe
            .steps
            .iter()
            .find(|s| s.starts_with("for f in"))
            .expect("a copy step when untracked sources exist");
        assert!(copy_step.contains("'has space/file.rs'"), "{copy_step}");
        assert!(copy_step.contains("dirname -- "), "{copy_step}");
        assert!(copy_step.contains("cp -- "), "{copy_step}");

        let req_without = ImpactRequest {
            untracked_sources: &[],
            ..ImpactRequest {
                mode: &SeedMode::WorkingTree,
                paths: &[],
                base_revision: "0123456789abcdef",
                changes: &changes,
                pre_contents: &pre_contents,
                pre_change_hash: "different",
                index_hash: "h",
                db: "index.db",
                workspace: "ws",
                prefix: "",
                reconstructible: true,
                untracked_sources: &untracked,
                range_head: None,
                depth: 1,
                order: OrderMode::Unranked,
            }
        };
        let recipe = recovery_recipe(&req_without);
        assert!(!recipe.steps.iter().any(|s| s.starts_with("for f in")));
    }

    // A range answer's recipe carries a second, head-side worktree and re-runs the query in a
    // subshell `cd`'d to it; a working-tree answer's recipe carries neither, and skips the
    // untracked-copy step for a range even when untracked sources exist (they belong to neither
    // commit the range straddles).
    #[test]
    fn range_recipe_carries_a_head_worktree_and_a_subshell_rerun() {
        let changes: Vec<FileChange> = vec![];
        let pre_contents = BTreeMap::new();
        let untracked = vec!["scratch.rs".to_string()];
        let mode = SeedMode::Revspec("main..HEAD".to_string());
        let range_req = ImpactRequest {
            mode: &mode,
            paths: &[],
            base_revision: "0123456789abcdef",
            changes: &changes,
            pre_contents: &pre_contents,
            pre_change_hash: "different",
            index_hash: "h",
            db: "index.db",
            workspace: "ws",
            prefix: "",
            reconstructible: false,
            untracked_sources: &untracked,
            range_head: Some("fedcba9876543210"),
            depth: 1,
            order: OrderMode::Unranked,
        };
        let recipe = recovery_recipe(&range_req);
        assert_eq!(
            recipe
                .steps
                .iter()
                .filter(|s| s.starts_with("git worktree add"))
                .count(),
            2,
            "a range recipe adds a worktree at the base and one at the head: {:?}",
            recipe.steps
        );
        assert!(
            recipe
                .steps
                .iter()
                .any(|s| s.starts_with("( cd ") && s.contains("c10r impact")),
            "the re-run happens in a subshell cd'd to the head worktree: {:?}",
            recipe.steps
        );
        assert!(
            !recipe.steps.iter().any(|s| s.starts_with("for f in")),
            "untracked files belong to neither commit a range straddles, so no copy step: {:?}",
            recipe.steps
        );

        let wt_req = ImpactRequest {
            range_head: None,
            ..ImpactRequest {
                mode: &SeedMode::WorkingTree,
                paths: &[],
                base_revision: "0123456789abcdef",
                changes: &changes,
                pre_contents: &pre_contents,
                pre_change_hash: "different",
                index_hash: "h",
                db: "index.db",
                workspace: "ws",
                prefix: "",
                reconstructible: false,
                untracked_sources: &[],
                range_head: Some("unused"),
                depth: 1,
                order: OrderMode::Unranked,
            }
        };
        let recipe = recovery_recipe(&wt_req);
        assert_eq!(
            recipe
                .steps
                .iter()
                .filter(|s| s.starts_with("git worktree add"))
                .count(),
            1,
            "a working-tree recipe adds only the base worktree: {:?}",
            recipe.steps
        );
        assert!(!recipe.steps.iter().any(|s| s.starts_with("( cd ")));
    }

    // _(An approximate answer carries a runnable recovery recipe)_ — "runnable" is checked, not
    // assumed: the emitted block parses as a shell script under `sh -n`, so the quoting that splices
    // a spaced path, a spaced workspace identity, and the untracked-copy loop into command lines
    // cannot silently produce something a caller could not paste and run. `sh -n` parses without
    // executing, so nothing in the recipe runs here.
    #[test]
    fn recipe_parses_as_a_shell_script() {
        let changes: Vec<FileChange> = vec![];
        let pre_contents = BTreeMap::new();
        let untracked = vec!["has space/file.rs".to_string(), "it's/quoted.rs".to_string()];
        let req = ImpactRequest {
            mode: &SeedMode::Staged,
            paths: &[PathBuf::from("has space/file.rs"), PathBuf::from("it's/quoted.rs")],
            base_revision: "0123456789abcdef",
            changes: &changes,
            pre_contents: &pre_contents,
            pre_change_hash: "different",
            index_hash: "h",
            db: "path with space/index.db",
            workspace: "ws 'name'",
            prefix: "sub/",
            reconstructible: false,
            untracked_sources: &untracked,
            range_head: None,
            depth: 2,
            order: OrderMode::Unranked,
        };
        let script = recovery_recipe(&req).steps.join("\n");

        let status = std::process::Command::new("sh")
            .arg("-n")
            .arg("-c")
            .arg(&script)
            .status()
            .expect("sh runs");
        assert!(status.success(), "the emitted recipe is not valid shell:\n{script}");
    }

    // The range shape — a second, head-side worktree plus a subshelled re-run — also parses as valid
    // shell.
    #[test]
    fn range_recipe_parses_as_a_shell_script() {
        let changes: Vec<FileChange> = vec![];
        let pre_contents = BTreeMap::new();
        let mode = SeedMode::Revspec("main...HEAD".to_string());
        let req = ImpactRequest {
            mode: &mode,
            paths: &[PathBuf::from("has space/file.rs")],
            base_revision: "0123456789abcdef",
            changes: &changes,
            pre_contents: &pre_contents,
            pre_change_hash: "different",
            index_hash: "h",
            db: "index.db",
            workspace: "ws",
            prefix: "",
            reconstructible: false,
            untracked_sources: &[],
            range_head: Some("fedcba9876543210"),
            depth: 1,
            order: OrderMode::Unranked,
        };
        let script = recovery_recipe(&req).steps.join("\n");

        let status = std::process::Command::new("sh")
            .arg("-n")
            .arg("-c")
            .arg(&script)
            .status()
            .expect("sh runs");
        assert!(status.success(), "the range recipe is not valid shell:\n{script}");
    }

    // `SeedOutcome` is `NoIndexedSymbolTouched` for a change touching no declaration,
    // `NoneResolvable` when only unmappable regions were produced, and `Seeded` otherwise —
    // exercised above via the dedicated scenarios; this test pins the plain `Seeded` case alongside
    // its siblings for a single point of comparison.
    #[test]
    fn seed_outcome_seeded_when_a_seed_resolves() {
        let fixture = build_fixture();
        let hash = "h";
        write_metadata(&fixture.store, hash);
        let engine = engine(&fixture.store, hash);
        let body1_line = fixture.text.lines().position(|l| l.contains("body1")).unwrap() as u32 + 1;
        let change = modified_change(
            "doc.rs",
            vec![LineRange {
                start: body1_line,
                count: 1,
            }],
        );
        let changes = vec![change];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("doc.rs".to_string(), fixture.text.clone());
        let req = request(&changes, &pre_contents, hash, hash);

        let answer = engine.impact(&req).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        assert_eq!(results[0].seed_outcome, SeedOutcome::Seeded);
    }
}
