# Proposal: store-ownership-guard

> **Status: DRAFT — scope ratified, specs & design generated.**
> Created 2026-08-01 from an is-tested review finding, probe-confirmed but out of that change's scope.
> Scope ratified 2026-08-02 in review discussion; delta specs and `design.md` generated 2026-08-02; wrong-index disclosure folded in 2026-08-02; `tasks.md` deliberately not generated yet.

## Intent

`c10r build` treats `PRAGMA user_version` as proof that the database at the `--db` path belongs to c10r: a version mismatch deletes the file wholesale, and a foreign database that happens to be stamped with the current version silently receives c10r's tables.
Probe (2026-08-01): pointing `--db` at a foreign SQLite file with its own data and running `build` destroyed that data — the replace path is real, not theoretical.
The exposure is not build-only: `GraphStore::open` — the query path — writes the schema SQL into any version-matching database, auto-creates a fresh store file when the path does not exist, and `cache` deletes any SQLite database carrying a nonzero `user_version`, which describes most versioned SQLite files in the wild, not c10r's alone.
The behavior is currently _contracted_: the baseline code-graph requirement "Incompatible index stores are replaced or refused, never half-used" says a build over a version-mismatched store SHALL replace it and succeed, so fixing this is a baseline spec revision, not a patch.
Each schema migration entrenches `user_version` further as both schema version and ownership proof; the cost of separating them grows with every release.
A sibling gap rides the same identity work: the store records no durable link to the project it describes, so a query pointed at another project's genuine c10r index answers from the wrong graph — the freshness gate mislabels it "stale" when the truth is "different workspace" — and nothing tells the user which project a store belongs to.

## Prior art in-repo

The `cache` command already refuses to delete a file that fails an ownership check (SQLite magic bytes + nonzero `user_version` at header bytes 60–63) and names the manual `rm` instead — the project has once before decided that destructive operations must recognize their own artifacts.
`build` never got the same discipline because replace-on-mismatch _is_ its migration story.
The `cache` recognizer is itself too weak to survive this change: once build and queries refuse foreign databases, "nonzero `user_version`" would leave `cache` as the one command still willing to delete them, so it upgrades to the shared recognizer.

## User Stories

### Story: own-store-only

As a user pointing c10r at a database path, I want every c10r command to refuse to touch a database c10r did not create, so that a mistyped `--db`, a symlinked `.c10r` directory, or a path collision can never destroy or pollute unrelated data.

Laddering: not touching data that is not yours is the floor for any application, not a feature that earns its own north-star outcome; misbehavior here betrays trust wholesale.
Recorded as a floor guarantee that strengthens (supersets) the **Calibrated trust** outcome.

### Story: wrong-index-disclosed

As a user querying an index, I want every answer to disclose when the store describes a different workspace than the one I am querying, so that a confidently-wrong graph is never presented as merely stale.

Laddering: directly to the **Calibrated trust** outcome — every answer carries its provenance, and a wrong-workspace answer is the confident wrong answer the north star says the product dies by.

## Scope

**In scope:**

- A stable ownership marker written at store creation, checked before any replace, table write, or delete.
- One shared recognizer with three consumers: build's replace path, every store open (all queries and `status`), and `cache`'s delete decision.
- A cross-cutting baseline requirement stating the guarantee once as a property of the application — no c10r command writes to or deletes a file it does not recognize as its own store — so a future command that touches the store inherits the obligation automatically.
- Revised replace-or-refuse: replace applies to _recognized c10r stores_ at any version; an unrecognized database refuses with a teaching error, for build and query alike.
- No auto-create on query: a query or `status` against a path where no file exists refuses with instructions to run `c10r build`, creating nothing.
- Legacy handling: pre-guard stores are refused with rebuild guidance.
- Workspace identity recorded in the store at build; answers derived from a store recorded for a different workspace carry a workspace-mismatch marker, distinct from and composing with staleness; an unevaluable comparison is disclosed as unknown, never presented as matched; a build over a recognized current-version store recorded for a different workspace discloses both identities and proceeds.
- Ownership refusals exit with their own distinct code (contracted in the exit-code taxonomy), and `manifest`'s index state distinguishes an unrecognized file at the index path from an absent index without failing.
- Store schema version bump (v13) as the artifact-change signal, covering both the ownership marker and the recorded workspace identity in one migration.
- Regression tests covering every refusal arm with data-intact assertions, and every workspace-mismatch disclosure arm.

**Out of scope:**

- **Wrong-workspace refusal.**
  The mismatch marker labels; it never refuses.
  The only workspace identity recordable today is weak against legitimate change (repos move, containers remount paths), so a hard refusal would false-positive on the user's own index; a rebuild in place re-records the identity and clears the marker.
- **Path canonicalization.**
  Ownership recognition satisfies the story as written: a symlink redirecting `--db` to a foreign database now refuses.
  A symlink pointing at a legitimate c10r store elsewhere is replaced — destroying a replayable index c10r owns, which is contracted behavior for recognized stores.
  Canonicalization would serve a different story ("writes land where I pointed, not where a link redirects") that no one has told.

## Approach

- **Ownership marker:** `PRAGMA application_id` — the SQLite-idiomatic slot at header bytes 68–71 — stamped at store creation alongside `user_version`, separating "whose file is this" from "which schema does it carry".
- **Recognition without contact:** the recognizer reads the file header directly (SQLite magic bytes + c10r's `application_id`) before any SQLite connection opens, so recognition itself never writes, locks, or creates sidecar files next to a foreign database.
- **Refusal message shape:** conditional — "if this is an old c10r index, delete it and rebuild; if it is not yours, fix `--db`" — never a bare delete instruction, because a refused file may be data the user must not delete.
- **Workspace identity:** the store records the canonicalized workspace root at build (one metadata column, riding the same v13 bump); queries compare it against the root they are invoked with — an input every query command already carries — and a difference sets the mismatch marker in the answer envelope and human render.
  The existing `workspace_id` (a directory name) stays as display identity only; it is too weak for comparison.
- **Legacy:** pre-guard stores (nonzero `user_version`, no `application_id`) are refused, not recognized by schema shape or version range. c10r is greenfield — the only existing stores belong to this project — and the index is replayable, so one manual rebuild per store is acceptable.
  A `user_version`-range heuristic is rejected outright: small integers are exactly what other applications' migration counters look like.
- **Regression surface:** foreign database refused by build with data intact; foreign database stamped at the current schema version refused by build and by query with data intact; non-SQLite file at the `--db` path refused with the typed error, not a raw storage error; `cache` refuses a foreign versioned SQLite database; query against a missing file refuses with build guidance and creates nothing; symlinked parent directory does not let a replace destroy unrecognized data; a recognized current-version store builds and queries normally; a query from a different workspace root carries the mismatch marker (composing with staleness, not replacing it); a matching root carries none; an unevaluable comparison is disclosed as unknown; ownership refusal exits with its distinct code; `manifest` reports an unrecognized file distinctly and still succeeds.

## Resolved questions

1. ~~Is `application_id` alone sufficient, or does the header-bytes check `cache` uses belong in a shared recognizer?~~
   Resolved: one shared recognizer — magic bytes + `application_id`, header-readable — with build, open, and cache as its three consumers.
2. ~~What do pre-`application_id` c10r stores get?~~
   Resolved: refusal with rebuild guidance; greenfield makes this a non-issue.
3. ~~Does the symlink concern warrant path canonicalization?~~
   Resolved: no — ownership recognition alone covers the story; canonicalization is out of scope.
