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

#[cfg(test)]
mod tests {
    use super::super::syntax::{Language, SyntaxTree};
    use super::*;

    /// The clone keys of the sole function declared in `source`.
    fn keys_of(source: &str) -> CloneKeys {
        let tree = SyntaxTree::parse(source, Language::Rust).expect("fixture parses");
        let decl = tree
            .all_declarations()
            .into_iter()
            .find(|d| d.node_kind == "function_item")
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
}
