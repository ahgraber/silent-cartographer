# Code Navigation — Delta: impact

## ADDED

### Requirement: Diff-seeded impact assessment

Serves: diff-seeded-blast-radius

The system SHALL provide a CLI command `impact` that assesses the reverse-reachability impact of a change without the caller naming a symbol: it derives a set of seed symbols from a git diff, and returns the union of those seeds' `dependents` as a depth-bounded impact answer carrying the same honest-horizon disclosure a `dependents` trace carries.
The impact set SHALL be exactly the reverse-reachability closure of the seed set — no forward or callee-direction reach — and a change that touches no indexed symbol SHALL be a typed-empty successful answer, distinct from an unavailable or failed one.

#### Scenario: Impact of a change touching one symbol

- **GIVEN** a workspace whose diff changes a single indexed symbol that other declarations depend on
- **WHEN** `impact` is invoked
- **THEN** the dependents of that symbol are returned as a depth-bounded impact answer

#### Scenario: Impact unions the dependents of every touched symbol

- **GIVEN** a workspace whose diff changes several indexed symbols
- **WHEN** `impact` is invoked
- **THEN** the answer is the union of the dependents of all the changed symbols, each dependent reported once with its shortest distance

#### Scenario: A signature change reaches its callers

- **GIVEN** a workspace whose diff changes the signature of an indexed symbol while its callers are unchanged
- **WHEN** `impact` is invoked
- **THEN** those callers appear in the impact answer as dependents of the changed symbol

#### Scenario: A change touching no indexed symbol is typed absence

- **GIVEN** a workspace whose diff touches only comments, blank lines, or files the graph does not track
- **WHEN** `impact` is invoked
- **THEN** an empty impact set is returned as a definite "none", distinct from an unavailable or failed answer

#### Scenario: Impact carries the horizon disclosure

- **GIVEN** a change whose reach extends beyond the requested depth bound
- **WHEN** `impact` is invoked at that depth
- **THEN** the detailed rows stop at the bound and the answer reports the beyond-bound aggregate and its horizon, exactly as a `dependents` trace does

### Requirement: Change seed selection

Serves: scope-the-diff, diff-seeded-blast-radius

The system SHALL seed the impact assessment from a git diff selected by a seed mode — the working-tree change (default), the staged change, or a revision range — optionally narrowed to a set of paths, and SHALL recognize a renamed file as the same file across the change, so that path narrowing reaches it by either its pre-change or post-change path.
The seed SHALL be taken from the change's pre-change side — content and path alike — so that a symbol removed or renamed by the change is still resolved to its prior identity and its dependents are still reported.
Because a revision range's dependents are drawn from the single current index rather than from either endpoint, an answer seeded from a range SHALL disclose which snapshot its dependents reflect.
The change a continuation token was issued against SHALL be part of the query identity that token binds to, so a token presented after the seeding change has moved is rejected rather than resumed against a different change's results.

#### Scenario: A range answer discloses which snapshot its dependents reflect

- **GIVEN** a revision range whose seeds resolve against the range's pre-change side
- **WHEN** `impact` is invoked for that range
- **THEN** the answer states that the dependents reported are those the current index holds, not those of either endpoint

#### Scenario: A continuation token is refused after the change moves

- **GIVEN** a truncated impact answer and its continuation token
- **WHEN** the seeding change is altered and the token is presented again
- **THEN** it is rejected as a usage error rather than resumed against the altered change's results

#### Scenario: Working-tree change seeds the assessment

- **GIVEN** a workspace with uncommitted edits to an indexed symbol
- **WHEN** `impact` is invoked in its default mode
- **THEN** the seed is the symbol touched by the working-tree change and its dependents are returned

#### Scenario: Staged mode seeds only staged changes

- **GIVEN** a workspace with both staged and unstaged edits to different indexed symbols
- **WHEN** `impact` is invoked in staged mode
- **THEN** the seed is drawn only from the staged change, and the unstaged edit does not contribute to the seed

#### Scenario: Revision-range mode seeds from the range

- **GIVEN** two revisions between which an indexed symbol changed
- **WHEN** `impact` is invoked for that revision range
- **THEN** the seed is the symbol changed across the range and its dependents are returned

#### Scenario: Path narrowing restricts the seed

- **GIVEN** a change touching indexed symbols in two directories
- **WHEN** `impact` is invoked narrowed to one of those directories
- **THEN** only symbols under that path contribute to the seed

#### Scenario: A deleted symbol still resolves to its dependents

- **GIVEN** an index matching the change's pre-change state, and a change that deletes an indexed symbol other declarations depended on
- **WHEN** `impact` is invoked
- **THEN** the deleted symbol is resolved from the pre-change side and its dependents appear in the impact answer

#### Scenario: Path narrowing reaches a renamed file by its post-change path

- **GIVEN** an index matching the change's pre-change state, and a change that renames a file into a directory and edits an indexed symbol within it
- **WHEN** `impact` is invoked narrowed to the file's post-change directory
- **THEN** the edited symbol is still seeded from its pre-change side, rather than the narrowing reducing the change to an addition with no pre-change side

#### Scenario: A renamed file's changes still map

- **GIVEN** an index matching the change's pre-change state, and a change that renames a file and edits an indexed symbol within it
- **WHEN** `impact` is invoked
- **THEN** the edited symbol is resolved at its pre-change path and its dependents appear in the impact answer

### Requirement: Git seed as an external boundary

Serves: honest-diff-reach, scope-the-diff

The system SHALL treat obtaining the diff as an external boundary with typed failures: a git executable that is absent, a directory that is not a git worktree, and a git subprocess that does not complete within a bounded time SHALL each be reported as a typed environment failure through the exit-code taxonomy, distinct from a malformed or unresolvable revision specification, which SHALL be reported as a usage error before any traversal is attempted.

#### Scenario: Absent git executable is a typed environment failure

- **GIVEN** an environment with no usable `git` executable
- **WHEN** `impact` is invoked
- **THEN** the failure names the missing tool and exits through the environment/setup-failure code, not as a usage error

#### Scenario: Not a git worktree is a typed environment failure

- **GIVEN** a directory that is not inside a git worktree
- **WHEN** `impact` is invoked
- **THEN** the failure states that the directory is not a git worktree and exits through the environment/setup-failure code

#### Scenario: A hung git subprocess times out

- **GIVEN** a `git` invocation that does not respond within the bounded time
- **WHEN** `impact` is invoked
- **THEN** it reports a bounded timeout rather than hanging, and exits through the environment/setup-failure code

#### Scenario: An unresolvable revision spec is a usage error

- **GIVEN** a revision range that does not resolve in the repository
- **WHEN** `impact` is invoked for it
- **THEN** the invocation is rejected as a usage error before any traversal, distinct from an environment failure

### Requirement: Impact freshness across the straddle

Serves: honest-diff-reach

Because the index is a point-in-time snapshot that may not match the change being asked about, the system SHALL label every impact answer as either exact — the index matches the change's pre-change state, meaning the workspace's discovered sources with the change reverted — or approximate — it does not.
A change whose pre-change side the system cannot reconstruct from the workspace SHALL be labeled approximate rather than assumed to match.
A changed region in a document the index does not hold SHALL be reported as unmappable rather than silently dropped; a region the index holds a document for but no declaration at SHALL NOT be reported unmappable, because a span index cannot distinguish a region that never held a declaration from one whose declaration this index is missing, and an approximate answer already discloses that declarations may be absent entirely.
A changed file whose pre-change content cannot be obtained at all SHALL be reported unmappable in whole, whether or not the index holds that document: no region within it can be located, so this is not the case above of a located region that no declaration overlaps.
An approximate answer SHALL carry a runnable recovery procedure that names the resolved base revision, the index location, and the workspace identity needed to produce an exact answer, so the caller is never left to reconstruct those by hand.
The procedure SHALL NOT modify the index the query was answered from, and following it SHALL yield an exact answer for the seed mode it was emitted for.

#### Scenario: A change whose pre-change side cannot be reconstructed is approximate

- **GIVEN** a staged change to a file that also carries unstaged edits, so reverting the staged change alone does not reconstruct the file's pre-change content
- **WHEN** `impact` is invoked in staged mode
- **THEN** the answer is labeled approximate rather than exact

#### Scenario: The recovery procedure leaves the queried index untouched

- **GIVEN** an approximate impact answer whose recovery procedure has been followed to completion
- **WHEN** the index the original query was answered from is inspected
- **THEN** it is unchanged, rather than replaced by an index built at the change's base revision

#### Scenario: Following the recovery procedure yields an exact answer

- **GIVEN** an approximate impact answer for a revision range
- **WHEN** its recovery procedure is followed and the assessment re-run as the procedure directs
- **THEN** the re-run answer is labeled exact

#### Scenario: Matching index yields an exact answer

- **GIVEN** an index whose content matches the change's pre-change state
- **WHEN** `impact` is invoked
- **THEN** the answer is labeled exact

#### Scenario: An untracked source file does not forfeit exactness

- **GIVEN** an index built while an untracked source file was present, with no edits since apart from the change under assessment
- **WHEN** `impact` is invoked
- **THEN** the answer is labeled exact

#### Scenario: Mismatched index yields an approximate answer

- **GIVEN** an index whose content does not match the change's pre-change state
- **WHEN** `impact` is invoked
- **THEN** the answer is labeled approximate rather than presented as exact

#### Scenario: A changed symbol absent from the index is reported unmappable

- **GIVEN** an approximate run in which a changed region maps to no symbol present in the index
- **WHEN** `impact` is invoked
- **THEN** that region is reported as unmappable rather than omitted without trace

#### Scenario: A file whose pre-change content cannot be obtained is unmappable in whole

- **GIVEN** a change to a file the index holds a document for, whose pre-change content cannot be obtained
- **WHEN** `impact` is invoked
- **THEN** the whole file is reported unmappable, rather than its regions being dropped as merely overlapping no declaration

#### Scenario: An approximate answer carries a runnable recovery recipe

- **GIVEN** an approximate impact answer
- **WHEN** its disclosure is rendered
- **THEN** it includes a runnable recovery procedure naming the resolved base revision, the index location, and the workspace identity that together produce an exact answer
