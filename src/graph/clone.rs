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

use sha2::{Digest, Sha256};

use super::syntax::{SpelledToken, TokenClass};

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
pub const FRAGMENT_KGRAM_LENGTH: usize = 20;

/// K-grams in each winnowing window.
pub const FRAGMENT_WINDOW: usize = 21;

/// The shortest shared token run that is guaranteed to share a fingerprint under k-gram length `k`
/// and window `w`.
pub const fn guaranteed_fragment_length(k: usize, w: usize) -> usize {
    w + k - 1
}

/// Winnow a token sequence into fingerprints: `(hash, token_ordinal)` pairs, where `token_ordinal`
/// is the selected k-gram's start index in `tokens`.
///
/// Every identifier is one placeholder and every literal another, so a consistently or
/// inconsistently renamed copy fingerprints alike. Any run of at least
/// [`guaranteed_fragment_length`]`(k, w)` tokens shared by two sequences yields a shared
/// fingerprint at the same offset within the run in both. A sequence shorter than that length
/// yields none.
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

/// Hash one k-gram of placeholder tokens: the first 8 bytes of SHA-256 over each token's class tag
/// and length-prefixed text, read as a signed integer. Identifiers and literals contribute their
/// class tag only, so no source text of either reaches the hash.
fn kgram_hash(kgram: &[SpelledToken]) -> i64 {
    let mut hasher = Sha256::new();
    for token in kgram {
        let (tag, text) = match token.class {
            TokenClass::Identifier => (0u8, ""),
            TokenClass::Literal => (1u8, ""),
            TokenClass::Other => (2u8, token.text.as_str()),
        };
        hasher.update([tag]);
        hasher.update((text.len() as u64).to_le_bytes());
        hasher.update(text.as_bytes());
    }
    let digest = hasher.finalize();
    i64::from_le_bytes(digest[..8].try_into().expect("a SHA-256 digest holds 32 bytes"))
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

    /// `count` distinct tokens of `class` whose texts begin with `tag`.
    fn tokens(class: TokenClass, tag: &str, count: usize) -> Vec<SpelledToken> {
        (0..count)
            .map(|i| SpelledToken {
                class,
                text: format!("{tag}{i}"),
                start: i,
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
}
