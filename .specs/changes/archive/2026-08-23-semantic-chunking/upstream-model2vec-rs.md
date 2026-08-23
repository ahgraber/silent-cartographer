# Draft upstream report: model2vec-rs — batch padding contaminates pooled vectors

For the user to file against <https://github.com/MinishLab/model2vec-rs> (observed at v0.2.1).

## Summary

`StaticModel` honours whatever padding configuration the loaded `tokenizer.json` serializes.
When that configuration is `BatchLongest` — which the exported tokenizer of `minishlab/potion-code-16M-v2` carries — every text in an `encode_batch` call is padded to the longest text in its batch, and the pad ids flow into mean pooling: `pool_ids` filters only the `[UNK]` id, never the pad id.
A static model has no attention mask to exclude pad positions, so a batched vector becomes a function of the longest text that happened to share its batch.

## Reproduction

```rust
use model2vec_rs::model::StaticModel;

let model = StaticModel::from_pretrained("minishlab/potion-code-16M-v2", None, None, None)?;
let short = "fn parse_config(text: &str) -> Config".to_string();
let long = "word ".repeat(200_000); // any much longer neighbor
let batched = model.encode_with_args(&[short.clone(), long], None, 1024);
let alone = model.encode_with_args(&[short], None, 1024);
// batched[0] != alone[0]: the short text's vector is dominated by pad rows.
```

## Measured effect

Observed on a corpus of 5,794 code texts (the Faker Python library, whole-batch encode):

- 2,656 of 5,794 batched vectors sit below cosine 0.5 against the same text embedded alone; the worst pair is cosine −0.17.
  A 201-byte text sharing a batch with a 770 KB one is, after pooling, mostly the pad token.
- The `[PAD]` embedding row is not neutral: norm 0.0072 against a median row norm of 8.96, so enough pad rows drag the mean toward it while shrinking every real component's share.
- Padding to the batch maximum also allocates a rectangle: peak heap in the encode step was 22,395 MiB; with padding disabled it is 122 MiB, and wall time falls 89.1 s → 1.1 s.
- With padding disabled, batched vectors are bit-identical to singly-embedded ones (0 of 5,794 differ), so batching itself needs no padding: a static model pools a ragged batch.

## Workaround

Strip the padding (and truncation) configuration from the tokenizer bytes before constructing the model:

```rust
let mut tokenizer: serde_json::Value = serde_json::from_slice(&tokenizer_bytes)?;
tokenizer["padding"] = serde_json::Value::Null;
let model = StaticModel::from_bytes(serde_json::to_vec(&tokenizer)?, model_bytes, config_bytes, None)?;
```

## Suggested fix

Either disable padding when constructing the tokenizer (a static model never needs a rectangular batch), or filter the pad id in `pool_ids` the way the `[UNK]` id is already filtered.
The first also removes the batch-maximum memory rectangle; the second alone leaves the allocation cost in place.
