//! The human rendering of a query answer: a deterministic projection of the same `Answer<T>` value
//! the `--json` path serializes.
//!
//! The renderer reads only fields already present on the answer — it never fetches new data, never
//! reorders or filters results — so the human view and the JSON view can never disagree about
//! membership or order. Content-bearing tiers (a signature, interface, or body) are printed as source
//! text, never `Debug`-escaped, with only the terminal-injection / display-spoofing control class
//! made visible as replacement characters ([`sanitize_content`]) so verbatim source cannot command
//! the reader's terminal; the machine (`--json`) answer stays byte-exact. Styling is applied only to
//! structure (header lines, a stale marker) and only when the caller passes `styled = true`; source
//! text is always plain.

use crate::cli::ColorArg;
use crate::query::impact::{Exactness, ImpactReport, SeedOutcome};
use crate::query::output::{Answer, ContentLines, Location, Outcome, PageInfo, SymbolView, WorkspaceRelation};
use crate::query::search::{CloneCertainty, SearchItem, SimilarItem};
use crate::query::{DependentsReport, FindItem, HorizonDisclosure, SymbolDetail, TraceItem};

const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

/// Replace every terminal-control and display-control character in `text` with the Unicode
/// replacement character: the C0 controls (`0x00`-`0x1f`, including newline and tab), DEL (`0x7f`),
/// the C1 controls (U+0080-U+009F — e.g. U+009B, a one-character CSI that C1-honoring terminals
/// execute with no ESC byte), and the Unicode bidirectional controls (U+202A-U+202E and
/// U+2066-U+2069, the Trojan-Source display-spoofing class).
///
/// Structural fields (a symbol's canonical identity and name, its kind, a document path, the
/// header's analyzer name/version) render verbatim from persisted or externally-sourced text; a
/// hostile repository or index file can embed ANSI/OSC control bytes in a name or path, which
/// rendered verbatim would command the reader's terminal — the same escape-injection class `git log`
/// guards against for commit metadata — or embed a raw newline, which would forge an extra row in a
/// row-bearing answer. Legitimate names and paths never carry these characters, so the substitution
/// is lossless in practice. Applied to field values before they are composed into a line, never to
/// the composed line itself, so `c10r`'s own styling escapes (added afterward) survive.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if is_injection_control(c as u32) { '\u{FFFD}' } else { c })
        .collect()
}

/// Replace the terminal- and display-control characters [`sanitize`] does, but keep the source
/// whitespace `\n`, `\t`, and `\r` intact.
///
/// Content-bearing tier text (a signature, interface, or body, and the content projected onto a
/// `trace`/`dependents` row) is rendered as source, so its own newlines, tabs, and carriage returns
/// must survive — a CRLF file must not sprout replacement characters. Everything else in the
/// injection-hazard class (ANSI/OSC escapes, C1 controls, bidirectional overrides) is still made
/// visible as the replacement character: verbatim source text could otherwise command the reader's
/// terminal, or later splice into a syntax-highlighter's control sequences. Applied to the tier value
/// before any styling composition, the same discipline the structural [`sanitize`] follows.
pub fn sanitize_content(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == '\n' || c == '\t' || c == '\r' {
                c
            } else if is_injection_control(c as u32) {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

/// Whether a Unicode scalar is in the terminal-injection / display-spoofing class: the C0 controls
/// (`0x00`-`0x1f`), DEL (`0x7f`), the C1 controls (U+0080-U+009F), and the bidirectional controls
/// (U+202A-U+202E and U+2066-U+2069).
fn is_injection_control(cp: u32) -> bool {
    cp <= 0x1f
        || cp == 0x7f
        || (0x80..=0x9f).contains(&cp)
        || (0x202a..=0x202e).contains(&cp)
        || (0x2066..=0x2069).contains(&cp)
}

/// Decide whether the human rendering should carry styling.
///
/// Styling never reaches the machine answer: `--json` is always unstyled. Otherwise the `--color`
/// gate decides — `never` off, `always` on (an explicit override that holds even when redirected, the
/// cross-tool convention), and `auto` on only when standard output is a terminal.
pub fn should_style(color: ColorArg, json: bool, stdout_is_terminal: bool) -> bool {
    if json {
        return false;
    }
    match color {
        ColorArg::Never => false,
        ColorArg::Always => true,
        ColorArg::Auto => stdout_is_terminal,
    }
}

/// Project an answer to human-readable text: a header line describing provenance and freshness, then
/// the outcome — found results (rendered per result type), a candidate list on ambiguity, or a
/// one-line sentence for a typed absence or empty relation.
pub fn to_human<T: HumanRender>(answer: &Answer<T>, styled: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(header_line(answer, styled));
    // The workspace disclosure sits directly under the header, above any result, so a human consumer
    // cannot miss what the structural field tells a machine consumer.
    if let Some(line) = workspace_relation_line(answer) {
        lines.push(line);
    }
    // The semantic-index provenance a `search`/`similar` answer carries, rendered with the header
    // block so the human view names the same identity the machine answer does.
    if let Some(semantic) = &answer.semantic_index {
        lines.push(format!(
            "semantic index: {} · corpus definition v{}",
            sanitize(&semantic.model_identity),
            semantic.corpus_definition_version
        ));
    }
    match &answer.outcome {
        Outcome::Found { results } => {
            // The heuristic-grade marker renders above the results, so a human consumer cannot
            // miss what the structural field tells a machine consumer. Each marker names its own
            // grade: estimation for a model-derived ranking, convention for classified test code.
            match answer.classification {
                Some("estimation") => lines.push(
                    "note: rows are the nearest candidates by model-derived estimation — not the complete set of \
                     relevant code"
                        .to_string(),
                ),
                Some(_) => lines
                    .push("note: results are convention-classified test code, not resolved semantic fact".to_string()),
                None => {}
            }
            // The ordering disclosure's heuristic note, likewise: only a ranked answer is presented
            // as heuristic — an unranked ordering derives from stable structural keys alone.
            if answer.ordering == Some("ranked") {
                lines.push(
                    "note: rows within a distance layer are ordered by a structural importance heuristic".to_string(),
                );
            }
            T::render_found(results, &mut lines, styled)
        }
        Outcome::Ambiguous {
            candidates,
            candidates_total,
        } => {
            lines.push(bold(&format!("ambiguous: {} candidates", candidates.len()), styled));
            for candidate in candidates {
                lines.push(candidate_line(candidate));
            }
            // When the candidate list was capped at the effective limit, disclose how many more exist
            // so the caller knows to narrow the reference rather than paging (a refusal has no cursor).
            if let Some(total) = candidates_total {
                let more = total.saturating_sub(candidates.len());
                lines.push(format!("  …and {more} more — narrow the reference"));
            }
        }
        // The two typed-none outcomes read as definite, distinct from a failure and from each other.
        // A marked empty answer scopes its absence to its grade: an estimation-marked empty says the
        // corpus offered no candidates — never that no relevant code exists; a convention-marked
        // empty says no convention-classified site was found — never that nothing tests the subject.
        Outcome::Absent => lines.push("absent: nothing resolved (a definite none, not a failure)".to_string()),
        Outcome::Empty => lines.push(match answer.classification {
            Some("estimation") => {
                "empty: the semantic corpus offered no candidates (not proof that no relevant code exists)".to_string()
            }
            Some(_) => {
                "empty: no convention-classified test reference found (not proof that nothing tests the subject)"
                    .to_string()
            }
            None => "empty: the relation holds no instances (a definite empty set)".to_string(),
        }),
    }
    if let Some(page) = &answer.page {
        push_page(&mut lines, page, styled);
    }
    lines.join("\n")
}

/// Append the result-set paging disclosure: which page was returned and how much of the total, and,
/// when truncated, the continuation token a human can pass back through `--cursor` to resume.
fn push_page(lines: &mut Vec<String>, page: &PageInfo, styled: bool) {
    lines.push(bold(
        &format!(
            "page {}: {} of {} results{}",
            page.page_index,
            page.returned,
            page.total,
            if page.truncated { " (truncated)" } else { "" }
        ),
        styled,
    ));
    if let Some(cursor) = &page.cursor {
        lines.push(format!("  resume with --cursor {cursor}"));
    }
}

/// A found result set projected into `lines`. Each answer payload type controls how its results read
/// as human text, while the answer envelope (header, ambiguity, absence) is shared. A row-bearing
/// answer type added later slots in by implementing this trait.
pub trait HumanRender {
    /// Append the human rendering of a found result set to `lines`. `styled` gates structural styling
    /// only; any source/tier text is appended verbatim.
    fn render_found(results: &[Self], lines: &mut Vec<String>, styled: bool)
    where
        Self: Sized;
}

impl HumanRender for SymbolDetail {
    fn render_found(results: &[Self], lines: &mut Vec<String>, styled: bool) {
        use crate::query::DetailPayload;
        for detail in results {
            lines.push(symbol_identity_line(&detail.symbol, styled));
            match &detail.payload {
                DetailPayload::Location { location } => lines.push(location_line(location.as_ref())),
                DetailPayload::Signature { signature } => push_content(lines, signature.as_deref()),
                DetailPayload::Interface { interface } => push_content(lines, interface.as_deref()),
                DetailPayload::Body { body } => push_content(lines, body.as_deref()),
            }
            push_content_window(lines, detail.content_lines.as_ref());
        }
    }
}

impl HumanRender for TraceItem {
    fn render_found(results: &[Self], lines: &mut Vec<String>, styled: bool) {
        lines.push(results_header(results.len(), styled));
        for item in results {
            match item {
                TraceItem::Symbol {
                    symbol,
                    location,
                    content,
                    content_truncated,
                } => {
                    let mut line = format!(
                        "  {}  {}  [{}]{}",
                        sanitize(symbol.canonical_id.as_str()),
                        sanitize(&symbol.name),
                        sanitize(&symbol.kind),
                        external_tag(symbol.external)
                    );
                    if let Some(loc) = location {
                        line.push_str(&format!(" at {}", span_of(loc)));
                    }
                    lines.push(line);
                    if let Some(text) = content {
                        push_content(lines, Some(text));
                    }
                    push_truncation(lines, *content_truncated);
                }
                TraceItem::Reference {
                    subject,
                    location,
                    enclosing,
                    test_rule: _,
                    content,
                    content_truncated,
                } => {
                    let attributed = match enclosing {
                        Some(id) => format!(" in {}", sanitize(id.as_str())),
                        None => " in <module>".to_string(),
                    };
                    lines.push(format!(
                        "  {}  {}  at {}{}",
                        sanitize(subject.canonical_id.as_str()),
                        sanitize(&subject.name),
                        span_of(location),
                        attributed
                    ));
                    if let Some(text) = content {
                        push_content(lines, Some(text));
                    }
                    push_truncation(lines, *content_truncated);
                }
            }
        }
    }
}

impl HumanRender for FindItem {
    fn render_found(results: &[Self], lines: &mut Vec<String>, styled: bool) {
        lines.push(results_header(results.len(), styled));
        for item in results {
            lines.push(format!(
                "  {}  {}  at {}",
                sanitize(item.symbol.canonical_id.as_str()),
                sanitize(&item.symbol.name),
                location_at(item.location.as_ref())
            ));
        }
    }
}

impl HumanRender for SearchItem {
    fn render_found(results: &[Self], lines: &mut Vec<String>, styled: bool) {
        lines.push(results_header(results.len(), styled));
        for item in results {
            lines.push(format!(
                "  {}  {}  [{}]{} at {}",
                sanitize(item.symbol.canonical_id.as_str()),
                sanitize(&item.symbol.name),
                sanitize(&item.symbol.kind),
                external_tag(item.symbol.external),
                location_at(item.location.as_ref())
            ));
            if let Some(text) = &item.content {
                push_content(lines, Some(text));
            }
            push_truncation(lines, item.content_truncated);
        }
    }
}

impl HumanRender for SimilarItem {
    fn render_found(results: &[Self], lines: &mut Vec<String>, styled: bool) {
        lines.push(results_header(results.len(), styled));
        for item in results {
            // The clone marker renders on the row as deterministic fact — the certainty tier above
            // the estimated ranking — never as part of the estimation grade.
            let certainty = match item.clone_certainty {
                Some(CloneCertainty::ExactClone) => "  [exact clone: identical token sequence]",
                Some(CloneCertainty::VariantClone) => {
                    "  [variant clone: identical token structure, names/literals substituted — not behavioral \
                     equivalence]"
                }
                None => "",
            };
            lines.push(format!(
                "  {}  {}  [{}]{} at {}{}",
                sanitize(item.symbol.canonical_id.as_str()),
                sanitize(&item.symbol.name),
                sanitize(&item.symbol.kind),
                external_tag(item.symbol.external),
                location_at(item.location.as_ref()),
                certainty
            ));
            if let Some(text) = &item.content {
                push_content(lines, Some(text));
            }
            push_truncation(lines, item.content_truncated);
        }
    }
}

impl HumanRender for DependentsReport {
    fn render_found(results: &[Self], lines: &mut Vec<String>, styled: bool) {
        for report in results {
            lines.push(bold(
                &format!(
                    "dependents: depth_bound={} horizon={} reach={}",
                    report.depth_bound,
                    report.horizon,
                    disclosure_label(report.disclosure)
                ),
                styled,
            ));
            lines.push(bold(&format!("{} detailed", report.detail.len()), styled));
            for item in &report.detail {
                lines.push(format!(
                    "  {}  {}  {} at distance {} via {}",
                    sanitize(item.symbol.canonical_id.as_str()),
                    sanitize(&item.symbol.name),
                    location_at(item.location.as_ref()),
                    item.distance,
                    sanitize(&item.kind)
                ));
                if let Some(text) = &item.content {
                    push_content(lines, Some(text));
                }
                push_truncation(lines, item.content_truncated);
            }
            if !report.beyond_bound.is_empty() {
                lines.push(bold("beyond bound", styled));
                for aggregate in &report.beyond_bound {
                    lines.push(format!(
                        "  {} at distance {} via {}",
                        aggregate.count,
                        aggregate.distance,
                        sanitize(&aggregate.kind)
                    ));
                }
            }
        }
    }
}

impl HumanRender for ImpactReport {
    fn render_found(results: &[Self], lines: &mut Vec<String>, styled: bool) {
        for report in results {
            lines.push(bold(
                &format!(
                    "impact: mode={} base={} exactness={}",
                    sanitize(report.seed_mode),
                    sanitize(&report.base_revision),
                    exactness_label(report.exactness)
                ),
                styled,
            ));
            // A range's seeds resolve against its pre-change side while its dependents come from the
            // one index that exists, so a range answer straddles two snapshots and says so. The other
            // modes do not: their base revision already is the snapshot the dependents reflect, so
            // the line would be noise there.
            if report.seed_mode == "range" {
                lines.push(
                    "  dependents are those the current index holds, not those of either end of the range".to_string(),
                );
            }
            match report.seed_outcome {
                // A definite none: the change touches nothing the graph tracks, distinct from the
                // resolution gap below.
                SeedOutcome::NoIndexedSymbolTouched => {
                    lines.push("none: the change touches nothing the graph tracks (a definite none)".to_string());
                }
                // A resolution gap: at least one changed region sat in a document the index never
                // saw, so this is not a confident "nothing changed here."
                SeedOutcome::NoneResolvable => {
                    lines.push("unresolved: no changed region resolved against this index".to_string());
                }
                SeedOutcome::Seeded => {
                    lines.push(bold(&format!("{} seeds", report.seeds.len()), styled));
                    for seed in &report.seeds {
                        lines.push(format!(
                            "  {}  {}  [{}]{} at {}",
                            sanitize(seed.symbol.canonical_id.as_str()),
                            sanitize(&seed.symbol.name),
                            sanitize(&seed.symbol.kind),
                            external_tag(seed.symbol.external),
                            location_at(seed.location.as_ref())
                        ));
                    }
                }
            }
            if !report.unmappable.is_empty() {
                lines.push(bold("regions this index cannot resolve", styled));
                for region in &report.unmappable {
                    lines.push(format!(
                        "  {}:{}-{}",
                        sanitize(&region.document_path),
                        region.span_start,
                        region.span_end
                    ));
                }
            }
            // A definite none carries no seed/dependent blocks: a dependents union over an empty
            // seed set is trivially empty and would only echo the "none" line above as a redundant
            // header.
            if report.seed_outcome != SeedOutcome::NoIndexedSymbolTouched {
                DependentsReport::render_found(std::slice::from_ref(&report.dependents), lines, styled);
            }
            if let Some(recovery) = &report.recovery {
                lines.push(bold("approximate: run these steps to produce an exact answer", styled));
                // The residual an approximate answer cannot rule out: unmappability is witnessed at
                // the document level, so a declaration this index never recorded reads as a region
                // touching nothing rather than as one it could not resolve.
                lines.push(
                    "  this index may be missing declarations the change touched, so a region reported as \
                     touching nothing may be one it cannot see"
                        .to_string(),
                );
                for step in &recovery.steps {
                    lines.push(format!("  {}", sanitize(step)));
                }
            }
        }
    }
}

/// The answer header: analyzer provenance and freshness. The freshness label itself names any
/// staleness and its reason, so a stale answer surfaces succinctly here.
fn header_line<T>(answer: &Answer<T>, styled: bool) -> String {
    let base = format!(
        "{} {} · {}",
        sanitize(&answer.provenance.analyzer_name),
        sanitize(&answer.provenance.analyzer_version),
        freshness_label(answer.freshness)
    );
    bold(&base, styled)
}

/// The workspace-relationship disclosure line, when the answer carries one: a matched workspace
/// carries no line, mirroring the machine answer's absent field.
///
/// The recorded root is caller-supplied text that reached the store through `--db`/the build root, so
/// it is sanitized like every other structural value the terminal sees.
fn workspace_relation_line<T>(answer: &Answer<T>) -> Option<String> {
    match answer.workspace_relation.as_ref()? {
        WorkspaceRelation::Mismatched { recorded_root } => Some(format!(
            "warning: this index describes a different workspace (built for {})",
            sanitize(recorded_root)
        )),
        WorkspaceRelation::Unknown => {
            Some("warning: whether this index describes this workspace could not be determined".to_string())
        }
    }
}

#[cfg(test)]
mod workspace_disclosure_tests {
    use super::*;
    use crate::graph::store::Freshness;
    use crate::query::output::Provenance;

    fn answer(relation: Option<WorkspaceRelation>) -> Answer<SymbolDetail> {
        Answer::empty(
            Provenance {
                analyzer_name: "test".to_string(),
                analyzer_version: "0".to_string(),
            },
            Freshness::Fresh,
        )
        .with_workspace_relation(relation)
    }

    // Each disclosed state reaches the human render as its own line, and a matched workspace reaches
    // it as nothing at all — the same three-way shape the machine answer carries.
    #[test]
    fn each_workspace_state_renders_as_its_own_line() {
        let matched = to_human(&answer(None), false);
        assert!(
            !matched.contains("workspace"),
            "a match renders no workspace line: {matched}"
        );

        let mismatched = to_human(
            &answer(Some(WorkspaceRelation::Mismatched {
                recorded_root: "/projects/other".to_string(),
            })),
            false,
        );
        assert!(
            mismatched.contains("different workspace") && mismatched.contains("/projects/other"),
            "a mismatch names the workspace the store describes: {mismatched}"
        );

        let unknown = to_human(&answer(Some(WorkspaceRelation::Unknown)), false);
        assert!(
            unknown.contains("could not be determined"),
            "an unevaluable comparison reads as undetermined, not as a match: {unknown}"
        );
        assert!(
            !unknown.contains("different workspace"),
            "an unevaluable comparison is not reported as a mismatch: {unknown}"
        );
    }

    // The recorded root reaches the terminal sanitized: it is caller-supplied text that entered the
    // store through a build root, so it gets the same escape guard every structural value gets.
    #[test]
    fn the_recorded_root_is_sanitized_before_it_reaches_the_terminal() {
        let hostile = to_human(
            &answer(Some(WorkspaceRelation::Mismatched {
                recorded_root: "/projects/\u{1b}[31mred".to_string(),
            })),
            false,
        );
        assert!(
            !hostile.contains('\u{1b}'),
            "no raw escape byte reaches the render: {hostile:?}"
        );
    }
}

/// The "N results" header for a row-bearing answer.
fn results_header(count: usize, styled: bool) -> String {
    let word = if count == 1 { "result" } else { "results" };
    bold(&format!("{count} {word}"), styled)
}

/// A symbol's identity line: its canonical identity, name, kind, and an external marker.
fn symbol_identity_line(view: &SymbolView, styled: bool) -> String {
    bold(
        &format!(
            "{}  {}  [{}]{}",
            sanitize(view.canonical_id.as_str()),
            sanitize(&view.name),
            sanitize(&view.kind),
            external_tag(view.external)
        ),
        styled,
    )
}

/// One candidate line in an ambiguity list.
fn candidate_line(view: &SymbolView) -> String {
    format!(
        "  {}  {}  [{}]{}",
        sanitize(view.canonical_id.as_str()),
        sanitize(&view.name),
        sanitize(&view.kind),
        external_tag(view.external)
    )
}

/// A `location` detail line, or an honest "no location" line for an external symbol.
fn location_line(location: Option<&Location>) -> String {
    match location {
        Some(l) => format!("  {}", span_of(l)),
        None => "  (no location — external symbol)".to_string(),
    }
}

/// Append a content-bearing tier verbatim as a source block, or an honest "no content" line when the
/// tier carries nothing (an external symbol, or a symbol with no persisted span).
fn push_content(lines: &mut Vec<String>, content: Option<&str>) {
    match content {
        Some(text) => lines.push(sanitize_content(text)),
        None => lines.push("  (no content — external symbol or no persisted span)".to_string()),
    }
}

/// Append a per-result content-truncation marker when a `trace`/`dependents` row's content was capped
/// by `--max-lines`. Nothing is appended for an untruncated result.
fn push_truncation(lines: &mut Vec<String>, truncated: bool) {
    if truncated {
        lines.push("  … (content truncated)".to_string());
    }
}

/// Append a `get` content-window disclosure: which lines the window covers of the whole tier text,
/// and the literal recovery to name — the next window when more lines remain below, or the full-text
/// recovery when the window already reaches the end (or was requested past it). Nothing is appended
/// when the whole content is shown (`content_lines` is absent).
fn push_content_window(lines: &mut Vec<String>, content_lines: Option<&ContentLines>) {
    let Some(cl) = content_lines else { return };
    if cl.start > cl.total {
        // Requested past the end: the content is empty; name the total and the full-text recovery.
        lines.push(format!(
            "  (no lines: --from {} is past the end — content has {} lines; full text: --max-lines 0 --from 1)",
            cl.start, cl.total
        ));
    } else if cl.end < cl.total {
        // More lines remain below the window: name the next window to fetch them.
        lines.push(format!(
            "  lines {}-{} of {} — next: --from {}",
            cl.start,
            cl.end,
            cl.total,
            cl.end + 1
        ));
    } else {
        // The window reaches the end but omits the head: name the full-text recovery, not a next page.
        lines.push(format!(
            "  lines {}-{} of {} — full text: --max-lines 0 --from 1",
            cl.start, cl.end, cl.total
        ));
    }
}

/// A location rendered as `path:start-end`.
fn span_of(location: &Location) -> String {
    format!(
        "{}:{}-{}",
        sanitize(&location.document_path),
        location.span_start,
        location.span_end
    )
}

/// A dependent's location as `path:start-end`, or `<external>` when it has no source here.
fn location_at(location: Option<&Location>) -> String {
    match location {
        Some(l) => span_of(l),
        None => "<external>".to_string(),
    }
}

fn external_tag(external: bool) -> &'static str {
    if external { " (external)" } else { "" }
}

fn freshness_label(freshness: crate::query::output::FreshnessLabel) -> &'static str {
    use crate::query::output::FreshnessLabel;
    match freshness {
        FreshnessLabel::Fresh => "fresh",
        FreshnessLabel::StaleContent => "stale (content changed)",
        FreshnessLabel::StaleVersion => "stale (analyzer version changed)",
        FreshnessLabel::StaleEnvironment => "stale (environment changed)",
    }
}

fn exactness_label(exactness: Exactness) -> &'static str {
    match exactness {
        Exactness::Exact => "exact",
        Exactness::Approximate => "approximate",
    }
}

fn disclosure_label(disclosure: HorizonDisclosure) -> &'static str {
    match disclosure {
        HorizonDisclosure::EndsWithinBound => "ends within bound",
        HorizonDisclosure::BeyondBound => "extends beyond bound",
        HorizonDisclosure::CutAtHorizon => "cut at horizon",
    }
}

/// Apply bold styling to a structural line when styling is enabled; otherwise return it plain.
fn bold(text: &str, styled: bool) -> String {
    if styled {
        format!("{BOLD}{text}{RESET}")
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::impact::{RecoveryRecipe, UnmappableRegion};

    #[test]
    fn json_is_never_styled_whatever_the_color_gate() {
        // `--json` overrides every `--color` value and the terminal state: the machine answer is plain.
        for color in [ColorArg::Auto, ColorArg::Always, ColorArg::Never] {
            for is_terminal in [true, false] {
                assert!(
                    !should_style(color, true, is_terminal),
                    "json is unstyled for {color:?}"
                );
            }
        }
    }

    #[test]
    fn never_and_always_ignore_the_terminal_state() {
        assert!(
            !should_style(ColorArg::Never, false, true),
            "never is off on a terminal"
        );
        assert!(!should_style(ColorArg::Never, false, false), "never is off redirected");
        assert!(
            should_style(ColorArg::Always, false, true),
            "always is on on a terminal"
        );
        assert!(
            should_style(ColorArg::Always, false, false),
            "always forces styling even redirected"
        );
    }

    // A definite-none impact answer (`NoIndexedSymbolTouched`) renders its one-line "none" sentence
    // and nothing else — no seed header, no dependents block — since a dependents union over an
    // empty seed set is trivially empty and would only echo the sentence as a redundant header.
    #[test]
    fn impact_no_indexed_symbol_touched_renders_no_dependent_block() {
        let report = ImpactReport {
            seed_mode: "working_tree",
            base_revision: "abc123".to_string(),
            exactness: Exactness::Exact,
            seed_outcome: SeedOutcome::NoIndexedSymbolTouched,
            recovery: None,
            seeds: Vec::new(),
            unmappable: Vec::new(),
            dependents_snapshot: "current_index",
            dependents: DependentsReport {
                depth_bound: 1,
                horizon: 1,
                disclosure: HorizonDisclosure::EndsWithinBound,
                detail: Vec::new(),
                beyond_bound: Vec::new(),
            },
        };
        let mut lines = Vec::new();
        ImpactReport::render_found(std::slice::from_ref(&report), &mut lines, false);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("the change touches nothing the graph tracks"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("dependents:"),
            "no dependents block for a definite none: {rendered}"
        );
        assert!(
            !rendered.contains("seeds"),
            "no seed header for a definite none: {rendered}"
        );
    }

    /// An `ImpactReport` fixture with everything but `seed_mode`, `recovery`, and `unmappable` held
    /// constant, so a test can vary just the field under scrutiny.
    fn impact_report_fixture(
        seed_mode: &'static str,
        recovery: Option<RecoveryRecipe>,
        unmappable: Vec<UnmappableRegion>,
    ) -> ImpactReport {
        ImpactReport {
            seed_mode,
            base_revision: "abc123".to_string(),
            exactness: if recovery.is_some() {
                Exactness::Approximate
            } else {
                Exactness::Exact
            },
            seed_outcome: SeedOutcome::NoIndexedSymbolTouched,
            recovery,
            seeds: Vec::new(),
            unmappable,
            dependents_snapshot: "current_index",
            dependents: DependentsReport {
                depth_bound: 1,
                horizon: 1,
                disclosure: HorizonDisclosure::EndsWithinBound,
                detail: Vec::new(),
                beyond_bound: Vec::new(),
            },
        }
    }

    // A range-seeded answer's human render discloses that its dependents come from the current
    // index, not the range's endpoints; a working-tree-seeded answer's render carries no such line,
    // since its base revision already is the snapshot the dependents come from.
    #[test]
    fn range_mode_discloses_the_dependents_snapshot_and_other_modes_do_not() {
        let range_report = impact_report_fixture("range", None, Vec::new());
        let mut lines = Vec::new();
        ImpactReport::render_found(std::slice::from_ref(&range_report), &mut lines, false);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("current index"),
            "a range answer discloses the dependents snapshot: {rendered}"
        );

        let working_tree_report = impact_report_fixture("working_tree", None, Vec::new());
        let mut lines = Vec::new();
        ImpactReport::render_found(std::slice::from_ref(&working_tree_report), &mut lines, false);
        let rendered = lines.join("\n");
        assert!(
            !rendered.contains("current index"),
            "a working-tree answer carries no snapshot disclosure: {rendered}"
        );
    }

    // An approximate answer's human render states that the index may be missing declarations the
    // change touched; an exact answer's render carries no such caveat.
    #[test]
    fn approximate_answer_discloses_that_declarations_may_be_missing() {
        let recovery = RecoveryRecipe {
            base_revision: "abc123".to_string(),
            db: "index.db".to_string(),
            workspace: "ws".to_string(),
            steps: vec!["echo rebuild".to_string()],
        };
        let approximate_report = impact_report_fixture("working_tree", Some(recovery), Vec::new());
        let mut lines = Vec::new();
        ImpactReport::render_found(std::slice::from_ref(&approximate_report), &mut lines, false);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("may be missing declarations"),
            "an approximate answer discloses that declarations may be missing: {rendered}"
        );

        let exact_report = impact_report_fixture("working_tree", None, Vec::new());
        let mut lines = Vec::new();
        ImpactReport::render_found(std::slice::from_ref(&exact_report), &mut lines, false);
        let rendered = lines.join("\n");
        assert!(
            !rendered.contains("may be missing declarations"),
            "an exact answer carries no such caveat: {rendered}"
        );
    }

    // An exact answer's header labels itself `exact`; an approximate one labels itself
    // `approximate` — the label a regression could invert without any other assertion catching it.
    #[test]
    fn exactness_label_reflects_exact_or_approximate() {
        let exact_report = impact_report_fixture("working_tree", None, Vec::new());
        let mut lines = Vec::new();
        ImpactReport::render_found(std::slice::from_ref(&exact_report), &mut lines, false);
        let rendered = lines.join("\n");
        assert!(rendered.contains("exactness=exact"), "{rendered}");
        assert!(!rendered.contains("exactness=approximate"), "{rendered}");

        let recovery = RecoveryRecipe {
            base_revision: "abc123".to_string(),
            db: "index.db".to_string(),
            workspace: "ws".to_string(),
            steps: vec!["echo rebuild".to_string()],
        };
        let approximate_report = impact_report_fixture("working_tree", Some(recovery), Vec::new());
        let mut lines = Vec::new();
        ImpactReport::render_found(std::slice::from_ref(&approximate_report), &mut lines, false);
        let rendered = lines.join("\n");
        assert!(rendered.contains("exactness=approximate"), "{rendered}");
        assert!(!rendered.contains("exactness=exact"), "{rendered}");
    }

    // An approximate answer's human render includes the recovery procedure's steps verbatim, and
    // names the base revision, the index location, and the workspace identity the procedure needs —
    // the caller must never be left to reconstruct those by hand.
    #[test]
    fn approximate_answer_renders_the_recovery_steps_and_names_base_db_and_workspace() {
        let recovery = RecoveryRecipe {
            base_revision: "deadbeef1234".to_string(),
            db: "/tmp/c10r-exact.db".to_string(),
            workspace: "my-workspace".to_string(),
            steps: vec![
                "git worktree add /tmp/c10r-wt deadbeef1234".to_string(),
                "c10r build /tmp/c10r-wt --db /tmp/c10r-exact.db --workspace my-workspace".to_string(),
            ],
        };
        let report = impact_report_fixture("working_tree", Some(recovery), Vec::new());
        let mut lines = Vec::new();
        ImpactReport::render_found(std::slice::from_ref(&report), &mut lines, false);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("git worktree add /tmp/c10r-wt deadbeef1234"),
            "the render includes the recovery procedure's steps verbatim: {rendered}"
        );
        assert!(
            rendered.contains("c10r build /tmp/c10r-wt --db /tmp/c10r-exact.db --workspace my-workspace"),
            "the render includes the recovery procedure's steps verbatim: {rendered}"
        );
        assert!(
            rendered.contains("deadbeef1234"),
            "the render names the base revision: {rendered}"
        );
        assert!(
            rendered.contains("/tmp/c10r-exact.db"),
            "the render names the index location: {rendered}"
        );
        assert!(
            rendered.contains("my-workspace"),
            "the render names the workspace identity: {rendered}"
        );
    }

    // A report carrying unmappable regions renders the unresolvable-regions block, naming each
    // region's document; a report with none renders no such block.
    #[test]
    fn unmappable_regions_render_the_unresolvable_block_and_absence_renders_none() {
        let unmappable = vec![UnmappableRegion {
            document_path: "src/unreachable.rs".to_string(),
            span_start: 10,
            span_end: 20,
        }];
        let report = impact_report_fixture("working_tree", None, unmappable);
        let mut lines = Vec::new();
        ImpactReport::render_found(std::slice::from_ref(&report), &mut lines, false);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("regions this index cannot resolve"),
            "unmappable regions render the unresolvable-regions block: {rendered}"
        );
        assert!(
            rendered.contains("src/unreachable.rs"),
            "the block names the region's document: {rendered}"
        );

        let report_without = impact_report_fixture("working_tree", None, Vec::new());
        let mut lines = Vec::new();
        ImpactReport::render_found(std::slice::from_ref(&report_without), &mut lines, false);
        let rendered = lines.join("\n");
        assert!(
            !rendered.contains("regions this index cannot resolve"),
            "no unmappable regions means no unresolvable-regions block: {rendered}"
        );
    }

    #[test]
    fn auto_follows_the_terminal_state() {
        assert!(should_style(ColorArg::Auto, false, true), "auto styles a terminal");
        assert!(
            !should_style(ColorArg::Auto, false, false),
            "auto stays plain when redirected"
        );
    }
}
