# Tasks: similarity-refinements

## Documentation-blind token sequence

- [ ] In `spelled_tokens` ([src/graph/syntax.rs](../../../src/graph/syntax.rs)), skip the node `python_leading_docstring` returns for every Python function, class, and module body inside the span (design.md § DocumentationLeavesTheTokenSequence); update the `comment_kinds` doc comment, which states that a docstring stays in the sequence.
- [ ] Unit tests in [src/graph/clone.rs](../../../src/graph/clone.rs): two Python functions differing only in docstring text share the formatting-insensitive key; a docstring present in one copy only is disregarded; a renamed copy with a docstring in one copy only shares only the substitution-insensitive key; a bare string statement after the first statement stays a literal; two Rust functions differing only in doc comments share the formatting-insensitive key.
- [ ] Run the existing clone-key tests unchanged to confirm the other key scenarios still hold.

## Store schema

- [ ] In [src/graph/schema.rs](../../../src/graph/schema.rs), add `clone_token_count INTEGER` to `symbols`, an index on `clone_substitution_key`, and the `clone_fingerprints(hash, passage_id, token_ordinal)` table with an index on `hash` (design.md § FragmentEvidenceStorage).
- [ ] Bump `SCHEMA_VERSION` by one above the value on `main` at branch time and add the migration note (design.md § Versions); update store tests only where they assert the literal version.
- [ ] Add store functions to write fingerprints and token counts in the build transaction, to read a symbol's fingerprints, to look up the passages holding a hash, and to clear both in `clear_derived`.

## Fragment evidence at build

- [ ] Add a winnowing function in [src/graph/clone.rs](../../../src/graph/clone.rs) over placeholder tokens with k = 20 and w = 21, the SHA-256 k-gram hash, and rightmost-minimum selection; return nothing for a sequence shorter than the guaranteed length.
- [ ] Unit tests: two sequences sharing a run of exactly the guaranteed length share a fingerprint at the same diagonal; the same input yields the same fingerprints on repeated calls; a sequence shorter than the guaranteed length yields none.
- [ ] In the build ([src/graph/mod.rs](../../../src/graph/mod.rs)), write each leaf's token count and fingerprints next to its keys.
- [ ] Test: a built fixture holds fingerprints for a long function and none for a container type or a short function.
- [ ] Test: rebuilding the same fixture yields identical `clone_fingerprints` rows; after a leaf's source changes, its rows derive from the new source.
- [ ] Test: a build that fails after the store opens leaves the prior build's fingerprint rows unchanged.

## Similar internals

- [ ] Split `similar_of` ([src/query/search.rs](../../../src/query/search.rs)) into a function that returns the ordered candidate identities with their clone classes and a function that materializes the rows of one page (design.md § BatchSimilar); fetch clone-key matches with the new key index instead of per row.
- [ ] Run the existing `similar` tests unchanged and compare `similar --json` output for ten subjects on the httpx2 and ripgrep dogfood stores before and after the split; record that the outputs are byte-identical.

## Relation labels

- [ ] Add store lookups for a `uses` edge between two symbols in a given direction, a symbol's test rule, a symbol's declaring type, and whether two declaring types are linked by a `type_hierarchy` edge or share an `in_workspace` supertype.
- [ ] Add `relations` to `SimilarItem` and compute it after ordering on the shared materialization path (design.md § RelationLabels, § AnswerShapes); render the labels in the human `similar` output.
- [ ] Tests (single-subject path): a callee carries `used_by_subject`; a caller carries `uses_subject`; a test that calls the subject carries `uses_subject` and `tests_subject` with its rule; sibling Python methods carry `type_family`; a Rust trait default method and an implementing type's method carry `type_family`; an external common base gives no `type_family`; two methods of one class carry no `type_family`; an unrelated row carries an empty set.
- [ ] Test: for a fixture subject, the rows and their order with labels equal the rows and order computed before labels were added.

## Relation exclusion

- [ ] Add the repeatable `--exclude <label>` flag to `SimilarArgs` in [src/cli.rs](../../../src/cli.rs), remove excluded rows before paging, bind the sorted set into the page identity, and emit `excluded_relations` (design.md § RelationExclusion).
- [ ] Tests (single-subject path): an excluded test row is absent and the other rows keep their order; the limit counts remaining rows; `excluded_relations` names the label; a token issued under one exclusion is refused under none.

## Batch similar

- [ ] Add `--subjects-from <path|->` to `SimilarArgs` with its conflicts, read and validate the list at the boundary (unreadable path, invalid UTF-8, `-` on a terminal), and emit the `answers` array and the human headings (design.md § BatchSimilar).
- [ ] Test: three references yield three answers in order.
- [ ] Test: for each of a resolved, an ambiguous, and an unresolved reference, the per-subject JSON equals the single-subject `similar --json` output byte for byte.
- [ ] Test: a truncated per-subject answer's token resumes a single-subject query.
- [ ] Tests: standard input supplies the list; an empty list is typed-empty; a missing path fails with the usage exit code and names the path; `-` with a terminal on standard input is refused.
- [ ] Tests (batch path): rows carry relation labels, and `--exclude` removes labeled rows, exactly as on the single path.

## Mutual nearest pairs

- [ ] Add `--mutual` to `SimilarArgs` with its conflicts, and implement the memoized identity-order walk with the walk-position continuation token (design.md § AllPairsDiscovery).
- [ ] Tests: a mutual pair appears once; a one-sided nearest neighbor is not a pair; an exact-clone pair carries the marker and both label sets; excluding `tests_subject` drops a test and subject pair.
- [ ] Test: paging a fixture with more pairs than the limit to the end yields the unbounded answer.
- [ ] Test: for every pair of a fixture, each member's single-subject `similar` first row is the other member.
- [ ] Test: the mutual answer carries the estimation marker and the semantic-index identity.

## Corpus clone groups

- [ ] Add the `clones` command to [src/cli.rs](../../../src/cli.rs) and [src/commands.rs](../../../src/commands.rs) with an optional reference, `--at`, `--limit`, and `--cursor` (design.md § Surfaces).
- [ ] Implement the grouped key query and group ordering in the query engine (design.md § CorpusCloneGroups).
- [ ] Tests: three exact copies form one exact group; a variant group identifies its exact subset; a unique function is in no group; a long-function group precedes a short-function group; a corpus with no shared key is typed-empty.
- [ ] Test: paging the groups to the end yields the unbounded answer.

## Shared fragments

- [ ] Implement the per-subject answer: whole-symbol clones by key, then fingerprint candidates verified by diagonal extension and consistent-interval scanning over tokens re-spelled from the indexed module text (design.md § FragmentVerification).
- [ ] Unit tests for the consistent-interval scan: a fully consistent run; a run with one merged identifier, split at the conflict; an exact run marked exact; a renamed run marked variant.
- [ ] Tests: a block copied into a long method is found from the shorter function; a renamed block is a variant fragment; an inconsistently renamed block with no consistent run of the guaranteed length is not reported; a shorter shared run is not reported; an exact whole copy has no fragment rows; a docstring difference does not break a run; a Rust fixture reports a shared block.
- [ ] Tests: a container subject yields `not_comparable`; a function with no shared run or key yields typed-empty; `--at` inside a method takes the method; an ambiguous reference yields the candidate set.
- [ ] Test: no reported pair is contained in another pair for the same two symbols, over a fixture with an overlapping repeated block.
- [ ] Test: after a source file changes on disk without a rebuild, the fragment answer still reports runs from the indexed text and is marked stale.

## Clone answer labeling

- [ ] Emit `grade: "token_equivalence"` and `guaranteed_fragment_length` on the group answer, the subject answer, the typed-empty answer, and the `not_comparable` answer, and state token equivalence in the human rendering.
- [ ] Tests: group and subject answers carry the marker and the length and no estimation marker; a typed-empty answer carries the marker; an ambiguous subject carries no marker; a stale answer carries both the staleness flag and the marker.

## Surfaces and versions

- [ ] Bump `SURFACE_VERSION` in [src/manifest.rs](../../../src/manifest.rs) by one above the value on `main` at branch time, and confirm the surface index lists `clones` and the new `similar` flags with their values.
- [ ] In [mcp/src/c10r_mcp/server.py](../../../mcp/src/c10r_mcp/server.py), add the `clones` tool and the `subjects`, `mutual`, and `exclude` parameters on `similar`, writing `subjects` to standard input with `--subjects-from -`; update the server instructions; move the pin in [mcp/src/c10r_mcp/surface.py](../../../mcp/src/c10r_mcp/surface.py).
- [ ] MCP tests: the tool list names `clones`; one `similar` call with three subjects returns three answers in order; `exclude` removes labeled rows; the schema-against-surface tests pass.

## Documentation

- [ ] Add [docs/commands/clones.md](../../../docs/commands/clones.md) covering both forms, the token-equivalence grade, the guaranteed length, and `not_comparable`.
- [ ] Update [docs/commands/similar.md](../../../docs/commands/similar.md) for relation labels, `--exclude`, `--subjects-from`, `--mutual`, the mutual-pair cost, and the docstring rule for clone markers.
- [ ] Add a changelog entry for the schema bump, the changed clone markers, and the new surface.

## Measurements

- [ ] On every dogfood store, measure build time and store size before and after, and the fingerprint row count; record them in design.md § Risks.
- [ ] Measure in-process `similar` time per subject and the full `--mutual` time on the httpx2, Faker, and ripgrep stores, and on a sympy store; replace the cost estimate in design.md § AllPairsDiscovery.
- [ ] Measure the slowest `clones <subject>` queries and the fragment row counts per subject on the Python and Rust stores; record them in design.md § Risks.
- [ ] Re-run the audit repository's five-line case and the renamed-test-with-docstring case; record whether `clones` reports the block and marks the tests as variant clones, as the gate for the guaranteed length.

## Close-out

- [ ] Run the full Rust and MCP test suites and every pre-commit hook on the changed files, and report the results with counts.
