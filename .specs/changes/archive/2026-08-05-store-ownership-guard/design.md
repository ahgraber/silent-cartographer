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

The recognizer inspects the store path as a literal filename, so every connection must resolve that same path.
SQLite reads a filename beginning `file:` as a URI naming a different file, and this build enables that interpretation globally — withholding `SQLITE_OPEN_URI` does not switch it off — so a relative `--db file:something` would let the guard clear one file while the command read or wrote another.
Connections therefore receive the path absolutized: an absolute path cannot begin with `file:`, and absolutizing is lexical, so which file is named is otherwise unchanged.

**Alternatives considered:**

- `PRAGMA application_id` after `Connection::open`: side effects on foreign files (locks, potential sidecars) before the decision to refuse is made.
- Schema-shape probing (table names): requires opening the database, and couples recognition to the very schema that migrations change.

### Decision: A path that cannot be examined is refused in the ownership category, without an ownership claim

**Chosen:** the recognizer keeps surfacing an I/O failure rather than folding it into a verdict, and its consumers convert that failure into a typed refusal of its own — naming the path and why the read failed, asserting neither that the target is nor that it is not a store this binary created.
It carries the ownership exit code, because the caller's next action is identical to an ownership refusal (do not build here, correct the path), and it reports its own index state in `manifest`.
The human message adds a cause-specific hint only where the operating system's own words do not imply the fix: a directory at the path is told that `--db` wants the database file inside it, and a permissions failure is told to check the file's owner.
Every other cause carries the operating system's words alone.
Every refusal stays on one line, because they all reach the caller through the diagnostic sanitizer that makes control characters visible rather than executing them: a literal newline in a message renders as a replacement character, so multi-line refusals are damage, not formatting.

**Rationale:** an unreadable path is an unevaluated ownership question, not an answered one, and this change already fixed the shape of that answer once — an unevaluable workspace comparison is disclosed as unknown rather than folded into "matched".
Folding "cannot read" into "unrecognized" would state two things the binary has not established: that a store built by another user is not c10r's, and — via the refusal's rebuild branch — that removing it is a recovery, when the caller may not be able to remove it or may be looking at their own live index directory.
Leaving the failure to surface raw is the opposite error: it says nothing actionable, and it lets `manifest` report the path as an absent index, whose remedy (build here) is the one instruction the guard exists to withhold.
Splitting the answer — the exit code carries what to do, the message carries what is known — gives the machine caller the same branch it would get from an ownership refusal without making the human message assert an ownership fact.

**Alternatives considered:**

- Fold the I/O failure into `Unrecognized`: no spec change and the smallest diff, but it manufactures an ownership verdict out of a failed read, which is the calibration error the north star names.
- A fourth `StoreRecognition` verdict carrying the failure: the verdict enum is `Copy`/`PartialEq` and compared directly in the recognizer's tests; carrying an error inside it would cost those derives to express something only the consumers need.
- Its own exit code: the taxonomy exists so a caller can branch on what to do next, and there is no action that distinguishes this from an ownership refusal.

### Decision: An occupied build path is an ownership refusal, not an internal error

**Chosen:** the refusal raised when a file already occupies the path a store is built at is contracted under the ownership requirement and carries the ownership exit code, and its message states removal only as conditional on the file being an interrupted build's leftover.

**Rationale:** the exclusive create exists precisely because ownership of that path is unproven — a build file's name embeds a process id, and process ids recycle — so a message asserting the file is c10r's leftover claims the very thing the check declines to assume.
Contracting it under the ownership requirement rather than as its own category keeps one rule for one question: a path this binary cannot prove is its own is refused, named, and left alone.

**Alternatives considered:**

- Leave it uncontracted as an internal operational error, like a full disk: it is user-visible, path-specific, and recoverable by a user action, which is what separates a contracted refusal from an operational failure.

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

### Decision: The ownership category belongs to the target, not to the command that met it

**Chosen:** `cache`'s refusal of a target it cannot confirm is c10r's own carries the ownership exit code — the same code a build or a query reaches over that same file — while keeping reset's own message shape (name the manual `rm`).
A failure raised _after_ the target cleared recognition, such as an unlink the operating system denies, stays a generic operational failure.

**Rationale:** the taxonomy requirement names "a target refused because the system cannot confirm it is a store the system created" as one category, and the category describes the target, not the command.
Scoping it to build and query would leave one file answering two ways depending on which command met it, and reset is the command whose next step is destructive — the one where a caller most needs to branch on the category rather than parse prose.
It would also undercut the reason an unexaminable path carries this code: that the caller's next action is identical to an ownership refusal.
Within `cache` that claim only holds once reset's own ownership refusal carries the same code.

**Alternatives considered:**

- Keep reset's refusal on the generic code and narrow the taxonomy requirement to build and query: contracts the inconsistency instead of resolving it, and leaves an agent branching on the ownership code blind at exactly the destructive command.

### Decision: A recognized store at a schema version this binary does not read is its own index state

**Chosen:** `manifest` reports such a store as a distinct index-state reading, alongside absent, unrecognized, and unexaminable.
Every other read failure after recognition clears keeps the absent shape.

**Rationale:** an index does exist at that path, and every query against it names the version mismatch and refuses with the incompatible-store code; orientation reporting nothing there would make `manifest` the one surface that disagrees with the rest about the same file.
The remedy rhymes with an absent index's — `c10r build` either way — but orientation's job is to describe what is there, and "a c10r index this binary cannot read" is a different fact from "nothing here".
It is also the state every store built before this change is in, so it is what an agent meets on the first run after an upgrade.

**Alternatives considered:**

- Report it as absent because both remedies are `c10r build`: collapses a distinction the rest of the surface makes, and hides the one condition a just-upgraded caller is most likely to hit.

### Decision: Workspace identity is the canonicalized root path, compared at query time, label-never-refuse

**Chosen:** build records the canonicalized workspace root in `index_metadata` (one new column); every query command — all of which already receive both `--db` and the root — canonicalizes its root and compares.
A difference sets a workspace-mismatch field in the answer envelope and a corresponding human-render line; it never refuses, and it composes with (never replaces) the staleness flag.
Build over a recognized store recorded for a different root prints a disclosure naming both roots, proceeds, and re-records.
The existing `workspace_id` stays as display identity only.

The recorded root is stored as text only when the canonical path converts exactly; a path that does not is recorded as absent, and the comparison then reports unknown.
A lossy rendering would map distinct paths onto one string, and two workspaces colliding there would compare equal — a false "matched", the one answer this comparison must never give wrongly.

**Rationale:** the wrong-project case degrades answer honesty, not data safety — the store is c10r's own, replayable artifact — so the calibration principle applies: disclose, don't block.
A canonical path is a strong signal of "different project" but a weak proof (repos legitimately move; containers and bind mounts remount the same project at new paths), so a hard refusal would false-positive against the user's own index; a rebuild in place re-records the identity and clears the marker.
When the comparison cannot be evaluated — in practice only when canonicalizing the query root fails, since every recognized post-v13 store records its identity at creation — the answer discloses the workspace relationship as unknown: typed absence, never an implied match, per the north-star rule that absence is typed.
The build handoff disclosure reads the old store's metadata, which is guaranteed readable only at the current schema version; over a version-mismatched recognized store the replace proceeds with best-effort disclosure, and the spec's disclosure guarantee is scoped to current-version stores.

**Alternatives considered:**

- Compare `workspace_id` (directory name): too weak in both directions — two repos named `backend` collide, and a renamed directory false-positives.
- Refuse on mismatch: punishes moved repos and containerized checkouts; the answer-honesty problem needs a label, not a wall.
- Separate follow-up change: rejected — the recorded identity is a store-artifact change, and folding it here rides the same v13 bump, one migration instead of two.

### Decision: `SURFACE_VERSION` does not bump

**Chosen:** the surface version stays where it is; the checked-in surface snapshot is unchanged.

**Rationale:** the pinned structure is the command/flag tree — commands, positional arguments, flags, their enumerated values and defaults — and this change touches none of it: no command, flag, value, or default is added, removed, or renamed.
The surface answer's index-state block gains a distinct reading for an unrecognized file, but that block is the current state of the workspace's index, not part of the pinned structure, and the snapshot fixture does not carry it.
The exit-code taxonomy is contracted in the command-surface spec rather than enumerated in the surface answer, so the new category adds nothing there either.

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
