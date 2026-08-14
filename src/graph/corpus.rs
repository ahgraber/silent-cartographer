//! The semantic corpus: which symbols contribute an entry, and the deterministic per-symbol render
//! that becomes the entry's content.
//!
//! The corpus covers non-nested content only: a leaf (a symbol containing no other persisted
//! symbol) contributes its own source content; a container contributes its interface tier. No entry
//! derives from a container's full body — enclosure duplicates content at every level, so indexing
//! nested bodies would double-count lexical statistics and return duplicate hits.
//!
//! The render front-loads retrieval evidence the raw source spells poorly: the symbol's name split
//! into words (a compound name like `withBackoff` is findable as "with backoff" only if split), its
//! kind, its module-path words, its signature, and its own documentation, followed by the entry's
//! content text. The same render feeds both the embedding and the lexical index, so both retrieval
//! signals see the same evidence.

use crate::identity::CanonicalId;

/// The corpus definition version: the second half of the semantic-index identity, alongside the
/// embedding model identity. Bumped whenever corpus membership or the render changes, and any bump
/// ships with a schema-version bump so stores built under the old definition refuse wholesale.
pub const CORPUS_DEFINITION_VERSION: u32 = 1;

/// The corpus-relevant view of one persisted in-workspace symbol.
pub struct CorpusSource<'a> {
    /// The symbol's canonical identity.
    pub canonical_id: &'a CanonicalId,
    /// The symbol's display name (its terminal name).
    pub display_name: &'a str,
    /// The persisted kind tag (`function`, `type`, `module`, …).
    pub kind: &'a str,
    /// The signature tier, when persisted.
    pub signature_text: Option<&'a str>,
    /// The interface tier, when persisted.
    pub interface_text: Option<&'a str>,
    /// The body tier (the symbol's full source text), when persisted.
    pub span_text: Option<&'a str>,
    /// Whether the symbol contains another persisted symbol (it is the source of a `contains`
    /// edge). Containers contribute their interface tier; leaves their own source content.
    pub contains_persisted: bool,
}

impl CorpusSource<'_> {
    /// Whether this symbol contributes a corpus entry: its tier content is persisted, and that
    /// content is more than its own bare name token.
    ///
    /// A symbol whose every tier degraded to its name (a function parameter, an assignment name —
    /// shapes the declaration walk does not recognize) carries nothing beyond its identity; its
    /// short render would win BM25 length normalization and displace content-bearing candidates,
    /// and its single-token clone keys would collide with every same-named parameter in the
    /// workspace. The clone-key pass shares this predicate, so an ineligible symbol carries no
    /// keys either.
    pub fn contributes(&self) -> bool {
        content_contributes(
            self.display_name,
            self.signature_text,
            self.interface_text,
            self.span_text,
        )
    }
}

/// The corpus-eligibility predicate over a symbol's tier content: persisted, and more than the
/// bare name token. The build consults it before assembly too — containment for corpus purposes
/// counts contributing children only, so the container/leaf split needs the verdict per symbol
/// ahead of the [`CorpusSource`] construction that carries it.
pub fn content_contributes(
    display_name: &str,
    signature_text: Option<&str>,
    interface_text: Option<&str>,
    span_text: Option<&str>,
) -> bool {
    let Some(span_text) = span_text else {
        return false;
    };
    let name_only =
        span_text == display_name && signature_text == Some(span_text) && interface_text == Some(span_text);
    !name_only
}

/// One corpus entry: a symbol identity and the render text its representations derive from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusEntry {
    /// The contributing symbol.
    pub symbol_id: CanonicalId,
    /// The deterministic render text.
    pub render: String,
}

/// Assemble the corpus over the given symbols: one entry per eligible symbol, ordered by canonical
/// identity so identical inputs always yield identical corpora regardless of input order.
///
/// Eligibility is [`CorpusSource::contributes`]: tier content persisted, and more than the bare
/// name token. A leaf's entry content is its own source; a container's is its interface tier, so
/// no entry ever derives from a container's full body.
pub fn assemble(sources: &[CorpusSource<'_>]) -> Vec<CorpusEntry> {
    let mut entries: Vec<CorpusEntry> = sources
        .iter()
        .filter(|s| s.contributes())
        .map(|s| CorpusEntry {
            symbol_id: s.canonical_id.clone(),
            render: render(s),
        })
        .collect();
    entries.sort_by(|a, b| a.symbol_id.as_str().cmp(b.symbol_id.as_str()));
    entries.dedup_by(|a, b| a.symbol_id == b.symbol_id);
    entries
}

/// The deterministic render for one symbol: name words, kind, module-path words, signature, own
/// documentation, then the entry's content text (leaf body or container interface).
fn render(source: &CorpusSource<'_>) -> String {
    let mut parts: Vec<String> = Vec::new();
    let name_words = split_words(source.display_name);
    if !name_words.is_empty() {
        parts.push(name_words.join(" "));
    }
    parts.push(source.kind.to_string());
    let path_words = module_path_words(source.canonical_id);
    if !path_words.is_empty() {
        parts.push(path_words.join(" "));
    }
    if let Some(sig) = source.signature_text {
        parts.push(sig.to_string());
    }
    if let Some(docs) = own_documentation(source.signature_text, source.interface_text) {
        parts.push(docs);
    }
    let content = if source.contains_persisted {
        // A container's content is its interface tier; its signature tier is the fallback for the
        // degenerate shapes whose tiers collapsed to the same text.
        source.interface_text.or(source.signature_text)
    } else {
        source.span_text
    };
    if let Some(content) = content {
        parts.push(content.to_string());
    }
    parts.join("\n")
}

/// The symbol's own documentation, recovered from its tier pair.
///
/// The interface tier is the signature plus the symbol's own documentation — before it (a Rust doc
/// run) or after it (a Python docstring, and a module's documentation under its qualified name) —
/// so the documentation is the interface with the signature removed from whichever end it occupies.
/// `None` when the tiers are equal (no documentation) or when the pair has no recognizable shape.
fn own_documentation(signature: Option<&str>, interface: Option<&str>) -> Option<String> {
    let signature = signature?;
    let interface = interface?;
    if interface == signature {
        return None;
    }
    if let Some(prefix) = interface.strip_suffix(signature) {
        let docs = prefix.trim();
        return (!docs.is_empty()).then(|| docs.to_string());
    }
    if let Some(suffix) = interface.strip_prefix(signature) {
        let docs = suffix.trim();
        return (!docs.is_empty()).then(|| docs.to_string());
    }
    None
}

/// The words of the symbol's module path: every path segment between the workspace segment and the
/// terminal name, split into words. A trailing `#<rank>` disambiguator is identity, not vocabulary,
/// and contributes nothing.
fn module_path_words(id: &CanonicalId) -> Vec<String> {
    let qualified = id.as_str().split_once("::").map(|(_, rest)| rest).unwrap_or("");
    let without_rank = match qualified.rsplit_once('#') {
        Some((head, rank)) if rank.chars().all(|c| c.is_ascii_digit()) => head,
        _ => qualified,
    };
    let segments: Vec<&str> = without_rank.split("::").collect();
    let Some((_terminal, path)) = segments.split_last() else {
        return Vec::new();
    };
    path.iter().flat_map(|seg| split_words(seg)).collect()
}

/// Split an identifier into lowercase words: `_`/`-`/`.` separators, camel-case boundaries, and
/// acronym runs (`HTTPClient` → `http client`). Digit runs stay attached to their word (`sha256`
/// stays one word).
pub fn split_words(identifier: &str) -> Vec<String> {
    let chars: Vec<char> = identifier.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        if !current.is_empty() {
            let prev = chars[i - 1];
            let camel_boundary = (prev.is_lowercase() || prev.is_numeric()) && c.is_uppercase();
            let acronym_end =
                prev.is_uppercase() && c.is_uppercase() && chars.get(i + 1).is_some_and(|next| next.is_lowercase());
            if camel_boundary || acronym_end {
                words.push(std::mem::take(&mut current));
            }
        }
        current.extend(c.to_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> CanonicalId {
        CanonicalId::from_raw(raw)
    }

    /// A leaf declaration's entry derives from its own source content and identity.
    #[test]
    fn leaf_contributes_its_own_content() {
        let leaf_id = id("ws::app::retry::with_backoff");
        let sources = [CorpusSource {
            canonical_id: &leaf_id,
            display_name: "with_backoff",
            kind: "function",
            signature_text: Some("fn with_backoff(tries: u32)"),
            interface_text: Some("/// Retry a call with exponential backoff.\nfn with_backoff(tries: u32)"),
            span_text: Some("fn with_backoff(tries: u32) { sleep(tries) }"),
            contains_persisted: false,
        }];
        let corpus = assemble(&sources);
        assert_eq!(corpus.len(), 1);
        assert_eq!(corpus[0].symbol_id.as_str(), "ws::app::retry::with_backoff");
        assert!(
            corpus[0]
                .render
                .contains("fn with_backoff(tries: u32) { sleep(tries) }")
        );
        assert!(corpus[0].render.contains("with backoff"));
        assert!(corpus[0].render.contains("Retry a call with exponential backoff."));
    }

    /// A container's entry derives from its interface tier; no entry carries its members' bodies
    /// through it, so nothing in the corpus derives from a container's full body.
    #[test]
    fn container_contributes_interface_only() {
        let type_id = id("ws::app::Client");
        let method_id = id("ws::app::Client::send");
        let sources = [
            CorpusSource {
                canonical_id: &type_id,
                display_name: "Client",
                kind: "type",
                signature_text: Some("struct Client"),
                interface_text: Some("/// An HTTP client.\nstruct Client"),
                span_text: Some("struct Client { fn send(&self) { wire_bytes_out() } }"),
                contains_persisted: true,
            },
            CorpusSource {
                canonical_id: &method_id,
                display_name: "send",
                kind: "method",
                signature_text: Some("fn send(&self)"),
                interface_text: Some("fn send(&self)"),
                span_text: Some("fn send(&self) { wire_bytes_out() }"),
                contains_persisted: false,
            },
        ];
        let corpus = assemble(&sources);
        assert_eq!(corpus.len(), 2);
        let container = corpus.iter().find(|e| e.symbol_id == type_id).unwrap();
        assert!(container.render.contains("An HTTP client."));
        assert!(!container.render.contains("wire_bytes_out"));
    }

    /// A module's entry derives from its interface tier (qualified name plus module documentation),
    /// never from the whole document.
    #[test]
    fn module_entry_excludes_member_bodies() {
        let module_id = id("ws::app::retry");
        let sources = [CorpusSource {
            canonical_id: &module_id,
            display_name: "retry",
            kind: "module",
            signature_text: Some("app::retry"),
            interface_text: Some("app::retry\nRetry policies for outbound calls."),
            span_text: Some(
                "//! Retry policies for outbound calls.\nfn with_backoff() { sleep_inside_module_body() }",
            ),
            contains_persisted: true,
        }];
        let corpus = assemble(&sources);
        assert_eq!(corpus.len(), 1);
        assert!(corpus[0].render.contains("Retry policies for outbound calls."));
        assert!(!corpus[0].render.contains("sleep_inside_module_body"));
    }

    /// A symbol with no persisted tier content contributes nothing, and each contributor appears
    /// exactly once.
    #[test]
    fn eligibility_and_exactly_once() {
        let external_id = id("ws::dep::serde::Serialize");
        let leaf_id = id("ws::app::main");
        let sources = [
            CorpusSource {
                canonical_id: &external_id,
                display_name: "Serialize",
                kind: "trait",
                signature_text: None,
                interface_text: None,
                span_text: None,
                contains_persisted: false,
            },
            CorpusSource {
                canonical_id: &leaf_id,
                display_name: "main",
                kind: "function",
                signature_text: Some("fn main()"),
                interface_text: Some("fn main()"),
                span_text: Some("fn main() {}"),
                contains_persisted: false,
            },
        ];
        let corpus = assemble(&sources);
        assert_eq!(corpus.len(), 1);
        assert_eq!(corpus[0].symbol_id, leaf_id);
    }

    /// A symbol whose every tier is its bare name token contributes nothing: name-only content
    /// carries nothing beyond the identity, and its short render would displace content-bearing
    /// candidates.
    #[test]
    fn name_only_symbols_are_ineligible() {
        let param_id = id("ws::app::shout::word");
        let sources = [CorpusSource {
            canonical_id: &param_id,
            display_name: "word",
            kind: "other",
            signature_text: Some("word"),
            interface_text: Some("word"),
            span_text: Some("word"),
            contains_persisted: false,
        }];
        assert!(!sources[0].contributes());
        assert!(assemble(&sources).is_empty());
    }

    /// Identical inputs yield identical corpora, regardless of input order.
    #[test]
    fn assembly_is_deterministic() {
        let a_id = id("ws::app::alpha");
        let b_id = id("ws::app::beta");
        let make = |canonical_id, name| CorpusSource {
            canonical_id,
            display_name: name,
            kind: "function",
            signature_text: Some("fn x()"),
            interface_text: Some("fn x()"),
            span_text: Some("fn x() {}"),
            contains_persisted: false,
        };
        let forward = [make(&a_id, "alpha"), make(&b_id, "beta")];
        let reversed = [make(&b_id, "beta"), make(&a_id, "alpha")];
        assert_eq!(assemble(&forward), assemble(&reversed));
    }

    /// Compound names split into their words, in every spelling convention.
    #[test]
    fn split_words_covers_naming_conventions() {
        assert_eq!(split_words("withBackoff"), ["with", "backoff"]);
        assert_eq!(split_words("HTTPClient"), ["http", "client"]);
        assert_eq!(split_words("parse_config"), ["parse", "config"]);
        assert_eq!(split_words("sha256_digest"), ["sha256", "digest"]);
        assert_eq!(split_words("Client"), ["client"]);
    }
}
