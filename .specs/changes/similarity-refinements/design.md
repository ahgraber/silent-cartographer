# Design: similarity-refinements

## Context

`similar` ranks candidates by reciprocal rank fusion (Cormack, Clarke, and Büttcher, SIGIR 2009) of two signals that read the persisted passage render: a sqlite-vec KNN over the chunk embeddings and an FTS5 BM25 query over the subject's render words (`src/query/search.rs`, `similar_of` and `rrf_fuse`).
The vector signal requests every chunk, clamped to `KNN_MAX_K` (4096), so each subject costs one exhaustive scan of the vector table.
After fusion, `similar_of` fetches the symbol row and clone keys of every fused candidate, sorts the rows into clone-certainty classes, and the shared paging code cuts the page.
Answers expose order only; no distance, BM25, or fusion number reaches an answer.

The `graph-aware-similarity` change (archived 2026-08-14) put dependency-neighborhood overlap into the fusion and was reverted.
Fusion reads ranks, not scores, so a weak graph overlap still earned a near-top rank, and a symbol's own members and its crate root flooded the answer.
This change keeps all graph evidence out of `rrf_fuse`.

The clone keys (`src/graph/clone.rs`) hash a leaf's spelled token sequence (`SyntaxTree::spelled_tokens`, `src/graph/syntax.rs`).
The token walk skips comment nodes.
Rust doc comments are comment nodes, so they are skipped.
A Python docstring is a `string` node, so it stays in the sequence as a literal.
`python_leading_docstring` already identifies a docstring for the interface tier.

The store holds `contains`, `uses`, `imports`, and `type_hierarchy` edges, a per-symbol test rule, and the symbol `class` (`in_workspace` or `external`).
`type_hierarchy` edges join classes and types only (`src/graph/mod.rs`); no edge joins a method to the method it overrides.
The c10r build reads no SCIP relationships.
On 2026-10-03, scip-python 0.6.6 was checked to emit `is_implementation` relationships from a subclass method to the base method for explicit subclasses, explicit `Protocol` subclasses, and ABC subclasses, and none for structural `Protocol` satisfaction.

Chunk ranges are not persisted (`src/graph/schema.rs`, `semantic_vectors`).
At the default chunk size of 512 tokens about 96% of passages are one chunk, so the existing vectors cannot locate a span inside a symbol.

The store holds each module's whole document text as the module's body tier, so the indexed text of every document is available at query time without reading the workspace.

`SCHEMA_VERSION` on `main` is 16.
The Python kind and module display-name fix in the working tree sets 17, and `stale-files-and-line-positions` shares 17 with that fix.
`SURFACE_VERSION` on `main` is 11; `stale-files-and-line-positions` takes 12, and `ty-scip-backend` takes one above `main` when its branch is cut.

The audit corpus held about 2,700 corpus symbols.
The dogfood stores under `~/.cache/silent-cartographer` hold between 426 (fd) and 5,794 (Faker) passages.
The largest planned dogfood repository is sympy; its passage count is not yet measured and is expected to be in the tens of thousands.

## Decisions

### Decision: DocumentationLeavesTheTokenSequence

**Chosen:** `spelled_tokens` skips a Python leading docstring node of any function, class, or module body inside the span, found with `python_leading_docstring`.
The keys and the fragment evidence both read this sequence.
Rust is unchanged.
A `#[doc = "..."]` attribute stays in the Rust sequence, because the interface tier does not read it as documentation either.

**Rationale:** The interface tier already defines what counts as a symbol's documentation, so one definition serves both features, and Rust and Python get the same treatment.
A string statement after the first statement is code that a reader may depend on, so it stays.

**Alternatives considered:**

- Drop every string statement that is not an assignment: also removes strings that are not documentation, such as a bare string used as a block comment, which Python does not treat as a docstring.
- Treat docstrings as literals with a fixed value: a docstring present in one copy and absent in the other would still change the sequence.

### Decision: FragmentDetection

**Chosen:** Winnowing (Schleimer, Wilkerson, and Aiken, "Winnowing: Local Algorithms for Document Fingerprinting", SIGMOD 2003, the algorithm behind MOSS) over each leaf's token sequence with every identifier replaced by one identifier placeholder and every literal by one literal placeholder.
Candidates found through shared fingerprints are verified against the real token sequences before anything is reported.

Comparison:

| Approach                                          | Source                                                                                                                                        | Precision of a reported span           | Build cost                                      | Storage                                                                      | Query cost                                    | Determinism                     | Grounding              |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------- | ----------------------------------------------- | ---------------------------------------------------------------------------- | --------------------------------------------- | ------------------------------- | ---------------------- |
| Winnowing over normalized tokens, verified        | Schleimer, Wilkerson, Aiken 2003                                                                                                              | Exact after verification               | One pass over tokens per leaf                   | About 2/(w+1) fingerprints per token                                         | Index lookups plus verification of candidates | Deterministic with a fixed hash | Exact token match      |
| Suffix tree or suffix array over the token stream | Baker, "On Finding Duplication and Near-Duplication in Large Software Systems", WCRE 1995; Kamiya, Kusumoto, Inoue, "CCFinder", IEEE TSE 2002 | Exact; finds every maximal repeat      | O(n) to O(n log n) over the whole corpus stream | 4 to 8 bytes per corpus token plus the token stream                          | Fast once built                               | Deterministic                   | Exact token match      |
| AST subtree hashing                               | Baxter, Yahin, Moura, Sant'Anna, Bier, "Clone Detection Using Abstract Syntax Trees", ICSM 1998                                               | Exact for whole subtrees               | One pass over the tree                          | One hash per subtree                                                         | Index lookups                                 | Deterministic                   | Exact subtree match    |
| AST characteristic vectors with LSH               | Jiang, Misherghi, Su, Glondu, "DECKARD", ICSE 2007                                                                                            | Estimated; vectors of node-kind counts | Vector per subtree and per sibling window       | Vector per subtree                                                           | LSH lookups                                   | Needs a seeded hash family      | Estimated region       |
| Sliding-window or late-chunked embeddings         | Günther et al., "Late Chunking: Contextual Chunk Embeddings Using Long-Context Embedding Models", arXiv 2409.04701, 2024                      | Estimated                              | One embedding per window                        | 1 KiB per window at 256 float32 dimensions                                   | KNN per subject window                        | Deterministic with this model   | Estimated window       |
| Token-level late interaction (MaxSim)             | Khattab and Zaharia, "ColBERT", SIGIR 2020                                                                                                    | Estimated token alignment              | One vector per token                            | One vector per token, compressed in ColBERTv2 (Santhanam et al., NAACL 2022) | MaxSim over candidate token matrices          | Deterministic                   | Estimated alignment    |
| Local alignment over tokens                       | Smith and Waterman, "Identification of Common Molecular Subsequences", J. Mol. Biol. 1981                                                     | Exact endpoints, gapped interior       | None                                            | None                                                                         | O(m·n) per pair                               | Deterministic                   | Alignment with a score |

**Rationale:** Winnowing gives a guarantee that matches the contract.
With k-gram length k and window w, any shared run of at least w + k - 1 tokens produces at least one shared fingerprint, because the window that lies inside the run in both leaves selects the same k-gram at the same offset in both.
The query then reads the real tokens, so every reported run is an exact or consistent-substitution match, and every qualifying run of the guaranteed length is found.
The answer is grounded in tokens and carries the deterministic grade, which the calibration principle requires to stay distinct from estimated answers.
Storage is a fraction of the token count, and the build adds one pass over tokens it already spells for the keys.

**Alternatives considered:**

- Suffix array: finds every repeat without a length guarantee gap, but needs the whole corpus token stream at build or query time.
  The store does not hold token streams, and holding one costs several bytes per token plus the array.
  It is the right tool for a corpus-wide fragment report, which is out of scope.
- AST subtree hashing: exact, but a copied run of statements inside a longer method is a sequence of sibling subtrees, not one subtree, and the audit's miss was that case.
- DECKARD: finds near-miss regions, but the report is an estimate from count vectors, and LSH needs a seeded hash family and a distance threshold.
- Window embeddings and late chunking: the model is a static Model2Vec model (`potion-code-16M-v2`), which has no context, so late chunking adds nothing over plain windows.
  Window vectors multiply vector storage and give an estimated span with window-sized boundaries.
- ColBERT MaxSim: needs a contextual encoder and one vector per token, which is too costly on CPU, and the span is still an estimate.
- Smith-Waterman: handles inserted statements, but it needs gap penalties and a score threshold, and it is too slow to generate candidates over a corpus.
  It is the natural verifier for a later near-miss fragment class, applied only to winnowing candidates.

### Decision: FragmentEvidenceStorage

**Chosen:** A new table `clone_fingerprints(hash INTEGER NOT NULL, passage_id INTEGER NOT NULL, token_ordinal INTEGER NOT NULL)` with an index on `hash`, and a new `clone_token_count INTEGER` column on `symbols`, set where the keys are set.
The build writes them in the build transaction, after the keys, for every leaf with keys whose token count is at least the guaranteed length.
Parameters: k = 20 tokens, w = 21 k-grams, so the guaranteed length is w + k - 1 = 40 tokens, about five Python lines.
The k-gram hash is the first 8 bytes of SHA-256 over each placeholder token's class tag and length-prefixed text, read as a signed 64-bit integer.
The class tag keeps a structural token from hashing like an identifier or literal placeholder.
Selection is robust winnowing: the rightmost minimum hash in each window, recorded once per selected position.
The values of k and w are two named constants in one place.
Each build records them in `index_metadata`, in new columns `fragment_kgram_length` and `fragment_window`, as it records the chunk size and overlap.
A `clones` query reads k and w from the store, not from the binary, and reports the guaranteed length they give.
Changing a constant therefore needs no schema version bump: the next build writes fingerprints under the new values, and a store built earlier answers under the values it records.

**Rationale:** A fixed cryptographic hash needs no seed and gives the same fingerprints on every machine.
The 40-token length is a first value that the audit's five-line case gates, so it must stay cheap to tune.
A query never recomputes a fingerprint, so a store that records its own parameters answers correctly under them, and no compatibility check is needed.
Exposing k and w later as build flags, as `--chunk-size` is, adds only the flags.
`passage_id` is an integer, so a fingerprint row stays small.
A leaf shorter than the guaranteed length cannot hold a reportable run, so it needs no fingerprints.
The token count orders the corpus clone groups without reading any source text.

**Alternatives considered:**

- A Karp-Rabin rolling hash (Karp and Rabin, IBM J. Res.
  Dev.
  1987): cheaper per k-gram, but the build already spends far more on analysis, and SHA-256 is already a dependency.
- Store each leaf's token sequence: verification would need no parse, but storage grows by about 20 bytes per token.
- Fingerprint every leaf: adds rows for leaves that can never report a fragment.

### Decision: FragmentVerification

**Chosen:** For a subject A and each other leaf B that shares a fingerprint hash with A and does not share A's substitution-insensitive key:

1. Re-spell the tokens of A and B by parsing their documents' indexed text from the module body tier, once per document per query.
2. Group the shared positions by diagonal (position in A minus position in B).
3. On each diagonal, extend every seed to the maximal run on which the placeholder tokens are equal.
4. Inside each such run, find the maximal intervals that are consistent under a one-to-one substitution, one map per token class, with a two-pointer scan.
5. Keep intervals of at least the guaranteed length; mark an interval exact when the spelled tokens are equal throughout, and variant otherwise.

Rows order whole-symbol clones first (exact, then variant, then canonical identity), then fragments by run length descending, then the other symbol's canonical identity, then the subject run's start.
The answer is computed whole and then paged.

**Rationale:** Consistency under substitution holds for every sub-interval of a consistent interval, so the two-pointer scan finds every maximal consistent interval in linear time, and every qualifying run lies inside one.
Re-parsing the indexed text, not the workspace file, keeps the answer consistent with the index when the workspace has changed, as every answer is.
A hash collision only adds a seed that step 3 discards.

### Decision: RelationLabels

**Chosen:** After `similar` has ordered its rows, each row gets labels from store lookups: `uses_subject` and `used_by_subject` from the `uses` edges in each direction, `tests_subject` from the row's test rule plus `uses_subject`, and `type_family` from the `type_hierarchy` edges of the two declaring types.
A declaring type is the symbol itself when it is a type, else its `contains` parent when that parent is a type.
The common supertype must have `class = 'in_workspace'`.
Two members of the same type get no `type_family` label.

**Rationale:** Each label is a lookup on persisted, resolved edges, so it is a fact and needs no heuristic grade, except `tests_subject`, which carries the convention rule as the `tests` relation does.
Labels are computed after ordering and never reach `rrf_fuse`, so the ranking cannot change.
The in-workspace rule stops `object`, `Protocol`, `ABC`, and standard-library traits from joining unrelated types into one family.

**Alternatives considered:**

- Demote labeled rows inside the ranking: the reverted change showed that rank fusion cannot weigh such evidence.
- A label for two members of the same type: no audit category needed it.
- A strict test-wrapper label for a test whose body is one call: needs body-shape analysis, and `tests_subject` already lets the reader exclude every test that uses the subject.

### Decision: RelationSource

**Chosen:** This change uses only the existing `uses`, `type_hierarchy`, and `contains` edges and the test classification.
Method-to-method implementation links are left to their own change.

**Rationale:** Consuming SCIP `is_implementation` relationships means a new persisted edge kind, a schema change, and a new extraction path.
The scip-python indexer, version 0.6.6, emits them; rust-analyzer's emission is not verified, and the `ty-scip-backend` change adds a Python backend whose emission is unknown.
Under the contract-floor rule, a link only some backends provide must be an optional, queryable capability, and its contract also serves `trace implementers` for methods, which is wider than `similar`.
That is a change of its own size.
`type_family` already covers the audit's parallel-implementation category at the type level.

**Alternatives considered:**

- Consume SCIP relationships here: doubles the change and couples it to backend capability work.
- Match method names within a type family: a heuristic.
  It is not presented as a fact; if it is ever offered, it needs a heuristic-grade label.
- Use Rust's language rule that a method in `impl Trait for T` implements the trait item of the same name: exact, but Rust-only, so it belongs with the optional capability above.

### Decision: RelationExclusion

**Chosen:** A repeatable `--exclude <label>` flag on `similar`, valued in the four labels.
The excluded rows are removed from the ordered list before the shared paging code cuts the page.
The page identity binds the sorted exclusion set.
The machine answer carries `excluded_relations` when an exclusion is given.

**Rationale:** Removing rows before paging keeps the limit meaningful and the order unchanged.
Binding the set makes a token from one exclusion fail under another, as the paging contract requires.

### Decision: BatchSimilar

**Chosen:** `similar --subjects-from <path|->` reads one reference per line, trims surrounding whitespace, and skips blank lines.
It conflicts with a positional reference, `--at`, `--cursor`, and `--mutual`.
`-` reads standard input and is refused as a usage error when standard input is a terminal.
A line that is not valid UTF-8 or a path that cannot be read is a usage error naming the source.
The machine answer is `{"answers": [{"reference": <line>, "answer": <single answer>}]}` in list order, where `<single answer>` is the object a single-subject `similar --json` returns.
The human rendering prints each reference as a heading, then its single-subject rendering.

`similar_of` is split into a function that returns the ordered candidate identities with their clone classes and a function that materializes rows for the candidates it is given.
The single, batch, and mutual paths all call the same two functions on one store connection.
The single and batch paths materialize every candidate, because the page is cut after the answer is built, in the paging code that every result-bearing command shares.
The mutual path materializes only the first rows of the pairs it reports.

**Rationale:** One process opens the store and loads the model once, which removes the per-launch cost the audit paid.
Calling the same code keeps each per-subject answer identical to the single answer, which a test checks byte for byte.
Clone classes come from one indexed lookup of the subject's substitution key, not from a key read for each candidate.
Cutting the page before materialization would change the shared paging code for every command, so it waits for a measured need; one ripgrep subject takes about 0.12 s in-process with every candidate materialized.
The flag form keeps the single-subject answer shape unchanged; the shape never depends on how many references the caller passes.

**Alternatives considered:**

- Several positional references: the answer shape would change between one and two references.
- A JSON Lines stream with one answer per line: harder for the MCP server to return as one tool result.

### Decision: AllPairsDiscovery

**Chosen:** Mutual nearest pairs reuse the per-subject `similar` computation in one process.
The walk visits corpus symbols in canonical-identity order.
For a symbol a, it computes the first row f(a) under the requested exclusion.
When f(a) = b and b sorts after a, it computes f(b), and emits (a, b) when f(b) = a.
When b sorts before a, the pair was already decided when the walk visited b.
Results are memoized within the process.
Pairs come out ordered by their lower member, so a page ends at a walk position, and the continuation token records the last visited symbol together with the index identity and the exclusion set.
The walk stops once it holds one pair more than the limit, so it knows that more pairs remain without walking the rest of the corpus.
A mutual answer therefore reports no total: its page disclosure omits `total`, and every other answer keeps it.
A limit of 0 walks the whole corpus once.
For each visited symbol, the walk ranks the candidates and labels them only until it reaches the first candidate that no exclusion removes; it builds no other rows.
The corpus clone groups use a grouping on the clone keys, with no vector work.

Comparison for the corpus-wide problems:

| Approach                                                | Source                                                                                                                                                                      | Answers match `similar`                                      | Cost as the corpus grows                           | Determinism and replay                                                                                                | Fit                               |
| ------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ | -------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | --------------------------------- |
| Group on the existing exact and variant keys            | Hash join                                                                                                                                                                   | Yes, for clone markers                                       | O(N) with an index on the key                      | Deterministic                                                                                                         | Chosen for clone groups           |
| Per-subject sqlite-vec KNN and FTS5 in one process      | The existing `similar` path                                                                                                                                                 | Yes, by construction                                         | O(N) per subject, O(N^2) for all pairs             | Deterministic; exhaustive scan, ties broken by identity                                                               | Chosen for batch and mutual pairs |
| HNSW graph index                                        | Malkov and Yashunin, IEEE TPAMI 2020                                                                                                                                        | No; recall below 1                                           | About O(log N) per query after an O(N log N) build | Level assignment is random and the graph depends on insertion order; needs a seed and single-threaded build to replay | Rejected                          |
| IVF or IVF with product quantization                    | Jégou, Douze, Schmid, "Product Quantization for Nearest Neighbor Search", IEEE TPAMI 2011; Johnson, Douze, Jégou, "Billion-scale similarity search with GPUs" (FAISS), 2019 | No; probes a subset of cells                                 | Sub-linear per query after k-means training        | k-means needs a seed; results depend on training                                                                      | Rejected                          |
| MinHash with LSH banding over token shingles            | Broder, "On the Resemblance and Containment of Documents", 1997; Indyk and Motwani, STOC 1998; Leskovec, Rajaraman, Ullman, "Mining of Massive Datasets", ch. 3             | No; a third similarity definition                            | Near-linear candidate generation                   | Needs a seeded hash family                                                                                            | Rejected                          |
| Embedding threshold deduplication (SemHash)             | MinishLab, SemHash (github.com/MinishLab/semhash): Model2Vec embeddings with a Vicinity nearest-neighbor backend, USearch by default                                        | No; a cosine threshold, not the fused `similar` order        | Sub-linear per query with the default ANN backend  | Exact only with the flat backend; the default ANN index is approximate                                                | Rejected                          |
| Exact in-memory blocked matrix product over all vectors | Brute force                                                                                                                                                                 | Not guaranteed; floating-point order differs from sqlite-vec | O(N^2 d), much faster per pair than SQL            | Deterministic                                                                                                         | Deferred                          |

A mutual nearest pair follows the mutual-nearest-neighbor relation of Gowda and Krishna ("Agglomerative Clustering Using the Concept of Mutual Nearest Neighbourhood", Pattern Recognition 1978), taken over the `similar` order rather than over a raw distance.

**Rationale:** The contract defines a mutual pair by the `similar` answer, so only a method that computes that answer can return the right pairs.
An approximate index would list pairs that `similar` does not confirm and miss pairs it does, and it would need a seed to replay.
MinHash would add a similarity definition with its own threshold beside the estimated ranking and the exact keys.
SemHash is the closest existing tool: it embeds with the same model family as c10r (Model2Vec) and groups records whose cosine similarity passes a threshold.
That threshold is a similarity score, which c10r does not expose, and the grouping reads only the vector leg, so its pairs would differ from the fused `similar` answer.
It is a Python library and c10r is Rust, so only its approach could be reused.
The walk visits each symbol once and each partner at most once, so a full listing costs between N and 2N per-subject computations, and a page costs only the symbols its walk reaches.
The exhaustive scan and identity tie-break already make `similar` reproducible, so no seed is needed.

Cost estimate, to be replaced by measurement: one in-process `similar` on the audit corpus takes tens of milliseconds, so a full mutual listing of about 2,700 symbols takes minutes, against about 10 minutes of process launches in the audit.
At sympy scale, the per-subject cost grows linearly and the full listing quadratically, so a full listing is expected to take on the order of an hour.
Paging lets a caller stop early.

**Alternatives considered:** as in the table.
The in-memory matrix product is deferred: it could cut the vector cost by a large factor, but distances summed in a different order can swap near-ties, and then a per-subject answer would differ from `similar`.
It is worth revisiting only if it can reproduce sqlite-vec's distances exactly.
Storing each symbol's ranked first candidates at build time is also deferred: it adds a full `similar` pass to every build, including builds whose users never ask for mutual pairs.
A full walk computed on every page was rejected: each page of a mutual answer would cost a full walk, the cost the audit set out to remove.

### Decision: CorpusCloneGroups

**Chosen:** `clones` with no subject runs one grouped query over `symbols` on `clone_substitution_key`, keeping groups of two or more, with exact subsets on `clone_formatting_key`.
A new index on `clone_substitution_key` serves both this query and the per-subject whole-symbol lookup.
Groups order by `clone_token_count` descending, then member count descending, then the lowest member's canonical identity.
Each member carries an `exact_set` ordinal that is equal for members sharing a formatting-insensitive key.
The limit counts groups, and the answer is computed whole and then paged.

**Rationale:** Every member of a substitution group has the same token count, because substitution keeps the sequence length, so the count is a property of the group.
Longest-first puts substantial copies ahead of the many small groups of trivial accessors and stubs, without a size threshold.

### Decision: AnswerShapes

**Chosen:**

- `SimilarItem` gains `relations: [{"label": <label>, "rule": <test rule, only for tests_subject>}]`, always present and empty when no label holds.
- A mutual pair row carries both members' symbol views and locations, the clone-certainty marker, and two label sets, one for each member as the other's first row.
- A `clones` answer carries `grade: "token_equivalence"` and `guaranteed_fragment_length`.
- A group row carries `certainty`, `token_count`, and members with `exact_set`.
- A subject row carries `kind` (`whole_symbol` or `fragment`), `certainty`, the other symbol's view, and for a fragment the run's location in the subject and in the other symbol and its token length.
- A keyless subject yields a new answer state, `not_comparable`, distinct from `empty`.
- No answer carries a distance, score, or fusion value.
- A mutual answer's page disclosure omits `total`; `page.total` is optional in the answer shape and present on every other answer.

**Rationale:** Adding fields breaks no JSON consumer.
A separate `not_comparable` state keeps a container subject from reading as "no clones exist".
Leaving scores out follows the user's decision and the existing rule that uncalibrated numbers invite thresholds that break on every model change.

### Decision: Surfaces

**Chosen:** CLI: `clones [<reference>] [--at <position>] [--limit] [--cursor]`; `similar` gains `--exclude`, `--subjects-from`, and `--mutual`.
`clones` takes no `--detail`; a caller reads content with `get`.
MCP: a new `clones` tool with `reference`, `at`, `limit`, and `cursor`; the `similar` tool gains `subjects` (a list the server writes to the command's standard input with `--subjects-from -`), `mutual`, and `exclude`.
The server instructions name `clones` as the deterministic duplication tool and `similar` as the estimated one.

**Rationale:** `clones` is deterministic and `similar` is estimated, so separate commands keep the two grades apart, as `find` and `search` are kept apart.
The MCP parameters mirror the command options, which the schema-against-surface tests check.

### Decision: Versions

**Chosen:** `SCHEMA_VERSION` rises by one above the value on `main` when the branch is cut: 18 when the kind fix and `stale-files-and-line-positions` have landed with 17.
The bump covers the docstring change to the keys, `clone_fingerprints`, `clone_token_count`, the two fragment-parameter columns, and the key index.
Later changes to k or w need no bump (§ FragmentEvidenceStorage).
The migration is the existing replace-or-refuse rule: a build replaces an older store, and a query refuses it with the rebuild command.
`SURFACE_VERSION` and the MCP pin rise by one above `main` when the branch is cut: 12, 13, or 14 depending on whether `stale-files-and-line-positions` and `ty-scip-backend` have merged.
No release may ship two different schemas under one version number.

**Rationale:** The index is derived data, so a rebuild is the migration, as for every earlier schema change.
Taking the next free number at branch time avoids a collision with the two changes that are already proposed.

## Architecture

```text
build
  spelled_tokens (docstrings skipped) ──> clone keys ──────────> symbols.clone_*_key, clone_token_count
                                     └──> placeholder tokens ──> winnowing ──> clone_fingerprints

similar <ref> | --subjects-from | --mutual
  ordered candidates (vector KNN + FTS5 ──> rrf_fuse ──> clone classes)     <- unchanged ranking
     └──> --exclude removes labeled rows ──> page ──> materialize rows
            └──> relation labels: uses edges, test rule, type_hierarchy of declaring types
  --mutual: walk corpus in identity order, memoized first rows, page by lower member

clones
  no subject: GROUP BY clone_substitution_key ──> exact sets ──> order by token count ──> page
  subject:    whole-symbol clones by key
              + fingerprints of subject ──> shared hashes ──> re-spell indexed text
                ──> diagonal extension ──> consistent intervals >= guaranteed length ──> page
```

## Risks

- **Common idioms produce many fragment rows.**
  Shared decorators, test setup, and boilerplate can each exceed the guaranteed length.
  Mitigation: longest runs first; the dogfood task records row counts per subject on the Python and Rust clones.
- **Frequent fingerprints slow a query.**
  A fingerprint shared by thousands of leaves makes the query verify many candidates.
  Dropping frequent fingerprints, as MOSS does, would break the completeness claim.
  Mitigation: measure the slowest subjects on the largest store and record them here.
- **Fragment storage.**
  At w = 21, about 0.09 fingerprints per token are stored, an estimated 1 to 2 MB for the audit corpus.
  Mitigation: measure store growth on every dogfood store and record it here.
- **Mutual pairs at large scale.**
  A full listing is quadratic in corpus size.
  Mitigation: paging by walk position, the documented cost, and a measurement on the largest store.
- **`type_family` on unrelated members.**
  A Rust inherent method of a type that also implements a workspace trait gets the label with the trait's methods.
  The label states the type relation only, and the documentation says so.
- **Clone markers change after the rebuild.**
  Pairs that differed only in docstrings become exact or variant clones, which is the intended effect.
  Mitigation: the changelog states it.
- **Batch output size.**
  A long list produces a long answer, with each per-subject answer bounded by `--limit`.
  The caller chose the list, so the batch itself is not capped.
- **A per-subject answer drifts from the single answer.**
  Mitigation: the single, batch, and mutual paths share the candidate and materialization functions, and a test compares the outputs byte for byte.
