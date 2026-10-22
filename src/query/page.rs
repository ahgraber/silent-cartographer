//! The shared result-set bounding step: offset paging over the deterministic result order every
//! answer already carries, with an opaque continuation token bound to both the query parameters and
//! the recorded index identity (content-hash plus analyzer provenance).
//!
//! Paging is applied after the engine produces a full answer, so the engine's query methods stay
//! bounding-agnostic. Only a found result set is paged; typed absence, an empty relation, and an
//! ambiguity refusal carry no result set to bound.

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::exit::Failure;
use crate::identity::CanonicalId;

use super::impact::ImpactReport;
use super::output::{Answer, Outcome, PageInfo};
use super::search::RelationLabel;
use super::{DependentItem, DependentsReport};

/// The query identity a continuation token is bound to: the command, subject, relation/detail, the
/// depth bound, the order selector and ranking-model version (for orderable answers), the output
/// bounds (result limit, content-line bound, and `get`'s window start), and the recorded index
/// identity (content-hash plus analyzer provenance). A token resumes only against the exact same
/// identity — a different query, an index rebuilt underneath (changed sources or a changed analyzer
/// version), or a changed ranking model, changes the hash and is rejected.
///
/// The identity is serialized to produce that hash, so every field it carries is bound by
/// construction: a parameter added here cannot be left out of the comparison, which is the one way a
/// token could otherwise resume against a result sequence it was not issued for.
#[derive(Serialize)]
pub struct PageIdentity {
    /// The command name (`get`, `trace`, `find`, `search`, `similar`, or `impact`).
    pub command: &'static str,
    /// The subject reference (or source-position string), as presented.
    pub reference: String,
    /// The relation, for `trace`; `None` for `get`.
    pub relation: Option<&'static str>,
    /// The detail level, when one was requested.
    pub detail: Option<&'static str>,
    /// The requested depth bound (the `dependents` relation); `None` elsewhere.
    pub depth: Option<u32>,
    /// The ordering identity for an orderable answer (`dependents` traces and `impact`); `None`
    /// for commands whose answers carry no orderable rows. Bound so a token issued under one
    /// ordering, or under a prior ranking model, is rejected rather than resumed against a
    /// differently-ordered sequence.
    pub ordering: Option<OrderingIdentity>,
    /// The effective result limit — the applied default or an explicit value; `None` for an
    /// explicit unbounded request (`--limit 0`).
    pub limit: Option<usize>,
    /// The effective per-result content-line bound; `None` for an unbounded request (`--max-lines 0`).
    pub max_lines: Option<usize>,
    /// The `get` content-window start line (1-based); `None` for commands without a window (`trace`,
    /// `find`).
    pub from: Option<usize>,
    /// The position unit (`line` or `byte`), bound only when the query reads an `--at` position: the
    /// same `--at` string names a different position under each unit, while every other query's
    /// answer, token included, is the same under either.
    pub position_unit: Option<&'static str>,
    /// The aggregate hash of the source files on disk, bound only by `status`: a token resumes only
    /// while the source files are as they were when its page was drawn. `None` for every query.
    pub source_hash: Option<String>,
    /// The relation labels a `similar` query excludes, sorted and deduplicated; empty for every other
    /// query and for an unexcluded `similar`, where it adds nothing to the hash.
    /// Bound so a token issued under one exclusion is refused under another.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub excluded_relations: Vec<RelationLabel>,
    /// The recorded index identity the answer was drawn from: the index content-hash composed with
    /// the recorded analyzer provenance (name and version). A same-source rebuild under a different
    /// analyzer version changes this even when the content-hash is unchanged, so a stale token is
    /// rejected rather than resumed against possibly-shifted extraction.
    pub index_hash: String,
}

/// The ordering half of a query identity: the selector and the ranking model's version, sealed
/// into one value so a site that binds the mode cannot fail to bind the version.
///
/// The version is not a parameter: [`OrderingIdentity::current`] — the only production
/// constructor — stamps [`crate::graph::rank::RANK_VERSION`] itself, making "the selector and the
/// ranking model's version are part of the query identity" a property of the type rather than a
/// discipline every identity-building site must remember. Within one binary the constant cannot
/// vary, so no runtime test can catch a site that binds the mode but not the version; sealing the
/// pair is what closes that untestable gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OrderingIdentity {
    mode: &'static str,
    rank_version: u32,
}

impl OrderingIdentity {
    /// The ordering identity for `mode` under the ranking model this binary ships.
    pub fn current(mode: crate::query::OrderMode) -> Self {
        Self {
            mode: mode.label(),
            rank_version: crate::graph::rank::RANK_VERSION,
        }
    }

    /// An ordering identity with an arbitrary version, for exercising cross-version token
    /// rejection — constructible only in tests, so production sites cannot bind a stale version.
    #[cfg(test)]
    fn with_version(mode: &'static str, rank_version: u32) -> Self {
        Self { mode, rank_version }
    }
}

impl PageIdentity {
    /// The parameter-identity hash: a hex digest over the whole identity. It is deliberately not the
    /// token itself — the token additionally carries the page index and is opaque.
    ///
    /// Digesting the serialized struct rather than a hand-listed set of fields is what makes "every
    /// query parameter is bound" a property of the type. It also keeps an absent optional parameter
    /// distinct from a present one carrying an empty value, which a rendering of both to `""` would
    /// have collapsed.
    fn hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(
            serde_json::to_vec(self)
                .expect("the query identity serializes")
                .as_slice(),
        );
        to_hex(&hasher.finalize())
    }
}

/// The cursor checks that need no store, hoisted so they run before the query store is opened.
///
/// A `--cursor` must pair with `--limit` (a cursor resumes into a page size), and it must decode to a
/// well-formed token. Both are usage errors independent of any index, so they are reported before the
/// store opens — ahead of the no-index outcome, matching the usage-before-no-index precedence. The
/// identity-hash comparison necessarily stays in [`apply_pagination`]: it needs the recorded index
/// identity, which is only available after the store opens.
pub fn precheck_cursor(cursor: Option<&str>, limit: Option<usize>) -> anyhow::Result<()> {
    let Some(token) = cursor else { return Ok(()) };
    if limit.is_none() {
        return Err(Failure::Usage(
            "--cursor cannot resume an unbounded result set (--limit 0); re-issue with a positive --limit".to_string(),
        )
        .into());
    }
    if decode_token(token).is_none() {
        return Err(Failure::Usage(
            "the continuation token is malformed; re-issue the query without --cursor".to_string(),
        )
        .into());
    }
    Ok(())
}

/// Apply the result-set bound to an answer.
///
/// With no `--limit`, results are returned whole and no paging disclosure is attached. With a limit,
/// the answer carries a [`PageInfo`]; when a page does not exhaust the matched results, it discloses
/// truncation and an opaque continuation token. A `--cursor` resumes a prior page: the token is
/// decoded and its identity hash recomputed and compared, a mismatch or an out-of-range page is a
/// usage error naming the recovery.
pub fn apply_pagination<T>(
    mut answer: Answer<T>,
    limit: Option<usize>,
    cursor: Option<&str>,
    identity: &PageIdentity,
) -> anyhow::Result<Answer<T>> {
    let param_hash = identity.hash();
    let page_index = resolve_page_index(cursor, limit, &param_hash)?;
    cap_reference_groups(&mut answer, limit);

    let Answer {
        provenance,
        freshness,
        stale,
        refresh,
        classification,
        token_equivalence,
        workspace_relation,
        ordering,
        excluded_relations,
        not_compared_completely,
        unresolved_calls,
        unconfirmed_uses,
        override_relations,
        empty_scope,
        outcome,
        page: _,
    } = answer;

    // An ambiguity refusal is not a resumable result set: its candidate list is capped at the
    // effective limit — disclosing the total when it exceeds the limit — and carries no page block.
    if let Outcome::Ambiguous {
        candidates,
        candidates_total: _,
    } = outcome
    {
        if page_index > 0 {
            return Err(out_of_range(0));
        }
        let total = candidates.len();
        let (candidates, candidates_total) = match limit {
            Some(cap) if total > cap => (candidates.into_iter().take(cap).collect(), Some(total)),
            _ => (candidates, None),
        };
        return Ok(Answer {
            provenance,
            freshness,
            stale,
            refresh,
            classification,
            token_equivalence,
            workspace_relation,
            ordering,
            excluded_relations,
            not_compared_completely,
            unresolved_calls,
            unconfirmed_uses,
            override_relations,
            empty_scope,
            outcome: Outcome::Ambiguous {
                candidates,
                candidates_total,
            },
            page: None,
        });
    }

    // Only a found result set carries results to page. A cursor presented against a non-found outcome
    // has already had its identity validated above; only page 0 is in range for a set with no results.
    let Outcome::Found { results } = outcome else {
        if page_index > 0 {
            return Err(out_of_range(0));
        }
        return Ok(Answer {
            provenance,
            freshness,
            stale,
            refresh,
            classification,
            token_equivalence,
            workspace_relation,
            ordering,
            excluded_relations,
            not_compared_completely,
            unresolved_calls,
            unconfirmed_uses,
            override_relations,
            empty_scope,
            outcome,
            page: None,
        });
    };

    let Some(limit) = limit else {
        // No row cap: the whole set is returned with no paging disclosure.
        return Ok(Answer {
            provenance,
            freshness,
            stale,
            refresh,
            classification,
            token_equivalence,
            workspace_relation,
            ordering,
            excluded_relations,
            not_compared_completely,
            unresolved_calls,
            unconfirmed_uses,
            override_relations,
            empty_scope,
            outcome: Outcome::Found { results },
            page: None,
        });
    };

    let (page_results, page) = slice_page(results, limit, page_index, &param_hash)?;

    Ok(Answer {
        provenance,
        freshness,
        stale,
        refresh,
        classification,
        token_equivalence,
        workspace_relation,
        ordering,
        excluded_relations,
        not_compared_completely,
        unresolved_calls,
        unconfirmed_uses,
        override_relations,
        empty_scope,
        outcome: Outcome::Found { results: page_results },
        page,
    })
}

/// Cap the unresolved-call and unconfirmed-use groups at `limit`. The groups are not a paged result
/// set: the cap ignores the page index, so every page carries the same groups, and it issues no
/// token.
fn cap_reference_groups<T>(answer: &mut Answer<T>, limit: Option<usize>) {
    let Some(cap) = limit else { return };
    if let Some(group) = &mut answer.unresolved_calls {
        group.truncated = group.calls.len() > cap;
        group.calls.truncate(cap);
    }
    if let Some(group) = &mut answer.unconfirmed_uses {
        group.truncated = group.uses.len() > cap;
        group.uses.truncate(cap);
    }
}

/// Apply the result-set bound to a report-shaped answer, paging the detail rows `detail` selects out
/// of the single report the answer carries.
///
/// A report-shaped answer is one found result whose inner row vector is the set a caller bounds,
/// while the rest of the report — a depth bound, a horizon, a disclosure, an aggregate, a freshness
/// grade — is context repeated on every page rather than rows to page. Ambiguity, absence, and empty
/// carry no report and page exactly as any other answer's envelope, so those delegate to
/// [`apply_pagination`], which also caps an ambiguous candidate list and validates the cursor. The
/// token machinery is shared verbatim — one [`encode_token`]/[`decode_token`] path, the same identity
/// hash — the page merely spans the selected rows rather than the outer result vec.
pub fn apply_report_pagination<T>(
    answer: Answer<T>,
    limit: Option<usize>,
    cursor: Option<&str>,
    identity: &PageIdentity,
    detail: impl FnOnce(&mut T) -> &mut Vec<DependentItem>,
) -> anyhow::Result<Answer<T>> {
    if !matches!(answer.outcome, Outcome::Found { .. }) {
        return apply_pagination(answer, limit, cursor, identity);
    }

    let param_hash = identity.hash();
    let page_index = resolve_page_index(cursor, limit, &param_hash)?;

    let Answer {
        provenance,
        freshness,
        stale,
        refresh,
        classification,
        token_equivalence,
        workspace_relation,
        ordering,
        excluded_relations,
        not_compared_completely,
        unresolved_calls,
        unconfirmed_uses,
        override_relations,
        empty_scope,
        outcome,
        page: _,
    } = answer;
    let Outcome::Found { results } = outcome else {
        unreachable!("guarded to a found outcome above");
    };

    let Some(limit) = limit else {
        // No row cap: the whole report is returned with no paging disclosure.
        return Ok(Answer {
            provenance,
            freshness,
            stale,
            refresh,
            classification,
            token_equivalence,
            workspace_relation,
            ordering,
            excluded_relations,
            not_compared_completely,
            unresolved_calls,
            unconfirmed_uses,
            override_relations,
            empty_scope,
            outcome: Outcome::Found { results },
            page: None,
        });
    };

    // A found report-shaped answer carries exactly one report; its selected rows are the pageable set.
    let mut report = results
        .into_iter()
        .next()
        .expect("a found report-shaped answer carries exactly one report");
    let rows = detail(&mut report);
    let (page_rows, page) = slice_page(std::mem::take(rows), limit, page_index, &param_hash)?;
    *rows = page_rows;

    Ok(Answer {
        provenance,
        freshness,
        stale,
        refresh,
        classification,
        token_equivalence,
        workspace_relation,
        ordering,
        excluded_relations,
        not_compared_completely,
        unresolved_calls,
        unconfirmed_uses,
        override_relations,
        empty_scope,
        outcome: Outcome::Found { results: vec![report] },
        page,
    })
}

/// Page a bounded row set: the rows of page `page_index` at `limit` rows per page, with the page
/// block that discloses a partial view and the token that resumes the next page. The one slice-and-
/// token step the answer paths and `status` share.
///
/// A page block is attached only when the rows were truncated or a later page was resumed, so a set
/// that fits the limit reads like an unbounded one. A `page_index` past the last page is a usage
/// error naming the valid range.
fn slice_page<T>(
    rows: Vec<T>,
    limit: usize,
    page_index: usize,
    param_hash: &str,
) -> anyhow::Result<(Vec<T>, Option<PageInfo>)> {
    let total = rows.len();
    let page_count = total.div_ceil(limit).max(1);
    if page_index >= page_count {
        return Err(out_of_range(page_count - 1));
    }

    let start = page_index * limit;
    let page_rows: Vec<T> = rows.into_iter().skip(start).take(limit).collect();
    let returned = page_rows.len();
    let truncated = start + returned < total;
    let cursor = truncated.then(|| encode_token(param_hash, page_index + 1));
    let page = (truncated || page_index > 0).then_some(PageInfo {
        truncated,
        page_index,
        returned,
        total: Some(total),
        cursor,
    });
    Ok((page_rows, page))
}

/// Page `rows` for a command whose answer is not a query [`Answer`]: validate `cursor` against
/// `identity`, then return the page it names and the page block, or the whole set and no page block
/// when `limit` is `None`.
pub fn page_rows<T>(
    rows: Vec<T>,
    limit: Option<usize>,
    cursor: Option<&str>,
    identity: &PageIdentity,
) -> anyhow::Result<(Vec<T>, Option<PageInfo>)> {
    let param_hash = identity.hash();
    let page_index = resolve_page_index(cursor, limit, &param_hash)?;
    match limit {
        Some(limit) => slice_page(rows, limit, page_index, &param_hash),
        None => Ok((rows, None)),
    }
}

/// Apply the result-set bound to a `dependents` impact answer, paging the report's detailed rows.
pub fn apply_dependents_pagination(
    answer: Answer<DependentsReport>,
    limit: Option<usize>,
    cursor: Option<&str>,
    identity: &PageIdentity,
) -> anyhow::Result<Answer<DependentsReport>> {
    apply_report_pagination(answer, limit, cursor, identity, |report| &mut report.detail)
}

/// Apply the result-set bound to a diff-seeded `impact` answer, paging its dependents' detailed rows.
pub fn apply_impact_pagination(
    answer: Answer<ImpactReport>,
    limit: Option<usize>,
    cursor: Option<&str>,
    identity: &PageIdentity,
) -> anyhow::Result<Answer<ImpactReport>> {
    apply_report_pagination(answer, limit, cursor, identity, |report| &mut report.dependents.detail)
}

/// Resolve which page a request names: page 0 with no cursor, or the cursor's page after validating
/// its identity against `param_hash`.
fn resolve_page_index(cursor: Option<&str>, limit: Option<usize>, param_hash: &str) -> anyhow::Result<usize> {
    Ok(resolve_resume(cursor, limit, param_hash)?.map_or(0, |(page, _)| page))
}

/// Validate `cursor` against `param_hash` and return the page index and walk position it carries, or
/// `None` when there is no cursor.
fn resolve_resume(
    cursor: Option<&str>,
    limit: Option<usize>,
    param_hash: &str,
) -> anyhow::Result<Option<(usize, Option<String>)>> {
    let Some(token) = cursor else { return Ok(None) };
    // A cursor resumes into a bounded page; an unbounded set (`--limit 0`) has no pages to resume.
    if limit.is_none() {
        return Err(Failure::Usage(
            "--cursor cannot resume an unbounded result set (--limit 0); re-issue with a positive --limit".to_string(),
        )
        .into());
    }
    let (token_hash, page, position) = decode_token(token).ok_or_else(|| {
        Failure::Usage("the continuation token is malformed; re-issue the query without --cursor".to_string())
    })?;
    if token_hash != param_hash {
        return Err(Failure::Usage(
            "the continuation token was issued for different query parameters or a since-rebuilt index; \
             re-issue the query without --cursor"
                .to_string(),
        )
        .into());
    }
    Ok(Some((page, position)))
}

/// Where a walk-paged answer (`similar --mutual`) resumes: the page index the token names and the
/// last visited symbol, `None` for a first page.
pub struct WalkResume {
    /// The zero-based index of the page the cursor asks for.
    pub page_index: usize,
    /// The symbol after which the walk continues.
    pub after: Option<CanonicalId>,
}

/// Resolve the walk position `cursor` names, after validating it against `identity`. A token that
/// carries no walk position, as every ordinary page token, is malformed here.
pub fn resolve_walk_resume(
    cursor: Option<&str>,
    limit: Option<usize>,
    identity: &PageIdentity,
) -> anyhow::Result<WalkResume> {
    match resolve_resume(cursor, limit, &identity.hash())? {
        None => Ok(WalkResume {
            page_index: 0,
            after: None,
        }),
        // A page with no successor index cannot issue its next cursor; only a hand-edited token names one.
        Some((page_index, Some(position))) if page_index < usize::MAX => Ok(WalkResume {
            page_index,
            after: Some(CanonicalId::from_raw(position)),
        }),
        Some(_) => Err(Failure::Usage(
            "the continuation token is malformed; re-issue the query without --cursor".to_string(),
        )
        .into()),
    }
}

/// The paging disclosure of a walk-paged answer, which cannot state a total. Attached only when the
/// walk stopped with results remaining (`resume_after`) or resumed a later page, matching
/// [`slice_page`]'s rule.
pub fn walk_page(
    returned: usize,
    page_index: usize,
    resume_after: Option<&CanonicalId>,
    identity: &PageIdentity,
) -> Option<PageInfo> {
    let truncated = resume_after.is_some();
    (truncated || page_index > 0).then(|| PageInfo {
        truncated,
        page_index,
        returned,
        total: None,
        cursor: resume_after.map(|after| encode_walk_token(&identity.hash(), page_index + 1, after)),
    })
}

/// The usage error for a page beyond the last, naming the valid range.
fn out_of_range(last_page: usize) -> anyhow::Error {
    Failure::Usage(format!(
        "the continuation token names a page past the last; valid pages are 0..={last_page}"
    ))
    .into()
}

/// Encode a continuation token: the opaque hex of `parameter-hash:page-index`. The composition is not
/// a contract — callers treat the token as opaque and never parse it.
fn encode_token(param_hash: &str, page: usize) -> String {
    to_hex(format!("{param_hash}:{page}").as_bytes())
}

/// Encode a walk-paged continuation token: `parameter-hash:page-index:last-visited-symbol`, hex-encoded
/// as an ordinary token is. The symbol identity may itself contain `:`, so it comes last.
fn encode_walk_token(param_hash: &str, page: usize, after: &CanonicalId) -> String {
    to_hex(format!("{param_hash}:{page}:{}", after.as_str()).as_bytes())
}

/// Decode a continuation token back to its `(parameter-hash, page-index, walk position)`, or `None`
/// when it is not a well-formed token. The walk position is present only on a walk-paged token.
fn decode_token(token: &str) -> Option<(String, usize, Option<String>)> {
    let raw = from_hex(token)?;
    let text = String::from_utf8(raw).ok()?;
    let (hash, rest) = text.split_once(':')?;
    let (page, position) = match rest.split_once(':') {
        Some((page, position)) => (page, Some(position.to_string())),
        None => (rest, None),
    };
    Some((hash.to_string(), page.parse().ok()?, position))
}

/// Lowercase-hex encode a byte slice.
fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Decode a lowercase-hex string to bytes, or `None` when it is not valid hex.
fn from_hex(hex: &str) -> Option<Vec<u8>> {
    // Valid hex is ASCII; a multibyte string is rejected before byte-indexed slicing, which would
    // otherwise panic on a non-boundary index.
    if !hex.is_ascii() || !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    //! Unit coverage for the out-of-range-page branch: the CLI cannot honestly mint a well-formed,
    //! hash-matching token naming a page past the last (every token it issues names an existing next
    //! page), so a process-level test can only reach hash-mismatch or malformed-decode rejection.
    //! This branch is reachable only by encoding a token directly through the module's own
    //! [`encode_token`], which is why it is exercised here rather than through `tests/`.

    use super::*;
    use crate::graph::store::Freshness;
    use crate::query::output::Provenance;
    use crate::query::{DependentItem, HorizonDisclosure};

    fn provenance() -> Provenance {
        Provenance {
            analyzer_name: "test".to_string(),
            analyzer_version: "0".to_string(),
        }
    }

    fn identity(limit: usize) -> PageIdentity {
        PageIdentity {
            command: "get",
            reference: "subject".to_string(),
            relation: None,
            detail: None,
            depth: None,
            ordering: None,
            limit: Some(limit),
            max_lines: None,
            from: None,
            position_unit: None,
            source_hash: None,
            excluded_relations: Vec::new(),
            index_hash: "hash".to_string(),
        }
    }

    // _(Dependents order selector: a continuation token does not outlive its ranking model)_ — a
    // well-formed token hashed under one rank version is refused when presented under another,
    // through both pagination write-sites: the plain result-set path and the report-shaped
    // dependents path.
    #[test]
    fn a_token_hashed_under_a_different_rank_version_is_refused_on_both_paths() {
        let stale_identity = |order, version| {
            let mut identity = identity(1);
            identity.ordering = Some(OrderingIdentity::with_version(order, version));
            identity
        };

        // Plain path: a found result set under the current model refuses a prior model's token.
        let token = encode_token(&stale_identity("ranked", 1).hash(), 1);
        let answer = Answer::found(vec!["a".to_string(), "b".to_string()], provenance(), Freshness::Fresh);
        let err = apply_pagination(answer, Some(1), Some(&token), &stale_identity("ranked", 2))
            .expect_err("a prior rank version's token is refused, not resumed");
        assert!(
            err.to_string().contains("different query parameters"),
            "the refusal names the identity mismatch: {err}"
        );

        // Report-shaped path: the dependents pagination refuses through the same identity check.
        let detail: Vec<DependentItem> = (0..2)
            .map(|i| DependentItem {
                symbol: crate::query::output::SymbolView {
                    canonical_id: crate::identity::CanonicalId::from_raw(format!("test-ws::dep{i}")),
                    name: format!("dep{i}"),
                    kind: "function".to_string(),
                    external: false,
                },
                kind: crate::graph::store::DependencyKind::Uses,
                distance: 1,
                location: None,
                content: None,
                content_truncated: false,
            })
            .collect();
        let report = DependentsReport {
            depth_bound: 1,
            horizon: 1,
            disclosure: HorizonDisclosure::EndsWithinBound,
            detail,
            beyond_bound: Vec::new(),
        };
        let answer = Answer::found(vec![report], provenance(), Freshness::Fresh);
        let token = encode_token(&stale_identity("ranked", 1).hash(), 1);
        let err = apply_dependents_pagination(answer, Some(1), Some(&token), &stale_identity("ranked", 2))
            .expect_err("the report path refuses a prior rank version's token too");
        assert!(
            err.to_string().contains("different query parameters"),
            "the refusal names the identity mismatch: {err}"
        );
    }

    // _(Ranked ordering of dependents: a token issued under the first ranking model)_ — a token
    // hashed under rank version 1 is refused against the identity this binary builds.
    #[test]
    fn a_rank_version_one_token_is_refused_by_the_current_model() {
        let with_ordering = |ordering| {
            let mut identity = identity(1);
            identity.ordering = Some(ordering);
            identity
        };
        let token = encode_token(&with_ordering(OrderingIdentity::with_version("ranked", 1)).hash(), 1);
        let current = with_ordering(OrderingIdentity::current(crate::query::OrderMode::Ranked));
        let answer = Answer::found(vec!["a".to_string(), "b".to_string()], provenance(), Freshness::Fresh);
        let err = apply_pagination(answer, Some(1), Some(&token), &current)
            .expect_err("a version 1 token is refused, not resumed");
        assert!(
            err.to_string().contains("different query parameters"),
            "the refusal names the identity mismatch: {err}"
        );
    }

    // _(Dependents order selector: a continuation token binds its ordering)_ — two identities
    // differing only in the order mode hash differently, so a token issued under one ordering can
    // never validate against the other.
    #[test]
    fn identities_differing_only_in_order_mode_hash_differently() {
        let mut ranked = identity(1);
        ranked.ordering = Some(OrderingIdentity::with_version("ranked", 1));
        let mut unranked = identity(1);
        unranked.ordering = Some(OrderingIdentity::with_version("unranked", 1));
        assert_ne!(ranked.hash(), unranked.hash());
    }

    // _(Dependents order selector: a continuation token does not outlive its ranking model)_ —
    // two identities differing only in the rank version hash differently, so a token never
    // outlives its ranking model.
    #[test]
    fn identities_differing_only_in_rank_version_hash_differently() {
        let mut old = identity(1);
        old.ordering = Some(OrderingIdentity::with_version("ranked", 1));
        let mut new = identity(1);
        new.ordering = Some(OrderingIdentity::with_version("ranked", 2));
        assert_ne!(old.hash(), new.hash());
    }

    // _(Bounded and resumable answers: mismatched continuation token refused)_ — every field of the
    // query identity affects the parameter hash, so a token issued under one value of any parameter
    // is refused rather than resumed under another.
    //
    // The exhaustive struct literal is deliberate: a field added to `PageIdentity` fails compilation
    // here until it is populated, which is what puts a new parameter in front of the author. The
    // hash covering it is then structural — `hash` serializes the whole struct — so this test guards
    // the property against a return to hand-listed fields or a stray `#[serde(skip)]`.
    #[test]
    fn every_query_identity_field_affects_the_hash() {
        let base = || PageIdentity {
            command: "get",
            reference: "subject".to_string(),
            relation: Some("references"),
            detail: Some("body"),
            depth: Some(1),
            ordering: Some(OrderingIdentity::with_version("ranked", 1)),
            limit: Some(10),
            max_lines: Some(20),
            from: Some(3),
            position_unit: Some("line"),
            source_hash: Some("disk-hash".to_string()),
            excluded_relations: Vec::new(),
            index_hash: "index-hash".to_string(),
        };

        /// A named single-field change to a query identity.
        type Variation = (&'static str, fn(&mut PageIdentity));

        let variations: [Variation; 13] = [
            ("command", |id| id.command = "find"),
            ("reference", |id| id.reference = "other".to_string()),
            ("relation", |id| id.relation = Some("contains")),
            ("detail", |id| id.detail = Some("signature")),
            ("depth", |id| id.depth = Some(2)),
            ("ordering", |id| {
                id.ordering = Some(OrderingIdentity::with_version("unranked", 1))
            }),
            ("limit", |id| id.limit = Some(11)),
            ("max_lines", |id| id.max_lines = Some(21)),
            ("from", |id| id.from = Some(4)),
            ("position_unit", |id| id.position_unit = Some("byte")),
            ("source_hash", |id| id.source_hash = Some("edited".to_string())),
            ("excluded_relations", |id| {
                id.excluded_relations = vec![RelationLabel::TypeFamily]
            }),
            ("index_hash", |id| id.index_hash = "rebuilt".to_string()),
        ];

        let baseline = base().hash();
        for (field, vary) in variations {
            let mut varied = base();
            vary(&mut varied);
            assert_ne!(
                baseline,
                varied.hash(),
                "changing `{field}` must change the parameter hash, or a token issued under one \
                 value resumes against results produced under another"
            );
        }
    }

    // _(Bounded and resumable answers: mismatched continuation token refused)_ — an absent optional
    // parameter and a present one spelled as the empty string are distinct query identities, so a
    // token issued for one is refused for the other.
    #[test]
    fn an_absent_parameter_and_an_empty_one_hash_differently() {
        let mut absent = identity(1);
        absent.relation = None;
        let mut empty = identity(1);
        empty.relation = Some("");
        assert_ne!(absent.hash(), empty.hash());
    }

    // _(Bounded and resumable answers: mismatched continuation token refused — out-of-range page)_ —
    // a token whose parameter hash matches but whose page index names a page past the last is
    // refused as a usage error naming the valid range, not silently clamped or served out of bounds.
    #[test]
    fn out_of_range_page_names_the_valid_range() {
        let identity = identity(1);
        let answer = Answer::found(
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
            provenance(),
            Freshness::Fresh,
        );
        // 3 results at limit 1: pages 0..=2 are valid; page 5 is honestly minted (through the
        // module's own `encode_token`) but past the end.
        let token = encode_token(&identity.hash(), 5);

        let err = apply_pagination(answer, Some(1), Some(&token), &identity)
            .expect_err("a page past the last is refused, not silently served");
        let message = err.to_string();
        assert!(
            message.contains("0..=2"),
            "the rejection names the valid page range: {message}"
        );
    }

    // _(Bounded and resumable answers: mismatched continuation token refused — out-of-range page,
    // dependents)_ — the same refusal holds when paging a `dependents` report's detail rows.
    #[test]
    fn dependents_out_of_range_page_names_the_valid_range() {
        let identity = identity(1);
        let detail: Vec<DependentItem> = (0..3)
            .map(|i| DependentItem {
                symbol: crate::query::output::SymbolView {
                    canonical_id: crate::identity::CanonicalId::from_raw(format!("test-ws::dep{i}")),
                    name: format!("dep{i}"),
                    kind: "function".to_string(),
                    external: false,
                },
                kind: crate::graph::store::DependencyKind::Uses,
                distance: 1,
                location: None,
                content: None,
                content_truncated: false,
            })
            .collect();
        let report = DependentsReport {
            depth_bound: 1,
            horizon: 1,
            disclosure: HorizonDisclosure::EndsWithinBound,
            detail,
            beyond_bound: Vec::new(),
        };
        let answer = Answer::found(vec![report], provenance(), Freshness::Fresh);
        // 3 detail rows at limit 1: pages 0..=2 are valid; page 5 is out of range.
        let token = encode_token(&identity.hash(), 5);

        let err = apply_dependents_pagination(answer, Some(1), Some(&token), &identity)
            .expect_err("a page past the last is refused, not silently served");
        let message = err.to_string();
        assert!(
            message.contains("0..=2"),
            "the rejection names the valid page range: {message}"
        );
    }

    // _(Bounded and resumable answers: mismatched continuation token refused — out-of-range page,
    // impact)_ — the same refusal holds when paging an `impact` answer's dependents' detail rows,
    // exercising [`apply_report_pagination`] through its second named entry point.
    #[test]
    fn impact_out_of_range_page_names_the_valid_range() {
        use crate::query::impact::{Exactness, ImpactReport, SeedOutcome};

        let identity = identity(1);
        let detail: Vec<DependentItem> = (0..3)
            .map(|i| DependentItem {
                symbol: crate::query::output::SymbolView {
                    canonical_id: crate::identity::CanonicalId::from_raw(format!("test-ws::dep{i}")),
                    name: format!("dep{i}"),
                    kind: "function".to_string(),
                    external: false,
                },
                kind: crate::graph::store::DependencyKind::Uses,
                distance: 1,
                location: None,
                content: None,
                content_truncated: false,
            })
            .collect();
        let report = ImpactReport {
            seed_mode: "working_tree",
            base_revision: "abc123".to_string(),
            exactness: Exactness::Exact,
            seed_outcome: SeedOutcome::Seeded,
            seeds: Vec::new(),
            unmappable: Vec::new(),
            dependents_snapshot: "current_index",
            dependents: DependentsReport {
                depth_bound: 1,
                horizon: 1,
                disclosure: HorizonDisclosure::EndsWithinBound,
                detail,
                beyond_bound: Vec::new(),
            },
        };
        let answer = Answer::found(vec![report], provenance(), Freshness::Fresh);
        // 3 detail rows at limit 1: pages 0..=2 are valid; page 5 is out of range.
        let token = encode_token(&identity.hash(), 5);

        let err = apply_impact_pagination(answer, Some(1), Some(&token), &identity)
            .expect_err("a page past the last is refused, not silently served");
        let message = err.to_string();
        assert!(
            message.contains("0..=2"),
            "the rejection names the valid page range: {message}"
        );
    }

    // A hand-edited walk token whose page index has no successor is malformed, not resumed.
    #[test]
    fn a_walk_token_whose_page_cannot_advance_is_refused() {
        let identity = identity(1);
        let token = encode_walk_token(&identity.hash(), usize::MAX, &CanonicalId::from_raw("test-ws::a"));
        let err = resolve_walk_resume(Some(&token), Some(1), &identity)
            .err()
            .expect("an unadvanceable page index is refused");
        assert!(err.to_string().contains("malformed"), "{err}");
    }
}
