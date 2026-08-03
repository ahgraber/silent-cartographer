# Design: store-ownership-guard

## Context

The store is a single SQLite file whose `PRAGMA user_version` currently serves two roles at once: schema version and, implicitly, proof that the file belongs to c10r.
The second role is unsound — `user_version` is the slot nearly every application uses for its own migration counter — and it is what lets `build` delete foreign data and lets queries write schema SQL into version-coincident foreign databases.
A sibling identity gap rides along: the store records no durable link to the project it describes (`index_metadata.workspace_id` is only the root's directory name, and nothing compares it), so a query pointed at another project's genuine c10r index answers from the wrong graph, and the freshness gate mislabels the situation "stale" when the truth is "different workspace".
Constraints shaping the design: replace-on-mismatch remains the migration story for recognized stores (the index is derived, replayable data); the exit-code taxonomy is closed with one distinct code per category; c10r is greenfield, so no shipped store carries the new marker yet; and the store-touching surface is narrow — every file contact funnels through `GraphStore::open`, `GraphStore::open_or_replace`, and `cache`'s delete decision.
The threat model is accidental misdirection — a mistyped `--db`, a symlinked directory, a stale path — not an adversary racing file swaps against the process; check-then-act windows between recognition and a destructive step are accepted under that model, with the check placed immediately before the step it licenses.
A store comprises the main database file plus its SQLite journal sidecars (`-wal`, `-shm`): recognition of the main file extends to its sidecars, which is what licenses the replace path's sidecar sweep without contradicting the prohibition on deleting unrecognized files.

## Decisions

### Decision: Ownership marker is `PRAGMA application_id` with the ASCII value "c10r"

**Chosen:** stamp `PRAGMA application_id = 0x63313072` (the bytes of the ASCII string `c10r`) at store creation, alongside `user_version`.

**Rationale:** `application_id` is the slot SQLite documents for exactly this purpose — identifying which application a database belongs to.
It lives at header bytes 68–71, so it is readable without opening the database, and it is orthogonal to `user_version`, so every future schema migration leaves ownership proof untouched.
The ASCII-of-project-name value is self-describing in a hex dump.
Checked 2026-08-02 against SQLite's registry of application ids (`magic.txt` in the SQLite repository): `0x63313072` collides with no registered identifier.
The value is a permanent file-format commitment; changing it later would orphan every stamped store.

**Alternatives considered:**

- A marker table inside the schema: requires opening the database to check, adds a table the schema does not otherwise need, and cannot be read without SQLite contact.
- Path convention (only touch files named `index.db` under `.c10r/`): does not survive `--db` overrides, which are exactly the mistyped-path scenario the story protects against.
- `user_version` range heuristic: rejected in the proposal — small integers are other applications' migration counters.

### Decision: Recognition is a pure header read before any SQLite open

**Chosen:** one shared recognizer that reads the first 100 bytes of the file with plain file I/O and requires both the 16-byte magic `SQLite format 3\0` at offset 0 and c10r's `application_id` (big-endian `u32` at offset 68).
Its three consumers are build's replace path, every store open (all queries and `status`), and `cache`'s delete decision — `cache` drops its nonzero-`user_version` test for the shared check.
Any file that fails the read or the check — too short, zero-length, wrong magic, wrong or absent `application_id` — is unrecognized, with one exception: a path where no file exists is "absent", a distinct outcome from "unrecognized" (absent lets build create; unrecognized always refuses).

**Rationale:** opening a foreign database through SQLite is itself contact — it can take locks and, under WAL, materialize `-wal`/`-shm` sidecar files next to data c10r does not own.
A plain header read touches nothing, and it is the technique `cache` already uses, so the recognizer unifies the two existing check styles instead of adding a third.

**Alternatives considered:**

- `PRAGMA application_id` after `Connection::open`: side effects on foreign files (locks, potential sidecars) before the decision to refuse is made.
- Schema-shape probing (table names): requires opening the database, and couples recognition to the very schema that migrations change.

### Decision: Store creation goes through a temp file and atomic rename

**Chosen:** `create` builds the new store in a temporary file in the same directory (schema SQL, `application_id`, `user_version`, committed), then renames it onto the final path.
The temp file is created exclusively (`create_new`), so a stale leftover from an earlier crash is never silently reused.
The replace path becomes: recognize → remove old store files → create-via-rename.

**Rationale:** direct creation at the final path has a crash window in which a partially-initialized, unstamped file sits at the store path; under strict recognition, the next build would refuse c10r's own crash leftover and demand a manual `rm` — precisely the out-of-band forced error this change exists to eliminate.
With rename, the store path only ever holds complete, stamped stores or nothing.

**Alternatives considered:**

- Direct creation with pragmas stamped first: shrinks the window but cannot close it, and a crash still strands an unrecognized file at the store path.
- Treating zero-length files as absent: papers over only one shape of leftover and weakens the strict refusal rule ratified in the proposal.

A crash before rename strands only a distinctly-named temp file (e.g. `index.db.c10r-tmp`), which never blocks later builds; it is inert litter, not a refusal trigger.

### Decision: Query connections open read-only

**Chosen:** every read path — queries, `status`, and `manifest`'s index-state probe — opens the store with SQLite's read-only open flag; the schema DDL currently executed on every open moves to the create path only.

**Rationale:** a recognized, current-version store is complete by construction once creation is atomic, so open-time DDL is dead weight — and it is precisely the write that turns a query against a version-coincident foreign database destructive today.
A read-only connection makes "read operations write nothing" a property of the connection itself, enforced by SQLite, rather than a discipline the recognizer alone upholds.

**Alternatives considered:**

- Keep `CREATE TABLE IF NOT EXISTS` on open as belt-and-braces: rejected — the braces are the hazard; any future path that reaches open-time DDL against the wrong file re-creates the original defect.

### Decision: Legacy pre-guard stores are refused, not recognized

**Chosen:** stores with a nonzero `user_version` but no `application_id` — every store c10r shipped before this change — are unrecognized and refused with the conditional message's rebuild branch.

**Rationale:** ratified in the proposal. c10r is greenfield (the only existing stores belong to this project), the index is replayable, and any recognizer generous enough to accept unmarked stores is generous enough to accept foreign databases.

**Alternatives considered:**

- Recognition by schema shape: bounded value (one upgrade cycle) against permanent recognizer complexity.
- Version-range recognition: unsafe, per the marker decision above.

### Decision: Ownership refusal gets its own exit code

**Chosen:** a new distinct exit code `UnrecognizedStore = 6`, alongside the existing taxonomy (0 success, 1 generic failure, 2 usage, 3 no-index, 4 incompatible-store, 5 indexer-setup).
The refusal message is the conditional two-branch text from the proposal: rebuild recovery for a genuine old index, path correction for an unrelated file — never an unconditional delete instruction.
`cache`'s refusal keeps its existing shape (name the manual `rm`): its caller has already expressed the intent to delete, so naming the manual step is informed consent, not a footgun.

**Rationale:** the taxonomy exists so agents can branch on outcomes without parsing prose.
Code 4 means "rebuild fixes this"; an ownership refusal means the opposite — "do not rebuild here, check your path" — and conflating them invites scripted rebuilds (or deletions) against foreign data.
Exit codes are published protocol external callers depend on, so the new category is contracted: this change's command-surface delta modifies the Exit-code taxonomy requirement to name both store-related categories (the incompatible-store category rides along, retro-naming what code 4 already implements).
The missing-file case on the query path maps to the existing no-index code 3 — unchanged externally; the only behavioral change there is that nothing is created.

**Alternatives considered:**

- Reuse `IncompatibleStore = 4`: conflates "rebuild fixes it" with "rebuild would destroy it" — the one distinction an automated caller must not miss.
- Generic failure 1: erases the category entirely.

### Decision: Workspace identity is the canonicalized root path, compared at query time, label-never-refuse

**Chosen:** build records the canonicalized workspace root in `index_metadata` (one new column); every query command — all of which already receive both `--db` and the root — canonicalizes its root and compares.
A difference sets a workspace-mismatch field in the answer envelope and a corresponding human-render line; it never refuses, and it composes with (never replaces) the staleness flag.
Build over a recognized store recorded for a different root prints a disclosure naming both roots, proceeds, and re-records.
The existing `workspace_id` stays as display identity only.

**Rationale:** the wrong-project case degrades answer honesty, not data safety — the store is c10r's own, replayable artifact — so the calibration principle applies: disclose, don't block.
A canonical path is a strong signal of "different project" but a weak proof (repos legitimately move; containers and bind mounts remount the same project at new paths), so a hard refusal would false-positive against the user's own index; a rebuild in place re-records the identity and clears the marker.
When the comparison cannot be evaluated — in practice only when canonicalizing the query root fails, since every recognized post-v13 store records its identity at creation — the answer discloses the workspace relationship as unknown: typed absence, never an implied match, per the north-star rule that absence is typed.
The build handoff disclosure reads the old store's metadata, which is guaranteed readable only at the current schema version; over a version-mismatched recognized store the replace proceeds with best-effort disclosure, and the spec's disclosure guarantee is scoped to current-version stores.

**Alternatives considered:**

- Compare `workspace_id` (directory name): too weak in both directions — two repos named `backend` collide, and a renamed directory false-positives.
- Refuse on mismatch: punishes moved repos and containerized checkouts; the answer-honesty problem needs a label, not a wall.
- Separate follow-up change: rejected — the recorded identity is a store-artifact change, and folding it here rides the same v13 bump, one migration instead of two.

### Decision: Schema version bumps to v13

**Chosen:** `SCHEMA_VERSION` 12 → 13, stamped together with `application_id` at creation.

**Rationale:** the store artifact definition now includes the ownership marker; the bump signals the artifact change per the project's versioning rule, and it makes pre-guard v12 stores doubly distinct (wrong version and no marker).
The migration story is the legacy refusal: no in-place upgrade path exists or is needed.

## Architecture

```text
                         store path
                             |
                     file exists at path?
                     /                  \
                   no                    yes
                   |                      |
      build: create via temp file    header read (100 bytes, plain I/O,
             + atomic rename          no SQLite open, no locks/sidecars)
      query/status: refuse            magic + application_id == "c10r"?
             (no-index, code 3),     /                        \
             create nothing        no: UNRECOGNIZED            yes: OURS
                                    |                            |
                          build/query/status:            user_version == SCHEMA_VERSION?
                          refuse (code 6),               /                    \
                          conditional message,          no                     yes
                          file untouched                 |                      |
                          cache: refuse,          build: replace          proceed normally
                          name manual rm          (remove + recreate)     (cache: delete)
                          (code per Index         query/status: refuse
                          reset contract)         (incompatible, code 4)
```

The recognizer is one function with the three consumers shown; no command touches the file before its verdict.

## Risks

- **`application_id` collision**: another application could stamp the same value; recognition is best-effort discipline, not proof of ownership.
  Accepted — the magic-plus-marker check reduces the accident surface by orders of magnitude, and chasing certainty here is the correctness rabbit-hole the project deliberately avoids.
- **First post-upgrade build refuses the project's own dogfood stores** (v12, unmarked): expected, one-time, and the refusal's rebuild branch names the recovery.
  Dogfood clones under `~/.cache/silent-cartographer` will hit this once.
- **Stranded temp files** after a crash mid-create: inert by construction (distinct name, never at the store path); documented, not swept.
- **Exit-code surface growth**: code 6 must appear wherever the surface enumerates exit codes (manifest/surface docs); if the manifest lists exit codes, `SURFACE_VERSION` bumps with it.
  Check at apply time.
- **WAL sidecars of refused files**: never touched — refusal happens before any SQLite open, and `remove_store_files`'s sidecar sweep only runs on recognized stores, whose sidecars are part of the store by the Context definition.
- **Mismatch marker false-positives in moved or containerized checkouts**: the same project at a new canonical path reads as a different workspace until the next build re-records it.
  Accepted — the marker labels and never refuses, so the cost is one advisory line, and under-claiming belonging is the calibration direction the north star prefers.
- **Answer-envelope growth**: every query answer gains the workspace-mismatch field.
  Additive, but consumers pinned to an exact response shape would notice; covered by the regression suite's envelope assertions at apply time.
