# Proposal: impact

> Status: PROPOSAL — delta specs, design.md, and tasks.md generated alongside this document.
> Depends on the CLI-alignment change, now SHIPPED; inherits its output contract wholesale.

## Intent

`c10r` can already tell you what depends on a symbol once you name that symbol (`trace <symbol> --relation dependents`).
But the person most in need of that answer is the one who just made an edit and does not yet know which symbols they touched — they have a diff, not a symbol id.
This change closes that gap with a new top-level verb, `impact`: it seeds the existing reverse-reachability dependents traversal from a git diff, so someone who just changed code can ask "what could my actual edits affect?"
and point review and testing where the change really reaches, instead of guessing symbol by symbol.

The reverse-reachability traversal and the position-to-symbol resolution already exist and are reused unchanged.
What is net-new — and why this is its own change — is the git integration as a new external boundary (where correctness and safety come first, CLAUDE.md directive #1) and the honesty layer that keeps a diff-seeded answer calibrated when the index and the working tree have drifted apart.

## User Stories

### Story: diff-seeded-blast-radius

As someone who just made edits, I want to see what my actual diff could affect — the dependents of every symbol my changes touched — so that I focus review and testing where the change really reaches rather than on the files I happened to open.
Ladders to north-star outcome #2 (Blast radius before change): the dependents assessment, reached from the change I actually made instead of from a symbol I have to name first.

### Story: honest-diff-reach

As someone reviewing a diff mid-edit, I want the diff-seeded impact to carry the same freshness and horizon disclosure every other answer does — and, when my index no longer matches the change I am asking about, to be told plainly that the reach is approximate and handed the exact steps to make it exact — so that I am never quietly given a confident-but-wrong reach for an in-flight change.
Ladders to north-star outcome #2 (Blast radius before change) and #4/#5 (Honest under edit / Calibrated trust): the seed is edited-but-not-yet-settled code, so the answer must stay calibrated rather than overclaim, and the tool must keep me in its exact path rather than let me drift into silent degradation.

### Story: scope-the-diff

As someone working on a branch or a staged subset, I want to choose which diff seeds the assessment — working tree, staged changes, or a revision range, optionally narrowed to paths — so that the reach I see matches the change I actually mean to reason about.
Ladders to north-star outcome #2 (Blast radius before change): controlling the seed is controlling which change's blast radius is being asked for.

## Scope

**In scope:**

- A new top-level `impact` command: seed the dependents traversal from the symbols a git diff touched, and return the union of their dependents with the same depth-bounded, honest-horizon disclosure `trace --relation dependents` already gives.
- A git-diff seed as a new external boundary: shell to `git diff`, parse its hunks into changed `(file, byte-range)` pairs, and map each range to the symbol(s) it falls inside — reusing the existing position-to-symbol resolution.
- Reverse-reachability only: the union of the seeds' `dependents` (over `uses` + `imports` + `type_hierarchy`), with the existing depth bound and horizon disclosure.
  Forward/callee reach is out of scope.
- Seed from the diff's **pre-change side**, so a symbol deleted or renamed by the change is still resolved to its old identity and its dependents are still found.
- Seed modes: the working-tree diff (default, base = `HEAD`), the staged diff (base = `HEAD`), and a revision range (`A..B`, `A...B`, a single commit), with optional path narrowing that mirrors `git diff -- <paths>`, recognizing a renamed file by either side's path.
- Typed errors at the git boundary — git-not-installed, not-a-git-worktree, and a git subprocess that exceeds a bounded time — kept distinct from a malformed or unresolvable revision spec, which is a usage error.
  A genuinely empty diff (or a diff that touches no indexed symbol) is a successful empty answer, not a failure.
- Freshness honesty across the straddle: when the index matches the diff's pre-change side, the answer is exact; when it does not, the answer is labeled **approximate**, any changed symbol absent from the index is reported **unmappable** rather than silently dropped, and the answer carries a runnable recovery recipe — with the resolved base revision, the index path, and the workspace identity already substituted — that produces an exact answer.
- A command that installs the post-commit hook which refreshes the index, resolving the hook path through `git` and refusing to overwrite an existing hook.
  The hook is not a convenience here: the pre-change side of both flagship seed modes is the committed state, so an index refreshed at each commit is what keeps the exact path reachable rather than leaving every answer approximate.
- A README section (or docs page) on the most effective patterns for using `c10r`, including building after every commit or wiring `build` as a commit hook, so the index stays aligned to `HEAD` and `impact` (and every other query) stays in its exact, fully-trustworthy path.

**Out of scope:**

- Any change to the reverse-reachability traversal itself — `QueryEngine::dependents` and the position-to-symbol resolution are reused as-is, not reworked here.
- Forward impact, callee-direction reach, or bidirectional analysis.
  Reverse-reachability seeded from a diff is the whole of this change; forward/graph-diff reach is a possible future capability, not this one.
- The output contract — bounding (`--limit` / `--cursor` / content windowing), human rendering, the exit-code taxonomy, stdout/stderr discipline — is inherited from the CLI-alignment change, not redefined here; `impact` adds only the git boundary's own failure modes on top.
- Extending `build` to take a commit SHA for point-in-time rebuilds.
  It would force `c10r` to own git-tree materialization and environment reconstruction, the naive form is destructive to the working tree, and it is already composable via `git worktree` plus the existing `build <root> --db`.
  The recovery recipe emits exactly that composition.
  An opt-in `impact --exact` that performs the temp-worktree dance internally is noted as a deferred follow-up, to be reached for only if the recipe proves clunky in dogfooding.
- Per-file freshness localization.
  The index's content-hash is a single whole-workspace digest, so drift is disclosed globally (this whole answer is exact / approximate), not per-file.
  Per-document hashing is a possible later refinement, not scoped here.
- Cross-repository or multi-worktree diffs — the focus stays one codebase (north-star scope boundary).
- Any config file or new configurable default.

## Approach

`impact` is a composition of parts that already exist plus one new boundary:

1. **New boundary** — obtain a diff by invoking `git diff` in the requested mode (working tree / staged / revspec, optionally path-narrowed), bounded by a timeout, and parse its hunks into changed `(file, byte-range)` pairs against the pre-change side, resolving a renamed file at its pre-change path.
2. **Reuse** — map each changed range to its enclosing symbol via the existing position-to-symbol resolution, exactly as `get --at` does.
3. **Reuse** — take the union of those seed symbols' dependents through the existing `dependents` traversal, preserving its depth bound, per-symbol shortest-hop collapse, and horizon disclosure.
4. **Honesty layer** — compare the index's whole-workspace content-hash against the diff's pre-change side; on a match the answer is exact, on a mismatch it is labeled approximate, unmappable seeds are reported, and a runnable, fully-substituted recovery recipe is attached.

**Verb, not a `trace` mode.**
`impact` is its own top-level command rather than a diff seed-mode of `trace`.
It keeps the just-shipped `trace` contract untouched (additive), it is discoverable as a first-class capability, and its answer shape (a seed set plus a dependents union) genuinely differs from `trace`'s subject-plus-rows.

**Git-boundary timeout** reuses the bounded-probe pattern introduced by CLI-alignment (`src/semantic/probe.rs`), so a hung `git` degrades to a typed timeout rather than an indefinite hang on the query's hot path.

This proposal fixes the diff-to-dependents spine, the git boundary's error taxonomy, and the straddle-honesty contract; it leaves the finer mechanism (hunk parsing, byte-range derivation from git's line-oriented output, seed-union assembly, and the exact recovery-recipe rendering) to `design.md`.

## Open Questions

- **Git-boundary failure → exit code.**
  Which taxonomy code carries git-absent / not-a-worktree / timeout — the existing indexer/setup-failure code (git as an unmet environment prerequisite) or the generic-failure code — while a bad revspec stays a usage error.
  To be pinned in `design.md` without modifying the shipped command-surface exit-taxonomy requirement.
- **Range-mode dependents snapshot.**
  For `A..B`, the seed maps against the `A`-side, but the dependents come from the single current index.
  Which snapshot's dependents the answer claims (and how loudly it discloses that) needs a design paragraph; the working-tree/staged modes do not have this wrinkle.
- **Rename/copy depth.**
  Following a plain rename to its post-change path is in scope; whether copy detection and content-move (a symbol relocated across files) need any handling beyond plain rename-follow, or are explicitly deferred, is open.
- **Byte-range derivation.** `git diff` is line-oriented; deriving pre-change-side byte offsets that align with the indexed spans (e.g. via `git show <base>:<path>`) is a mechanism detail for `design.md`.
- **Seed-empty vs. seed-unmappable.**
  A diff that touches only non-symbol regions (comments, blank lines, generated files) yields an empty seed set; whether that is reported identically to an empty diff, or distinguished so the caller can tell "no change" from "changed nothing the graph tracks," is open.
