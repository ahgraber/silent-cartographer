# Design: cli-alignment

## Context

The query engine already produces every answer as a `serde`-serializable type (`Answer<T>`, `SymbolView`, `SymbolDetail`, `TraceItem`, `DependentsReport` in `src/query/`), and the store already holds everything these commands need: `display_name` for `find`, the `imports` and `type_hierarchy` edge tables for the two new relations, and the tier columns (`signature_text`, `interface_text`, `span_text`) for content rendering.
The CLI (`src/cli.rs`, `src/commands.rs`, `src/main.rs`) currently renders answers with `{:#?}` and applies no bounding.
This change is therefore surface work over an unchanged graph: no schema bump, no new extraction, no new persisted data.
The named-relation decision, the folded `get`/`trace` spine, and the honest refusal of `callers`/`callees`/`tests` are settled in `proposal.md`; this document records how the surface is built.

The build-dependency order — contract primitives (rendering, streams, exit codes) before the answer-shaping they carry (bounding), before the commands that emit answers (`find`, relations), before the operational/introspection commands (`doctor`, `cache`, `manifest`) — is the order of the decisions below and of the task groups.

## Decisions

### Decision: HumanRenderProjectsTheAnswer

**Chosen:** a rendering module projects each already-serializable answer type to human text; the JSON path serializes the same value with `serde_json`, so the two views read one source of truth.
Rendering is detail-aware: a `location` result is one line (`path:start-end`); a row-bearing answer (`trace`, `find`, `dependents`) is a header line plus one line per result (`  <canonical_id>  <name>  at <path:span>`); a content-bearing detail (`signature`/`interface`/`body`) is the tier text rendered as a source block.
The renderer reads only fields already present on the answer; it introduces no new answer data.
A `Symbol` row — the `contains`/`containers`/`importers`/`implementers` relations — carries its own `at <path:span>` too, populated from the same `location_of` lookup the other row kinds already use; the suffix is absent when the symbol carries no definition location (an external symbol), rendered honestly rather than fabricated.

**Rationale:** the north star makes the machine answer the source of truth and the human view a render of it — deriving the render from the same serialized value is the structural guarantee that the two can never disagree about membership or order.
`{:#?}` leaks Rust struct syntax and escapes multi-line source into one line, which is exactly what the legible-terminal-output story rejects.

Structural fields — a symbol's canonical identity and name, its kind, a document path, and the header's analyzer name/version — are sanitized before they are composed into a line: every terminal-control and display-control character is replaced with the Unicode replacement character — the C0 controls (`0x00`-`0x1f`, including newline and tab), DEL (`0x7f`), the C1 controls (U+0080–U+009F, whose U+009B is a one-character CSI that C1-honoring terminals execute with no ESC byte), and the Unicode bidirectional controls (U+202A–U+202E and U+2066–U+2069, the Trojan-Source display-spoofing class).
A hostile repository, or a hostile index file, can embed ANSI/OSC control bytes or a raw newline in a symbol name or a path; rendered verbatim, that string could command the reader's terminal, or a raw newline could forge an extra row in a row-bearing answer — the same escape-injection class `git log` guards against for commit metadata.
Content-bearing tier text (signature/interface/body, and the content projected onto `trace`/`dependents` rows) passes through the same class map with three exemptions — `\n`, `\t`, and `\r` stay verbatim, so a CRLF source file sprouts no replacement characters — because source text is exactly where hostile bytes arrive from a hostile repository, and "source-faithful" means the text as it reads in the file, not the terminal commands it could smuggle.
Diagnostics on standard error pass through the same sanitizer at the process edge: an error message can embed user-supplied text (a `--db` or `--at` path, a reference string).
`--json` output is deliberately exempt: `serde_json`'s string escaping already neutralizes control bytes for the machine answer without lossy substitution, so the machine answer stays byte-exact.
Sanitization is applied to field values before a line is composed — and to a content value before any styling composition — never to the already-composed line, so `c10r`'s own styling escapes (added afterward, when `--color` is in effect) are never eaten by the same pass.

**Alternatives considered:**

- Keep `{:#?}`: fails the source-faithful-content requirement and reads as a debug dump.
- A second render-only data path independent of the JSON value: reintroduces the disagreement risk the projection design removes.

### Decision: PlainSourceFloorWithAColorGate

**Chosen:** content-bearing details render as source-faithful, multi-line plain text; a `--color=<auto|always|never>` flag gates styling, with `auto` (the default) emitting styling only when standard output is a terminal (via `std::io::IsTerminal`), `always` forcing styling even to a redirected stream (the cross-tool convention, e.g. piping into `less -R`), and `--json` never styled under any setting.
Syntax highlighting and theming are deliberately not implemented in this change; the plain floor satisfies every content-rendering scenario, and a highlighter's theme is a user preference that would introduce c10r's first configuration surface.

**Rationale:** c10r has no configuration surface today — only flags and functional environment discovery (`VIRTUAL_ENV`, `PATH`) — and a highlighter needs a theme, which on a light-versus-dark terminal is a genuine preference, not a fixed rendering.
That preference belongs with a future config surface (the same horizon as the MCP change), not smuggled in behind a highlight flag here.
Keeping the dependency-free `--color` gate now means the styling discipline — never to a pipe or the machine answer — already holds, so a later highlighting enhancement is purely additive over a gate that already enforces the contract.
The content sanitization pass runs on the tier value before any styling is composed, which is also what keeps that future highlighter safe: an escape sequence embedded in source text could otherwise splice into the highlighter's own control sequences, so by the time any styling layer sees the text, the injection class is already neutralized.
`--color=<when>` is the cross-tool-standard spelling (git, ls, bat), satisfying the vocabulary-consistency principle better than a boolean `--no-color`.

**Alternatives considered:**

- Ship `syntect`/`two-face` highlighting with one fixed theme now: a dark theme is unreadable on a light terminal, so "one theme" is not neutral; it adds a heavy dependency for an enhancement the contract does not require, and opens a theme-preference question the tool has nowhere to answer.
- Shell out to `bat`: a runtime executable dependency the tool cannot guarantee is installed.
- Defer the `--color` gate too: the gate is cheap and stops styling from ever reaching the machine surface; keeping it makes a later highlighter additive rather than a contract change.

### Decision: ExitCodeTaxonomy

**Chosen:** a closed set — `0` success (including a typed-empty answer), `1` generic operational failure, `2` usage error, `3` no index discovered, `4` incompatible index store, `5` indexer/setup failure — surfaced through one process-exit mapping applied at the top of `main`.

**Rationale:** distinct codes per category let an agent branch on outcome without scraping prose (the scriptable-cli story).
`4` reuses the store-incompatibility outcome the graph already refuses on (`code-graph`'s "Incompatible index stores are replaced or refused"); `3` and `5` separate "no map yet" from "the tool that builds the map is missing," which are different recovery actions.

**Alternatives considered:**

- Collapse `3`/`4`/`5` into `1`: forces the agent to parse stderr to know whether to build, install an indexer, or reset — the opposite of the taxonomy's purpose.

### Decision: ClosedFlagVocabularyEnforcedByTest

**Chosen:** the canonical flags are fixed — `--json`, `--detail`, `--relation`, `--depth`, `--limit`, `--max-lines`, `--from`, `--cursor`, `--color`, plus the existing global `--db`/`--workspace` — and clap defines no aliases; a test walks the built clap `Command` tree and asserts the flag set matches the recorded vocabulary and that known banned alias spellings (e.g. `--format`, `--output`, `--top-k`, `--no-color`, and the retired `--max-chars`) are undefined.

**Rationale:** clap will reject any undefined flag as a usage error for free, so the closed vocabulary is enforced by construction; the test guards against a future flag drifting the surface or a banned alias creeping in, making the closure a checked invariant rather than a convention.

**Alternatives considered:**

- Rely on review only: the vocabulary consistency principle explicitly calls for mechanical enforcement over review vigilance.

### Decision: OffsetPaginationBoundToParameterAndIndexIdentity

**Chosen:** paging is offset-based over the deterministic result order the calibrated output contract already guarantees: page _n_ returns results `[n·limit, (n+1)·limit)`.
The continuation token is opaque — a short encoding of `(parameter-identity-hash, page-index)`, where the parameter-identity-hash covers the command, subject reference, relation/detail, the effective limit, the effective content-line bound and `get`'s window start, and any filters, **and the index identity: its content-hash and the recorded analyzer provenance (name and version)**.
The effective limit is what the hash binds, so a token issued under the default limit resumes under the same default; `--limit 0` is unbounded and has no pages, so a cursor against it is a usage error.
Resuming recomputes the hash from the presented parameters and index and compares; a mismatch (different query, or the index rebuilt underneath) is rejected as a usage error naming the recovery (re-issue without the token); a page beyond the last is a usage error naming the valid page range.

**Rationale:** the result order is already deterministic, so offset paging is deterministic without a keyset cursor.
Binding the token to the index content-hash means a rebuild between pages invalidates in-flight cursors honestly (rejected, not silently mis-paged against shifted results) — the calibration principle applied to pagination.
The token stays opaque so its composition can change later without breaking callers.

**Alternatives considered:**

- Keyset/seek cursor over `canonical_id`: more robust to insertions mid-scan, but the answers are already fully ordered and page sizes are small; the offset form is simpler and the content-hash binding already covers the mid-scan-change case by rejection.
- Unbound token (page index only): a token from one query would silently resume against another — the mis-page failure the mismatch rejection exists to prevent.

### Decision: ContentBoundIsPerResultTierText

**Chosen:** content is bounded by **lines**, not characters, through the flag `--max-lines`.
The bound caps the tier text on each result (the `content` a detail projects, and the body/interface/signature `get` returns); truncation is disclosed per result in the answer, independently of the result-set `--limit`.
The two bounds compose: `--limit` caps how many results, `--max-lines` caps how many lines of each result's content.

Defaults are **per command**, reflecting each command's content shape: `trace` (row-oriented, many rows, each a projected declaration) defaults to **10** lines per row; `get` (a single symbol at depth) defaults to **100** lines.
`--max-lines 0` means unbounded — the whole tier text.
`find` carries no content, so it has no `--max-lines` (nor `--from`); passing it there is an unknown-argument usage error.
`dependents` runs through the `trace` command, so it inherits trace's default.

`get` additionally supports **windowed** content access via `--from <N>` (get-only): the 1-based line where the returned window starts (default 1); the window is the lines `[from, from + max-lines)`.
When the returned content is not the whole tier text (windowed or truncated), the answer carries an additive `content_lines` block (start line, end line, total lines; skip-if-absent) alongside the existing `content_truncated` bool (true iff the window does not cover the whole text); the human render names the literal recovery — the next window (`--from <end+1>`) when lines remain below, or the full-text recovery (`--max-lines 0 --from 1`) when the window already reaches the end.
A `--from` past the end of the content returns the answer with empty content and a `content_lines` block naming the total line count — a data-dependent outcome, **not** an error.
`trace` has no window by design: it is a many-row breadth view where a per-row window would be a per-row cursor the surface deliberately does not carry; its rows are head-capped only.

Both `--max-lines` and `--from` apply only to a **content-bearing** detail (`signature`/`interface`/`body`).
Explicitly passing either when no content-bearing detail is in play (no `--detail`, or `--detail location`) is a modal usage error naming the accepting details — mirroring the `--depth`-with-wrong-relation teaching error — decided from clap's `ValueSource` so a **defaulted** value stays dormant (only an explicitly typed flag errors).
This modal check applies on both `get` and `trace` (trace content exists only under `--detail signature|interface|body`).

**Rationale:** the two axes of context blow-up — too many rows, and one enormous body — are independent, so they need independent bounds; a module body is a whole document and can dwarf a hundred trace rows, so content bounding is not optional.
Lines are the unit a human and an agent both reason about source in (an editor, a diff, a stack trace are all line-addressed), and a line-based window is directly re-addressable — `--from <end+1>` is the honest "next" — where a character cut is not.
Per-command defaults acknowledge that a row list wants a short per-row peek while a single symbol wants most of its body.

**Alternatives considered:**

- Character-based `--max-chars`: cuts mid-line and mid-token, is not re-addressable as a window, and forces the reader to reason about bytes rather than the lines they actually read.
- A single total byte budget across the whole answer: couples row count and content size, making truncation unpredictable per result and harder for an agent to reason about.
- A window on `trace` too: a per-row window is a per-row cursor; the surface keeps trace a breadth view and reserves windowing for the single-symbol `get`.

### Decision: DefaultResultLimitIsTwentyFive

**Chosen:** `--limit` defaults to **25** on `get`, `trace`, and `find` when omitted; an explicit `--limit 0` means unbounded (the whole set, with no page block); an explicit `--limit N` caps at N through the existing PageInfo/cursor machinery.
The default is applied at the CLI/command layer, not inside `QueryEngine`, so the engine stays bounding-agnostic.
A page block is disclosure of a partial view: it is attached only when the answer is truncated to the limit or resumes a later page, so a default-bounded query whose set fits within the limit reads like an unbounded one.
The ambiguous-candidate list is likewise capped at the effective limit (default or explicit), disclosing the total candidate count when capped (additive `candidates_total`, skip-if-absent); the human render says "…and N more — narrow the reference".
An ambiguity is a refusal, not a resumable result set, so a capped candidate list carries no cursor.

A `dependents` answer is a single report whose **detailed rows** are the result set the limit bounds, not the outer one-element result vector — so the limit pages `report.detail` under the same cursor machinery every other row-bearing answer uses (page only on a partial view; a cursor resumes deterministically; the identity hash already binds depth and limit).
The report's summary metadata — the depth bound, horizon, disclosure, and the beyond-bound aggregate — is context, not rows, so it is repeated on every page rather than paged, and the human render carries the paged rows, that summary, and the standard page line.
A depth-1 impact answer with more direct dependents than the limit thus details at most the limit's rows, discloses the truncation with a continuation token, and still reports the deeper reach in aggregate on each page.

**Rationale:** an agent-first surface must be safe by default — an unbounded result set is a context-window hazard the caller did not opt into — so the default is a bound, and truncation is always disclosed (with a cursor to resume), keeping the default honest rather than silently lossy.
`0` as the unbounded sentinel keeps "give me everything" a single explicit token rather than a magic large number.
Applying the default at the command layer (not the engine) keeps engine-level tests and callers that want the raw, unbounded answer untouched.

**Alternatives considered:**

- No default (unbounded unless `--limit`): the common case dumps an unbounded set into the caller's context — the exact hazard the calibrated-output north star rejects.
- Making `--limit 0` an error (the prior ranged parser): leaves no single explicit way to ask for the whole set now that absent means "bounded to the default."

**Future work:** the per-command default bounds (25 rows; 10/100 lines) and a pager over resumable pages are candidates for a later global-config change, alongside the color/highlighting horizon — the same config surface the tool does not have today.

### Decision: FindIsCaseInsensitiveSubstring

**Chosen:** `find <fragment>` selects symbols whose `display_name` contains the fragment, matched case-insensitively (`display_name LIKE '%fragment%'` with the fragment's `LIKE` metacharacters escaped), ordered deterministically by `canonical_id`, bounded by the `--limit` contract.
Case folding is ASCII-only (SQLite `LIKE`'s default): a non-ASCII character in the fragment matches exactly, case-sensitively — a documented limitation, accepted because Unicode case folding is locale-fraught machinery that the deferred find-by-intent change (trigram/FTS5 robustness considered together with semantic search) replaces wholesale.
No new column, index, or extraction.

**Rationale:** substring over the existing `display_name` is the cheapest honest thing that is genuinely more than exact resolution, and it is the story's stated "first, cheapest rung."
Deterministic identity order (not a relevance ranking) keeps the answer reproducible and avoids implying a match-quality judgment the substring match cannot actually make — relevance ranking, typo tolerance, and semantic matching are the deferred find-by-intent change's job, recorded here so this `find` does not pretend to be that.

**Alternatives considered:**

- Prefix-only match: misses the interior-fragment case the half-remembered-name story wants.
- Trigram index / edit distance now: net-new persisted structure and the deferred change's territory; shipping it here would commit that change to preserving these semantics.
- Relevance-ranked order: implies a quality signal substring matching does not have; deterministic identity order is the honest ordering.

### Decision: NewRelationsReverseWalkExistingEdges

**Chosen:** `importers` reverse-walks the `imports` edge (the modules whose `imports` edge targets the subject); `implementers` reverse-walks the `type_hierarchy` edge (the types whose `type_hierarchy` edge targets the subject).
Both are one-hop and reuse the existing `edges_by_dst` index; neither introduces a new edge kind.

**Rationale:** these edges are already persisted and already indexed by destination for the `dependents` reverse walk; exposing a single-hop, single-kind view of them is a query addition, not a graph change.
They are named relations rather than a `--kind` filter on `dependents` per the proposal's folded-surface decision (discoverability, and `dependents` stays the transitive-impact answer).

**Alternatives considered:**

- A `--kind` filter on `dependents`: rejected in the proposal — less discoverable and conflates the one-hop structural question with the transitive impact answer.

### Decision: DoctorAndCacheAreThinOperationalCommands

**Chosen:** `doctor` probes each required indexer the build already invokes (`rust-analyzer`, `scip-python`) for presence and version and prints a fixed-width report, exiting `5` when a required one is missing; `cache` removes the index file at the discovered `--db` path, reports the path, treats an already-absent index as success (`0`), and maps an OS removal error to `1` naming the path.
Removal applies only to a recognizable index store: before unlinking, `cache` reads the target's header and refuses unless it carries the SQLite magic (`SQLite format 3\0`) and a nonzero big-endian `user_version` stamp — the schema-version pragma every build writes.
A refused target (a text file, a SQLite file with no stamp) is left intact with a plain operational failure (`1`, not a usage error — the invocation is well-formed) whose message names the manual alternative (`rm <path>`); an empty 0-byte file is a failed-create artifact and stays removable; a store stamped with a different (nonzero) version is still an index and stays removable, since resetting it is the recovery path.
WAL/SHM sidecars are removed only alongside the primary, their paths built byte-preservingly from the primary's os-string.

**Rationale:** both are surfaces over things that already exist (the backends the builder locates; the known index path), held to the same stream and exit-code discipline as the query commands.
`cache` needs no `--force`/`--dry-run` guard: the index is cheap to rebuild and removal is idempotent, so the explicit subcommand is a sufficient mutation boundary.
The header guard exists because `--db` is caller-supplied: a mis-pointed path would otherwise let the reset subcommand unlink any writable file, and recognizing the store by its own on-disk stamp costs one 64-byte read while keeping the command flag-free.

**Alternatives considered:**

- Gate `cache` behind a confirmation flag: over-engineering for an idempotent, recoverable reset; the non-interactive principle prefers no prompt.

### Decision: BoundedProbesUnboundedWork

**Chosen:** every short-lived probe of an external tool — the version/readiness checks (`rust-analyzer --version`, `cargo metadata --no-deps`, `scip-python --version`, the resolved interpreter's `--version`) — runs through one shared, std-only bounded-probe helper; the long-running real indexing work (`rust-analyzer scip`, `scip-python index`) stays on a plain blocking invocation, unbounded by design.
The helper spawns the child with piped stdout/stderr, drains each pipe on its own thread into a **64 KiB-capped** buffer (reading past the cap and discarding the excess, so a flooding child never blocks on a full pipe yet the parent never buffers unbounded output), polls for exit against a **10-second deadline**, and on expiry kills and reaps the direct child and returns a typed timed-out outcome.
On the timeout path the drain threads are **abandoned, never joined**: killing the direct child does not kill its descendants, and a surviving grandchild can inherit and hold the pipe write-end open, so a reader blocked on that pipe never reaches EOF — joining it would re-create the exact indefinite hang the helper exists to remove.
The abandoned threads are harmless detached threads reaped at process exit; both `doctor` and the query freshness read are short-lived.
On the success path the child has exited, both pipes are at EOF, and joining the readers returns promptly.

`doctor` reports three states, not two: an indexer is present, absent (missing/unspawnable), or **unresponsive** (present but silent past the deadline), the last carrying an investigation hint distinct from the install hint; the exit-code taxonomy signals the indexer/setup failure for absent **or** unresponsive.
On the query path a probe **timeout** keeps the existing recorded-provenance / absent-environment fallback (the machine answer is unchanged) but emits one disclosing stderr diagnostic through the standard `sanitize` guard; a **missing** tool behaves exactly as before with no new noise, so only a genuine timeout — the rare wedged-tool case — is disclosed.

**Rationale:** `std::process::Command::output()` blocks forever on a hung child and buffers unbounded output; because the query freshness read runs on every `get`/`trace`/`find`, a single wedged `rust-analyzer` would hang plain queries, not just `doctor`.
Bounding the probes removes that failure mode while leaving the genuinely long indexing work unbounded, since that work is the user's explicit request and has no sensible fixed ceiling.
The 10-second deadline and 64 KiB caps live here as the single source of the numbers; the helper takes the deadline as a parameter so tests can pass sub-second values and stay fast while production passes the 10-second constant.
`C10R_PROBE_TIMEOUT` is noted as a **future** pressure-release only — a way to raise the deadline on a genuinely slow host — and is deliberately **not** implemented here, since the tool has no configuration surface yet and one hard-coded ceiling covers every observed case.

**Alternatives considered:**

- The `wait-timeout` crate: it bounds the wait on the child, but the drain-thread machinery that dominates this helper — the two capped readers and the abandon-on-timeout rule — would still be needed on top of it, so it removes no complexity and adds a dependency; declined.
- Joining the reader threads on timeout for a clean shutdown: a descendant holding the pipe open makes the join block forever, which is the very hang being removed; abandoning the threads is the correct trade.
- Bounding the analysis runs too: those are the user's explicit long-running request with no fixed ceiling; a deadline there would abort legitimate work, so they stay unbounded.

### Decision: ManifestFromClapWithASurfaceVersion

**Chosen:** `manifest` derives the command/flag structure by walking clap's `Command` tree (names, value enumerations, defaults) and emits it as JSON alongside a `surface_version` constant and the current index state (reusing the `status` freshness read).
`surface_version` is a hand-maintained integer, bumped when the command or flag structure changes; a test snapshots the derived structure and fails when it changes without a `surface_version` bump.

**Rationale:** deriving structure from clap keeps the Layer 2 index in lockstep with the actual parser rather than a hand-written copy that drifts.
The snapshot-versus-version test makes drift detection (the `detect-contract-drift` story) a checked invariant: the surface cannot change silently without either a version bump or a failing test.
`manifest` deliberately omits per-flag prose and response-shape vocabulary — those are Layer 1 (`--help`), which clap renders from the same doc comments.

**Alternatives considered:**

- Hand-authored manifest JSON: duplicates the parser definition and drifts from it.
- Auto-derive the version from a structure hash: removes the human decision point, but the story wants a stable, meaningful version an agent can compare, not a churn-sensitive hash that changes on cosmetic edits.

### Decision: CompletionsDeriveFromTheParser

**Chosen:** a `completions <shell>` subcommand, using the `clap_complete` crate (same clap 4 family; the one new dependency this change takes), generates a completion script from the SAME `Cli::command()` tree that `--help` and `manifest` derive from, and writes it to standard output.
The supported shells are whatever `clap_complete::Shell`'s `ValueEnum` offers; an unsupported shell name is rejected with clap's own out-of-set diagnostic, for free.
An explicit `--json` together with `completions` is a usage error: a script is not an answer, so `--json` has nothing to select — consistent with the modal teaching-error convention the content bounds already use.
Dynamic value completion (e.g. completing a live symbol name against the index) is out of scope; this change is static command/flag completion only, noted as a possible later enhancement.
The README documents the install pattern per shell — that is the documentation surface, not a new command output.

**Rationale:** deriving the script from `Cli::command()` is the same drift-proofing `manifest` already relies on: the completed surface is mechanically the parsed surface, so a renamed flag or a removed command breaks completion by construction rather than through a maintainer remembering to update a second copy.
`clap_complete` is clap's own completion-generation crate, so it stays in the same dependency family the CLI already commits to rather than adding an unrelated tool.

**Alternatives considered:**

- Hand-written completion scripts per shell: drift from the actual parser the moment a flag changes; the exact failure mode `manifest` was built to avoid for `--help`.
- Build-time generation into packaged files (a build script emitting scripts into the release artifact): adds packaging complexity, and the generated script can still drift from the installed binary if the two are built or distributed separately; the runtime subcommand instead always reflects the binary actually running.

## Architecture

```text
invocation
  └─ clap parse ──(usage error → exit 2, enumerating valid values)
        │
        ▼
   command dispatch
        ├─ get / trace / find / dependents ─► QueryEngine ─► Answer<T>  (serde value)
        │                                          │
        │                                   bounding: --limit (row cap, default 25) + --max-lines (content cap) + --from (get window)
        │                                          │            + continuation token (param+index hash)
        │                                          ▼
        │                                     render layer
        │                                       ├─ --json ─► serde_json ─► stdout (no color)
        │                                       └─ human  ─► detail-aware projection ─► stdout
        │                                                       └─ content block ─► plain source (styling gated: iff tty & --color)
        ├─ doctor  ─► probe backends ─► report            (missing → exit 5)
        ├─ cache   ─► remove index at --db path ─► report (absent → exit 0; os error → exit 1)
        └─ manifest ─► walk clap tree + surface_version + index state ─► serde_json ─► stdout

   diagnostics ─────────────────────────────────────────────────────► stderr
   outcome ────────────────────────────────────────────────────────► exit-code taxonomy (0/1/2/3/4/5)
```

## Risks

- **Terminal legibility without highlighting**: the plain source floor is uncolored, which is less scannable than a highlighted block; accepted for this change, since highlighting requires a theme preference the tool has no config surface for.
  If dogfood shows the plain read is too costly, highlighting is added later over the `--color` gate that already exists, with no contract change.
- **Offset-pagination drift when the index rebuilds mid-paging**: mitigated by binding the continuation token to the index content-hash, so a rebuilt index rejects the stale token (usage error) instead of resuming against shifted results.
- **`surface_version` maintenance forgotten**: mitigated by the snapshot test that fails on an un-versioned surface change; the version cannot drift silently.
- **`find` full-scan cost on a large index**: `display_name LIKE '%…%'` cannot use an index; acceptable at current single-repo scale and bounded by `--limit`, and the deferred search change replaces it with an indexed structure.
  Noted, not optimized now.
- **`implementers`/`importers` for a language whose backend under-populates the edge**: the relation returns typed-empty honestly rather than erroring; the dogfood record samples both languages so an unexpectedly empty relation is caught as a data-coverage observation, not a silent contract gap.
- **Human-render layout is not a stability surface**: the JSON answer is the machine contract; the exact human header/row/color layout may evolve without a version bump, and the surface-version snapshot covers structure (commands/flags), not render cosmetics.
