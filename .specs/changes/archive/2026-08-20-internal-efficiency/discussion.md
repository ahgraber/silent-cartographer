# Discussion: applying parser data-layout engineering to c10r internals

Captured 2026-07-13, with the measurement record of 2026-08-16 appended below.
This is the analysis behind `proposal.md`, preserved in case the change is continued later.
It is a research note, not a contract — nothing here is a spec.

> The 2026-07-13 note below reasoned from reading the code.
> Where the measurements of 2026-08-16 contradict it, the measurements win; the note is kept as written rather than edited, so the reasoning that produced the wrong prior stays visible.
> Two of its claims did not survive: that the external indexer subprocess dominates a build, and that the re-parsing is a micro-optimization.

## Source

> Arshad Yaseen, "Engineering High-Performance Parsers," _arshad.fyi_ (writings).
> URL: <https://www.arshad.fyi/writings/engineering-high-performance-parsers>
> Accessed 2026-07-13.
> Subject: **Yuku**, a JavaScript/TypeScript parser the author wrote in **Zig**.
> Thesis (author's words): "Design the data structure first, let the machine's access patterns dictate its shape, and the speed follows almost for free" — and "A fast parser is not the product of a clever algorithm bolted onto an ordinary data structure. It is the product of an ordinary algorithm, recursive descent, running over a data structure designed for the machine."

## The essay in one paragraph

Two hardware realities drive the design: a main-memory cache miss (~100 ns) dwarfs an arithmetic op (a fraction of a ns), and a general-purpose allocator call is not free when a 100 KB file yields ~50,000 nodes.
So the essay never changes the parsing _algorithm_ (textbook recursive descent + precedence climbing); it changes the _representation_ the algorithm writes into.
Techniques: **indices not pointers** (`u32` into one flat node array, half the size, position-independent, bulk-freeable); **arena allocation with geometric growth** (reserve from source-length estimate, hot path is bounds-check + bump); **struct-of-arrays** (columnar, so a pass touching one field doesn't drag siblings into cache); **side tables** for variable-length children (offset+len descriptors into one flat extras array); **scratch buffers** reused across recursive descent and flushed in bulk; **offset-based strings** (a string is a `{start,end}` byte range into the source — zero copy for the common case, interned only for synthesized/escaped text); **token metadata packed into bit-fields** (precedence/is-keyword/is-operator answered by a mask+shift, no branch, no memory access); **two-level Unicode tries** for identifier classification with an ASCII fast path; and the capstone — **the flat AST is already a wire format**, so it can be handed across a language boundary, memory-mapped from disk, cached between runs, or shared read-only across threads with no serialize/deserialize step.
Everything is bounded by compile-time size assertions so layouts can't silently drift.

## Why most of it does not apply directly

c10r's north star commits to **Adopt over build**: it reuses tree-sitter and external SCIP indexers rather than writing parsers.
So the essay's literal techniques — recursive descent, token bit-fields, Unicode tries, arena-allocated AST nodes, offset-based lexing — **already live inside c10r's dependencies**. c10r gets them for free.
The essay is, if anything, strong evidence that "adopt over build" was correct: one engineer wrote a whole essay's worth of work to make _one_ language's parser fast; c10r inherits that for two languages without spending it.

What can transfer is only the essay's _meta-principles_, applied to c10r's **own** hot paths — the guarded join, ingest, the identity strings, and SQLite persistence — none of which are parsers.

## What transfers, ranked by leverage

Ranked against c10r's actual goals (trust and token cost, per the north star), not against raw speed.

### 1. "Do the expensive work once" → stop re-parsing in ingest

The essay's control-plane/data-plane discipline: setup can be slow (runs twice); per-node work must be tight (runs millions of times).
Parsing a file is control-plane work — it should happen once per file.

Today, `ingest` (`src/graph/mod.rs`) re-parses each source file many times per build: once per symbol in `definition_span`, once per occurrence in `parent_of_definition`, and again for every file in the `type_hierarchy` derivation loop.
Each call allocates a fresh tree-sitter `Tree` **and a full `String` copy of the source** (`src/graph/syntax.rs`).
Meanwhile the guarded join already parses each file exactly once into a `PreparedDocument` cache (`src/graph/join.rs`) — and ingest throws it away and re-parses.

For c10r the win is framed as **consistency, not speed**: one syntax parse is one syntax oracle, so ingest's derived spans/parents cannot diverge from the tree the join aligned against.
Correctness-neutral refactor, no new data structures.
This is the strongest candidate — see the laddering caveat in `proposal.md` Open Questions.

### 2. "Indices, not pointers" → intern `CanonicalId` into typed handles

The essay replaces 8-byte pointers with 4-byte `u32` indices — smaller, cache-friendly, and _hard to misuse_. c10r's analog: `CanonicalId` is a `String` (`src/identity.rs`) cloned heavily through the join and ingest, and dozens of `HashMap<CanonicalId, …>` are keyed on owned strings and rebuilt every build.
A typed handle (newtype over an interned integer) turns clones into cheap copies and string-hashes into integer-hashes.

For c10r the primary payoff is **type safety → trust**, not microseconds: a `SymbolId` can't be accidentally confused with a document path or a display string the way a bare `String` can.
This is parse-don't-validate applied to identity.
Speed is a side effect.

### 3. Roadmap lens: "the flat tree is a wire format" → incremental / daemon

The essay's capstone: because the AST is a flat buffer, it _is_ a serialization format — memory-map it, cache it between runs, skip re-parsing. c10r currently re-derives everything each build (full re-parse, re-run the SCIP indexer, whole-index staleness).
The north star defers a resident daemon, a live semantic overlay, and incremental rebuild to Beyond-v1.

The insight to carry into those changes: **design the persisted representation so it can be reused, and use content hashes to skip unchanged work rather than reject it.** c10r already has the `content_hash` gate (`src/graph/mod.rs`) — today it only _rejects_ a stale join; the same fingerprint applied per-file is the seed of _skipping_ unchanged files.
Worth keeping in mind so the schema being versioned now (currently v10) doesn't paint the daemon into a full-rebuild corner.
Not built here.

## What does not transfer (so it isn't force-fit)

- **Struct-of-arrays / columnar layout.**
  SoA pays off when you scan one numeric field across millions of elements and want to avoid dragging neighbors into cache. c10r's hot data is SQLite rows and string-keyed `HashMap`s with heavy per-item work (string compares, tree walks) — it is not memory-bandwidth-bound, so SoA would add complexity for no real win.
- **Token bit-fields, Unicode tries, offset-based lexing, recursive descent, arena-allocated nodes.**
  All inside tree-sitter and rust-analyzer.
  Re-implementing them is exactly the trap **Adopt over build** exists to avoid.
- **Prepared-statement reuse on inserts.**
  The per-row `conn.execute` inserts (`src/graph/store.rs`) re-prepare their statement every row, where `insert_discrepancies` prepares once and loops.
  This is a real "amortize the setup" analog — but it ladders to no north-star outcome (build latency isn't measured), so it is scope-to-question, not scope-to-build, until a profile says otherwise.

## Honest priority framing

c10r's dominant cost is the **external indexer subprocess** (`rust-analyzer scip` / `scip-python index`), not anything in its Rust.
Everything above is a micro-optimization against a backdrop where speed is explicitly _not_ the v1 success metric — trust is.
So the honest framing is: these are **hygiene/correctness wins that happen to also be faster**, not a performance project.
If c10r ever profiles, the subprocess and the join's nested loop come first — not the string clones.

## Code pointers referenced

- Redundant re-parsing in ingest: `src/graph/mod.rs` (`definition_span`, `parent_of_definition`, `type_hierarchy` loop)
- One-parse-per-document cache the join already holds: `src/graph/join.rs` (`PreparedDocument`)
- Tree + owned source copy: `src/graph/syntax.rs`
- `CanonicalId` as `String`: `src/identity.rs`
- Per-row inserts vs. prepared loop: `src/graph/store.rs`
- Whole-index content-hash gate: `src/graph/mod.rs`
- Product posture (adopt over build; speed not a v1 goal; deferred daemon/overlay): `.specs/NORTH-STAR.md`

## Measurement record (2026-08-16)

The investigation started from a user report: `build --language python` over Faker peaked at 27.8 GB of memory.
Ingest was instrumented with a counting allocator and phase markers to attribute peak heap and wall time per phase.
The instrument sources are kept under `._scratch/internal-efficiency-probes/`.

### What the memory turned out to be

None of it was the graph work.
Every ingest phase held under 110 MiB live; the peak came from one call, embedding the semantic corpus, at 22.4 GiB.
That defect — the model's tokenizer pads a batch to its longest member, and a static model has no attention mask to exclude the pad tokens from the pooled mean — belongs to the `semantic-chunking` change, together with the corpus-side bound it needs.
It is recorded here only because it is why this change was reopened.

### Redundant parsing, measured

Ingest spends nearly all of its time in the two derivations that re-parse, and almost none of it anywhere else.
Phase attribution on Faker (813 Python documents, 8.9 MB), from a build instrumented with the counting allocator:

| phase                                              | wall  |
| -------------------------------------------------- | ----- |
| join, including one parse of all 813 documents     | 2.4 s |
| building symbol rows (`definition_content`)        | 53 s  |
| inserting symbols and occurrences                  | 1 s   |
| deriving `contains` edges (`parent_of_definition`) | 42 s  |
| deriving `uses`/`imports` edges                    | 0.7 s |
| type hierarchy, discrepancies, corpus assembly     | 1 s   |

Everything that is not re-parsing costs about six seconds together, and one parse of every document costs 2.4 s of that.
The parse counts behind the two expensive phases, derived from each dogfood repository's own index — one parse per persisted symbol, one per aligned definition occurrence, and three per-document passes:

| repository | documents | parses | per document | source | bytes parsed | ratio |
| ---------- | --------- | ------ | ------------ | ------ | ------------ | ----- |
| httpx2     | 119       | 12,137 | 102x         | 1.0 MB | 223 MB       | 219x  |
| flask      | 83        | 7,987  | 96x          | 0.5 MB | 161 MB       | 295x  |
| ripgrep    | 98        | 10,478 | 107x         | 1.7 MB | 762 MB       | 453x  |
| fd         | 24        | 1,526  | 64x          | 0.2 MB | 33 MB        | 134x  |
| Faker      | 813       | 34,383 | 42x          | 9.2 MB | 1,091 MB     | 119x  |

The ratio column is bytes parsed against bytes of source.
The redundancy is universal across both languages and scales with symbol density, so the densest repository (ripgrep) pays the highest ratio while the widest (Faker) pays the largest absolute cost.

### Is a rebuild cheaper than a first build?

No: `run_build` re-runs the indexer over the whole workspace, re-reads every source file, and `clear_derived` discards the prior build's rows before re-deriving them.
Embedding vectors whose render text is unchanged are the only work that survives a rebuild.

Three consecutive builds, measured with a temporary timer around the analyze and ingest calls (`build-timing.patch` in the probe directory):

| build                     | Faker: indexer / ingest / total | ripgrep: indexer / ingest / total |
| ------------------------- | ------------------------------- | --------------------------------- |
| 1, cold, no store         | 53.7 / 67.3 / 121.0 s           | 5.2 / 41.6 / 46.8 s               |
| 2, sources unchanged      | 53.7 / 69.7 / 123.3 s           | 4.6 / 39.7 / 44.3 s               |
| 3, one source file edited | 53.5 / 66.6 / 120.1 s           | 4.6 / 40.0 / 44.5 s               |

Ingest is 56% of a Faker build and 89% of a ripgrep build.
`rust-analyzer` produces ripgrep's entire semantic index in about five seconds; c10r then spends forty seconds re-parsing the same hundred files.

These totals are from an uninstrumented binary; the phase table above is from the instrumented one.

### Two priors the measurements overturned

**"c10r's dominant cost is the external indexer subprocess."**
Ingest is the larger half of a Python build and nearly the whole of a Rust one, and re-parsing is nearly all of ingest.
On ripgrep the indexer costs five seconds against ingest's forty.

**"This is a micro-optimization with no product ladder."**
A constant-factor argument would be, but the parse count is not a constant factor: it is the symbol count.
A repository with twice the symbols per file pays twice the redundant parsing on the same source bytes.

### Candidates this change does not carry

Recorded so they are not lost, and deliberately left out of `proposal.md`, which lists only work a reader might otherwise expect this change to include.

**Interning `CanonicalId` to an integer**, from item 2 of the 2026-07-13 note above.
The note argued it as type safety, but `CanonicalId` is already a distinct type rather than a bare `String` (`src/identity.rs`), so that part is already in place.
What remained is cheaper copying and hashing, which does not appear in the measured profile.

**Prepared-statement reuse on the per-row inserts**, from the same note.
Symbol and occurrence insertion together take about a second on Faker.

**Restricting prepared-corpus retention to the index's documents**, from the 2026-08-20 review round.
The prepared corpus keeps every discovered source's tree for the build's duration, where the prior passes dropped each tree after reading it; a workspace with many discovered-but-unindexed sources (a vendored source directory) pays roughly four times those source bytes at peak.
The dogfood repositories pay nothing measurable (6 MiB on Faker), so the restructure — extract the classification signals at preparation, then drop trees for documents the index does not reference — waits for a real workspace that pays it.

**Re-hashing sources after analysis**, from the same review round.
The external indexer reads the live workspace while ingest joins against the sources collected before it ran, so an edit made during analysis skews the two.
The build order chosen here makes that skew self-announcing — the recorded hash is the snapshot's, so a persisting mid-analysis edit reports the store stale and the next build repairs it, where the prior order (collect after analysis) reported it fresh — and any surviving per-occurrence disagreement lands in the discrepancy accounting.
Recollecting and re-hashing after analysis, and refusing or retrying on a change, would shrink the window further; it is hardening on a pre-existing condition, not a defect this change introduced.

**Two query-side observations**, routed here by earlier reviews and still unmeasured.
Every trace relation materializes its full result set before pagination slices it, and the `tests` relation adds an attributed-symbol lookup per reference site on every resumed page (is-tested review, 2026-08-01).
`search` and `similar` hydrate a full symbol row for every fused candidate before pagination returns a page (semantic-investigation review, 2026-08-12).
Both sit on the query path rather than the build path, so they belong to a measured change of their own.

## Measurement record: the currency check (2026-08-19)

Group 2's checkpoint, measured with the working-tree release binary (`target/release/c10r`) and the probe script `level1measure.sh`: a cold build, a rebuild over unchanged sources, a forced rebuild, and a rebuild after appending one comment line to one source file.

| build                     | Faker (Python) | ripgrep (Rust) |
| ------------------------- | -------------- | -------------- |
| 1, cold, no store         | 122.0 s        | 70.8 s         |
| 2, sources unchanged      | 0.195 s        | 0.066 s        |
| 3, unchanged, `--force`   | 121.5 s        | 67.4 s         |
| 4, one source file edited | 119.1 s        | 67.5 s         |

The check does what scope item 1 asked: a build over unchanged inputs costs a fifth of a second instead of two minutes, `--force` restores the full build on demand, and any edit still costs a full build — which is the gap the remaining groups exist to close.

The probe's fifth build (restore the edited file, rebuild) also rebuilt, on both repositories.
That is correct, not a defect: build 4 re-recorded the store against the edited sources, so restoring the file is simply the next one-file change.
The skip only ever describes the store's latest recorded state.

## Measurement record: one-pass derivation (2026-08-20)

Group 5's checkpoint, after groups 3 and 4 (one parse per document, enclosure by location).

### Behaviour neutrality

Stores built by the pre-change binary (HEAD plus the `semantic-chunking` padding line, so both sides embed with the same code) and the post-change binary were compared row for row on all five dogfood repositories: symbols, occurrences, edges, discrepancies, corpus renders, and the whole metadata row including the alignment accounting (vectors excluded, per the design's mitigation).

| repository | compared rows | result    |
| ---------- | ------------- | --------- |
| fd         | 38,316        | identical |
| flask      | 101,213       | identical |
| httpx2     | 178,818       | identical |
| ripgrep    | 253,928       | identical |
| Faker      | 1,072,487     | identical |

### Timing

The wall-clock figures below carry a caveat: the Faker script run was contaminated by machine load (its cold build read 19m37s and its final rebuild 40m49s — an order of magnitude off two adjacent clean runs), so Faker's figures come from repeated single builds under normal load, and the indexer/ingest split from a temporary probe around the analyze and ingest calls (since reverted).

| measure                         | Faker (Python)     | ripgrep (Rust)    |
| ------------------------------- | ------------------ | ----------------- |
| full rebuild, before groups 3–4 | ~121 s             | ~67 s             |
| full rebuild, after             | ~110–120 s         | 10–16 s           |
| — of which indexer subprocess   | 82.8 s (probe run) | 6.8 s (probe run) |
| — of which ingest               | 4.5 s              | 3.3 s             |
| unchanged rebuild (skip)        | 0.2–0.3 s          | 0.066 s           |

Ingest fell from ~67 s to 4.5 s on Faker and from ~40 s to 3.3 s on ripgrep — the redundant-parse cost the phase attribution predicted, gone.
A ripgrep edit now costs 10–16 s instead of 67 s.
A Faker edit still costs about two minutes, because the scip-python subprocess is now over 90% of the build (82.8 s in the probe run; 53.7 s when measured on 2026-08-16 — it swings with machine load either way).

### What this says about group 6

Per-document re-derivation can only save ingest time, and ingest is now 4.5 s (Python) / 3.3 s (Rust) of a build whose floor is the whole-workspace indexer.
The most a perfect group 6 could recover is a few seconds per edit, bought with a schema bump, a row-ownership model for rows with no single owning document, and modifications to at least five baseline "per build" requirements — while its known blockers (identity collision ranks, cross-document test gating, `type_by_name`, workspace-wide duplicate marking) stand unresolved.
The measured recommendation is to drop group 6 (option c) unless the indexer itself becomes incremental first, which is outside this change's scope.

**Decision (2026-08-20): parked.**
The user parked per-document re-derivation rather than building it, on this measurement.
The revisit trigger is the indexer's share of a rebuild becoming less dominant — the measurement points at the Python indexer itself as where the remaining leverage lives, and that is a change of its own.

## Measurement record: close-out (2026-08-20)

The full sequence — cold build, unchanged rebuild, forced rebuild, one-file-edit rebuild — on all five dogfood repositories, on a quiet machine, with the working-tree release binary.
Peak RSS is the maximum resident set of the whole build process tree, external indexer included, read from `getrusage(RUSAGE_CHILDREN)` (the sandbox denies `/usr/bin/time -l` its sysctl).
These figures supersede the group 5 table's Faker range, which was inflated by machine load: the true post-change Faker build is ~86 s, matching the probe split (indexer 82.8 s + ingest 4.5 s).

| repository | cold   | unchanged | forced | one-file edit | peak RSS  |
| ---------- | ------ | --------- | ------ | ------------- | --------- |
| fd         | 5.1 s  | 0.1 s     | 4.9 s  | 5.0 s         | 893 MiB   |
| flask      | 5.9 s  | 0.2 s     | 5.2 s  | 5.3 s         | 592 MiB   |
| httpx2     | 8.8 s  | 0.2 s     | 8.6 s  | 8.6 s         | 820 MiB   |
| ripgrep    | 9.9 s  | 0.1 s     | 9.7 s  | 9.7 s         | 1,087 MiB |
| Faker      | 86.3 s | 0.3 s     | 86.5 s | 85.1 s        | 2,452 MiB |

Against the pre-change baseline: a ripgrep edit fell from ~67 s to ~10 s, and a Faker edit from ~120 s to ~85 s — the whole remaining Faker cost is scip-python.
Faker's peak RSS of 2.4 GiB is measured with the `semantic-chunking` padding line in the working tree; the 27.8 GB peak that opened this investigation belongs to that change's record, not this one's.

### Peak-memory attribution, isolated (2026-08-20)

Asked directly whether this change's work lowers the reported 27.8 GB peak, the four combinations were measured: a cold Faker build (fresh store, so embedding runs) under each code state, peak RSS of the whole process tree.

| code state                          | peak RSS   | wall    |
| ----------------------------------- | ---------- | ------- |
| HEAD, no padding fix (the original) | 26,920 MiB | 259.4 s |
| this change, no padding fix         | 26,890 MiB | 166.6 s |
| HEAD + padding fix                  | 2,359 MiB  | 184.4 s |
| this change + padding fix           | 2,353 MiB  | 86.3 s  |

The peak is a pure function of the padding line: fixing it collapses 26.9 GiB to 2.4 GiB regardless of this change, and this change moves the peak by about 30 MiB (0.1%) regardless of the padding line.
This confirms the original attribution — every ingest phase held under 110 MiB live and the peak was the embedding batch — and it de-risks resuming `semantic-chunking`: its one-line fix alone removes the memory blow-up, with no dependency on this change.
Wall time is the opposite story — the two no-fix rows swap heavily and embed pad tokens, so their walls are inflated and noisy; the clean comparison is between the two padding-fixed rows, where one-pass derivation takes the build from 184 s to 86 s.

The `._scratch/internal-efficiency-probes/` instruments all stay: every file there is depended on by a recorded measurement — this change's (`memprobe.rs`, `ingestprobe.rs`, `scipdump.rs`, `build-timing.patch`) or `semantic-chunking`'s (`cmpvec.rs`, `scanbench.rs`, `embedprobe.rs`, and the Python scripts).
