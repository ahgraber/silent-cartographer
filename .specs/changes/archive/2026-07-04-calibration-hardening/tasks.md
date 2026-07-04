# Tasks: calibration-hardening

## symbol-identity

- [x] Rank duplicate-descriptor collision groups by definition location (document path, then range start), with the deterministic fallback order for members lacking a definition occurrence.
- [x] Derive the default workspace identity from the canonicalized workspace root's directory name; keep explicit `--workspace` verbatim; refuse with a teaching error when the root has no name component.
- [x] Test (definition-anchored): two definitions sharing a descriptor receive distinct identities ordered by their definition locations. _(Identity uniqueness within a workspace — duplicate branch)_
- [x] Test (order stability): indexing the same sources with file order reversed yields the same identity for each duplicated definition. _(Identity uniqueness within a workspace — discovery-order branch)_
- [x] Test (default derived): a build with no workspace supplied namespaces every identity by the root's directory name. _(Derived default workspace identity)_
- [x] Test (default deterministic): two builds of the same root derive identical workspace identities. _(Derived default workspace identity)_
- [x] Test (override wins): a build with an explicit `--workspace` uses it verbatim, not the derived default. _(Derived default workspace identity — supplied branch)_

## code-graph

- [x] Add the `duplicate-ambiguous` join outcome: detect duplicated descriptors from the ranked collision groups; attach definition occurrences by co-location; route non-definition occurrences of duplicated descriptors to the new outcome.
- [x] Extend the join accounting and index metadata with the duplicate-ambiguous count; extend the conservation identity to the four semantic-side buckets.
- [x] Define the `join_discrepancies` table (document path, span, outcome kind, expected name token, found source text truncated to the design bound) and rewrite it in the same transaction as ingest.
- [x] Persist a discrepancy row for every text-mismatch, semantic-only, and duplicate-ambiguous outcome during ingest.
- [x] Test (definition co-location): each definition occurrence of a duplicated descriptor attaches to the definition at its own location. _(Occurrences of duplicated descriptors are never arbitrarily attributed — definition branch)_
- [x] Test (reference ambiguous): a reference occurrence of a duplicated descriptor is recorded duplicate-ambiguous and attributed to no twin. _(Occurrences of duplicated descriptors are never arbitrarily attributed — reference branch)_
- [x] Test (unduplicated unaffected): references of a single-definition descriptor attribute through the ordinary guarded join and are never marked duplicate-ambiguous. _(Occurrences of duplicated descriptors are never arbitrarily attributed — unduplicated branch)_
- [x] Test (four counts recorded): a build records the duplicate-ambiguous count alongside the other three semantic-side counts and syntax-only. _(Join alignment accounting)_
- [x] Test (conservation with four buckets): with a fixture producing non-zero counts in all four semantic-side buckets, their sum equals the total occurrences processed. _(Join alignment accounting — conservation)_
- [x] Test (detail persisted): a text-mismatch occurrence persists location, kind, expected name, and found source text, retrievable after the build. _(Join discrepancies are inspectable)_
- [x] Test (detail superseded): discrepancy rows from a prior build are absent after a new build of changed sources. _(Join discrepancies are inspectable — supersession branch)_
- [x] Persist the discrepancy span as typed absence when the occurrence's coordinates cannot be normalized, and render it as such in both listings; never present a fabricated `(0,0)` location. _(Join discrepancies are inspectable — unnormalizable branch; verify remediation 2026-07-02)_
- [x] Test (span typed absence): an unnormalizable occurrence's discrepancy row shows typed span absence, and a duplicate-ambiguous row carries its real normalized span. _(Join discrepancies are inspectable — unnormalizable branch; verify remediation 2026-07-02)_
- [x] Order the discrepancy rewrite before the index-metadata publish so the metadata commit is the final write of a build. _(design.md Decision 3 coherence; verify remediation 2026-07-02)_
- [x] Test (found-text boundary): a mismatch whose found text exceeds the persistence bound is classified on full bytes and truncated on a UTF-8 boundary. _(Join discrepancies are inspectable — truncation bound; verify suggestion 2026-07-02)_
- [x] Stamp `PRAGMA user_version` at store creation and validate it at open: `build` replaces a mismatched store; queries refuse with a teaching error naming both versions and the recovery action; an unstamped store reads as version 0 and counts as mismatched. _(Incompatible index stores are replaced or refused; dogfood 2026-07-02)_
- [x] Test (build replaces): a build over a store stamped with a different schema version succeeds and leaves a store carrying the current version. _(Incompatible index stores are replaced or refused — build branch)_
- [x] Test (query refuses): `status` or a query against a mismatched store returns the typed teaching error naming both versions, never a storage-level error. _(Incompatible index stores are replaced or refused — query branch)_
- [x] Test (matching passes): a current-version store opens and operates normally under every command. _(Incompatible index stores are replaced or refused — matching branch)_
- [x] Implement rule dispatch in the join: every acceptance is a named alignment rule with name-token equality as the default rule, and each persisted attribution carries its accepting rule as provenance. _(Guarded positional join; amendment 2026-07-02)_
- [x] Implement the crate-root rule: a crate-root descriptor occurrence accepts the descriptor's own package name or the `crate` keyword. _(Guarded positional join — crate-root branch)_
- [x] Implement the operator-desugar rule over the closed correspondence in design.md, matching the syntax-tree construct at the occurrence location (operator spans may sit adjacent to the sigil). _(Guarded positional join — operator branch)_
- [x] Implement the module-span rule: a module definition occurrence whose range spans its whole document. _(Guarded positional join — module-span branch)_
- [x] Extend the accounting to per-rule acceptance buckets and update the conservation identity: rule buckets plus refusal buckets equal the total. _(Join alignment accounting)_
- [x] Test (crate-root package name): a use-site reference to an external crate root aligns under the crate-root rule. _(Guarded positional join — crate-root branch)_
- [x] Test (crate keyword): a `crate::` path segment reference aligns under the crate-root rule. _(Guarded positional join — crate-root branch)_
- [x] Test (operator accepted): a try-expression occurrence for `branch` aligns under the operator-desugar rule. _(Guarded positional join — operator branch)_
- [x] Test (operator span adjacency): a binary-operator occurrence whose span sits beside the sigil still aligns via the syntax-tree construct. _(Guarded positional join — operator branch)_
- [x] Test (outside correspondence refused): a method occurrence not in the correspondence, failing name-token equality, stays refused. _(Guarded positional join — refusal branch)_
- [x] Test (module span accepted): a module definition spanning its whole document aligns under the module-span rule. _(Guarded positional join — module-span branch)_
- [x] Test (non-module whole span refused): a non-module occurrence spanning a whole document is not accepted by the module-span rule. _(Guarded positional join — module-span negative branch)_
- [x] Test (rule provenance): attributions accepted under the default rule and under a kind-scoped rule each carry their rule tag. _(Guarded positional join — provenance)_
- [x] Implement the self-keyword rule: a type reference at a `Self` keyword token (type position or `Self::` path segment) accepts only when the nearest enclosing impl's self-type base name equals the expected base name, generic arguments stripped from both. _(Guarded positional join — self-keyword branch; amendment 2026-07-04)_
- [x] Extend the per-rule accounting, status rendering, and the all-buckets conservation fixture with the self-keyword bucket. _(Join alignment accounting; amendment 2026-07-04)_
- [x] Test (Self in own impl accepted): a `Self` return-type reference inside an impl aligns under the self-keyword rule with its tag. _(Guarded positional join — self-keyword branch)_
- [x] Test (Self path segment accepted): the `Self` segment of a `Self::method(...)` call inside an impl aligns under the self-keyword rule. _(Guarded positional join — self-keyword branch)_
- [x] Test (generic base-name comparison): an expected name carrying generic arguments aligns at a `Self` token via base-name comparison. _(Guarded positional join — self-keyword generic branch)_
- [x] Test (foreign impl refused): a `Self` token whose enclosing impl names a different base type stays refused. _(Guarded positional join — self-keyword negative branch)_
- [x] Widen the self-keyword rule's target gate to type-or-implementation: accept symbols rust-analyzer resolves `Self` to — the impl symbol (kind `other`, descriptor carrying an `impl` path segment) as well as plain types. _(Guarded positional join — self-keyword branch; live fix 2026-07-04)_
- [x] Test (impl-symbol resolution accepted): a `Self` occurrence resolving to an impl-shaped symbol (terminal named for the type, non-Type kind, `impl` path segment) aligns under the self-keyword rule — the live rust-analyzer shape. _(Guarded positional join — self-keyword branch; live fix 2026-07-04)_
- [x] Test (operator families): one acceptance test per remaining construct family in the desugar correspondence — eq/ne (`==`), ordered comparison (`<` family), compound assign (`+=`), bitwise/shift (`&`, `<<`), unary (`-`, `!`), index (`[`), explicit deref (`*`), call expression, and for-loop (`into_iter`/`next`) — each pinning its family's construct match so a node-kind regression in any one family is caught. _(Guarded positional join — operator branch, per-family partitions; verify finding 2026-07-04)_
- [x] Test (qualifier stripping): a `Self` occurrence inside an impl whose header spells a path-qualified self type (`impl module::Type`) aligns via the base-name comparison. _(Guarded positional join — self-keyword branch, qualifier arm; design.md base_type_name; verify finding 2026-07-04)_
- [x] Test (references include rule-aligned): `trace` over `references` returns a site aligned under the operator-desugar rule. _(Guarded positional join — query-surface consequence; the silent under-reporting fix)_
- [x] Test (per-rule counts conserve): per-rule acceptance buckets and refusal buckets are recorded and sum to the total semantic occurrences. _(Join alignment accounting — conservation)_

## CLI surface

- [x] Implement the grouped bounded discrepancy summary on `status` (group by outcome kind and expected token, descending count, exemplar location per group, group cap with a truncation marker computed from full-set totals) and the explicit full listing switch.
- [x] Test (summary gists the full set): with more groups than the cap, the listing states it is truncated and its totals reflect every persisted discrepancy, not only displayed groups. _(Join discrepancies are inspectable — truncation branch)_
- [x] Test (full listing): the explicit switch returns every persisted discrepancy row. _(Join discrepancies are inspectable — full-listing branch)_
- [x] Test (summary shape): each displayed group carries outcome kind, expected token, count, and an exemplar location. _(Join discrepancies are inspectable — CLI surface)_
- [x] Render the per-rule acceptance buckets in `status` alongside the refusal counts. _(Join alignment accounting — CLI surface; amendment 2026-07-02)_
- [x] Test (status rule buckets): `status` reports each alignment rule's acceptance count alongside the refusal counts. _(Join alignment accounting — CLI surface)_
