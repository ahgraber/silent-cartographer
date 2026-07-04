# Design: calibration-hardening

## Context

First live dogfood of the v1 spine (2026-07-02: aligned 6226 / text-mismatch 748 / semantic-only 0 / syntax-only 4) exposed three calibration gaps this change closes before the blast-radius change contracts the dependency edges.
Constraints inherited from the north star and brainstorm: **calibration over coverage** (ambiguity is surfaced, never silently resolved), **safety by structure** (the bounded, trustworthy shape is the default; the full firehose is an explicit reach), and the §11 decision that each worktree/checkout is its own addressable codebase.

## Decisions

### Decision: Definition-anchored duplicate ranking

**Chosen:** A collision group of identical resolved descriptors is ranked by each member's definition location — document path first, then range start — and the existing `#rank` suffix is assigned in that order.
Members lacking a definition occurrence sort after those with one, ordered by their first occurrence location, so the total order is deterministic for every input shape.

**Rationale:** Definition location is stable under any discovery order, meaningful to a human reading the identity, and cheap (the definition occurrence is already in hand at projection time).

**Alternatives considered:**

- Input-index tiebreak (shipped behavior): rejected — discovery-order-dependent, so twins can swap identities between runs.
- Hash of definition contents: rejected — opaque in the identity string and unstable under any edit to the definition.

### Decision: Duplicate-ambiguous as a first-class join outcome

**Chosen:** After projection, descriptors with more than one distinct definition are known; each _definition_ occurrence attaches to the twin at its own location (co-location), and every _non-definition_ occurrence of a duplicated descriptor is recorded with a new `duplicate-ambiguous` join outcome — counted in the accounting, persisted in the discrepancy detail, attributed to no twin.

**Rationale:** SCIP data genuinely cannot say which twin a reference means; picking one is the silent misattribution the calibration principle forbids, and a typed outcome keeps the upstream defect measurable per build (its count is expected to move with rust-analyzer versions, which provenance already tracks).

**Alternatives considered:**

- Attribute all references to the first-ranked twin: rejected — confident misattribution, poisoning future `calls` edges.
- Drop references of duplicated descriptors: rejected — they vanish from the conservation law and the defect becomes invisible.

### Decision: Discrepancy persistence and the grouped bounded listing

**Chosen:** A per-build `join_discrepancies` table (document path, span, outcome kind, expected name token, found source text truncated to **120 bytes**), superseded wholesale by each build via an atomic rewrite ordered **before** the index-metadata publish, so the metadata commit is the final write of a build and a crash mid-build leaves the prior metadata authoritative.
A span whose coordinates cannot be normalized onto the source is persisted as **typed absence**, never a fabricated `(0,0)` location (adjudicated at verify, 2026-07-02).
The default CLI listing (`status --discrepancies`) is a grouped summary: GROUP BY (outcome kind, expected token), ordered by descending count, each group carrying its count, distinct-document count, and one exemplar location; capped at **50 groups** with a truncation marker whose totals are computed from the full persisted set, never the displayed subset.
`--all` streams the raw rows.
Both constants are provisional design values, to be superseded when the interface-layer pagination change defines the real bounded-output model.

**Rationale:** Grouping over already-persisted fields makes the dogfood categories (whole-file module spans, derive fallout) emerge without the deferred semantic classifiers, and full-set aggregation keeps a truncated diagnostic from lying about the aggregate picture.

**Alternatives considered:**

- Unbounded raw listing by default: rejected — violates the bounded-output principle; an agent calling it on a large repo floods its own context.
- Stratified/diversity row sampling: rejected — more machinery than grouping for no additional gist.
- Semantic categorization (module-span vs. derive-fallout classifiers): rejected here — explicitly deferred by the proposal; grouping is its substrate.

### Decision: Workspace default derived from the root directory name

**Chosen:** The default workspace identity is the final path component of the canonicalized workspace root (symlinks, `.` segments, and trailing separators resolved).
If canonicalization leaves no name component (e.g. the filesystem root), the build refuses with a teaching error directing the user to `--workspace` rather than silently sharing a namespace.
An explicit `--workspace` is always used verbatim.

**Rationale:** A pure function of the root path — no environment probing — so the default can never shift when unrelated configuration (e.g. a git remote) changes; portable across clones and machines, which is what agents persisting identities actually exercise; collisions between same-named roots are policed at the future multi-workspace registry, the surface that owns them (brainstorm §11, including the two-level remote-kinship design input).

**Alternatives considered:**

- Root name + short path-hash: rejected — unique per machine but identities differ between laptop, teammate clones, and CI for the same codebase.
- Full normalized path: rejected — same portability failure, and it bloats every identity string.
- Git-remote-derived with filesystem fallback: rejected for the _default_ — merges all clones and worktrees into one namespace (inverting the §11 per-worktree decision), and makes the default a function of environment state (adding a remote would silently orphan the graph); preserved as a registry-level design input instead.

### Decision: Schema-version guard at store open

**Chosen:** Every store stamps `PRAGMA user_version` with the schema version at creation; opening a store first compares that stamp — readable regardless of table shapes — against the binary's version.
On mismatch, the write path (`build`) deletes and recreates the store: the index is derived, replayable data, so rebuild _is_ the migration (pre-first-MINOR governance).
Read paths (`get` / `trace` / `status`) refuse with a teaching error naming both versions and the recovery action.
A pre-guard store carries no stamp, reads as version 0, and is treated as mismatched.
The in-row `schema_version` column remains as provenance.

**Rationale:** Version metadata stored inside a table can be unreadable exactly when it matters most — when the table shape changed; the PRAGMA survives any schema change.
Splitting write/read behavior keeps `build` self-healing without making read paths destructive.
Added 2026-07-02 after the live rebuild hit a raw `no column named duplicate_ambiguous_count` failure against a v1 store.

**Alternatives considered:**

- Refuse on build too: rejected — forces a manual delete of derived data; worse recovery than the story's "one rebuild away from fixed."
- Silent in-place column migration: rejected — migration plans are owed only after the first MINOR release, and ALTER-based patching invites drift between created and migrated stores.

### Decision: Typed alignment rules over guard loosening

**Chosen:** The join dispatches every occurrence to a **named alignment rule**; an attribution persists if and only if one rule's exact expectation is satisfied, and the accepting rule is stored as provenance on the attribution (the name-token default included).
The rules: **exact** (name-token equality, the default); **crate-root** (descriptor terminal `crate` accepts the descriptor's own package name — it is present in the SCIP symbol — or the `crate` keyword); **operator-desugar** (a closed method→construct correspondence, below); **module-span** (a module *definition* whose range spans its whole document); **self-keyword** (added 2026-07-04, second dogfood: ~50 refusals with found text `Self`).
The self-keyword rule accepts a reference occurrence resolving to a type at a `Self` keyword token — in type position or as a `Self::` path segment — only when the base name of the nearest enclosing `impl`'s self type equals the occurrence's expected base name, with generic arguments and path qualifiers stripped from both (`Answer<T>` and `module::Answer` both compare as `Answer`; qualifier stripping is required because the expected name is a terminal descriptor segment while an impl header may spell a qualified path).
The enclosure cross-check is what keeps this rule exact: token presence alone would accept coordinate drift that happens to land on any `Self` in the file, and the impl header is sitting in the tree we already hold.
Lowercase `self` is excluded (it resolves to a receiver local, and locals are excluded from the persisted base); `Self` inside a trait body has no impl to cross-check against and stays a typed refusal (negligible count).
Live finding (third dogfood, 2026-07-04): rust-analyzer resolves `Self` to the **impl symbol** (descriptor path carries an `impl` segment; the adapter maps its terminal to kind `other`), not the plain type — so the rule's target gate is type-or-implementation, and the impl-header base-name cross-check, which carries the rule's exactness, is unchanged.
The operator rule matches the **syntax-tree construct at the occurrence location**, not the raw bytes: rust-analyzer emits operator spans that can sit adjacent to the sigil (observed live: single-byte spans on whitespace beside `==` and `+`), so byte-equality against the sigil would false-refuse while node-kind matching stays exact.

The closed correspondence (extending it is a design amendment plus tests, never an implementation convenience), read **per-method sigil-exact**: `eq` accepts only a `==` construct, never another comparison sigil (adjudicated at verify, 2026-07-04):

| Methods | Construct |
| --- | --- |
| `branch` | try expression (`?`) |
| `eq`, `ne` | `==`, `!=` |
| `lt`, `le`, `gt`, `ge` | `<`, `<=`, `>`, `>=` |
| `add`, `sub`, `mul`, `div`, `rem` (+ `*_assign`) | binary arithmetic (+ compound assign) |
| `bitand`, `bitor`, `bitxor`, `shl`, `shr` (+ `*_assign`) | bitwise / shift (+ compound assign) |
| `neg`, `not` | unary `-`, `!` |
| `index`, `index_mut` | index expression |
| `deref`, `deref_mut` | explicit unary `*` only — autoderef at `.` is deliberately excluded (ambiguous site) |
| `call`, `call_mut`, `call_once` | call expression on a callable value |
| `into_iter`, `next` | `for` expression, only when the occurrence lands on the loop construct |

**Rationale:** Every acceptance remains an exact, deterministic check against a *kind-correct* expectation, so the drift detector's integrity survives; per-rule accounting buckets make any future regression show up as one named bucket moving; and the alternative — leaving these refused — makes `references` (and proposal 2's `callers`) silently under-report, which is the false-negative calibration violation the north star forbids.

**Alternatives considered:**

- Loosen or fuzzify the text-equality guard: rejected — a guard that tolerates mismatch is a dead drift detector (the miscalibrated-guard risk from the first change, realized deliberately).
- Leave the categories refused and document them: rejected — honest at the join layer, a lie at the query layer.
- Attribute macro-expansion fallout to invocation sites (`via-expansion`) now: rejected here — it is its own design decision with different failure modes; the derive residual stays a typed refusal this change.
- Self-keyword acceptance on token presence alone: rejected — forfeits the impl-header cross-check that is freely available from the syntax tree, weakening the rule's exactness for no gain.

## Architecture

```text
 project_all (ranked collision groups: def path → def range → fallback)
      │  duplicated descriptors known here
      ▼
 join_guarded ──► accept by rule: exact · crate-root · operator-desugar · module-span
                  refuse as: text-mismatch · semantic-only · duplicate-ambiguous   (+ syntax-only)
      │                                   │
      ▼                                   ▼
 store: metadata counts (4-way conservation)   join_discrepancies (kind, expected, found≤120B, location)
      │                                   │        atomic rewrite, then metadata publish last
      ▼                                   ▼
 status ── counts ── `--discrepancies` grouped summary (≤50 groups, full-set totals) ── `--all` raw rows
```

## Risks

- **Twin identities shift once on upgrade** — existing indexes disambiguated by discovery order re-rank on the next build.
  Pre-1.0 and pre-first-MINOR, so no migration plan is owed (governance rule); the rebuild is the migration, and staleness self-heals.
- **Grouped summary can conflate distinct root causes sharing an expected token** — mitigated by the per-group exemplar location and `--all`; semantic classification remains available as a later layer if reading proves insufficient.
- **Truncated `found` text (120 bytes) could clip evidence** — name tokens are far shorter; the equality check that classifies the outcome runs on full bytes before truncation, so classification is unaffected.

## Verification Waivers

None — every SHALL maps to runnable evidence (see `tasks.md`).
