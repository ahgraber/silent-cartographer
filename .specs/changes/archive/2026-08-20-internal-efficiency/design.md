# Design: internal-efficiency

## Context

Ingest reads syntax through seven derivations: the guarded join, tier content (`definition_content`), enclosure and definition parents (`parent_of_definition`), test classification (`classify_test_symbols`), declared subtype edges (the `type_hierarchy` loop), and clone-equivalence keys.
Each obtains its own tree.
The join builds a `PreparedDocument` per document — tree, line index, and declared alias bindings — inside `join()` and drops the map on return; `definition_content` and `parent_of_definition` call `SyntaxTree::parse` per symbol and per definition; the classification and subtype passes parse per document; the clone-key pass already keeps a per-document cache of its own.

Two existing baseline requirements describe tier content and enclosure as coming from "the same build's fresh syntax tree that anchors the guarded join" and from "the fresh syntax tree".
Today's implementation satisfies that by producing an equivalent tree rather than the same one.
This change makes the wording literal.

`SyntaxTree` owns a `String` copy of its document, so each parse also copies the source.

## Decisions

### 0. The currency check reuses the freshness predicate, and runs before analysis

`GraphStore::freshness` already compares a store's recorded content hash, analyzer provenance, and declared environment against the current ones, and `status` already trusts its verdict.
`run_build` never calls it.
The build gains that check ahead of analysis: resolve the adapter, collect sources, hash them, and ask the store — when the verdict is fresh, return the recorded accounting and touch nothing.

This reorders `run_build`, which today analyzes first and reads sources afterwards.
Sources must be read before the check, because the hash is computed from them; reading and hashing a workspace is the cost of the check.

The adapter is constructed once and reused for both the check and the analysis that may follow, so the version probe does not run twice.

A `--force` flag bypasses the check.
It is a new flag on `build`, so the structural surface manifest is regenerated and `SURFACE_VERSION` is bumped.
`build`'s machine projection gains a field distinguishing a build from a skip; the human line reports the skip in place of the accounting line.

### 1. A build-scoped prepared corpus owns the one parse per document

A new `graph::prepared` module holds `PreparedCorpus`: the map from document path to `PreparedDocument`, built once from a `SourceCorpus` and a `Language`, and borrowed by every derivation.
`PreparedDocument` moves there from `join.rs`, unchanged in shape.

`join()` takes `&PreparedCorpus` instead of building its own map, and `SourceCorpus` remains the text-in type that `PreparedCorpus::prepare` consumes.
Keeping both types preserves the existing join test surface, which constructs a `SourceCorpus` directly.

Alternatives rejected:

- **Return the map from `join()` in `JoinResult`.**
  The result would then carry both an answer and an intermediate, and every consumer of the join's answer would borrow the whole parse set.
- **Thread the map as a bare `HashMap` parameter.**
  A named type can state and enforce the invariant — one entry per document, built once — and can carry the accessors the derivations need.

### 2. Preparation is eager over the analyzed sources

`PreparedCorpus::prepare` parses every document in the source corpus, in one pass, before the join runs.

Lazy preparation would parse only what a build touches, but it needs interior mutability to memoize behind a shared reference, and handing out `&PreparedDocument` from a `RefCell` is not expressible without either cloning the document or restructuring every consumer into a closure.
Eager preparation costs nothing that is not already paid: the classification and subtype passes already parse every source document today, so the prepared set is the same set.

A document that fails to parse has no entry.
Absence therefore means "attempted and unavailable", and because nothing parses outside `prepare`, no retry is possible — which is how the requirement's at-most-once attempt is satisfied structurally rather than by a guard.

### 3. A prepared document indexes its own declarations by name span

Removing the repeated parse is not sufficient on its own.
`definition_content` locates a symbol's declaration by calling `all_declarations`, which walks the whole tree and allocates a vector of every declaration in the document, then searches it linearly — once per symbol.
A build that parsed each document once but kept that walk would still perform per-symbol whole-tree walks — the same waste in a different resource — so tier content resolves by lookup instead.

`PreparedDocument` therefore carries its declaration list and an index into it by name span, both built during preparation, and tier content resolves by lookup.

The join's syntax-only accounting visits every declaration in a document exactly once per document, which is already within the bound, so it reads the same prepared list — one whole-tree declaration walk per document, shared by both consumers.

### 4. Enclosing-declaration resolution reads a location-keyed map of definitions

`enclosing_symbol` currently iterates `def_name_span` — identity to location — comparing every entry's document and span against the declaration it is resolving.
The change builds the inverse map once per build, `(document, name_span) -> identity`, restricted to definition-role aligned occurrences, and resolves by lookup.

Its second path is unchanged: a declaration whose node kind is `impl_item` resolves through `type_by_name` to the type the block implements, because an impl block is not itself a persisted declaration and so has no entry in a location map.
That path is already a keyed lookup rather than a scan.

The two location maps the build already assembles for other passes — `occ_by_location` for subtype edges and `occ_at` for test classification — cover all aligned occurrences, not definitions alone.
They are not merged with this one: a reference occurrence sharing a definition's name span would silently change which identity an enclosure resolves to.

### 5. A location holding two definitions attributes to neither

Two persisted symbols can hold definition-role occurrences at one location.
It occurs once across the five dogfood repositories: `faker/decode/codes.py` opens with `codes = (`, and both the module symbol and the constant `codes` take a definition at bytes 0–5, because the module-name rule accepts the module's definition at a token spelling the module's terminal component.

Today's answer in that case is whichever entry the hash map happens to iterate first, which varies between process runs.
There is therefore no current behaviour to preserve, only a current nondeterminism.

The build refuses instead: a location holding more than one persisted definition resolves to no symbol, and the occurrence attributes to its module — the same outcome as an occurrence no declaration encloses.
This follows the product's rule that disagreement is represented rather than silently resolved, and it is the reason the requirement gained a clause rather than the design gaining a tie-break.

The choice is unobservable on today's corpora: Python's declaration kinds are `module`, `class_definition`, and `function_definition`, so an assignment's name span never enters an enclosing-declaration chain and the colliding location is never consulted.

### 6. Work bounds are observed by counters compiled only into tests

Two counters under `#[cfg(test)]` record documents parsed and candidate comparisons performed while resolving an attribution.
`SyntaxTree::parse` increments the first; the attribution lookup increments the second.

The evidence comes from in-crate unit tests, which call `ingest` and `build_from_index` directly, so `cfg(test)` applies and no counter reaches a shipped binary.
The counters are thread-local, because ingest is single-threaded and a thread-scoped count is exact regardless of other tests running in parallel in the same binary.

Alternatives rejected:

- **Assert elapsed time.**
  A timing threshold encodes the machine that ran it, fails intermittently under load, and would make the contract decay.
- **Count only inside `PreparedCorpus`.**
  That counts what the corpus prepared, not what the build parsed, so a stray `SyntaxTree::parse` elsewhere in ingest would pass unnoticed — which is the exact defect under repair.
- **Ship the counters as ordinary library state so integration tests can read them.**
  That puts observability into every binary for the benefit of tests that do not need to be integration tests.

### 7. Line indexes come from the prepared document

`module_by_document` builds a `LineIndex` per module symbol on the Rust path.
`PreparedDocument` already holds one per document, so the derivation reads it instead.
This is the same waste in a different resource and is fixed with the same map.

## Architecture

```text
sources ──▶ SourceCorpus ──▶ PreparedCorpus::prepare
                                        │
                                        │  per document, once:
                                        │    syntax tree
                                        │    line index
                                        │    alias bindings
                                        │    declarations by name span
                                        ▼
                                 PreparedCorpus
                                        │
        ┌───────────────────────────────┼───────────────────────────────┐
        ▼               ▼               ▼               ▼               ▼
 module_by_document    join     definition_content  parent_of_    type_hierarchy
                    (alignment)   (tier content)    definition      + clone keys
                                                   classification

join result ──▶ definitions_at: (document, name_span) -> identity
                        │
                        ▼
                 enclosing_symbol resolves one attribution by lookup
```

## Risks

**Peak memory holds every tree at once.**
The join already held every indexed document's tree simultaneously, measured at 38 MiB across all 813 Faker documents; the parsed set was also already every discovered source, because the classification and subtype passes parsed them all.
What grows is retention: those passes dropped each tree after reading it, and the prepared corpus keeps every discovered source's tree for the build's duration — so a workspace with many discovered-but-unindexed sources (a vendored source directory the analyzer skips) now holds their trees at peak, at roughly four times their source bytes.
On the dogfood repositories the two sets coincide and the measured whole-build peak moved by 6 MiB on Faker.
The per-document source copy that `SyntaxTree` owns is made once per document rather than once per parse, so per-document copying shrank.
Mitigation: peak RSS is recorded per dogfood repository alongside the timing indicator, and restricting retention to the index's documents (extracting classification signals at preparation, then dropping unindexed trees) is recorded in `discussion.md` as a follow-up candidate should a real workspace pay this.

**The refusal at a shared definition location could remove a working attribution.**
Decision 5 attributes such an occurrence to its module instead of to a symbol.
No current attribution resolves through such a location, so the byte-identical comparison should show no difference; if it does, an attribution that used to land on an arbitrary symbol now lands on the module.
Mitigation: the comparison names the affected occurrences, and each one is inspected rather than accepted in bulk.

**The behaviour-neutrality comparison straddles two changes.** `semantic-chunking` alters stored vectors deliberately.
Mitigation: both sides of the comparison are built from the same embedding code, and vectors are excluded from the compared columns.
