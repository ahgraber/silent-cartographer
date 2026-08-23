# Proposal: semantic-chunking

## Intent

A stored vector must represent the text it claims to represent.
Two defects break that today, and both were found by measurement, not review.

**A vector depends on what else was in its batch.**
The vendored tokenizer declares `BatchLongest` padding.
The embedding model is static — it averages the embedding rows of every token id it is handed and has no attention mask to exclude pad ids, because there is no forward pass for a mask to apply to.
So every text in a batch is padded out to the longest text in that batch, and those pad rows are averaged into the mean.
On the Faker workspace, 2,656 of 5,794 stored vectors sit below cosine 0.5 against the same text embedded alone, and the worst sit at cosine −0.17: a 201-byte method render sharing a batch with a 770 KB one is, in the store, mostly the pad token.

**A vector's scope is unbounded.**
One passage is one vector no matter how much text the passage holds.
Across the five dogfood workspaces, 4.2% of passages hold 55% of all corpus text; the largest is 332,199 tokens.
The embedding model is distilled from a code retriever fine-tuned on function-scale (query, document) pairs, so a mean over a six-figure token count is far outside the distribution it was trained for.
Such a vector is not merely imprecise — it carries no discriminative signal, while still occupying a rank position that a precise vector could hold.

The two compound: unbounded scope is what lets batch padding reach six figures, which is why a Faker build peaks at 22.4 GiB of heap inside one embedding call.
Fixing the padding alone yields cheap vectors that still cannot discriminate; bounding scope alone leaves rankings that depend on batch composition.

## User Stories

### Story: honest-vector

As an agent ranking `search` results, I want every stored vector to be a function of the text it represents and nothing else, so that a ranking never reflects an artifact of how the corpus happened to be batched.

> Ladders to the principle **Calibration over coverage** — "the product dies the moment it lies with confidence."
> A ranking derived from batch-contaminated vectors is confidently wrong with no signal that anything is off.
> Supports north-star outcome 6 (**Find by intent**).

### Story: discriminating-long-symbol

As a developer searching for code that lives inside a large symbol, I want that symbol's content represented at a granularity the model can discriminate, so that a match on one part of a long body surfaces the symbol instead of being averaged into noise.

> Ladders to north-star outcome 6 (**Find by intent**) and the principle **Precision is the moat; recall is additive** — semantic search is built on top of the precise graph, so its own ranking must be precise enough to be worth consulting.

### Story: nothing-unrepresented

As a developer, I want no part of a symbol's content to be silently absent from the semantic index, so that a symbol missing from an answer means it does not match, never that its content fell past a cap.

> Ladders to the principle **Safety by structure, not by caveat** — "absence is typed, never implied."
> This is the story that rules out truncation as the way to bound a vector's scope.

### Story: build-completes-on-a-laptop

As a developer indexing a wide workspace, I want build memory to stay proportional to the workspace rather than to its single largest symbol, so that the build finishes at all.

> Ladders indirectly: no north-star outcome is reachable from a build that exhausts memory, but build resource use is not itself an outcome the north star measures.
> Recorded honestly as a consequence of the other three stories rather than as independent justification — the bound and the padding fix deliver it without any work aimed at it.

## Scope

**In scope:**

- Pad tokens never contribute to a vector: a chunk's vector is a function of its own text alone, identical whether embedded alone or alongside any other chunks (code-graph).
  Serves: honest-vector.
- A bounded scope per vector: a passage whose content exceeds the chunk size is represented by several chunks, each within the size, together covering all of it (code-graph).
  Serves: discriminating-long-symbol, nothing-unrepresented.
- Each chunk carries the passage's contextualizing header — name words, kind, module-path words, signature, own documentation — so a chunk drawn from the middle of a long body still carries what the symbol is (code-graph).
  Serves: discriminating-long-symbol.
- The parameters in effect are recorded with the build and disclosed as part of the semantic-index identity; a build whose parameters differ from the recorded ones is not reported already current, and carries no vector forward from the prior build (code-graph).
  Serves: honest-vector.
- The chunk size and the overlap between adjacent chunks are build-time parameters with recommended defaults, so the retrieval-quality tradeoff can be measured rather than argued (command-surface).
  Serves: discriminating-long-symbol.
- `search` and `similar` score a symbol by its best-matching chunk, and a symbol still appears at most once per answer (code-navigation).
  Serves: discriminating-long-symbol, honest-vector.
- A dogfood experiment over the recorded query set at overlap 0, 64, and 128, reported with the change (code-navigation).
  Serves: discriminating-long-symbol.
- The corpus's per-symbol unit is renamed from **corpus entry** to **passage**, settling a three-level vocabulary — document, passage, chunk — across the specs, the code, and the documentation (code-graph, code-navigation).
  Serves: honest-vector, discriminating-long-symbol.
- Two vocabulary corrections the settlement surfaced, recorded as riding it rather than as independently justified: `tier` is confined to the persisted content levels and stops naming the query-time detail axis or a reference's addressing form, and the tier set is named as the three it has always been — signature, interface, and body — rather than two (code-graph, code-navigation).
  Serves no story; the glossary cannot record a term the requirement defining it misstates.
- Schema version bump and wholesale rebuild: every stored vector changes.
  Surface version bump: `build` gains the two parameters.

**Out of scope:**

- Choosing the overlap default by argument.
  The knob ships; the proposal's working default was no overlap, and moving it was delegated to the experiment's result — which moved it to 64 (`design.md`, `notes.md`).
- Late interaction / per-token vectors.
  This change's per-chunk vectors are the substrate a later late-interaction change would need; it builds none of it.
- Any change to the lexical (BM25) leg.
  It indexes the whole render per symbol and length-normalizes by design; literal data tables are content it serves well.
- Any change to corpus membership or to the render's composition — which symbols contribute, and what their render says, are unchanged.
- Any change to the answer shape of `search` or `similar`, their bounding, cursors, markers, or provenance fields.
- The redundant per-symbol re-parsing in ingest — that is build latency and stays in the `internal-efficiency` change.

## Approach

The vocabulary is settled first, because the level this change adds had no name of its own.
A **document** is a source file.
A **passage** is the corpus's unit: exactly one per corpus-contributing symbol, holding the render that symbol's content and identity produce.
A **chunk** is the text handed to the embedding model — a bounded run of a passage's content, carrying that passage's header.
That is the information-retrieval convention, and it retires "corpus entry", which competed with the unrelated source-document sense of "corpus" already in the code.

The corpus stays symbol-keyed: one passage per corpus-contributing symbol, and its render is what the lexical index reads, whole and unchanged.
Only the dense side becomes plural — a passage yields one or more chunks, one vector each.
The graph's node structure is untouched; a symbol is still one node with one identity.

Mechanism choices are for `design.md`; the ones already settled by measurement are recorded here so the design starts from evidence:

- **The default chunk size is 512 tokens**, bounding the whole chunk — header and content together — so the supplied number is the number the model receives.
  Across the dogfood corpora the median passage is 79 tokens and p90 is 289, so the size only engages the tail: 559 of 13,444 passages under the shipped rule, measured after the build landed. 512 leaves headroom for the header (median 29 tokens, p90 78) and for overlap.
  The published chunking evidence favours smaller chunks for precision at roughly flat recall, and favours overlap specifically for weak models — ours is a 16M static model.
- **The default overlap is none, and the change reports an experiment against that default.**
  Overlap's published benefit is repair of severed spans, which a boundary-respecting splitter should already prevent; whether it buys anything here is therefore an empirical question, not a derivable one.
  The knob exists so the question is answerable, and the change is not complete until it has been asked at 0, 64, and 128 over the recorded dogfood queries.
- **Projected cost is affordable across the knob's useful range.**
  At the default the corpus grows from 13,444 to 16,944 vectors (1.26x, measured after the build landed; the projection said 17,104); at overlap 64 the projection is 1.34x, at 128 it is 1.45x, at 256 it is 1.63x.
  Those projections already charge the header against the chunk size, so they describe the shipped rule rather than an approximation of it.
  No per-symbol chunk cap is planned: a cap discards content, which story `nothing-unrepresented` forbids, and the projection shows chunk count is not the pressure.
- **Splitting is boundary-respecting, over both code and prose.**
  Content is divided into units and units are packed up to the room the header leaves: syntax nodes from the tree the build already holds, descending into a node too large to fit; prose boundaries (paragraph, then sentence, then line) inside a prose unit such as a docstring, a doc-comment run, or an interface tier, which a syntax tree exposes as one indivisible node.
  Overlap is taken as whole trailing units, never a partial one, up to the overlap budget.
- **A unit that alone exceeds the room available is windowed.**
  Neither tree nor prose separators can divide a minified line or a single enormous literal, so the terminal fallback is a plain token window, and overlap within such a windowed run is plain token overlap — the only kind available where there are no units.
  Overlap applies to content only; the header repeats on every chunk by construction.
- **Padding is disabled at tokenizer load**, beside the truncation removal already there.
  Measured on the Faker corpus: peak heap in the embedding step falls from 22,395 MiB to 122 MiB, wall time from 89.1 s to 1.1 s, and batched vectors become bit-identical to singly-embedded ones (0 of 5,794 differ).
  Batching is retained — a static model pools a ragged batch, so equal lengths were never required.

At the default chunk size roughly 96% of passages yield exactly one chunk, so their vectors are unchanged apart from the padding fix.
Any movement in the dogfood queries is therefore attributable to the tail rather than to a corpus-wide shift — a property the experiment depends on.

Open mechanism, for `design.md`: how a header that is itself near the chunk size is handled (p99 is 277 tokens, the worst 7,011); and how `similar` compares two symbols whose passages each carry several chunks.

## Open Questions

1. **The retrieval-quality gate is a tripwire, not a metric.**
   The 14 recorded dogfood queries in `archive/2026-08-14-semantic-investigation/notes.md` — six leading queries kept as the regression floor, eight non-leading ones tallied HIT/PARTIAL/MISS — are what this change measures against, before and after, at each overlap setting.
   They can catch a regression and attribute a movement; they cannot compute a retrieval metric or tune a parameter, because they are known-target ranks rather than a labeled corpus.
   Anything stronger belongs to `eval-localization`, which the parameters exist to let it baseline without re-embedding first.

2. **What is a fair comparison for the dropped alternative?**
   The cheaper design — an oversized symbol contributes its interface tier and its body stays lexical-only — is not built here, on the reasoning that it discards representable content and extends to nothing.
   The dogfood set could compare them if that reasoning is ever doubted; it is recorded as the road not taken rather than as an open decision.

3. **How far does a header budget go before it distorts the passage?**
   Bounding an oversized header means trimming its documentation, which is retrieval evidence the render deliberately front-loads.
   The trimming rule is a design decision, but where it stops being harmless is not something the dogfood set can answer at 4% incidence.
