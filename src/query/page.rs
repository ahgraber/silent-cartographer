//! The shared result-set bounding step: offset paging over the deterministic result order every
//! answer already carries, with an opaque continuation token bound to both the query parameters and
//! the recorded index identity (content-hash plus analyzer provenance).
//!
//! Paging is applied after the engine produces a full answer, so the engine's query methods stay
//! bounding-agnostic. Only a found result set is paged; typed absence, an empty relation, and an
//! ambiguity refusal carry no result set to bound.

use sha2::{Digest, Sha256};

use crate::exit::Failure;

use super::impact::ImpactReport;
use super::output::{Answer, Outcome, PageInfo};
use super::{DependentItem, DependentsReport};

/// The query identity a continuation token is bound to: the command, subject, relation/detail, the
/// depth bound, the output bounds (result limit, content-line bound, and `get`'s window start), and
/// the recorded index identity (content-hash plus analyzer provenance). A token resumes only against
/// the exact same identity — a different query, or an index rebuilt underneath (changed sources or a
/// changed analyzer version), changes the hash and is rejected.
pub struct PageIdentity {
    /// The command name (`get`, `trace`, `find`, or `impact`).
    pub command: &'static str,
    /// The subject reference (or source-position string), as presented.
    pub reference: String,
    /// The relation, for `trace`; `None` for `get`.
    pub relation: Option<&'static str>,
    /// The detail level, when one was requested.
    pub detail: Option<&'static str>,
    /// The requested depth bound (the `dependents` relation); `None` elsewhere.
    pub depth: Option<u32>,
    /// The effective result limit — the applied default or an explicit value; `None` for an
    /// explicit unbounded request (`--limit 0`).
    pub limit: Option<usize>,
    /// The effective per-result content-line bound; `None` for an unbounded request (`--max-lines 0`).
    pub max_lines: Option<usize>,
    /// The `get` content-window start line (1-based); `None` for commands without a window (`trace`,
    /// `find`).
    pub from: Option<usize>,
    /// The recorded index identity the answer was drawn from: the index content-hash composed with
    /// the recorded analyzer provenance (name and version). A same-source rebuild under a different
    /// analyzer version changes this even when the content-hash is unchanged, so a stale token is
    /// rejected rather than resumed against possibly-shifted extraction.
    pub index_hash: String,
}

impl PageIdentity {
    /// The parameter-identity hash: a hex digest over every field that must match for a token to
    /// resume. It is deliberately not the token itself — the token additionally carries the page
    /// index and is opaque.
    fn hash(&self) -> String {
        let mut hasher = Sha256::new();
        for field in [
            self.command,
            self.reference.as_str(),
            self.relation.unwrap_or(""),
            self.detail.unwrap_or(""),
            &self.depth.map(|n| n.to_string()).unwrap_or_default(),
            &self.limit.map(|n| n.to_string()).unwrap_or_default(),
            &self.max_lines.map(|n| n.to_string()).unwrap_or_default(),
            &self.from.map(|n| n.to_string()).unwrap_or_default(),
            self.index_hash.as_str(),
        ] {
            hasher.update(field.as_bytes());
            hasher.update([0u8]);
        }
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
    answer: Answer<T>,
    limit: Option<usize>,
    cursor: Option<&str>,
    identity: &PageIdentity,
) -> anyhow::Result<Answer<T>> {
    let param_hash = identity.hash();
    let page_index = resolve_page_index(cursor, limit, &param_hash)?;

    let Answer {
        provenance,
        freshness,
        stale,
        classification,
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
            classification,
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
            classification,
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
            classification,
            outcome: Outcome::Found { results },
            page: None,
        });
    };

    let total = results.len();
    let page_count = total.div_ceil(limit).max(1);
    if page_index >= page_count {
        return Err(out_of_range(page_count - 1));
    }

    let start = page_index * limit;
    let page_results: Vec<T> = results.into_iter().skip(start).take(limit).collect();
    let returned = page_results.len();
    let truncated = start + returned < total;
    let cursor = truncated.then(|| encode_token(&param_hash, page_index + 1));

    // A page block is disclosure of a partial view: it is attached only when the answer is not the
    // whole set on the first page — i.e. the result set was truncated, or a later page was resumed.
    // A default-bounded query whose set fits within the limit thus reads like an unbounded one.
    let page = (truncated || page_index > 0).then_some(PageInfo {
        truncated,
        page_index,
        returned,
        total,
        cursor,
    });

    Ok(Answer {
        provenance,
        freshness,
        stale,
        classification,
        outcome: Outcome::Found { results: page_results },
        page,
    })
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
        classification,
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
            classification,
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
    let total = rows.len();
    let page_count = total.div_ceil(limit).max(1);
    if page_index >= page_count {
        return Err(out_of_range(page_count - 1));
    }

    let start = page_index * limit;
    let page_rows: Vec<DependentItem> = std::mem::take(rows).into_iter().skip(start).take(limit).collect();
    let returned = page_rows.len();
    let truncated = start + returned < total;
    let cursor = truncated.then(|| encode_token(&param_hash, page_index + 1));
    *rows = page_rows;

    // A page block is disclosure of a partial view: attached only when the detail rows were truncated
    // to the limit or a later page was resumed, so a report whose rows fit the limit reads unbounded.
    let page = (truncated || page_index > 0).then_some(PageInfo {
        truncated,
        page_index,
        returned,
        total,
        cursor,
    });

    Ok(Answer {
        provenance,
        freshness,
        stale,
        classification,
        outcome: Outcome::Found { results: vec![report] },
        page,
    })
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
    let Some(token) = cursor else { return Ok(0) };
    // A cursor resumes into a bounded page; an unbounded set (`--limit 0`) has no pages to resume.
    if limit.is_none() {
        return Err(Failure::Usage(
            "--cursor cannot resume an unbounded result set (--limit 0); re-issue with a positive --limit".to_string(),
        )
        .into());
    }
    let (token_hash, page) = decode_token(token).ok_or_else(|| {
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
    Ok(page)
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

/// Decode a continuation token back to its `(parameter-hash, page-index)`, or `None` when it is not a
/// well-formed token.
fn decode_token(token: &str) -> Option<(String, usize)> {
    let raw = from_hex(token)?;
    let text = String::from_utf8(raw).ok()?;
    let (hash, page) = text.rsplit_once(':')?;
    let page: usize = page.parse().ok()?;
    Some((hash.to_string(), page))
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
            limit: Some(limit),
            max_lines: None,
            from: None,
            index_hash: "hash".to_string(),
        }
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
                kind: "uses".to_string(),
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
                kind: "uses".to_string(),
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
            recovery: None,
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
}
