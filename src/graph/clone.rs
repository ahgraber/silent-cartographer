//! Clone-equivalence keys over a leaf declaration's spelled token sequence.
//!
//! Two keys, each an exactly-when equivalence over static token structure:
//!
//! - the **formatting-insensitive key** collides exactly when two token sequences are identical as
//!   spelled — the sources differ only in whitespace and comments;
//! - the **substitution-insensitive key** collides exactly when the sequences are additionally
//!   identical under a consistent one-to-one substitution of identifiers and literal values.
//!
//! The substitution canonicalization replaces each distinct identifier and each distinct literal
//! with its first-occurrence ordinal, one ordinal space per token class. Consistency falls out of
//! the ordinals: merging two distinct names (or two distinct literals) into one changes the ordinal
//! pattern, so an inconsistent substitution never collides. The keys assert token structure only —
//! never behavioral equivalence: a substituted literal changes behavior.

use std::collections::{BTreeMap, HashMap};

use sha2::{Digest, Sha256};

use crate::identity::CanonicalId;

use super::range::ByteSpan;
use super::syntax::{SpelledToken, SyntaxTree, TokenClass};

/// The pair of equivalence keys for one leaf declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneKeys {
    /// Collides exactly when the token sequences are identical as spelled.
    pub formatting_key: String,
    /// Collides exactly when the sequences match under a consistent one-to-one substitution of
    /// identifiers and literal values.
    pub substitution_key: String,
}

/// Compute both keys over a spelled token sequence. `None` for an empty sequence — no tokens, no
/// structure to assert.
pub fn clone_keys(tokens: &[SpelledToken]) -> Option<CloneKeys> {
    if tokens.is_empty() {
        return None;
    }
    let formatting_key = hash_sequence(tokens.iter().map(|t| t.text.as_str()));

    let mut identifier_ordinals: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut literal_ordinals: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let canonical: Vec<String> = tokens
        .iter()
        .map(|token| match token.class {
            TokenClass::Identifier => format!("i{}", ordinal(&mut identifier_ordinals, &token.text)),
            TokenClass::Literal => format!("l{}", ordinal(&mut literal_ordinals, &token.text)),
            TokenClass::Other => token.text.clone(),
            TokenClass::Boundary => unreachable!("clone keys read a leaf's spelled tokens, which hold no boundary"),
        })
        .collect();
    let substitution_key = hash_sequence(canonical.iter().map(String::as_str));

    Some(CloneKeys {
        formatting_key,
        substitution_key,
    })
}

/// The first-occurrence ordinal of `text` within its token class, assigning the next ordinal on
/// first sight.
fn ordinal<'a>(seen: &mut std::collections::HashMap<&'a str, usize>, text: &'a str) -> usize {
    let next = seen.len();
    *seen.entry(text).or_insert(next)
}

/// Hash a token sequence into a hex key. Each token is length-prefixed so no concatenation of
/// neighboring tokens can spell another sequence's bytes.
fn hash_sequence<'a>(tokens: impl Iterator<Item = &'a str>) -> String {
    let mut hasher = Sha256::new();
    for token in tokens {
        hasher.update((token.len() as u64).to_le_bytes());
        hasher.update(token.as_bytes());
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Tokens in each fingerprinted k-gram.
pub const FRAGMENT_KGRAM_LENGTH: usize = 16;

/// K-grams in each winnowing window.
pub const FRAGMENT_WINDOW: usize = 17;

/// The shortest shared token run that is guaranteed to share a fingerprint under k-gram length `k`
/// and window `w`.
pub const fn guaranteed_fragment_length(k: usize, w: usize) -> usize {
    w + k - 1
}

/// The corpus-contributing symbols a function contains, as `(span, identity)`: every candidate other
/// than `subject` whose span lies within `span` and differs from it. The build and the query both
/// select contained symbols through this rule, each passing the corpus-contributing symbols of the
/// subject's document.
pub fn contained_symbols<'a>(
    subject: &CanonicalId,
    span: (usize, usize),
    candidates: impl IntoIterator<Item = (&'a CanonicalId, (usize, usize))>,
) -> Vec<(ByteSpan, &'a CanonicalId)> {
    candidates
        .into_iter()
        .filter(|&(id, inner)| {
            id != subject && super::corpus::span_encloses(span, inner, super::corpus::OuterKind::Item)
        })
        .map(|(id, (start, end))| (ByteSpan { start, end }, id))
        .collect()
}

/// A symbol's comparable tokens: its spelled tokens over `span`, with the tokens of each contained
/// symbol replaced by one boundary token at the contained span's start. Overlapping contained spans
/// merge into one boundary, named by the merged group's earliest-starting, then longest, then
/// lowest-identity symbol. With nothing contained, this is [`SyntaxTree::spelled_tokens`] over
/// `span`.
///
/// The build and the query both call this, so the stored fingerprint ordinals index the sequence a
/// query re-spells.
pub fn comparable_tokens(
    tree: &SyntaxTree,
    span: ByteSpan,
    contained: &[(ByteSpan, &CanonicalId)],
) -> Vec<SpelledToken> {
    let mut ordered = contained.to_vec();
    ordered.sort_by(|a, b| {
        (a.0.start, std::cmp::Reverse(a.0.end), a.1).cmp(&(b.0.start, std::cmp::Reverse(b.0.end), b.1))
    });
    let mut merged: Vec<(ByteSpan, &CanonicalId)> = Vec::new();
    for (span, id) in ordered {
        match merged.last_mut() {
            Some((last, _)) if span.start < last.end => last.end = last.end.max(span.end),
            _ => merged.push((span, id)),
        }
    }
    let mut tokens: Vec<SpelledToken> = tree
        .spelled_tokens(span)
        .into_iter()
        .filter(|token| {
            !merged
                .iter()
                .any(|(span, _)| span.start <= token.start && token.start < span.end)
        })
        .collect();
    tokens.extend(merged.iter().map(|(span, id)| SpelledToken {
        class: TokenClass::Boundary,
        text: id.as_str().to_string(),
        start: span.start,
        data: false,
    }));
    tokens.sort_by_key(|token| token.start);
    tokens
}

/// Winnow a token sequence into fingerprints: `(hash, token_ordinal)` pairs, where `token_ordinal`
/// is the selected k-gram's start index in `tokens`.
///
/// Every identifier is one placeholder and every literal another, so a consistently or
/// inconsistently renamed copy fingerprints alike, except in a k-gram made only of data tokens,
/// which hashes its text verbatim. Any run of at least [`guaranteed_fragment_length`]`(k, w)`
/// tokens shared by two sequences under [`shared_runs`] with data-stretch length `k` yields a
/// shared fingerprint at the same offset within the run in both. A sequence shorter than that
/// length yields none.
pub fn winnow(tokens: &[SpelledToken], k: usize, w: usize) -> Vec<(i64, usize)> {
    if tokens.len() < guaranteed_fragment_length(k, w) {
        return Vec::new();
    }
    let hashes: Vec<i64> = tokens.windows(k).map(kgram_hash).collect();
    let mut selected: Vec<(i64, usize)> = Vec::new();
    for (window_start, window) in hashes.windows(w).enumerate() {
        // The rightmost minimum: `<=` lets a later equal hash displace an earlier one.
        let (offset, &hash) = window
            .iter()
            .enumerate()
            .reduce(|best, next| if next.1 <= best.1 { next } else { best })
            .expect("a window holds w >= 1 hashes");
        let position = window_start + offset;
        if selected.last().is_none_or(|&(_, last)| last != position) {
            selected.push((hash, position));
        }
    }
    selected
}

/// Hash one k-gram: the first 8 bytes of SHA-256 over each token's class tag and length-prefixed
/// text, read as a signed integer. In a k-gram that holds any token other than data, identifiers
/// and literals contribute their class tag only, so no source text of either reaches the hash. A
/// k-gram made only of data tokens contributes every token's text verbatim, so different tables
/// of values hash apart. A boundary contributes the contained symbol's identity, so the boundary
/// k-grams of different contained symbols differ.
fn kgram_hash(kgram: &[SpelledToken]) -> i64 {
    let verbatim = kgram.iter().all(|token| token.data);
    let mut hasher = Sha256::new();
    for token in kgram {
        let (tag, text) = match token.class {
            TokenClass::Identifier => (0u8, if verbatim { token.text.as_str() } else { "" }),
            TokenClass::Literal => (1u8, if verbatim { token.text.as_str() } else { "" }),
            TokenClass::Other => (2u8, token.text.as_str()),
            TokenClass::Boundary => (3u8, token.text.as_str()),
        };
        hasher.update([tag]);
        hasher.update((text.len() as u64).to_le_bytes());
        hasher.update(text.as_bytes());
    }
    let digest = hasher.finalize();
    i64::from_le_bytes(digest[..8].try_into().expect("a SHA-256 digest holds 32 bytes"))
}

/// A pair of token runs, one in each of two sequences, identical under a consistent one-to-one
/// substitution of identifiers and literal values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedRun {
    /// The run's first token ordinal in the first sequence.
    pub a_start: usize,
    /// The run's first token ordinal in the second sequence.
    pub b_start: usize,
    /// The run's length in tokens, equal in both sequences.
    pub length: usize,
    /// Whether the two runs are identical as spelled.
    pub exact: bool,
}

/// The maximal pairs of runs of `a` and `b`, each at least `min_length` tokens, that are
/// consistent, and whose placeholder-equal run on the same diagonal holds a seed. No returned pair
/// lies within another, in both sequences at once.
///
/// A pair of runs is consistent when, position by position, the two tokens have the same class and
/// the same data role, every structural token is spelled alike, the identifiers and the literals
/// are identical under a consistent one-to-one substitution, one substitution per token class, and
/// every token the runs include of a data stretch is spelled alike in both. A data stretch is a
/// maximal stretch of positions on the diagonal at which both sequences hold data tokens, at least
/// `data_stretch_length` tokens long over its whole length, including any part outside the runs.
///
/// `seeds` are `(ordinal in a, ordinal in b)` positions of shared fingerprints; a seed out of range
/// or on unequal placeholder tokens (a hash collision) contributes nothing, so every returned pair
/// is verified token by token.
///
/// Completeness: given the fingerprints [`winnow`] gives both sequences under k-gram length
/// `data_stretch_length` as seeds, and `min_length` at least the guaranteed fragment length, every
/// consistent pair of at least `min_length` tokens lies within a returned pair.
pub fn shared_runs(
    a: &[SpelledToken],
    b: &[SpelledToken],
    seeds: &[(usize, usize)],
    min_length: usize,
    data_stretch_length: usize,
) -> Vec<SharedRun> {
    // The data role is compared too: a data token hashes verbatim and its counterpart must as well.
    let placeholder_equal = |i: usize, j: usize| {
        a[i].class == b[j].class
            && a[i].data == b[j].data
            && a[i].class != TokenClass::Boundary
            && (a[i].class != TokenClass::Other || a[i].text == b[j].text)
    };
    let diagonal = |&(i, j): &(usize, usize)| i as i64 - j as i64;
    let mut seeds: Vec<(usize, usize)> = seeds
        .iter()
        .copied()
        .filter(|&(i, j)| i < a.len() && j < b.len() && placeholder_equal(i, j))
        .collect();
    seeds.sort_by_key(|seed| (diagonal(seed), seed.0));
    seeds.dedup();

    let mut runs: Vec<SharedRun> = Vec::new();
    // The diagonal and end ordinal in `a` of the last run extended: a later seed inside it adds nothing.
    let mut covered: Option<(i64, usize)> = None;
    // Completeness: a consistent pair lies within one maximal placeholder-equal run on its
    // diagonal. Within the pair, a k-gram made only of data tokens lies in a data stretch, since the
    // stretch holds at least those k tokens, and every token of it within the pair is spelled alike
    // in both, so the k-gram hashes alike verbatim; every other k-gram hashes alike over
    // placeholders. Winnowing therefore places a shared fingerprint at the same offset in both runs
    // of the pair, so that run holds a seed. The pair includes no break, and consistency holds for
    // every sub-interval of a consistent interval, so the scan of that run finds every maximal
    // consistent interval, and the pair lies within one.
    for seed in seeds {
        let (i, j) = seed;
        if covered.is_some_and(|(d, end)| d == diagonal(&seed) && i < end) {
            continue;
        }
        let back = (1..=i.min(j))
            .take_while(|&step| placeholder_equal(i - step, j - step))
            .count();
        let forward = (0..(a.len() - i).min(b.len() - j))
            .take_while(|&step| placeholder_equal(i + step, j + step))
            .count();
        let (run_a, run_b, length) = (i - back, j - back, back + forward);
        covered = Some((diagonal(&seed), run_a + length));
        let (a_run, b_run) = (&a[run_a..run_a + length], &b[run_b..run_b + length]);
        // Spelling mismatches before each offset, so an interval's exactness is one subtraction.
        let mut mismatches = vec![0usize; length + 1];
        for k in 0..length {
            mismatches[k + 1] = mismatches[k] + usize::from(a_run[k].text != b_run[k].text);
        }
        let breaks = data_breaks(a, b, (run_a, run_b), length, data_stretch_length);
        for (start, end) in consistent_intervals(a_run, b_run, &breaks) {
            if end - start >= min_length {
                runs.push(SharedRun {
                    a_start: run_a + start,
                    b_start: run_b + start,
                    length: end - start,
                    exact: mismatches[end] == mismatches[start],
                });
            }
        }
    }
    drop_contained(runs)
}

/// `runs` without every run lying within another in both sequences, longest first, then by start
/// in the first sequence, then in the second; a run listed more than once is kept once.
///
/// The sweep visits runs by start in the first sequence, longer first at one start, so every run
/// that can contain the current one is visited before it. The kept runs are indexed by their start
/// in the second sequence, in a Fenwick tree whose nodes each keep the ends, in both sequences, that
/// no other end in the node dominates. Testing a run and keeping it each cost O(log² n).
fn drop_contained(mut runs: Vec<SharedRun>) -> Vec<SharedRun> {
    let mut b_starts: Vec<usize> = runs.iter().map(|run| run.b_start).collect();
    b_starts.sort_unstable();
    b_starts.dedup();
    runs.sort_by_key(|run| (run.a_start, std::cmp::Reverse(run.length), run.b_start));
    // Node `i` (from 1) covers the `i & -i` ranks of second-sequence starts ending at rank `i`.
    let mut nodes: Vec<Staircase> = vec![Staircase::default(); b_starts.len() + 1];
    let mut kept: Vec<SharedRun> = Vec::new();
    for run in runs {
        let rank = b_starts.partition_point(|&start| start < run.b_start) + 1;
        let ends = (run.a_start + run.length, run.b_start + run.length);
        // Every kept run starts no later in the first sequence; the prefix over ranks adds the
        // same in the second, and the staircase the two ends.
        let mut node = rank;
        let mut within = false;
        while node > 0 && !within {
            within = nodes[node].dominates(ends);
            node &= node - 1;
        }
        if within {
            continue;
        }
        let mut node = rank;
        while node < nodes.len() {
            nodes[node].insert(ends);
            node += node & node.wrapping_neg();
        }
        kept.push(run);
    }
    kept.sort_by_key(|run| (std::cmp::Reverse(run.length), run.a_start, run.b_start));
    kept
}

/// Points none of which another dominates, keyed by their first coordinate: as it rises, the
/// second falls.
#[derive(Debug, Clone, Default)]
struct Staircase(BTreeMap<usize, usize>);

impl Staircase {
    /// Whether some point is at least `(x, y)` in both coordinates.
    fn dominates(&self, (x, y): (usize, usize)) -> bool {
        self.0.range(x..).next().is_some_and(|(_, &top)| top >= y)
    }

    /// Add `(x, y)`, dropping the points it dominates; a dominated point adds nothing.
    fn insert(&mut self, (x, y): (usize, usize)) {
        if self.dominates((x, y)) {
            return;
        }
        let dominated: Vec<usize> = self
            .0
            .range(..=x)
            .rev()
            .take_while(|&(_, &top)| top <= y)
            .map(|(&key, _)| key)
            .collect();
        for key in dominated {
            self.0.remove(&key);
        }
        self.0.insert(x, y);
    }
}

/// Which tokens of the placeholder-equal run of `length` tokens, starting at `run_a` in `a` and
/// `run_b` in `b`, are breaks: tokens of a data stretch spelled differently in the two sequences.
///
/// A data stretch is a maximal stretch of positions on the run's diagonal at which both sequences
/// hold data tokens, at least `data_stretch_length` tokens long, measured over its whole length: a
/// stretch that continues past either end of the run counts its tokens there too.
fn data_breaks(
    a: &[SpelledToken],
    b: &[SpelledToken],
    (run_a, run_b): (usize, usize),
    length: usize,
    data_stretch_length: usize,
) -> Vec<bool> {
    let both_data = |i: usize, j: usize| a[i].data && b[j].data;
    // The run is placeholder-equal, so within it the two sequences hold data tokens at the same
    // positions, and `a` alone locates the stretches below.
    // Reaching `data_stretch_length` outside the run is enough to decide every stretch at an end.
    let before = (1..=run_a.min(run_b))
        .take_while(|&step| both_data(run_a - step, run_b - step))
        .take(data_stretch_length)
        .count();
    let (end_a, end_b) = (run_a + length, run_b + length);
    let after = (0..(a.len() - end_a).min(b.len() - end_b))
        .take_while(|&step| both_data(end_a + step, end_b + step))
        .take(data_stretch_length)
        .count();
    let mut breaks = vec![false; length];
    let mut start = 0;
    for at in 0..=length {
        if at < length && a[run_a + at].data {
            continue;
        }
        let outside = if start == 0 { before } else { 0 } + if at == length { after } else { 0 };
        if at - start + outside >= data_stretch_length {
            for offset in start..at {
                breaks[offset] = a[run_a + offset].text != b[run_b + offset].text;
            }
        }
        start = at + 1;
    }
    breaks
}

/// The maximal non-empty `[start, end)` intervals over which the placeholder-equal runs `a` and `b`
/// (of equal length) are consistent, in order of start: no token of the interval is marked in
/// `breaks`, and the identifiers and the literals are identical under a consistent one-to-one
/// substitution, one per token class. Every interval that meets both conditions lies within a
/// returned one.
fn consistent_intervals(a: &[SpelledToken], b: &[SpelledToken], breaks: &[bool]) -> Vec<(usize, usize)> {
    // Both conditions hold for every sub-interval of an interval that meets them, so the smallest
    // valid start for each end never decreases, and one scan advances it. A break ends the window,
    // and the next one starts after it. Linear in the run length: each position enters and leaves
    // the window once.
    let slot = |class: TokenClass| match class {
        TokenClass::Identifier => Some(0),
        TokenClass::Literal => Some(1),
        TokenClass::Other | TokenClass::Boundary => None,
    };
    let mut maps: [Bijection; 2] = Default::default();
    let mut intervals = Vec::new();
    let mut left = 0;
    for right in 0..a.len() {
        if breaks[right] {
            if right > left {
                intervals.push((left, right));
            }
            maps = Default::default();
            left = right + 1;
            continue;
        }
        let Some(at) = slot(a[right].class) else {
            continue;
        };
        let (x, y) = (a[right].text.as_str(), b[right].text.as_str());
        if maps[at].conflicts(x, y) {
            intervals.push((left, right));
            while maps[at].conflicts(x, y) {
                if let Some(leaving) = slot(a[left].class) {
                    maps[leaving].remove(&a[left].text, &b[left].text);
                }
                left += 1;
            }
        }
        maps[at].insert(x, y);
    }
    if a.len() > left {
        intervals.push((left, a.len()));
    }
    intervals
}

/// A one-to-one pairing of texts within a window, each pair counted by its occurrences so it leaves
/// with its last one.
#[derive(Default)]
struct Bijection<'t> {
    forward: HashMap<&'t str, (&'t str, usize)>,
    backward: HashMap<&'t str, (&'t str, usize)>,
}

impl<'t> Bijection<'t> {
    /// Whether pairing `x` with `y` contradicts a pair already in the window, in either direction.
    fn conflicts(&self, x: &str, y: &str) -> bool {
        self.forward.get(x).is_some_and(|&(partner, _)| partner != y)
            || self.backward.get(y).is_some_and(|&(partner, _)| partner != x)
    }

    fn insert(&mut self, x: &'t str, y: &'t str) {
        self.forward.entry(x).or_insert((y, 0)).1 += 1;
        self.backward.entry(y).or_insert((x, 0)).1 += 1;
    }

    fn remove(&mut self, x: &str, y: &str) {
        for (map, key) in [(&mut self.forward, x), (&mut self.backward, y)] {
            let count = &mut map.get_mut(key).expect("a pair in the window is counted").1;
            *count -= 1;
            if *count == 0 {
                map.remove(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::syntax::{Language, SyntaxTree};
    use super::*;

    /// The clone keys of the sole function declared in `source`.
    fn keys_of(source: &str) -> CloneKeys {
        function_keys(source, Language::Rust, "function_item")
    }

    /// The clone keys of the sole function declared in the Python `source`.
    fn python_keys_of(source: &str) -> CloneKeys {
        function_keys(source, Language::Python, "function_definition")
    }

    fn function_keys(source: &str, language: Language, function_kind: &str) -> CloneKeys {
        let tree = SyntaxTree::parse(source, language).expect("fixture parses");
        let decl = tree
            .all_declarations()
            .into_iter()
            .find(|d| d.node_kind == function_kind)
            .expect("fixture declares a function");
        clone_keys(&tree.spelled_tokens(decl.full_span)).expect("a function spells tokens")
    }

    /// Sources differing only in whitespace and comments share the formatting-insensitive key.
    #[test]
    fn formatting_variants_share_the_formatting_key() {
        let a = keys_of("fn add(x: u32, y: u32) -> u32 { x + y }");
        let b = keys_of("fn add(\n    x: u32, // the left addend\n    y: u32\n) -> u32 {\n    x + y\n}");
        assert_eq!(a.formatting_key, b.formatting_key);
        assert_eq!(a.substitution_key, b.substitution_key);
    }

    /// A consistently renamed copy shares only the substitution-insensitive key.
    #[test]
    fn renamed_copy_shares_only_the_substitution_key() {
        let a = keys_of("fn add(x: u32, y: u32) -> u32 { let total = x + y; total }");
        let b = keys_of("fn plus(a: u32, b: u32) -> u32 { let sum = a + b; sum }");
        assert_ne!(a.formatting_key, b.formatting_key);
        assert_eq!(a.substitution_key, b.substitution_key);
    }

    /// A copy differing only in its literal values shares only the substitution-insensitive key.
    #[test]
    fn literal_substituted_copy_shares_only_the_substitution_key() {
        let a = keys_of("fn retries() -> u32 { 3 + 3 }");
        let b = keys_of("fn retries() -> u32 { 5 + 5 }");
        assert_ne!(a.formatting_key, b.formatting_key);
        assert_eq!(a.substitution_key, b.substitution_key);
    }

    /// Merging two distinct identifiers into one changes the ordinal pattern: an inconsistent
    /// substitution shares neither key.
    #[test]
    fn merged_identifiers_share_neither_key() {
        let a = keys_of("fn f(x: u32, y: u32) -> u32 { x + y }");
        let b = keys_of("fn f(x: u32, x2: u32) -> u32 { x + x }");
        assert_ne!(a.formatting_key, b.formatting_key);
        assert_ne!(a.substitution_key, b.substitution_key);
    }

    /// Structurally different code shares neither key.
    #[test]
    fn unrelated_code_shares_neither_key() {
        let a = keys_of("fn f(x: u32) -> u32 { x * 2 }");
        let b = keys_of("fn g(items: Vec<u32>) -> usize { items.len() }");
        assert_ne!(a.formatting_key, b.formatting_key);
        assert_ne!(a.substitution_key, b.substitution_key);
    }

    /// Comments are not tokens: a doc-commented copy spells the same sequence.
    #[test]
    fn comments_never_enter_the_sequence() {
        let a = keys_of("fn f() -> u32 { 1 }");
        let b = keys_of("/// Documented.\nfn f() -> u32 {\n    // inner note\n    1\n}");
        assert_eq!(a.formatting_key, b.formatting_key);
    }

    /// Rust doc comments with different text spell the same sequence.
    #[test]
    fn rust_doc_comment_text_is_disregarded() {
        let a = keys_of("/// Adds one.\nfn f(x: u32) -> u32 { x + 1 }");
        let b = keys_of("/// Increments the input.\nfn f(x: u32) -> u32 { x + 1 }");
        assert_eq!(a.formatting_key, b.formatting_key);
    }

    /// Python docstrings with different text spell the same sequence.
    #[test]
    fn python_docstring_text_is_disregarded() {
        let a = python_keys_of("def f(x):\n    \"\"\"Add one.\"\"\"\n    return x + 1\n");
        let b = python_keys_of("def f(x):\n    \"\"\"Increment the input.\"\"\"\n    return x + 1\n");
        assert_eq!(a.formatting_key, b.formatting_key);
    }

    /// A Python docstring present in only one copy is disregarded.
    #[test]
    fn python_docstring_in_one_copy_is_disregarded() {
        let a = python_keys_of("def f(x):\n    return x + 1\n");
        let b = python_keys_of("def f(x):\n    \"\"\"Add one.\"\"\"\n    return x + 1\n");
        assert_eq!(a.formatting_key, b.formatting_key);
    }

    /// A renamed Python test with a docstring in one copy only shares only the substitution key.
    #[test]
    fn renamed_python_copy_with_one_docstring_shares_only_the_substitution_key() {
        let a = python_keys_of("def test_add():\n    total = add(1, 2)\n    assert total == 3\n");
        let b = python_keys_of(
            "def test_plus():\n    \"\"\"Plus sums two numbers.\"\"\"\n    result = plus(1, 2)\n    assert result == 3\n",
        );
        assert_ne!(a.formatting_key, b.formatting_key);
        assert_eq!(a.substitution_key, b.substitution_key);
    }

    /// A leading formatted, bytes, or template string, alone or in a concatenation, is not a
    /// docstring: it stays in the sequence.
    #[test]
    fn python_leading_non_docstring_string_stays_in_the_sequence() {
        let plain = python_keys_of("def f():\n    return 1\n");
        for leading in ["f\"{effect()}\"", "b\"raw\"", "t\"{x}\"", "\"doc \" f\"{effect()}\""] {
            let keys = python_keys_of(&format!("def f():\n    {leading}\n    return 1\n"));
            assert_ne!(keys.formatting_key, plain.formatting_key, "{leading}");
        }
    }

    /// A raw or unicode-prefixed leading string is a docstring.
    #[test]
    fn python_prefixed_docstring_is_disregarded() {
        let plain = python_keys_of("def f():\n    return 1\n");
        for leading in ["r\"\"\"Doc.\"\"\"", "u\"Doc.\"", "R\"a\" \"b\""] {
            let keys = python_keys_of(&format!("def f():\n    {leading}\n    return 1\n"));
            assert_eq!(keys.formatting_key, plain.formatting_key, "{leading}");
        }
    }

    /// A bare string statement after the first statement is not a docstring: it stays a literal.
    #[test]
    fn python_string_after_the_first_statement_stays_a_literal() {
        let a = python_keys_of("def f(x):\n    y = x + 1\n    \"first note\"\n    return y\n");
        let b = python_keys_of("def f(x):\n    y = x + 1\n    \"second note\"\n    return y\n");
        assert_ne!(a.formatting_key, b.formatting_key);
        assert_eq!(a.substitution_key, b.substitution_key);
    }

    /// A string opening a nested block is not a docstring: it stays a literal.
    #[test]
    fn python_string_opening_a_nested_block_stays_a_literal() {
        let a = python_keys_of("def f(x):\n    if x:\n        \"first note\"\n    return x\n");
        let b = python_keys_of("def f(x):\n    if x:\n        \"second note\"\n    return x\n");
        assert_ne!(a.formatting_key, b.formatting_key);
    }

    const K: usize = FRAGMENT_KGRAM_LENGTH;
    const W: usize = FRAGMENT_WINDOW;

    /// `count` distinct tokens of `class` whose texts begin with `tag`, none of them data.
    fn tokens(class: TokenClass, tag: &str, count: usize) -> Vec<SpelledToken> {
        (0..count)
            .map(|i| SpelledToken {
                class,
                text: format!("{tag}{i}"),
                start: i,
                data: false,
            })
            .collect()
    }

    /// Two sequences sharing a run of exactly the guaranteed length share a fingerprint at the
    /// same offset from the run's start in both.
    #[test]
    fn a_shared_run_of_the_guaranteed_length_shares_a_fingerprint_on_one_diagonal() {
        let run = tokens(TokenClass::Other, "run", guaranteed_fragment_length(K, W));
        let a = [
            tokens(TokenClass::Other, "a", 7),
            run.clone(),
            tokens(TokenClass::Other, "after_a", 5),
        ]
        .concat();
        let b = [
            tokens(TokenClass::Other, "b", 13),
            run,
            tokens(TokenClass::Other, "after_b", 30),
        ]
        .concat();
        let from_a = winnow(&a, K, W);
        let from_b = winnow(&b, K, W);
        let shared_on_diagonal = from_a.iter().any(|&(hash_a, ordinal_a)| {
            from_b
                .iter()
                .any(|&(hash_b, ordinal_b)| hash_a == hash_b && ordinal_a as i64 - 7 == ordinal_b as i64 - 13)
        });
        assert!(shared_on_diagonal);
    }

    /// Identifier and literal text does not reach the fingerprints: a renamed copy fingerprints alike.
    #[test]
    fn identifier_and_literal_text_does_not_reach_the_fingerprints() {
        let length = guaranteed_fragment_length(K, W);
        for class in [TokenClass::Identifier, TokenClass::Literal] {
            assert_eq!(
                winnow(&tokens(class, "x", length), K, W),
                winnow(&tokens(class, "renamed", length), K, W)
            );
        }
    }

    #[test]
    fn repeated_calls_give_identical_fingerprints() {
        let input = tokens(TokenClass::Other, "t", 200);
        let first = winnow(&input, K, W);
        assert!(!first.is_empty());
        assert_eq!(first, winnow(&input, K, W));
    }

    #[test]
    fn a_sequence_shorter_than_the_guaranteed_length_yields_none() {
        let input = tokens(TokenClass::Other, "t", guaranteed_fragment_length(K, W) - 1);
        assert!(winnow(&input, K, W).is_empty());
    }

    /// Space-separated tokens: lowercase words are identifiers, digit strings literals, and
    /// anything else structure. Literals and the characters `, ; : ( ) [ ] { }` are data.
    fn spelled(text: &str) -> Vec<SpelledToken> {
        text.split(' ')
            .enumerate()
            .map(|(start, word)| {
                let class = if word.chars().all(|c| c.is_ascii_lowercase()) {
                    TokenClass::Identifier
                } else if word.chars().all(|c| c.is_ascii_digit()) {
                    TokenClass::Literal
                } else {
                    TokenClass::Other
                };
                let punctuation = word.chars().all(|c| ",;:()[]{}".contains(c));
                SpelledToken {
                    class,
                    text: word.to_string(),
                    start,
                    data: class == TokenClass::Literal || (class == TokenClass::Other && punctuation),
                }
            })
            .collect()
    }

    #[test]
    fn a_consistently_renamed_run_is_one_interval() {
        let a = spelled("x = y + 1 ; x = x * 1");
        let b = spelled("p = q + 2 ; p = p * 2");
        assert_eq!(scan(&a, &b, K), vec![(0, a.len())]);
    }

    /// Two identifiers of one run merged into one name in the other: the scan splits at each
    /// conflict, and every interval is maximal.
    #[test]
    fn a_merged_identifier_splits_the_run_at_the_conflict() {
        let a = spelled("x + y ; x");
        let b = spelled("p + p ; p");
        assert_eq!(scan(&a, &b, K), vec![(0, 2), (1, 4), (3, 5)]);
    }

    #[test]
    fn an_identical_run_is_exact() {
        let a = spelled("{ x = y + 1 ; }");
        assert_eq!(
            shared_runs(&a, &a, &[(2, 2)], 3, K),
            vec![SharedRun {
                a_start: 0,
                b_start: 0,
                length: a.len(),
                exact: true
            }]
        );
    }

    /// With nothing contained the comparable tokens are the spelled tokens; overlapping contained
    /// spans become one boundary at the merged span's start, and no token inside it remains.
    #[test]
    fn contained_spans_merge_into_one_boundary() {
        let source = "fn f() { a(); fn g() {} b(); fn h() {} c(); }";
        let tree = SyntaxTree::parse(source, Language::Rust).expect("fixture parses");
        let whole = ByteSpan {
            start: 0,
            end: source.len(),
        };
        assert_eq!(comparable_tokens(&tree, whole, &[]), tree.spelled_tokens(whole));

        let at = |needle: &str| source.find(needle).unwrap();
        let (g, h) = (CanonicalId::from_raw("g"), CanonicalId::from_raw("h"));
        let g_span = ByteSpan {
            start: at("fn g"),
            end: at(" b()"),
        };
        let overlapping_h = ByteSpan {
            start: at("{} b()") + 1,
            end: at(" c()"),
        };
        let texts: Vec<(TokenClass, String)> = comparable_tokens(&tree, whole, &[(overlapping_h, &h), (g_span, &g)])
            .into_iter()
            .map(|token| (token.class, token.text))
            .collect();
        let spelled: Vec<String> = "fn f ( ) { a ( ) ; | c ( ) ; }"
            .split(' ')
            .map(str::to_string)
            .collect();
        let expected: Vec<(TokenClass, String)> = spelled
            .into_iter()
            .map(|text| match text.as_str() {
                "|" => (TokenClass::Boundary, "g".to_string()),
                "f" | "a" | "c" => (TokenClass::Identifier, text),
                _ => (TokenClass::Other, text),
            })
            .collect();
        assert_eq!(texts, expected);
    }

    /// A boundary never matches, even another boundary of the same symbol: the run stops at it.
    #[test]
    fn a_run_never_includes_a_boundary() {
        let mut a = spelled("x = y + 1 ; | ; z = 2");
        a[6].class = TokenClass::Boundary;
        assert_eq!(
            shared_runs(&a, &a, &[(0, 0), (8, 8)], 3, K),
            vec![
                SharedRun {
                    a_start: 0,
                    b_start: 0,
                    length: 6,
                    exact: true
                },
                SharedRun {
                    a_start: 7,
                    b_start: 7,
                    length: 4,
                    exact: true
                },
            ]
        );
    }

    /// Tokens alike in class and text but differing in data role are not counterparts: no run spans them.
    #[test]
    fn no_run_spans_tokens_of_differing_data_role() {
        let a = spelled("{ x = y + 1 ; z = w + 2 ; }");
        let mut b = a.clone();
        let flipped = 4;
        assert!(!b[flipped].data);
        b[flipped].data = true;
        let runs = shared_runs(&a, &b, &[(0, 0), (a.len() - 1, a.len() - 1)], 3, K);
        assert!(!runs.is_empty());
        assert!(
            runs.iter()
                .all(|run| flipped < run.a_start || run.a_start + run.length <= flipped),
            "{runs:?}"
        );
    }

    #[test]
    fn a_renamed_run_is_variant() {
        let a = spelled("{ x = y + 1 ; }");
        let b = spelled("( ( { p = q + 2 ; }");
        assert_eq!(
            shared_runs(&a, &b, &[(1, 3)], 3, K),
            vec![SharedRun {
                a_start: 0,
                b_start: 2,
                length: a.len(),
                exact: false
            }]
        );
    }

    /// A table of `count` distinct data literals whose texts begin with `tag`, separated by commas.
    fn table(tag: &str, count: usize) -> Vec<SpelledToken> {
        (0..count)
            .flat_map(|i| {
                [
                    SpelledToken {
                        class: TokenClass::Literal,
                        text: format!("\"{tag}{i}\""),
                        start: 2 * i,
                        data: true,
                    },
                    SpelledToken {
                        class: TokenClass::Other,
                        text: ",".to_string(),
                        start: 2 * i + 1,
                        data: true,
                    },
                ]
            })
            .collect()
    }

    /// A k-gram made only of data tokens hashes its text: different tables share no fingerprint,
    /// and a copied table fingerprints alike.
    #[test]
    fn an_all_data_kgram_hashes_its_text() {
        let length = guaranteed_fragment_length(K, W);
        let hashes = |tokens: &[SpelledToken]| -> std::collections::HashSet<i64> {
            winnow(tokens, K, W).into_iter().map(|(hash, _)| hash).collect()
        };
        let first = hashes(&table("a", length));
        assert!(first.is_disjoint(&hashes(&table("b", length))));
        assert_eq!(first, hashes(&table("a", length)));
    }

    /// The consistent intervals of the whole of `a` and `b`, under data-stretch length `data_stretch_length`.
    fn scan(a: &[SpelledToken], b: &[SpelledToken], data_stretch_length: usize) -> Vec<(usize, usize)> {
        consistent_intervals(a, b, &data_breaks(a, b, (0, 0), a.len(), data_stretch_length))
    }

    /// A differing token of a data stretch is a break no interval includes, however little of the
    /// stretch the interval holds; a stretch shorter than the length is substitutable throughout.
    #[test]
    fn a_differing_token_of_a_data_stretch_is_a_break() {
        // The stretch `1 , 2 , 3 , 9` spans ordinals 4 to 10 and differs at ordinal 10.
        let a = spelled("p + q + 1 , 2 , 3 , 9 + r");
        let b = spelled("p + q + 1 , 2 , 3 , 8 + r");
        assert_eq!(scan(&a, &b, 4), vec![(0, 10), (11, 13)]);
        // The stretch `9 , 2 , 3 , 4` spans ordinals 2 to 8 and differs at ordinal 2.
        let a = spelled("p + 9 , 2 , 3 , 4 + q");
        let b = spelled("p + 8 , 2 , 3 , 4 + q");
        assert_eq!(scan(&a, &b, 4), vec![(0, 2), (3, 11)]);
        assert_eq!(scan(&a, &b, 8), vec![(0, a.len())]);
    }

    /// A stretch is measured over its whole length, past the end of the placeholder-equal run: the
    /// run `x + 1 , 2` holds three tokens of the stretch `1 , 2 ; 3`, which both sequences continue
    /// with data tokens spelled differently.
    #[test]
    fn a_data_stretch_is_measured_past_the_run() {
        let a = spelled("x + 1 , 2 ; 3");
        let b = spelled("x + 9 , 2 , 3");
        let breaks = |data_stretch_length| data_breaks(&a, &b, (0, 0), 5, data_stretch_length);
        assert_eq!(breaks(5), [false, false, true, false, false]);
        assert_eq!(breaks(6), [false; 5]);
    }

    /// A deterministic xorshift sequence for the generated cases.
    struct Generator(u64);

    impl Generator {
        fn below(&mut self, bound: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % bound as u64) as usize
        }
    }

    /// Whether `[start, end)` of the sequences `a` and `b`, which hold data tokens at the same
    /// positions, meets both conditions of [`shared_runs`], checked directly: consistent
    /// substitution within the interval, and no differing token of a maximal stretch of data
    /// tokens, measured over the whole sequences, at least `data_stretch_length` long.
    fn valid_interval(
        a: &[SpelledToken],
        b: &[SpelledToken],
        start: usize,
        end: usize,
        data_stretch_length: usize,
    ) -> bool {
        let mut forward: HashMap<(bool, &str), &str> = HashMap::new();
        let mut backward: HashMap<(bool, &str), &str> = HashMap::new();
        for at in start..end {
            if a[at].class == TokenClass::Other {
                continue;
            }
            let (x, y) = (a[at].text.as_str(), b[at].text.as_str());
            if *forward.entry((a[at].class == TokenClass::Literal, x)).or_insert(y) != y
                || *backward.entry((a[at].class == TokenClass::Literal, y)).or_insert(x) != x
            {
                return false;
            }
        }
        let stretch_length = |at: usize| {
            let left = (0..at).rev().take_while(|&i| a[i].data).count();
            let right = (at..a.len()).take_while(|&i| a[i].data).count();
            left + right
        };
        (start..end).all(|at| !(a[at].data && a[at].text != b[at].text && stretch_length(at) >= data_stretch_length))
    }

    /// The scan of any part of two sequences returns exactly the maximal non-empty intervals of that
    /// part that meet both conditions, so every interval that meets them lies within a returned one.
    #[test]
    fn the_scan_returns_every_maximal_valid_interval() {
        let mut generator = Generator(0x9e37_79b9_7f4a_7c15);
        for _ in 0..3000 {
            let length = 1 + generator.below(14);
            let data_stretch_length = 1 + generator.below(4);
            let (mut a, mut b) = (Vec::new(), Vec::new());
            for start in 0..length {
                let (class, data) = match generator.below(4) {
                    0 => (TokenClass::Identifier, false),
                    1 => (TokenClass::Literal, true),
                    2 => (TokenClass::Other, true),
                    _ => (TokenClass::Other, false),
                };
                let (x, y) = match class {
                    TokenClass::Other => {
                        let text = if data { "," } else { "+" }.to_string();
                        (text.clone(), text)
                    }
                    _ => (generator.below(3).to_string(), generator.below(3).to_string()),
                };
                a.push(SpelledToken {
                    class,
                    text: x,
                    start,
                    data,
                });
                b.push(SpelledToken {
                    class,
                    text: y,
                    start,
                    data,
                });
            }
            let run_start = generator.below(length);
            let run_end = run_start + 1 + generator.below(length - run_start);
            let valid = |s: usize, e: usize| valid_interval(&a, &b, s, e, data_stretch_length);
            let extensible =
                |s: usize, e: usize| (s > run_start && valid(s - 1, e)) || (e < run_end && valid(s, e + 1));
            let expected: Vec<(usize, usize)> = (run_start..run_end)
                .flat_map(|s| ((s + 1)..=run_end).map(move |e| (s, e)))
                .filter(|&(s, e)| valid(s, e) && !extensible(s, e))
                .map(|(s, e)| (s - run_start, e - run_start))
                .collect();
            let breaks = data_breaks(&a, &b, (run_start, run_start), run_end - run_start, data_stretch_length);
            assert_eq!(
                consistent_intervals(&a[run_start..run_end], &b[run_start..run_end], &breaks),
                expected,
                "{a:?} {b:?} {data_stretch_length} {run_start}..{run_end}"
            );
        }
    }

    /// The all-pairs removal of contained runs, which the sweep must reproduce.
    fn drop_contained_all_pairs(mut runs: Vec<SharedRun>) -> Vec<SharedRun> {
        runs.sort_by_key(|run| (std::cmp::Reverse(run.length), run.a_start, run.b_start));
        let within = |inner: &SharedRun, outer: &SharedRun| {
            outer.a_start <= inner.a_start
                && inner.a_start + inner.length <= outer.a_start + outer.length
                && outer.b_start <= inner.b_start
                && inner.b_start + inner.length <= outer.b_start + outer.length
        };
        let mut kept: Vec<SharedRun> = Vec::new();
        for run in runs {
            if !kept.iter().any(|outer| within(&run, outer)) {
                kept.push(run);
            }
        }
        kept
    }

    #[test]
    fn the_sweep_removes_the_runs_the_all_pairs_removal_removes() {
        let mut generator = Generator(0x2545_f491_4f6c_dd1d);
        for _ in 0..2000 {
            let count = generator.below(40);
            let span = 1 + generator.below(40);
            let runs: Vec<SharedRun> = (0..count)
                .map(|_| {
                    let a_start = generator.below(span);
                    let b_start = generator.below(span);
                    let length = 1 + generator.below(span);
                    SharedRun {
                        a_start,
                        b_start,
                        length,
                        exact: a_start == b_start,
                    }
                })
                .collect();
            assert_eq!(
                drop_contained(runs.clone()),
                drop_contained_all_pairs(runs.clone()),
                "{runs:?}"
            );
        }
    }

    /// The data role of each spelled token of `source`, as `(text, data)`.
    fn data_roles(source: &str, language: Language) -> Vec<(String, bool)> {
        let tree = SyntaxTree::parse(source, language).expect("fixture parses");
        let whole = ByteSpan {
            start: 0,
            end: source.len(),
        };
        tree.spelled_tokens(whole)
            .into_iter()
            .map(|token| (token.text, token.data))
            .collect()
    }

    /// The text of each spelled token of `source` that is data, in order.
    fn data_texts(source: &str, language: Language) -> Vec<String> {
        data_roles(source, language)
            .into_iter()
            .filter_map(|(text, data)| data.then_some(text))
            .collect()
    }

    /// Rust classifies a struct expression's type name, its field names, and its shorthand fields
    /// as data; a field access, a base expression, and an operator are not.
    #[test]
    fn rust_record_structure_is_data() {
        assert_eq!(
            data_texts("fn f() { g(Point { x: 1, y, ..base }.x + 2) }", Language::Rust),
            [
                "(", ")", "{", "(", "Point", "{", "x", ":", "1", ",", "y", ",", "}", "2", ")", "}"
            ]
        );
    }

    /// The signs of `source` that are data, in order, as `(sign, the token after it)`.
    fn data_signs(source: &str, language: Language) -> Vec<(String, String)> {
        let roles = data_roles(source, language);
        roles
            .windows(2)
            .filter(|pair| matches!(pair[0].0.as_str(), "-" | "+") && pair[0].1)
            .map(|pair| (pair[0].0.clone(), pair[1].0.clone()))
            .collect()
    }

    /// A sign directly before a numeric literal is data, in an expression and in a Rust pattern; a
    /// binary minus and a sign before a name are not.
    #[test]
    fn a_sign_before_a_number_is_data() {
        let rust = "fn f(v: i32) -> i32 { g(-1, v - 2, -v, -2.5); match v { -3 => 0, _ => 1 } }";
        let expected = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|&(sign, next)| (sign.to_string(), next.to_string()))
                .collect()
        };
        assert_eq!(
            data_signs(rust, Language::Rust),
            expected(&[("-", "1"), ("-", "2.5"), ("-", "3")])
        );
        let python = "f(-1, v - 2, -v, +2.5)\n";
        assert_eq!(
            data_signs(python, Language::Python),
            expected(&[("-", "1"), ("+", "2.5")])
        );
    }

    /// Python classifies no record structure: a dictionary is data through its literals and
    /// punctuation, and a keyword argument's name is code.
    #[test]
    fn python_record_structure_is_literals_and_punctuation() {
        assert_eq!(
            data_texts("f({\"a\": 1}, b=2)\n", Language::Python),
            ["(", "{", "\"a\"", ":", "1", "}", ",", "2", ")"]
        );
    }
}
