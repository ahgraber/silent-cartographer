//! The bundled static embedding model behind the semantic corpus.
//!
//! The model is `minishlab/potion-code-16M-v2` (MIT), vendored at upstream revision
//! `e9d2a44ca6a05ac6685f3b23709ea57eb7352d5b` under `models/potion-code-16M-v2/` and compiled into
//! the binary, so embedding needs no filesystem or network access anywhere the binary runs.
//! A static model embeds by token-vector lookup plus averaging: deterministic for a given model,
//! which is what makes carrying an unchanged entry's vector across builds unobservable.

use std::sync::OnceLock;

use model2vec_rs::model::StaticModel;

/// The embedding model identity recorded in store provenance: the upstream repository at the
/// vendored revision. Any change to this identity ships with a schema-version bump, so a store
/// embedded under a different model refuses wholesale before any vector is compared.
pub const MODEL_ID: &str = "minishlab/potion-code-16M-v2@e9d2a44ca6a05ac6685f3b23709ea57eb7352d5b";

/// The dimension of every vector the model produces.
pub const EMBEDDING_DIM: usize = 256;

static MODEL: OnceLock<StaticModel> = OnceLock::new();

/// The process-wide embedding model, parsed once from the compiled-in bytes.
///
/// The bytes are fixed at compile time, so a parse failure is a build defect rather than a runtime
/// condition — hence the expects.
///
/// The exported tokenizer serializes a 512-token truncation setting the model itself does not
/// have: a static model averages token vectors, and the model card's stated maximum length is
/// 1,000,000 tokens. Left in place, the tokenizer would apply it inside every encode and content
/// past token 512 could never affect a vector — silently falsifying the no-token-limit contract —
/// so the truncation config is removed before the tokenizer is constructed. Verified by the
/// tail-sensitivity test below.
fn model() -> &'static StaticModel {
    MODEL.get_or_init(|| {
        let mut tokenizer: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../models/potion-code-16M-v2/tokenizer.json").as_slice())
                .expect("the compiled-in tokenizer must parse");
        tokenizer["truncation"] = serde_json::Value::Null;
        let tokenizer_bytes = serde_json::to_vec(&tokenizer).expect("the truncation-free tokenizer re-serializes");
        StaticModel::from_bytes(
            tokenizer_bytes,
            include_bytes!("../../models/potion-code-16M-v2/model.safetensors").as_slice(),
            include_bytes!("../../models/potion-code-16M-v2/config.json").as_slice(),
            None,
        )
        .expect("the compiled-in embedding model must parse")
    })
}

/// Embed one text into the model's vector space.
///
/// No truncation is applied: a static model averages token vectors, so any input length embeds in
/// one pass and no sub-chunking exists.
pub fn embed(text: &str) -> Vec<f32> {
    model()
        .encode_with_args(std::slice::from_ref(&text.to_string()), None, 1)
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// Embed a batch of texts, one vector per text, in input order. Same no-truncation contract as
/// [`embed`].
pub fn embed_batch(texts: &[String]) -> Vec<Vec<f32>> {
    model().encode_with_args(texts, None, 1024)
}

/// Serialize a vector to the little-endian `f32` bytes persisted in the store — the byte layout
/// sqlite-vec's `vec0` table stores and matches against.
pub fn vector_bytes(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model loads from compiled-in bytes — no filesystem or network access — and embedding is
    /// deterministic: the same text embeds to bit-identical vectors across calls.
    #[test]
    fn embedding_is_deterministic_and_self_contained() {
        let a = embed("parse a configuration file and validate its fields");
        let b = embed("parse a configuration file and validate its fields");
        assert_eq!(a.len(), EMBEDDING_DIM);
        let a_bits: Vec<u32> = a.iter().map(|v| v.to_bits()).collect();
        let b_bits: Vec<u32> = b.iter().map(|v| v.to_bits()).collect();
        assert_eq!(a_bits, b_bits);
        // A non-degenerate embedding: some component is non-zero.
        assert!(a.iter().any(|v| *v != 0.0));
    }

    /// Content past token 512 affects the vector: two long texts sharing their first ~600 words
    /// and differing only in the tail embed differently. Regression for the exported tokenizer's
    /// serialized 512-token truncation, which is disabled at load.
    #[test]
    fn embedding_is_sensitive_to_content_past_512_tokens() {
        let head: String = (0..600).map(|i| format!("word{i} ")).collect();
        let a = embed(&format!("{head} zebra quantum waterfall"));
        let b = embed(&format!("{head} igloo cathedral spark"));
        let identical = a.iter().zip(b.iter()).all(|(x, y)| x.to_bits() == y.to_bits());
        assert!(!identical, "the tail past token 512 must affect the embedding");
    }
}
