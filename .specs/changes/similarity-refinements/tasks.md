# Tasks: similarity-refinements

## Documentation-blind token sequence

- [x] In `spelled_tokens` ([src/graph/syntax.rs](../../../src/graph/syntax.rs)), skip the node `python_leading_docstring` returns for every Python function, class, and module body inside the span (design.md § DocumentationLeavesTheTokenSequence); update the `comment_kinds` doc comment, which states that a docstring stays in the sequence.
- [x] Unit tests in [src/graph/clone.rs](../../../src/graph/clone.rs): two Python functions differing only in docstring text share the formatting-insensitive key; a docstring present in one copy only is disregarded; a renamed copy with a docstring in one copy only shares only the substitution-insensitive key; a bare string statement after the first statement stays a literal; two Rust functions differing only in doc comments share the formatting-insensitive key.
- [x] Run the existing clone-key tests unchanged to confirm the other key scenarios still hold.

## Store schema

- [x] In [src/graph/schema.rs](../../../src/graph/schema.rs), add `clone_token_count INTEGER` to `symbols`, an index on `clone_substitution_key`, and the `clone_fingerprints(hash, passage_id, token_ordinal)` table with an index on `hash`, and the `fragment_kgram_length` and `fragment_window` columns to `index_metadata` (design.md § FragmentEvidenceStorage).
- [x] Bump `SCHEMA_VERSION` by one above the value on `main` at branch time and add the migration note (design.md § Versions); update store tests only where they assert the literal version.
- [x] Add store functions to write fingerprints and token counts in the build transaction, to read a symbol's fingerprints, to look up the passages holding a hash, and to clear both in `clear_derived`.

## Fragment evidence at build

- [x] Add a winnowing function in [src/graph/clone.rs](../../../src/graph/clone.rs) over placeholder tokens with k and w as parameters, the SHA-256 k-gram hash, and rightmost-minimum selection; return nothing for a sequence shorter than the guaranteed length.
  Define k = 20 and w = 21 as two named constants in one place.
- [x] Unit tests: two sequences sharing a run of exactly the guaranteed length share a fingerprint at the same diagonal; the same input yields the same fingerprints on repeated calls; a sequence shorter than the guaranteed length yields none.
- [x] In the build ([src/graph/mod.rs](../../../src/graph/mod.rs)), write each leaf's token count and fingerprints next to its keys, and record k and w in `index_metadata`.
- [x] Test: a built fixture holds fingerprints for a long function and none for a container type or a short function.
- [x] Test: rebuilding the same fixture yields identical `clone_fingerprints` rows; after a leaf's source changes, its rows derive from the new source.
- [x] Test: a build that fails after the store opens leaves the prior build's fingerprint rows unchanged.

## Similar internals

- [x] Split `similar_of` ([src/query/search.rs](../../../src/query/search.rs)) into a function that returns the ordered candidate identities with their clone classes and a function that materializes the rows of one page (design.md § BatchSimilar); fetch clone-key matches with the new key index instead of per row.
- [x] Run the existing `similar` tests unchanged and compare `similar --json` output for ten subjects on the httpx2 and ripgrep dogfood stores before and after the split; record that the outputs are byte-identical.

## Relation labels

- [x] Add store lookups for a `uses` edge between two symbols in a given direction, a symbol's test rule, a symbol's declaring type, and whether two declaring types are linked by a `type_hierarchy` edge or share an `in_workspace` supertype.
- [x] Add `relations` to `SimilarItem` and compute it after ordering on the shared materialization path (design.md § RelationLabels, § AnswerShapes); render the labels in the human `similar` output.
- [x] Tests (single-subject path): a callee carries `used_by_subject`; a caller carries `uses_subject`; a test that calls the subject carries `uses_subject` and `tests_subject` with its rule; sibling Python methods carry `type_family`; a Rust trait default method and an implementing type's method carry `type_family`; an external common base gives no `type_family`; two methods of one class carry no `type_family`; an unrelated row carries an empty set.
- [x] Test: for a fixture subject, the rows and their order with labels equal the rows and order computed before labels were added.

## Relation exclusion

- [x] Add the repeatable `--exclude <label>` flag to `SimilarArgs` in [src/cli.rs](../../../src/cli.rs), remove excluded rows before paging, bind the sorted set into the page identity, and emit `excluded_relations` (design.md § RelationExclusion).
- [x] Tests (single-subject path): an excluded test row is absent and the other rows keep their order; the limit counts remaining rows; `excluded_relations` names the label; a token issued under one exclusion is refused under none.

## Batch similar

- [x] Add `--subjects-from <path|->` to `SimilarArgs` with its conflicts, read and validate the list at the boundary (unreadable path, invalid UTF-8, `-` on a terminal), and emit the `answers` array and the human headings (design.md § BatchSimilar).
- [x] Test: three references yield three answers in order.
- [x] Test: for each of a resolved, an ambiguous, and an unresolved reference, the per-subject JSON equals the single-subject `similar --json` output byte for byte.
- [x] Test: a truncated per-subject answer's token resumes a single-subject query.
- [x] Tests: standard input supplies the list; an empty list is typed-empty; a missing path fails with the usage exit code and names the path; `-` with a terminal on standard input is refused.
- [x] Tests (batch path): rows carry relation labels, and `--exclude` removes labeled rows, exactly as on the single path.

## Mutual nearest pairs

- [x] Add `--mutual` to `SimilarArgs` with its conflicts, and implement the memoized identity-order walk with the walk-position continuation token (design.md § AllPairsDiscovery).
- [x] Tests: a mutual pair appears once; a one-sided nearest neighbor is not a pair; an exact-clone pair carries the marker and both label sets; excluding `tests_subject` drops a test and subject pair.
- [x] Test: paging a fixture with more pairs than the limit to the end yields the unbounded answer.
- [x] Test: for every pair of a fixture, each member's single-subject `similar` first row is the other member.
- [x] Test: the mutual answer carries the estimation marker and the semantic-index identity.

## Corpus clone groups

- [x] Add the `clones` command to [src/cli.rs](../../../src/cli.rs) and [src/commands.rs](../../../src/commands.rs) with an optional reference, `--at`, `--limit`, and `--cursor` (design.md § Surfaces).
- [x] Implement the grouped key query and group ordering in the query engine (design.md § CorpusCloneGroups).
- [x] Tests: three exact copies form one exact group; a variant group identifies its exact subset; a unique function is in no group; a long-function group precedes a short-function group; a corpus with no shared key is typed-empty.
- [x] Test: paging the groups to the end yields the unbounded answer.

## Shared fragments

- [x] Implement the per-subject answer: whole-symbol clones by key, then fingerprint candidates verified by diagonal extension and consistent-interval scanning over tokens re-spelled from the indexed module text (design.md § FragmentVerification).
- [x] Unit tests for the consistent-interval scan: a fully consistent run; a run with one merged identifier, split at the conflict; an exact run marked exact; a renamed run marked variant.
- [x] Tests: a block copied into a long method is found from the shorter function; a renamed block is a variant fragment; an inconsistently renamed block with no consistent run of the guaranteed length is not reported; a shorter shared run is not reported; an exact whole copy has no fragment rows; a docstring difference does not break a run; a Rust fixture reports a shared block.
- [x] Tests: a container subject yields `not_comparable`; a function with no shared run or key yields typed-empty; `--at` inside a method takes the method; an ambiguous reference yields the candidate set.
- [x] Test: no reported pair is contained in another pair for the same two symbols, over a fixture with an overlapping repeated block.
- [x] Test: after a source file changes on disk without a rebuild, the fragment answer still reports runs from the indexed text and is marked stale.
- [x] Test: on a store whose recorded k and w give a guaranteed length different from the binary's constants, the subject answer filters runs by the recorded length and reports it.

## Clone answer labeling

- [x] Emit `grade: "token_equivalence"` and `guaranteed_fragment_length` on the group answer, the subject answer, the typed-empty answer, and the `not_comparable` answer, and state token equivalence in the human rendering.
- [x] Tests: group and subject answers carry the marker and the length and no estimation marker; a typed-empty answer carries the marker; an ambiguous subject carries no marker; a stale answer carries both the staleness flag and the marker.

## Surfaces and versions

- [x] Bump `SURFACE_VERSION` in [src/manifest.rs](../../../src/manifest.rs) by one above the value on `main` at branch time, and confirm the surface index lists `clones` and the new `similar` flags with their values.
- [x] In [mcp/src/c10r_mcp/server.py](../../../mcp/src/c10r_mcp/server.py), add the `clones` tool and the `subjects`, `mutual`, and `exclude` parameters on `similar`, writing `subjects` to standard input with `--subjects-from -`; update the server instructions; move the pin in [mcp/src/c10r_mcp/surface.py](../../../mcp/src/c10r_mcp/surface.py).
- [x] MCP tests: the tool list names `clones`; one `similar` call with three subjects returns three answers in order; `exclude` removes labeled rows; the schema-against-surface tests pass.

## Documentation

- [x] Add [docs/commands/clones.md](../../../docs/commands/clones.md) covering both forms, the token-equivalence grade, the guaranteed length, and `not_comparable`.
- [x] Update [docs/commands/similar.md](../../../docs/commands/similar.md) for relation labels, `--exclude`, `--subjects-from`, `--mutual`, the mutual-pair cost, and the docstring rule for clone markers.
- [x] Changelog: no hand-written entry.
  `just changelog` drafts the release section from this change's commits, which state the schema bump, the changed clone markers, and the new surface.

## Measurements

- [x] On every dogfood store, measure build time and store size before and after, and the fingerprint row count; record them in design.md § Risks.
- [x] Measure in-process `similar` time per subject and the full `--mutual` time on the httpx2, Faker, and ripgrep stores; replace the cost estimate in design.md § AllPairsDiscovery.
- [x] Measure the slowest `clones <subject>` queries and the fragment row counts per subject on the Python and Rust stores; record them in design.md § Risks.
- [x] Re-run the audit repository's five-line case and the renamed-test-with-docstring case; record whether `clones` reports the block and marks the tests as variant clones, as the gate for the guaranteed length.

## Amendments after the audit gate

- [x] Set k = 16 and w = 17 in [src/graph/clone.rs](../../../src/graph/clone.rs) (design.md § FragmentEvidenceStorage); update tests that depend on the literal length.
- [x] Add a boundary token class to `SpelledToken` that `placeholder_equal` never matches and `kgram_hash` tags with the contained symbol's canonical identity; add one function that computes a symbol's comparable tokens from a syntax tree, a span, and the merged spans of the corpus symbols it contains (design.md § ComparableContent).
- [x] Build: write `clone_token_count` and fingerprints for every function or method that contains corpus symbols, over its comparable tokens; keys stay leaf-only.
- [x] Query: re-spell both symbols through the same comparable-token function; gate `not_comparable` on `clone_token_count` instead of the keys.
- [x] Invariant test: over a built fixture store, the re-spelled comparable sequence of every symbol with a token count has exactly that length.
- [x] Tests on a Rust and a Python fixture: a function with a nested function contributes evidence for its own code; a block copied into the own code of a function with a nested function is found from the other function; no reported run covers a nested function's tokens; a type that contains methods is still `not_comparable`; `similar` output and corpus clone groups are unchanged.
- [x] Work limit: compute the candidate seed count per pair from the stored fingerprints before verification; a pair over the limit gets no fragment rows and is listed in `not_compared_completely` on the machine answer and in the human rendering (design.md § FragmentWorkLimit).
- [x] Measure the largest candidate seed count of any pair on the fd, Flask, httpx2, ripgrep, and audit-repository stores built at k = 16 and w = 17; set the limit by the rule in design.md § FragmentWorkLimit; record the value and the Faker pairs it excludes in design.md § Risks.
- [x] Tests: a pair of long literal tables is named as not compared completely with no fragment rows, while other symbols' rows are reported; the field is absent or empty when no pair reaches the limit.
- [x] Data stretches (design.md § DataStretches): classify data tokens (literal, punctuation, and record-literal structure from a per-backend node-kind table; Rust: struct-expression field names and its opening type name; Python: none); hash an all-data k-gram over its literal text; in verification, require every data stretch of at least k tokens within an interval to be identical.
- [x] Tests on Rust and Python: different literal tables share no fragment; a copied table is an exact fragment; a changed numeric constant in code remains a variant fragment; the completeness test of the shared-run scan still passes.
  Rust only (the backend that classifies record structure): different tables of struct literals share no fragment.
- [x] Rework the work-limit tests so they still reach the limit after the data-stretch rule (literal tables no longer share fingerprints): use a repetitive code fixture, and keep the assertions on naming, absence, paging, and rendering.
- [x] Make the removal of contained runs sub-quadratic (sort, then a sweep), with answers unchanged; the shared-run unit tests and the fragment tests pass unchanged.
- [x] Human rendering: name each symbol not compared completely by canonical identity, and word an empty answer with such symbols as "no clones found among the symbols compared".
- [x] Tighten data stretches (design.md § DataStretches): measure each stretch over its whole length in the aligned run; mark every differing token of a stretch as a break no interval includes; classify a sign directly before a numeric literal as a data token in Rust and Python.
- [x] Tests on Rust and Python: different table values do not extend a fragment (one shared statement between two pairs of differing tables yields no row); different tables of negative numbers share no fragment; a copied table with one edited value still reports the identical parts of at least the guaranteed length; the brute-force scan test and the completeness reasoning are updated to the per-token rule.
- [x] Rebuild the measured stores at k = 16 and w = 17; re-measure fingerprint rows, store growth, and the slowest `clones <subject>` queries, including Faker's ten heaviest subjects; update design.md § Risks.
- [x] Re-run the audit gate on the audit repository: record whether `clones` reports the five-line block, and record the result in design.md § FragmentEvidenceStorage.
- [x] Update [docs/commands/clones.md](../../../docs/commands/clones.md) for the 32-token length, comparable content, the boundary rule, the work limit, and `not_compared_completely`; update the MCP `clones` tool description if it mentions the length.

## Close-out

- [x] Run the full Rust and MCP test suites and every pre-commit hook on the changed files, and report the results with counts.
