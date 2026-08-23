# Tasks: semantic-chunking

## Vocabulary alignment (code-graph, code-navigation)

The three-level vocabulary — document, passage, chunk — is settled before anything is built on it, so no new code is written in the retired terms.
"Corpus" keeps its collection sense throughout: `semantic_corpus`, `CORPUS_DEFINITION_VERSION`, `SourceCorpus`, `PreparedCorpus`, and the adjective "corpus-contributing" are deliberately untouched.

- [x] Rename the corpus's unit from corpus entry to passage across `src/`: `CorpusEntry`, `insert_corpus_entry`, and the entry-sense locals in `src/graph/corpus.rs`, together with the doc-comments that name them.
- [x] Rename the corpus's unit from corpus entry to passage across `tests/`, including test names and the requirement-quoting comments that cite scenario titles.
- [x] Rename `DEPENDENTS_FRONTIER_CHUNK` to `DEPENDENTS_FRONTIER_BATCH` and update its references and doc-comments, so "chunk" carries exactly one meaning inside c10r.
- [x] Add a schema comment at the vector table recording that `sqlite-vec`'s `_chunks` shadow tables hold its internal vector blocks, unrelated to a chunk of text.
- [x] Sweep `README.md` and every doc-comment that describes the semantic index for the retired terms, and state the three levels once where the semantic index is introduced.
- [x] Record the settled vocabulary where it is discoverable: a glossary beside the north star, naming the terms of art the specs use and what each denotes.
- [x] Confine `tier` to the persisted content levels across the specs and the code: the query-time projection axis is a detail level, a reference's addressing shape is a form, and clone ordering is a certainty class.
- [x] Name the tier set as the three the store persists — signature, interface, body — in the requirement that defines it, the schema comment, and the glossary.

## Embedding fidelity (code-graph)

- [x] Disable the tokenizer's padding configuration where the compiled-in model is constructed, beside the existing truncation removal, and record in the doc-comment why a maskless static model must not receive pad ids.
- [x] Write a test for the Vector independence requirement: a chunk embedded alongside other chunks yields a vector bit-identical to the same chunk embedded alone.
- [x] Write the size-asymmetry partition test: a short chunk embedded in a batch holding a chunk orders of magnitude longer yields the vector it carries when embedded alone.
- [x] Write the order partition test: a size-asymmetric group embedded in the opposite order yields every chunk the same vector, pinning the third dependence axis the requirement names.
- [x] Extend the existing tail-sensitivity test so it asserts the no-token-limit property at the passage level — content past any single chunk still affects the passage's representation set — rather than at the single-vector level.

## Chunk splitting (code-graph)

- [x] Add a splitter module taking a passage's content, the chunk size, the header, and the overlap, and returning ordered chunks; content fitting beside the header returns exactly one chunk covering it whole.
- [x] Add a syntax accessor that returns the direct child spans of the node covering a given span, so the splitter can enumerate units without parsing; no existing accessor exposes them.
- [x] Implement level 1: divide content into syntax-node units read from the build's prepared per-document tree, descending into any node that alone exceeds the room the header leaves, and pack units into chunks up to the chunk size.
- [x] Implement level 2: inside a unit the tree reports as indivisible and too large to fit, divide on prose boundaries — paragraph, then sentence, then line.
- [x] Implement level 3: divide content offering no boundary within the room available by plain token window, as the terminal fallback.
- [x] Implement overlap: each chunk after the first carries whole trailing units of its predecessor up to the overlap budget, a trailing unit that does not fit contributing nothing; within a windowed run, plain token overlap.
- [x] Implement the header budget: bound the header to at most half the chunk size, trimming its documentation tail only, never the name words, kind, module-path words, or signature.
- [x] Write the parse-budget test for the splitter: splitting every passage of a symbol-dense document performs no parse beyond the document's own, asserted through the existing parse counter.
- [x] Write tests for the Bounded chunk scope requirement, one per partition: content fitting beside the header is one chunk; content exceeding the size is several whose union is the whole content; no chunk exceeds the size with its header included; chunks do not begin or end part-way through a declaration; prose chunks do not begin or end part-way through a sentence; boundary-less content is still bounded and wholly covered.
- [x] Write the overlap partition tests: with overlap none, no content appears in two chunks; with overlap set, each chunk after the first begins with whole trailing units of its predecessor.
- [x] Write the header-budget tests: an oversized header is trimmed to the budget with its identity-bearing head intact, and its passage's chunks still carry content.

## Store schema (code-graph)

- [x] Bump the schema version constant and reshape the vector table to hold one row per chunk, each carrying its passage's identity and its ordinal within that passage.
- [x] Extend the recorded semantic-index identity with the chunk size and the overlap the build ran under.
- [x] Write a store test: chunks of one passage round-trip with their ordinals, and the corpus table still holds exactly one row per symbol.
- [x] Write a test for the Semantic-index identity requirement: the recorded identity carries model identity, corpus definition version, chunk size, and overlap; two stores built under different parameters carry different identities.

## Build integration (code-graph)

- [x] Bump the corpus definition version, since the text a vector derives from changes.
- [x] Wire the splitter into the build: each passage's content becomes chunks, each embedded whole, each persisted as its own vector row; the lexical row stays one per passage over the whole render.
- [x] Thread the parameters in effect through the build and record them with the store.
- [x] Make carry-forward parameter-aware: a passage's prior vectors are reused only when its render **and** the recorded parameters are unchanged.
- [x] Make the build's currency check parameter-aware: a store whose recorded chunk parameters differ from those in effect is not current, so the build runs instead of reporting the index already current.
- [x] Write the currency-check partition test: a rebuild under a changed chunk size analyzes and rebuilds, while a rebuild under unchanged parameters still reports the index already current.
- [x] Write a test for the Chunk parameters requirement through the ordinary build path: a build with explicit parameters records them, and its vectors derive from them.
- [x] Write the carry-forward write-site test: rebuilding unchanged sources under a changed chunk size re-derives every vector and retains none from the prior build.
- [x] Write the carry-forward no-op test: rebuilding unchanged sources under unchanged parameters yields representations identical to the prior build's, with the skip active.
- [x] Write tests for the modified Semantic representations requirement: every passage carries at least one vector and exactly one lexical representation; a split passage's chunks each carry the passage's header; vanished symbols leave no representation; an edited symbol re-derives; a failed build leaves the prior build's representations authoritative.

## Build parameters on the command surface (command-surface)

- [x] Add `--chunk-size` and `--chunk-overlap` as canonical flags on `build`, each defaulting to its recommended value, with `--chunk-size` bounding the whole embedded chunk.
- [x] Validate the pair at the boundary: reject an overlap not smaller than the chunk size, and a chunk size too small to admit content beside a passage's header, as usage errors before any build begins.
- [x] Bump the surface version, regenerate the surface manifest snapshot, and raise the MCP server's recorded surface version to match, so the two packages still agree at startup.
- [x] Disclose the recorded chunk parameters in `status` alongside the rest of the semantic-index identity.
- [x] Write tests for the Chunk parameters on build requirement, one per scenario: defaults applied and recorded; supplied values applied and recorded; no chunk exceeds the supplied size; an overlap not smaller than the size refused; a size leaving no room refused.

## Query (code-navigation)

- [x] Request the dense candidate pool in chunks and deduplicate to symbols by best chunk before fusion, so a symbol enters the fused ranking once at its best score.
- [x] Implement `similar` scoring as the best pair over the subject's chunks and each candidate's, with the subject's own chunks excluded from its answer.
- [x] Write tests for the modified Search requirement: content late in a long symbol is findable by words drawn from it; a symbol whose several chunks all match appears exactly once; existing scenarios still hold.
- [x] Write tests for the modified Similar requirement: a multi-chunk subject excludes only itself; two symbols coinciding in one region only rank as similar.

## Verification and experiments

- [x] Rebuild all five dogfood workspaces at the defaults; record build wall time, peak memory, passage count, and chunk count per workspace.
- [x] Re-derive the split rate under the shipped rule and record it, since the header now counts against the chunk size; correct the proposal's stated share if it moved.
- [x] Re-run the 14 recorded dogfood queries from `archive/2026-08-14-semantic-investigation/notes.md` and tally each against its recorded outcome, attributing every movement to the padding fix or to splitting.
- [x] Build the boundary-sensitivity probe: for every passage that splits at the default chunk size, generate a query from distinctive content spanning a chunk boundary, and record the owning symbol's rank.
- [x] Run the probe at overlap 0, 64, and 128 across all five workspaces and report the rank distribution per setting against the pre-registered expectation in `design.md`.
- [x] Record the probe's verdict and, if it moves the default, the parameter change and the reason, in the change's dogfood notes.
- [x] Draft an upstream report for `model2vec-rs`: pooling counts pad ids, and the loader honours a `BatchLongest` tokenizer configuration, with the reproduction and the measured effect.
