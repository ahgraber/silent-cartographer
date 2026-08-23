# Discussion: evidence behind semantic-chunking

Captured 2026-08-16.
This is the measurement and research record behind `proposal.md`.
It is a research note, not a contract — nothing here is a spec.

The probes that produced the local numbers are parked in `._scratch/internal-efficiency-probes/` (a counting global allocator with phase markers, an ingest driver that replays a pre-produced SCIP index, a vector comparator over two stores, and the Python scripts below).

## How the defect surfaced

A `c10r build --language python` over a Faker checkout (748 source files, 8.9 MB) reported a 27.8 GB peak memory footprint.
Instrumenting ingest with a counting allocator attributed it to a single phase:

```text
ingest: corpus assembled      live=102.8 MiB   peak=103.0 MiB   at 100.4s
ingest: embeddings done       live=179.9 MiB   peak=22576.0 MiB at 194.3s
```

Every graph phase — join, identity projection, symbol rows, occurrences, edges, discrepancies — holds under ~110 MiB.
The peak is one call to the embedding step.

## Why the embedding step allocates that much

The vendored `tokenizer.json` (upstream revision `e9d2a44…`, SHA-pinned by the manifest and verified by `build.rs`, so it is upstream's file verbatim) declares:

```json
"truncation": {"max_length": 512, ...},
"padding": {"strategy": "BatchLongest", "pad_id": 0, "pad_token": "[PAD]", ...}
```

`tokenizers::Tokenizer::encode_batch_fast` applies `pad_encodings` whenever the tokenizer carries a padding config, so a batch is padded to its longest member.
`embed.rs` removes the truncation setting deliberately (the no-token-limit contract) but leaves padding, and hands the model batches of 1,024 entries.
Faker's largest passage is ~250k tokens, so its batch materializes 1,024 sequences of that length.

## Why padding also corrupts the vectors

Padding exists to make a batch into a rectangular tensor for a neural forward pass — the HF documentation states it directly: *"Batched inputs are often different lengths, so they can't be converted to fixed-size tensors.
Padding and truncation are strategies for dealing with this problem, to create rectangular tensors from batches of varying lengths."*
The API's default is `do_not_pad`.
Transformer stacks pair padding with an attention mask, and sentence-transformers' mean pooling divides by the mask sum, so pad positions contribute nothing.

A static model has no forward pass, no tensor, and no mask.
`model2vec`'s pooling averages the embedding rows of every id the tokenizer returns and filters only the `[UNK]` id — in both the Rust crate (0.2.1) and the Python reference (0.9.0, `Tokenizer.from_file` with no padding override).
The `[PAD]` row is not neutral: its norm is 0.0072 against a median vocabulary row norm of 8.96, so each pad adds a small vector in a fixed direction, and enough of them dominate the mean.

Measured in the reference Python implementation at its defaults, batching a short text with a 512-token one:

| tokens | cosine(alone, batched) |
| ------ | ---------------------- |
| 1      | 0.9116                 |
| 6      | 0.9312                 |
| 11     | 0.9653                 |
| 22     | 0.9912                 |

So the defect exists upstream, bounded by the 512 truncation the reference stack keeps.
Passing `max_length=None` to the Python API does **not** disable the tokenizer's own truncation; c10r is the only consumer running this model untruncated, which is what turns a 1–9% distortion into a total one.

Comparing the Faker store built with padding against the same build with padding disabled, over the 5,794 entries whose renders are identical:

| cosine      | entries |
| ----------- | ------- |
| > 0.9999    | 50      |
| 0.99–0.9999 | 2,724   |
| 0.9–0.99    | 92      |
| 0.5–0.9     | 272     |
| ≤ 0.5       | 2,656   |

Worst cases are 201-byte method renders at cosine −0.17.

Disabling padding at load (one line beside the truncation removal) measured:

|                                | peak         | wall   |
| ------------------------------ | ------------ | ------ |
| whole-corpus call, padding on  | 22,395.6 MiB | 89.1 s |
| whole-corpus call, padding off | 122.1 MiB    | 1.1 s  |
| one text per call              | 107.5 MiB    | 1.6 s  |

Batching is not what padding was buying: unpadded batching is both the smallest and the fastest of the three, because a static model pools a ragged batch and the pads were pure overhead.
Batched vectors become bit-identical to singly-embedded ones (0 of 5,794 differ).
The full test suite passes with the change (677 tests).

## Corpus size distribution

Over all five dogfood workspaces (httpx2, flask, ripgrep, fd, faker), 13,444 passages, tokenized with the vendored tokenizer with truncation and padding disabled:

```text
                  p50    p75    p90    p95    p99      max
render tokens      79    153    289    450   1823   332,199
prefix tokens      29     40     78    124    277     7,011
content tokens     45    103    216    345   1493   332,175
```

Characters per token run 3.13 (faker) to 4.00 (flask).
Entries over 512 tokens: 567 of 13,444 (4.2%), holding 1,740,855 of 3,160,199 corpus tokens (55%).

By symbol kind on Faker, the oversized entries are data-carrying leaves, not whole files: 162 of 1,245 `type` entries exceed 2 KB (largest 175 KB — a provider class holding a literal job list and no methods), 72 of 3,736 `method` entries (largest 6.8 KB), and only 3 of 813 `module` entries — because a module with persisted children already contributes its interface tier, so only childless data modules reach the corpus as bodies.

Projected vector counts, slicing each entry's content into `budget − prefix` sized pieces:

| budget | per-symbol cap | vectors | vs today | worst single symbol      |
| ------ | -------------- | ------- | -------- | ------------------------ |
| 256    | none           | 23,186  | 1.7x     | 1,432 chunks             |
| 256    | 32             | 19,594  | 1.5x     | 32 (759k tokens dropped) |
| 512    | none           | 17,104  | 1.3x     | 681 chunks               |
| 512    | 32             | 16,016  | 1.2x     | 32 (516k tokens dropped) |
| 1024   | none           | 14,937  | 1.1x     | 333 chunks               |

## Published evidence on chunk length

**The reference consumer.**
Semble is the same lab's code-search library built on this exact model.
It splits each file into code-aware chunks by recursively merging tree-sitter nodes toward `_DESIRED_CHUNK_LENGTH_CHARS = 750` (`_MIN_CHUNK_SIZE = 50`), embeds one vector per chunk, fuses with BM25 by RRF, and applies a file-coherence boost when several chunks of one file match.
Its published ablations vary the retrieval legs (BM25 0.675 raw / 0.834 ranked; potion-code-16M 0.650 / 0.821; fused 0.854), **not** chunk size — no chunk-length experiment from Minish was found.

**Chunk length against retrieval quality.**
Chroma's chunking evaluation ran its experiment with a strong model and repeated it with a weak one.

With `text-embedding-3-large`:

| Chunking  | Size | Overlap | Recall | Precision | Precision_Ω |
| --------- | ---- | ------- | ------ | --------- | ----------- |
| Recursive | 800  | 400     | 85.4   | 1.5       | 6.7         |
| Recursive | 400  | 200     | 88.1   | 3.3       | 13.9        |
| Recursive | 400  | 0       | 89.5   | 3.6       | 17.7        |
| Recursive | 200  | 0       | 88.1   | 7.0       | 29.9        |

With `all-MiniLM-L6-v2`:

| Chunking  | Size | Overlap | Recall | Precision | Precision_Ω |
| --------- | ---- | ------- | ------ | --------- | ----------- |
| TokenText | 250  | 125     | 82.4   | 3.6       | 11.4        |
| TokenText | 250  | 0       | 77.1   | 3.3       | 16.4        |
| Recursive | 250  | 0       | 78.5   | 5.4       | 26.7        |
| Recursive | 200  | 0       | 75.7   | 6.5       | 31.2        |

Two findings carry over: shrinking chunks roughly doubles precision at near-flat recall, and overlap behaves differently by model strength — it bought nothing on the strong model but moved weak-model recall from 77.1 to 82.4, which the authors summarize as _"for smaller context, overlapping chunks are necessary for high recall."_
They frame size as a dilution ceiling: _"recall could reach a maximum before relevant information is diluted within chunks … while chunks which are too small fail to capture necessary context within a single unit."_

**Why a bound exists at all.**
For transformers the limit is architectural — sentence-transformers states runtime and memory _"grows quadratic with the input length"_ — and that reason does not apply here: the model card records "Max
sequence length | 1,000,000 tokens (static, no limit in practice)".
The reason that does apply is distributional: potion-code-16M-v2 is distilled from `nomic-ai/CodeRankEmbed` and contrastively fine-tuned on CornStack (query, document) pairs, which are function-scale documents.

## What remains unmeasured

No retrieval-quality measurement exists in this project, so the choice between bounded chunks and the cheaper interface-tier fallback for over-bound symbols rests on architectural reasoning and on evidence from prose corpora with a different model.
Both published tables above are natural-language corpora with LLM-generated questions, not code, and neither uses a static model.

## Sources

- Hugging Face Transformers, "Padding and truncation" — <https://huggingface.co/docs/transformers/main/en/pad_truncation>
- Sentence Transformers, "Computing Embeddings" § Input Sequence Length — <https://sbert.net/examples/sentence_transformer/applications/computing-embeddings/README.html>
- Chroma, "Evaluating Chunking Strategies for Retrieval" — <https://research.trychroma.com/evaluating-chunking>
- Pinecone, "Chunking Strategies for LLM Applications" — <https://www.pinecone.io/learn/chunking-strategies/>
- `minishlab/potion-code-16M-v2` model card — <https://huggingface.co/minishlab/potion-code-16M-v2>
- Semble — <https://github.com/MinishLab/semble> (`src/semble/chunking/`, `benchmarks/README.md`)
- model2vec (Python 0.9.0) and model2vec-rs (0.2.1) pooling and loader paths
- `tokenizers` 0.21.4, `Tokenizer::encode_batch_fast`
