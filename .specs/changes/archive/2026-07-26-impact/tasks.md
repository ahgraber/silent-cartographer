# Tasks: impact

Groups run in build-dependency order: the git boundary first (the new external surface), then hunk parsing down to pre-change byte ranges, then the store's span-overlap lookup and seed resolution, then the dependents assembly, then the freshness/recovery layer that grades the answer, and finally the `impact` command that composes them behind the inherited output contract, with the usage-patterns documentation last.

## Git boundary

- [x] Add a full-capture variant of the bounded-subprocess helper: the same deadline + kill + reader-abandon discipline as `run_bounded` (`src/semantic/probe.rs`), with the excess-discarding 64 KiB capture cap parameterized so the diff payload is captured in full within the deadline; preflight commands keep using `run_bounded` unchanged.
- [x] Add a git boundary module (its own module, per design) owning every `git` invocation: preflight `git rev-parse --is-inside-work-tree`, resolve the base revision per seed mode (working tree default → `HEAD`, staged → `HEAD`, revspec → its pre-change side), and type the failures — git-absent, not-a-git-worktree, and probe timeout as setup failures (the `IndexerSetup` category), distinct from a malformed or unresolvable revspec as a usage failure rejected before any traversal.
- [x] Invoke `git diff` for the selected mode with rename detection enabled and optional trailing path narrowing (pathspecs reaching a renamed file by either side's path), capturing the full patch through the full-capture helper.
- [x] Test: the full-capture helper returns a payload larger than 64 KiB intact within the deadline, and still kills + reaps a child that exceeds the deadline (reader threads abandoned, never joined on the timeout path).
- [x] Test (unit, boundary module): not-a-worktree produces its typed setup failure, and an unresolvable revspec — including one whose dash-prefixed endpoint hides inside a range — produces the typed usage failure with no diff invoked.
  Git-absent and a hanging git stub need the executable resolution itself replaced, which is reachable only by handing a child process a stubbed `PATH`; both are covered by the process-level boundary test below rather than from a unit test that would have to mutate this process's own environment.

## Hunk parsing to pre-change ranges

- [x] Parse the unified diff into per-file pre-change line ranges: the pre-change path is the resolution path (a rename's post-change path retained only for narrowing and display), hunk headers yield the pre-change line ranges, a deleted file yields its removed ranges, and binary, mode-only, and added-file entries contribute no pre-change range (degrade to no seed, never an error).
- [x] Derive byte ranges: fetch each changed file's pre-change content via `git show <base>:<path>` (bounded, full-capture), build a line→byte-offset table, and map each hunk's pre-change line range to a byte range aligned with the indexed spans.
- [x] Test: a rename-plus-edit diff parses to ranges at the pre-change path with the post-change path retained for display; binary and mode-only entries yield no range; a deleted file yields its ranges; an added file yields none.
- [x] Test: line-to-byte mapping is exact over multibyte (UTF-8) content and CRLF line endings.

## Span-overlap seed resolution

- [x] Add an additive store query returning the symbols whose definition spans overlap a `(document, byte-range)`, deterministically ordered — no schema change; the point-resolution behind `get --at` stays untouched beside it.
- [x] Narrow each range's overlapping candidates to the innermost touched declarations per design: keep only those minimal under span containment, and drop any whose span covers the pre-change document end to end (a file module's span), so a whole-file span never turns an edit into the file's entire dependent set.
- [x] Resolve each pre-change range at its pre-change path through the overlap query and classify the outcome: the narrowed symbols join the seed union (deduplicated across hunks); a range overlapping no symbol in a document the index holds contributes nothing, benignly; a range in a document the index does not hold is recorded unmappable for disclosure.
- [x] Test: a hunk spanning two symbols seeds both; a hunk inside a method seeds the method rather than its type or its file module; a hunk in the gap between symbols seeds nothing without error; a symbol touched by two hunks is seeded once; a range in a document absent from the index is recorded unmappable.

## Impact assembly

- [x] Compose the impact answer: run the existing `dependents` traversal from each seed (depth bound preserved), merge into one union reporting each dependent once at its shortest distance across all seeds, carry the horizon/beyond-bound disclosure through, and make a diff yielding no seeds and no unmappables a typed-empty success.
- [x] Test: a multi-seed union reports a shared dependent once at its shortest distance; the horizon disclosure appears when reach exceeds the depth bound; a comment-only diff yields the typed-empty answer.
- [x] Test: against an index matching the pre-change state, a deleted symbol resolves from the pre-change side and its dependents appear in the answer.

## Freshness and recovery

- [x] Compute the pre-change hash as the diff-reverted set: run the same source discovery `build` runs, revert the diff on the discovered set (substitute a modified file's base content via `git show`, drop an added file, restore a deleted file, restore a renamed file at its pre-change path), hash with the same `content_hash`, and label the answer exact on a match with the index's stored hash, approximate otherwise.
- [x] Generate the recovery recipe on an approximate answer: the `git worktree add` + `build <tmp> --db <path> --workspace <id>` + re-run composition with the resolved base revision, index path, and workspace identity substituted with real values, including a copy step for untracked discovered sources exactly when any exist.
- [x] Test: an index matching the diff-reverted set labels exact; an unrelated file drifted since the build labels approximate; an untracked source file present at build time with no edits since still labels exact (design-risk regression); an untracked source file edited after the build labels approximate.
- [x] Test: the emitted recipe carries the substituted base revision, `--db` path, and `--workspace` identity, includes the untracked-copy step exactly when untracked discovered sources exist, and unmappable seeds are disclosed on the approximate answer rather than dropped.

## The `impact` command

- [x] Add the `impact` command to the clap surface: an optional revspec positional, a staged-mode flag (mutually exclusive with the revspec), trailing path narrowing after `--`, the inherited `--depth`/`--limit`/`--cursor`/`--json`/`--color` and global `--db`/`--workspace`; argument validation runs before any side effect.
- [x] Wire dispatch: boundary → parse → resolve → assemble → freshness; map the typed boundary failures onto the closed exit taxonomy (setup failures exit `5`, revspec usage exit `2`); serialize the answer (seed set, unmappables, freshness label plus recipe, dependents union) through the inherited output contract with the human render as a projection of the same value.
- [x] Page the answer's dependent rows under the effective `--limit` through the shared dependents pagination, repeating the summary and freshness disclosures on every page.
- [x] Bump `SURFACE_VERSION` and regenerate the surface snapshot for the new command.
- [x] Test (process): editing an indexed symbol in the working tree returns its dependents in the default mode; a signature-only edit returns the symbol's callers; staged mode seeds only the staged edit when an unstaged edit exists; a revision range seeds the symbol changed across the range; path narrowing restricts the seed to the named directory.
- [x] Test (process): with an index matching the pre-change state, a rename-plus-edit resolves the symbol at its pre-change path and returns its dependents.
- [x] Test (process): absent git (PATH stub) exits `5` naming the missing tool; a non-worktree directory exits `5`; a hanging git stub exits `5` within a bounded window; a bad revspec exits `2` before any traversal; a comment-only diff exits `0` with the typed-empty answer.
- [x] Test (process): `--json` and the human render present the same results in the same order; an over-limit impact answer is capped with truncation disclosed and the cursor resumes exactly; a diff larger than 64 KiB seeds every touched symbol (capture-cap regression).

## Boundary and seeding corrections

- [x] Disable textconv on the diff invocation, so hunk line numbers describe the file's real bytes rather than a driver's rewritten view of them, with a regression test that a configured driver's output never reaches the patch.
- [x] Separate a path absent at the base revision from an operational failure reading it: only a genuine "no such path in that revision" reads as absence, and a corrupt object, unreadable file, or permission failure surfaces as a boundary failure instead of a silently unresolvable region.
- [x] Apply path narrowing after rename detection rather than through the diff invocation: obtain the unnarrowed change set, then keep entries whose pre-change or post-change path is under a narrowing path.
- [x] Test: narrowing to a renamed file's post-change directory still seeds the symbol from its pre-change side (the narrowed-to-an-addition regression), and narrowing to its pre-change directory does too.
- [x] Exclude an insertion anchor that lands on a declaration's final line from seeding that declaration, so a declaration added after a closing brace does not report its neighbour's dependents.
- [x] Test: a line inserted inside a body still seeds that declaration; a declaration inserted immediately after another seeds neither it nor its neighbour, with and without a blank line between them.

## Answer identity and disclosure

- [x] Bind an impact continuation token to a digest of the patch it was seeded from, alongside the seed mode, resolved base revision, and narrowing paths.
- [x] Test: a token is refused after the seeding change is altered, and two revision ranges sharing a base revision do not share a token identity.
- [x] Disclose, on a range-seeded answer, that the dependents reported are those the current index holds rather than either endpoint's, in the machine answer and its human projection alike.
- [x] Test: a range-seeded answer carries the snapshot disclosure and a working-tree-seeded one does not.
- [x] Narrow unmappability to the document level: a region in a document the index does not hold is unmappable, a region in a held document with no overlapping declaration is not, and an approximate answer discloses that declarations may be missing entirely.
- [x] Test: a held document's non-overlapping region produces no unmappable region, and the approximate answer's disclosure states that declarations may be absent.

## Reconstructibility and recovery

- [x] Grade approximate when the pre-change side cannot be reconstructed: a file in the selected diff that also differs between the index and the worktree (staged mode), or a revision range whose head is not what is checked out.
- [x] Test: a staged change to a file that also carries unstaged edits grades approximate, matching the grade the same drift in an untouched file already produces.
- [x] Build the recovery recipe into its own throwaway index rather than the queried one, so following it never replaces the caller's index with one built at a historical revision.
- [x] Carry the workspace's prefix within the repository into the recipe, so a workspace below the repository root is rebuilt from its own directory in the temporary worktree rather than from the repository root.
- [x] Copy the discovered sources git does not track — ignored ones included — into the temporary worktree by explicit path, and pass each name to the copy step as an argument rather than by textual substitution so an option-looking or quoted name cannot break it.
- [x] For a range-seeded answer, materialize the range's head as a second throwaway worktree and re-run the assessment from there, so reverting the range's diff reconstructs its base and the procedure converges.
- [x] Test: the recipe leaves the queried index byte-for-byte unchanged; following it end to end for a revision range yields an exact answer; the copy step reaches a gitignored discovered source.

## Commit-hook installation

- [x] Add the hook-installation command to the clap surface, resolving the hook path through `git rev-parse --git-path hooks/post-commit` so a linked worktree or a relocated hook path is honored rather than assumed.
- [x] Write an executable hook that refreshes the index, report the path written, and refuse rather than overwrite when a hook already exists there, naming what was found; installing outside a git worktree raises the same typed environment failure the assessment does.
- [x] Bump `SURFACE_VERSION` and regenerate the surface snapshot for the new command.
- [x] Test (process): installation writes an executable hook and reports its path; an existing hook is left byte-for-byte intact and the refusal names it; installing outside a worktree exits through the environment/setup-failure code; a linked worktree receives its hook where git actually resolves it.

## Documentation

- [x] Add the README usage-patterns section: build after every commit (or as a post-commit hook) so the index tracks `HEAD` and `impact` (and every other query) stays in its exact path, with the impact workflow shown end to end.

## Verification remediation

Raised by `sdd-verify` and resolved in place; the delta specs above carry the one contract clarification.

- [x] Enter the workspace's own directory inside the head worktree on a range recipe's re-run, as the build step already does inside the base worktree, so a workspace below the repository root converges rather than recomputing a hybrid the index can never match.
- [x] Test: the emitted re-run step carries the workspace prefix, and following a range recipe end to end from a workspace below the repository root yields an exact answer.
- [x] Test: following the recipe end to end converges to an exact answer for the working-tree and revision-range modes alike — every step executed as emitted except the build, whose root, index path, and workspace identity are read out of the emitted build step itself so a misnamed target still fails.
- [x] Carry the caller's workspace root, `--db`, and `--workspace` into the installed hook as resolved absolute values, since git runs a commit hook from the worktree's top level rather than from the caller's directory or the workspace.
- [x] Test: the installed hook names the resolved root, index path, and workspace identity; executing it invokes `build` with exactly those arguments.
- [x] Refuse installation on a dangling symlink at the hook path, which an existence check follows and would write through.
- [x] Test: a relocated `core.hooksPath` receives the hook, installation from a subdirectory lands where git resolves it, and a dangling symlink is refused with nothing written through it.
- [x] State that a changed file whose pre-change content cannot be obtained is unmappable in whole, distinguishing it from a located region that no declaration overlaps — the region cannot be located at all, so the span-index rationale for staying silent does not apply.
- [x] Test: the reconstructibility arms a bare single-revision spec and a range at the checkout with an uncommitted source edit take; a continuation token issued for one of two ranges sharing a base is refused for the other.
- [x] Test: the impact set carries no callee-direction reach, with the seed's outgoing edges guarded as a premise so the assertion cannot pass vacuously.
- [x] Test: every page of a capped answer repeats its exactness label, recovery recipe, and unmappable regions; the human render projects the exactness label, the recovery steps, and the unresolvable-regions block.
- [x] Test: an operational failure reading a path at the base revision surfaces as a boundary failure rather than as absence; the bounded-timeout and absent-git diagnostics are pinned to their own wording rather than to a substring the other also carries.
