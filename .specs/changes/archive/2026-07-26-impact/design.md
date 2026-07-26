# Design: impact

## Context

- `impact` is a new query command layered over machinery that already exists: the position-to-symbol resolution behind `get --at`, and the reverse-reachability `dependents` traversal behind `trace --relation dependents` (over `uses` + `imports` + `type_hierarchy`, with a depth bound and horizon disclosure).
  Neither is reworked here.
- The index is a point-in-time snapshot, wholly superseded by each manual `build`.
  Its freshness is gated by a single whole-workspace content-hash (`content_hash` in `src/graph/mod.rs`); there is no per-file hash and no committed history of prior snapshots.
- The output contract — bounding (`--limit`/`--cursor`, content windowing), the human render as a projection of the machine answer, the exit-code taxonomy (`src/exit.rs`), and the stdout-payload/stderr-diagnostics split — is inherited wholesale from the shipped CLI-alignment change.
  `impact` adds only the git boundary's own failure modes and the exact/approximate freshness axis on top of it.
- The exit-code taxonomy is a closed set: `Success` 0, `General` 1, `Usage` 2, `NoIndex` 3, `IncompatibleStore` 4, `IndexerSetup` 5 — where `IndexerSetup` is documented as "a required language indexer **or setup step** was unavailable."

## Decisions

### ImpactIsItsOwnVerb

`impact` is a new top-level command, not a diff seed-mode of `trace`.

- **Rationale.**
  It is additive — the just-shipped `trace` contract is untouched, so nothing scripting `trace` is perturbed.
  It is discoverable as a first-class capability in `--help`, completions, and the manifest.
  Its answer shape (a seed set plus a dependents union, with an exact/approximate freshness label) genuinely differs from `trace`'s subject-plus-rows.
- **Alternatives.**
  Fold a `--diff` seed-mode into `trace` — rejected: reopens a shipped contract, makes the positional optional, and adds conditional-validity rules (`--diff` only with `--relation dependents`).
  A polymorphic positional accepting either a symbol or a diffspec — rejected: overloads one argument's type and hides the mode.

### ReverseReachabilityOnly

The impact set is the reverse-reachability closure of the seed set — dependents only, no forward/callee reach.

- **Rationale.**
  "What could my change break?"
  is answered by the callers/users of what changed.
  A changed symbol's own callees are, by definition, not the thing that changed; if a callee also changed, it is itself in the seed set and its dependents are traced anyway.
- **Alternatives.**
  Bidirectional (add forward reach) — deferred.
  Forward edges do carry information for _newly-formed_ interactions (a call site added by the diff), but distinguishing new from pre-existing edges is graph-diffing, a larger capability.
  Reverse is sufficient for v1.

### SeedFromThePreChangeSide

Seeds are taken from the diff's pre-change (old) side.
A changed region's byte offsets are resolved against the pre-change content of that side (e.g. `git show <base>:<path>`), which aligns with an index built at the base.
Resolution uses the pre-change path as well: path and offsets always come from the same side of the diff.
A renamed file's changed regions therefore resolve at the old path — where an index built at the base actually holds the symbol — and the rename is followed only for path narrowing and display.

- **Rationale.**
  Dependents attach to the _old_ identities.
  Seeding from the before side is the only way a symbol _deleted or renamed_ by the change still resolves to who depended on it — the highest-value blast-radius question for a removal.
  It also dissolves the uncompilable-working-tree problem: the before side is a committed, index-able state, so exactness never requires indexing in-flight edits.
  Newly-added symbols have no before-side range, but they also have no existing dependents, so their blast radius is legitimately empty.
- **Alternatives.**
  After-side seeding — rejected: its exact-recovery would be a trivial `c10r build`, but it misses the dependents of deleted/renamed symbols.
  Both-sides union — rejected: doubles the alignment work and inherits the after side's uncompilable-drift problem for no blast-radius gain.
  Resolving a renamed file at its post-change path — rejected: an index matching the base holds the symbol at the old path, so the exact path (the one the honesty layer steers users into) would miss and misreport the seed unmappable; against a drifted index the miss is already the disclosed approximate behavior, needing no special rename rule.

### GitDiffIsABoundedExternalBoundary

Obtaining the diff shells out to `git`, bounded by a deadline, and parses its hunks into changed `(file, byte-range)` pairs on the pre-change side; renames are detected so a renamed file's changed regions attach to its pre-change path, with the post-change path retained for narrowing and display.

- **Rationale.**
  Git is an external boundary, so its invocation must be time-bounded and its failures typed (CLAUDE.md directive #1).
  The bounded-subprocess discipline in `src/semantic/probe.rs` (deadline + kill + reader-abandon so a hung child can never block) is reused for the boundary safety.
- **Capture cap must not apply to the diff payload.**
  `run_bounded`'s `CAPTURE_CAP` (64 KiB, excess _discarded_) is correct for a version string but would silently truncate a large diff into an incomplete seed set.
  The diff must be captured in full within the deadline — either by lifting/parameterizing the cap for the diff stream or by streaming git's output to a temp file and reading it back.
  Small preflight commands (`rev-parse --is-inside-work-tree`, resolving the base revision) can use `run_bounded` unchanged; the diff itself cannot.
- **A path absent at the base is not an operational failure, but everything else is.**
  Reading a file's pre-change content answers "was this path present at the base?", and "no" is a legitimate data outcome the honesty layer discloses.
  A repository that cannot be read at all — a corrupt object, an unreadable file, a permission failure — is not that, and folding it into the same absent answer would dress a broken repository as a set of unresolvable regions on an otherwise successful assessment.
  The two are separated by what `git` reports, so only a genuine "no such path in that revision" reads as absence.
- **Textconv drivers are disabled.**
  A configured textconv driver rewrites what `diff` sees but not what the content read returns, and the byte offsets are measured against the latter — so a driver that changed a file's line count would land every seed on the wrong declaration with nothing in the answer disclosing it.
- **Alternatives.**
  A git library (e.g. `git2`/`gix`) instead of shelling — rejected for v1: it widens the dependency surface and reimplements diff/rename semantics `git` already owns; the shell boundary matches how the semantic adapters already invoke their tools.

### SeedsAreTheInnermostTouchedDeclarations

A changed byte range seeds the _innermost_ declarations its span overlaps, and never a declaration whose span covers the whole pre-change document.

- **Rationale.**
  Raw span overlap is not the seed rule, because definition spans nest.
  A file module's persisted span is the entire file, so plain overlap would attach every edit in a file — a comment, a blank line — to that file's module, and the module's dependents are everything that imports the file.
  That both destroys calibration (a doc-comment edit would report the file's whole dependent set) and makes the typed-empty answer unreachable, since a whole-file span overlaps every possible hunk.
  Two filters express the rule: of the overlapping declarations, keep only those minimal under span containment — so a hunk inside a method seeds the method, not its type or its module, while a hunk straddling two methods seeds both — and drop any candidate whose span covers the pre-change document end to end, because a whole-document span locates the change nowhere in particular and so is no evidence that the change touched that declaration's own meaning.
  An inline `mod` block's span is the block, not the file, so it is unaffected: only the file-spanning module is dropped.
  The consequence is deliberate: a change confined to text belonging to no declaration narrower than the file — a module-level comment or blank line — seeds nothing, which is exactly the typed-empty answer.
- **Alternatives.**
  Plain span overlap — rejected as above.
  The single tightest enclosing declaration per changed byte (what `get --at` resolves) — rejected: it collapses a hunk straddling two declarations onto one of them, and still yields the file module for a module-level comment.
  Ranking candidates by kind (never seed a `module`) — rejected: it would also drop an inline `mod` block whose own declaration really did change, and it encodes a kind taxonomy where a span relationship is the actual evidence.

### GitBoundaryFailuresReuseTheSetupFailureCode

Git-absent, not-a-worktree, and git-timeout map to `IndexerSetup` (exit 5); a malformed or unresolvable revision spec is a `Usage` failure (exit 2), rejected before any traversal.

- **Rationale.**
  A missing/unusable `git` is an unmet environment prerequisite — exactly the "or setup step" clause `IndexerSetup` already covers — so the shipped closed taxonomy is reused without modification.
  A bad revspec is caller input, so it is a usage error, kept distinct so the two surface differently.
- **Alternatives.**
  `General` (exit 1) for the boundary failures — rejected: `General` is "no more specific category," and prerequisite-missing is a specific setup category.
  A new exit code — rejected: it would modify the shipped closed taxonomy for no new caller value.

### PathNarrowingAppliesAfterRenameDetection

Rename detection runs over the whole change; the caller's path narrowing is applied afterwards, against either side's path.

- **Rationale.**
  Handing the pathspec to the same `git diff` that performs rename detection destroys the pre-change side of a renamed file: with the old path filtered out of the diff queue, git has nothing to pair the new path with and reports a plain addition, whose pre-change side is empty.
  The narrowed answer would then silently seed nothing for exactly the file the caller asked about.
  Obtaining the unnarrowed change set first and filtering it afterwards keeps the rename paired, and lets the filter match either the pre-change or the post-change path — which is what "narrowing reaches a renamed file by either path" actually requires.
  The cost is that narrowing becomes a path-prefix match rather than a full git pathspec: a directory or file path selects, but a glob does not.
  That is the narrower and more predictable contract, and it is the one the requirement is written against.
- **Alternatives.**
  Passing both sides' paths as pathspecs — rejected: the caller's paths are what is known up front; the other side's path is only discoverable _from_ the rename detection the pathspec would have already broken.
  Accepting the loss and documenting it — rejected: it contradicts a stated requirement, and the failure is silent.

### ReconstructibilityGatesTheExactClaim

`exact` is claimed only when the pre-change side can actually be reconstructed from the workspace; when it cannot, the answer is approximate.

- **Rationale.**
  The pre-change side is rebuilt by substituting each changed file's base content wholesale.
  That is exactly right when the selected diff accounts for every difference between the base and the file on disk — which the working-tree mode guarantees, since its diff spans base-to-worktree.
  It is wrong when a file carries changes the selected diff does not describe: a staged-mode file with unstaged edits loses those edits in the substitution and lands on the base state, which an index built at the base then matches, yielding `exact` for a workspace the index demonstrably does not describe.
  The same drift in a file the change did not touch is correctly graded approximate, so the whole-file substitution made the grade depend on which file the drift landed in rather than on whether the index matches.
  Detecting the condition — a file in the selected diff that also differs between the index and the worktree, or a range whose head is not what is checked out — and grading approximate is honest without needing a patch engine.
- **Alternatives.**
  Applying the inverse of the selected diff hunk by hunk — rejected for now: it reconstructs the pre-change side precisely, but requires a patch-application engine (or a further `git` invocation that can fail on its own terms) for a case the honest-approximate grade already covers.
  Redefining `exact` so that drift outside the changed files stops counting — rejected: an unrelated file that gained a reference to a seed since the build makes the dependents set incomplete, which is precisely what the grade exists to disclose.

### RecoveryNeverTouchesTheQueriedIndex

The recovery procedure builds into its own throwaway index and re-queries against that; the index the original answer came from is never written.

- **Rationale.**
  `impact` is a query.
  A recovery procedure that rebuilds into the live `--db` path replaces the caller's current index with one built at a historical revision and — after its worktree is removed — leaves that historical index installed, so every later query silently answers from the past.
  A read-only command must not hand the caller a procedure whose side effect is destroying the state it just read.
  Building into a temporary index beside the temporary worktree costs one extra substituted path and leaves nothing behind.
- **Alternatives.**
  Backing the live index up and restoring it — rejected: more steps, and a half-run procedure leaves the caller worse off than not starting.

### RangeRecoveryQueriesFromTheRangeHead

For a revision-range answer, the recovery procedure materializes the range's head as well as its base, and re-runs the assessment from the head.

- **Rationale.**
  Reverting an `A..B` diff from whatever happens to be checked out reconstructs `A` only when the checkout is `B`.
  Anywhere else it yields a hybrid state no index can ever match, so the answer is permanently approximate and the procedure — which rebuilds at `A` and re-runs from the original workspace — recomputes the same hybrid and never converges.
  Running the re-query from a worktree at `B` makes the discovered sources `B`'s tree, so reverting the range's diff yields `A`'s tree and the index built at `A` matches.
  The procedure then delivers the exact answer it promises.
- **Alternatives.**
  Declaring range mode incapable of exactness — rejected: it is reachable, and the honesty layer's whole purpose is to hand the caller the route rather than a dead end.

### WholeWorkspaceHashGatesExactness

The answer is labeled `exact` when the index's whole-workspace content-hash matches the diff's pre-change side, and `approximate` otherwise.
The pre-change side is defined as the current discovered source set with the diff reverted.
The label is global, not per-file.

- **Rationale.**
  The index carries one whole-workspace digest, so drift is detectable but not localizable to individual files.
  Honesty is by structure: an approximate answer is never presented as exact.
- **Computing the pre-change hash.**
  Run the same source discovery `build` runs, then undo exactly what the diff did: substitute a modified file's content with its base side (`git show <base>:<path>`), drop a file the diff added, restore a file it deleted with its base content, and restore a renamed file's content at its pre-change path; hash the result with the same `content_hash`.
  Untracked-but-discovered files (scratch scripts, generated sources) participate with their current content, exactly as `build` saw them — so an untracked file edited after the build correctly demotes the answer to approximate, drift a git-tree hash could never see.
- **Alternatives.**
  Hashing the base git tree (`git ls-tree` + `git show`) — rejected: the tree holds only tracked files, so any untracked source file present at build time mismatches the index hash forever — `exact` becomes unreachable even seconds after a fresh `build`, and a recipe rebuilding from a bare worktree can never converge either.
  Per-file/per-seed exact/approximate marking — deferred: it needs a per-document hash table (an additive schema change) the north star is not yet asking for.
  Refuse-on-mismatch — rejected: the working tree is almost always ahead of the index in the default mode, so refusal would make the flagship mode rarely work.
  Auto-rebuild — rejected: it turns a query into a mutation, is slow, and cannot index an uncompilable working tree anyway.

### ApproximateAnswerCarriesARunnableRecipe

An approximate answer attaches a runnable recovery procedure with the resolved base revision, the index (`--db`) path, and the workspace identity already substituted.

- **Rationale.**
  Telling a user "approximate, rebuild for exactness" without the commands is a poor experience.
  The recipe is the `git worktree` + `build --db --workspace` + `impact --db` composition, emitted with real values.
  Crucially it must pin `--workspace <current>`: `build` derives the workspace id from the root directory name, so a worktree in a differently-named temp dir would namespace symbols wrongly and the seed lookups would miss.
  Generating the recipe (rather than documenting a generic pattern) is what dodges that footgun.
  For the same reason, when untracked source files exist in the workspace the emitted recipe copies them into the temp worktree before `build`: exactness is graded against the current discovered set with the diff reverted, and a bare `git worktree add` materializes only tracked files, so an index rebuilt without the copy step would still fail the gate.
- **Alternatives.**
  A vague nudge — rejected (the poor experience above).
  A built-in `impact --exact` that performs the temp-worktree dance internally — deferred: more defensible than a general `build <sha>` (single-purpose, always a throwaway worktree, self-cleaning), to be reached for only if the emitted recipe proves clunky in dogfooding.

### NoHistoricalBuild

`build` is not extended to take a commit SHA for point-in-time rebuilds.

- **Rationale.**
  It would force `c10r` to own git-tree materialization and environment reconstruction (which venv/toolchain was current at that commit).
  The naive checkout form is destructive to the working tree — hostile to the impact workflow whose point is preserving in-flight edits.
  And it is already composable via `git worktree` + the existing `build <root> --db`, which the recovery recipe emits.
- **Alternatives.**
  `build --sha` — rejected as above.
  The composition stays git's job; `c10r` points at a directory.

### RangeModeDependentsComeFromTheCurrentIndex

For a revision-range seed (`A..B`), the seed maps against the `A`-side, but the dependents are drawn from the single current index; the answer discloses which snapshot the dependents reflect.

- **Rationale.**
  There is only one index.
  The honest claim is "the dependents as the current index holds them, seeded by the changes across the range," and the exact/approximate label already discloses whether that index matches `A`.
- **Alternatives.**
  Requiring an index at `A` or at `B` — rejected: that is the historical-build path already declined; disclosure plus the recovery recipe is the honest, composable answer.

### CursorsBindToTheChangeAsWellAsTheFlags

An impact continuation token's identity includes a digest of the patch the answer was seeded from.

- **Rationale.**
  Every other query's result set is a function of the index, so binding a token to the query flags plus the recorded index identity is sufficient.
  An impact answer is a function of the index _and the diff_, and the diff is not a flag: the working tree can change between two pages, and two different revision ranges sharing a base (`A..B` and `A..C`) present identical flags and identical resolved base revisions.
  Either case resumes a token against a result set it was never issued for — silently dropping or duplicating rows — which is exactly what the token contract exists to prevent.
  The patch text is already in hand when the identity is built, so a digest of it costs one hash.
- **Alternatives.**
  Digesting the resolved seed set instead — rejected: it is narrower than the thing that actually varies (two different patches can share a seed set but differ in the unmappable regions and the freshness grade the page repeats).
  Accepting the collision and documenting it — rejected: a silently wrong page is the failure mode the token machinery was built to eliminate.

### InsertionAnchorsSkipDeclarationEnds

A pure insertion anchors on the pre-change line it follows, except that an anchor on a declaration's final line does not seed that declaration.

- **Rationale.**
  An insertion has no pre-change text, so the anchor is the only evidence of where it landed.
  Anchoring on the preceding line is right for a line added inside a body — the anchor falls on an interior line of the declaration that genuinely changed.
  It is wrong for a declaration added immediately after another one, where the preceding line is the previous declaration's closing brace: the change would report everything depending on a neighbour the caller never touched.
  Whether that happens would otherwise depend on whether a blank line separates the two, letting source formatting decide a blast radius.
  Excluding an anchor that sits on a span's final line separates the two cases exactly: an interior insertion still seeds, a post-declaration insertion does not, and a single-line declaration is its own final line so an insertion after it correctly seeds nothing.
- **Alternatives.**
  Anchoring on the following line instead — rejected: it moves the same error onto the other neighbour.
  Seeding nothing for any pure insertion — rejected: a line added inside a body is a real change to that declaration and its dependents are the answer the caller wants.

### SeedEmptyIsDistinctFromSeedUnmappable

A diff that touches no symbol at all (only comments, blank lines, untracked/generated files) is a typed-empty successful answer.
A changed region in a document the index does not hold is reported `unmappable`.
A region in a document the index _does_ hold, but where no declaration overlaps, is not.

- **Rationale.**
  The two outcomes mean different things to the caller — "your change touches nothing the graph tracks" versus "your change touches a file this index never saw" — and collapsing them would hide the second behind the first.
  The dividing line is drawn at the document rather than at the declaration because that is the finest distinction a span index can actually witness.
  Deciding that a region _would_ have held a declaration requires parsing the pre-change content: the only declaration list available is the index's, and the index is precisely the thing suspected of being stale, so it cannot be its own witness.
  The residual gap — a stale index that holds a file but lacks a declaration changed within it — is bounded by the freshness discipline the commit hook installs, since the two flagship seed modes take their pre-change side from the committed state an at-commit rebuild indexes; and an approximate answer already discloses that declarations may be missing entirely.
- **Alternatives.**
  Parsing the pre-change content with the syntax oracle to obtain independent declaration evidence — deferred: it is the faithful answer, but it introduces a second source of truth about declarations for a case the freshness discipline largely retires.
  Reporting every non-overlapping region as unmappable whenever the answer is approximate — rejected: it makes every comment edit an unresolved region, burying the real signal in noise on exactly the answers that most need to be readable.

### CommitHookInstallation

`c10r` installs the post-commit hook itself rather than only documenting it, and resolves the hook path through `git` rather than assuming `.git/hooks`.

- **Rationale.**
  The honesty layer's advice is "keep the index at the committed state," and the pre-change side of both flagship seed modes _is_ the committed state — so the hook is not a convenience, it is the discipline that keeps the exact path reachable.
  Handing a caller a shell snippet to paste puts the one step that makes the rest trustworthy outside the tool.
  The path is resolved with `git rev-parse --git-path hooks/post-commit` because a linked worktree keeps its hooks in the common directory and `core.hooksPath` can relocate them outright, so a hard-coded `.git/hooks` would write somewhere git never reads.
  Installation refuses rather than overwrites: a commit hook is the caller's, and silently replacing one is a destructive act a query-shaped tool has no license to perform.
- **Alternatives.**
  Documentation only — rejected: it leaves the freshness discipline unautomated in the change whose correctness depends on it.
  Appending to an existing hook — rejected: it edits a file whose contents and shell dialect are unknown; refusing and naming the path lets the caller compose deliberately.
  Rebuilding on a filesystem watch instead of at commit — rejected outright: it would drag the index onto in-flight, possibly uncompilable edits and make nearly every impact answer approximate, inverting the design.

## Architecture

```text
impact <mode> [-- paths]
   │
   ▼
[ git boundary ]  bounded subprocess (deadline + kill), typed failures
   │  git diff (pre-change side) ── renames detected ── full capture (not 64 KiB-capped)
   ▼
[ hunk parse ]    changed (file, line-range) → pre-change byte-range  (git show <base>:<path>)
   │
   ▼
[ seed resolve ]  each byte-range → innermost overlapping declaration(s)  (additive span-overlap
   │              store query, then minimal-under-containment + drop whole-document spans)
   │              union → seed set;  absent-from-index → unmappable
   ▼
[ dependents ]    union of seeds' dependents           (reused traversal, depth bound + horizon)
   │
   ▼
[ honesty layer ] content-hash(diff-reverted set) == index hash ?  exact : approximate (+ recipe)
   │
   ▼
answer  ──►  inherited output contract (bounding, provenance/freshness, JSON, human render)
```

- **New code**: the git boundary, hunk parsing, the honesty/recipe layer (candidate home: `src/query/` alongside the other query surfaces, plus the git boundary as its own module), and a span-overlap symbol lookup on the store — additive, no schema change — because a hunk is a byte range while `get --at` resolves a single point.
- **Reused unchanged**: the position-to-symbol machinery the span-overlap lookup sits beside, the `dependents` traversal, the bounded-subprocess discipline (`src/semantic/probe.rs`), and the whole output contract.

## Risks

- **Diff-output truncation.**
  Reusing `run_bounded`'s 64 KiB discard cap would silently drop a large diff and under-report the seed set.
  Mitigation: capture the diff in full (parameterized cap or temp-file streaming); a regression test seeds a diff larger than 64 KiB and asserts every touched symbol is seeded.
- **Git output parsing fragility.**
  Binary files, mode-only changes, and rename/copy hunk formats can confuse a naive parser.
  Mitigation: non-text and non-symbol hunks map to no symbol and simply contribute nothing to the seed; parsing degrades to "no seed from this hunk" rather than erroring.
- **Byte-offset misalignment on drift.**
  When the index does not match the base, resolved offsets may land on the wrong or no symbol.
  Mitigation: this is exactly the `approximate` path — the answer is labeled, unmappable regions are reported, and the recipe offers exactness.
  Never presented as exact.
- **Exactness unreachable under untracked sources.**
  Grading exactness against the base git tree would make any untracked source file present at build time mismatch the hash forever, turning the flagship exact path into a permanent approximate that even the recovery recipe could not escape.
  Mitigation: the diff-reverted-set definition of the pre-change side plus the recipe's untracked-copy step; a regression test builds with an untracked source file present, makes no further edits, and asserts the gate reports exact.
- **Range-mode snapshot confusion.**
  A caller could read range-mode dependents as "as of B." Mitigation: the answer discloses which snapshot the dependents reflect, and the exact/approximate label states whether the index matches `A`.
- **Whole-file spans swallowing the seed.**
  A file module's definition span covers its entire document, so a plain span-overlap seed would attach every hunk to the file module and report the file's whole dependent set for a comment edit.
  Mitigation: the innermost-declaration seed rule above; a regression test asserts a comment-only change to an indexed file yields the typed-empty answer rather than the module's dependents.
- **Rename/copy depth.**
  Plain rename recognition is in scope; copy detection and cross-file content-moves are not handled beyond it in v1.
  Mitigation: documented boundary; unhandled cases degrade to ordinary add/delete seeding, not incorrect attribution.
