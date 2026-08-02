# Code Navigation Specification

## Purpose

Defines the query surface over the code graph: resolving a symbol reference across identity, qualified name, and shortname tiers; retrieving a symbol at a chosen detail; tracing named relations; and the calibrated output contract every answer carries.

## Requirements

### Requirement: Symbol retrieval at a chosen detail

The system SHALL provide a CLI command `get` that, given a symbol identified either by name or by a source position, returns that symbol at a requested detail level — its location, its signature, its interface (the signature together with the symbol's own documentation), or its full source body — where retrieval by position resolves to the symbol enclosing that position, and where a symbol carrying no documentation serves its signature as its interface.

#### Scenario: Retrieve definition location by name

- **GIVEN** an indexed Rust symbol
- **WHEN** `get` is invoked for it at location detail
- **THEN** the symbol's definition file and position are returned

#### Scenario: Retrieve full body by name

- **GIVEN** an indexed Rust symbol
- **WHEN** `get` is invoked for it at body detail
- **THEN** the returned text equals the symbol's source span byte-for-byte

#### Scenario: Retrieve signature by name

- **GIVEN** an indexed Rust symbol that has a declaration distinct from its body
- **WHEN** `get` is invoked for it at signature detail
- **THEN** the symbol's signature is returned without its full body

#### Scenario: Retrieve interface by name

- **GIVEN** an indexed Rust symbol carrying a doc comment
- **WHEN** `get` is invoked for it at interface detail
- **THEN** the doc comment and the signature are returned without the full body

#### Scenario: Interface without documentation falls back to signature

- **GIVEN** an indexed symbol with no documentation of its own
- **WHEN** `get` is invoked for it at interface detail
- **THEN** the symbol's signature is returned

#### Scenario: Retrieve module interface

- **GIVEN** an indexed module whose document carries module documentation
- **WHEN** `get` is invoked for the module at interface detail
- **THEN** the module's signature and its module documentation are returned without the whole document

#### Scenario: Retrieve module body returns the whole document

- **GIVEN** an indexed module
- **WHEN** `get` is invoked for the module at body detail
- **THEN** the returned text equals the module's document byte-for-byte

#### Scenario: Retrieve by position resolves the enclosing symbol

- **GIVEN** a source position inside a method body
- **WHEN** `get` is invoked for that position
- **THEN** the symbol enclosing the position is returned

#### Scenario: Retrieve full body of a Python symbol

- **GIVEN** an indexed Python function whose body spans several indentation levels
- **WHEN** `get` is invoked for it at body detail
- **THEN** the returned text equals the function's source span byte-for-byte, indentation included

#### Scenario: Retrieve Python interface

- **GIVEN** an indexed Python function with a docstring
- **WHEN** `get` is invoked for it at interface detail
- **THEN** the function's header and docstring are returned without the full body

#### Scenario: Retrieve Python symbol by position

- **GIVEN** a source position inside a Python method body
- **WHEN** `get` is invoked for that position
- **THEN** the enclosing method is returned

### Requirement: Relationship trace

The system SHALL provide a CLI command `trace` that, given a subject symbol and a relation kind, returns the symbols standing in that relation to the subject.
Relation kinds follow a shared-root directional convention (e.g. `containers`/`contains`), and the supported relations are `containers` (the declaration that directly encloses a subject), `contains` (the symbols a subject directly contains), `references` (the sites that reference a subject, which for a type subject are its type-occurrences), `dependents` (the symbols that depend on the subject directly or transitively, subject to a depth bound), `importers` (the modules that import the subject), `implementers` (the types that declare the subject as a supertype — a trait's implementors or a base type's subtypes), and `tests` (the reference sites of a subject whose enclosing declaration is classified test code — the answer to "what test code exercises this symbol").
The command's self-description SHALL present `dependents` as impact assessment — the answer to "what could break if this symbol changes" — and SHALL present `tests` as convention-based classification rather than resolved semantic fact.

#### Scenario: Trace contains

- **GIVEN** an indexed Rust type with several members
- **WHEN** `trace` is invoked for it over the `contains` relation
- **THEN** exactly its directly contained members are returned

#### Scenario: Trace containers

- **GIVEN** an indexed Rust method declared inside a type
- **WHEN** `trace` is invoked for it over the `containers` relation
- **THEN** the enclosing type is returned

#### Scenario: Trace references

- **GIVEN** an indexed Rust symbol used in several places
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** every reference site is returned and none is omitted

#### Scenario: References of a type are its occurrences

- **GIVEN** an indexed Rust type used at several sites
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** the type's use sites are returned with their locations

#### Scenario: Trace dependents

- **GIVEN** an indexed Rust symbol that other declarations use
- **WHEN** `trace` is invoked for it over the `dependents` relation
- **THEN** the depending symbols are returned, each labeled with the kind of dependency that connected it and its distance from the subject

#### Scenario: Trace dependents of a Python symbol

- **GIVEN** an indexed Python function used by other functions and imported by other modules
- **WHEN** `trace` is invoked for it over the `dependents` relation
- **THEN** the depending symbols are returned, each labeled with the kind of dependency that connected it and its distance from the subject

#### Scenario: Trace importers

- **GIVEN** an indexed module imported by several other modules
- **WHEN** `trace` is invoked for it over the `importers` relation
- **THEN** exactly the modules that import it are returned

#### Scenario: Trace implementers

- **GIVEN** an indexed trait implemented by several types
- **WHEN** `trace` is invoked for it over the `implementers` relation
- **THEN** exactly the types that declare it as a supertype are returned

#### Scenario: Trace tests of a Rust symbol

- **GIVEN** an indexed Rust symbol referenced both from a test-classified function and from a production function
- **WHEN** `trace` is invoked for it over the `tests` relation
- **THEN** exactly the reference sites whose enclosing declaration is classified test code are returned, and the production site is not

#### Scenario: Tests reach through a shared helper

- **GIVEN** an indexed symbol referenced only from a test-classified helper declaration
- **WHEN** `trace` is invoked for it over the `tests` relation
- **THEN** the helper's reference site is returned rather than an empty answer

#### Scenario: Module-scope test references count

- **GIVEN** an indexed symbol named by an import statement at module scope in a test-classified document
- **WHEN** `trace` is invoked for it over the `tests` relation
- **THEN** that module-scope reference site is returned, attributed to the document's module

#### Scenario: Trace tests of a Python symbol

- **GIVEN** an indexed Python function referenced from a declaration in a test-classified document
- **WHEN** `trace` is invoked for it over the `tests` relation
- **THEN** that reference site is returned with its location

#### Scenario: Empty relation is typed absence

- **GIVEN** an indexed symbol that stands in no instance of the requested relation
- **WHEN** `trace` is invoked for it over that relation
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer

### Requirement: Trace results at a chosen detail

The system SHALL support, on any relationship trace, a per-query detail level that projects each returned result at a chosen tier — location, signature, interface, or body — with location as the default; a result that denotes a symbol SHALL project that symbol's own tier content, a result that denotes a reference site SHALL project the tier content of the declaration the site is attributed to, and the chosen detail SHALL NOT alter which results are returned or their order.

#### Scenario: Default trace rows carry no tier content

- **GIVEN** an indexed symbol referenced in several places
- **WHEN** `trace` is invoked over the `references` relation with no detail requested
- **THEN** each row identifies its symbol and location and carries no tier content

#### Scenario: Trace at signature detail

- **GIVEN** an indexed symbol referenced in several places
- **WHEN** `trace` is invoked over the `references` relation at signature detail
- **THEN** each row carries the signature tier of its symbol

#### Scenario: Reference sites project their enclosing declaration

- **GIVEN** an indexed symbol referenced inside a method body
- **WHEN** `trace` is invoked over the `references` relation at signature detail
- **THEN** the row for that site carries the signature tier of the method the site is attributed to

#### Scenario: Trace dependents at interface detail

- **GIVEN** an indexed symbol with dependents connected through dependency edges
- **WHEN** `trace` is invoked over the `dependents` relation at interface detail
- **THEN** each detailed row carries its dependent's interface tier alongside the edge kind and distance

#### Scenario: Detail does not change the result set

- **GIVEN** an indexed symbol standing in a relation to several symbols
- **WHEN** the same trace is invoked at two different detail levels
- **THEN** both answers return the same symbols in the same order

### Requirement: Depth-bounded impact answer with an honest horizon

For a dependents query, the system SHALL return detailed results only up to a depth bound; each detailed result SHALL identify the dependent symbol, the kind of dependency edge that connected it, and its distance from the subject in hops; dependents beyond the bound SHALL be reported in aggregate — counts by edge kind and distance — up to a stated horizon; and the answer SHALL always distinguish between reach that ends within the bound, reach that extends beyond the bound, and reach whose aggregate is itself cut off at the horizon.

#### Scenario: Reach ends within the bound

- **GIVEN** a subject whose every dependent lies within the requested depth
- **WHEN** dependents are queried at that depth
- **THEN** the answer conveys that no reach extends beyond what is detailed

#### Scenario: Reach extends beyond the bound

- **GIVEN** a subject with dependents deeper than the requested depth
- **WHEN** dependents are queried at that depth
- **THEN** the detailed results stop at the bound and the answer reports aggregate counts of the deeper dependents by edge kind and distance

#### Scenario: Aggregate discloses its own horizon

- **GIVEN** a subject whose dependency network extends beyond the aggregate horizon
- **WHEN** dependents are queried
- **THEN** the answer states that the aggregate itself is bounded rather than presenting it as the total reach

#### Scenario: Bound at or beyond the horizon is still disclosed

- **GIVEN** a subject whose dependency network reaches the horizon
- **WHEN** dependents are queried with a depth bound at or beyond the horizon
- **THEN** the answer states that the traversal was cut off at the horizon rather than reporting that reach ends within the bound

#### Scenario: Detailed results carry kind and distance

- **GIVEN** a subject with dependents connected through more than one edge kind
- **WHEN** dependents are queried
- **THEN** each detailed result identifies its dependent symbol, the connecting edge kind, and its hop distance

### Requirement: Symbol reference resolution

The system SHALL resolve a symbol reference supplied as a short name, a qualified name, or a canonical identity to the symbol it denotes, and SHALL return a typed candidate set when a reference denotes more than one symbol rather than selecting one arbitrarily.

#### Scenario: Qualified name resolves uniquely

- **GIVEN** a qualified name that denotes exactly one indexed symbol
- **WHEN** it is supplied as a reference to a query
- **THEN** that single symbol is resolved and queried

#### Scenario: Python dotted qualified name resolves

- **GIVEN** an indexed Python symbol and its dotted module-qualified name
- **WHEN** the dotted name is supplied as a reference to a query
- **THEN** exactly that symbol is resolved

#### Scenario: Ambiguous short name returns candidates

- **GIVEN** a short name shared by several indexed symbols
- **WHEN** it is supplied as a reference
- **THEN** a typed candidate set of the matching symbols is returned rather than an arbitrary one being chosen

#### Scenario: Canonical identity round-trips

- **GIVEN** the canonical identity of an indexed symbol
- **WHEN** it is supplied as a reference
- **THEN** exactly that symbol is resolved

### Requirement: Calibrated output contract

The system SHALL return every query result with its provenance and freshness, SHALL identify each returned symbol by both its stable canonical identity and a human-readable name, SHALL represent an empty result as typed absence distinct from an unavailable or failed answer, SHALL order results deterministically for identical inputs, and SHALL offer the result as structured JSON.

#### Scenario: Fresh result carries provenance

- **GIVEN** a query against an index whose sources and analyzer are unchanged
- **WHEN** the result is returned as JSON
- **THEN** it carries the analyzer provenance and is marked fresh

#### Scenario: Result identifies symbols by identity and name

- **GIVEN** a query that returns one or more symbols
- **WHEN** the result is returned as JSON
- **THEN** each symbol carries both its stable canonical identity and a human-readable name

#### Scenario: Stale result is flagged

- **GIVEN** a query against an index whose sources changed since indexing
- **WHEN** the result is returned
- **THEN** it is marked stale rather than presented as current

#### Scenario: Deterministic ordering

- **GIVEN** a query whose result contains multiple locations
- **WHEN** the same query is run repeatedly against the same index
- **THEN** the locations are returned in the same order every time

### Requirement: Symbol search by name fragment

The system SHALL provide a CLI command `find` that returns the indexed symbols whose name contains a supplied fragment, matched case-insensitively over ASCII letters (a non-ASCII character matches exactly, case-sensitively), as a bounded result set, distinct from exact reference resolution — where a fragment matching no symbol is a typed-empty answer, not a failure.

#### Scenario: Fragment matches several symbols

- **GIVEN** several indexed symbols whose names share a common substring
- **WHEN** `find` is invoked with that substring
- **THEN** every symbol whose name contains the substring is returned

#### Scenario: Matching is case-insensitive

- **GIVEN** an indexed symbol whose name contains mixed-case ASCII letters
- **WHEN** `find` is invoked with the fragment in a different case
- **THEN** the symbol is still returned

#### Scenario: No match is typed absence

- **GIVEN** an indexed workspace
- **WHEN** `find` is invoked with a fragment no symbol's name contains
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer

### Requirement: Bounded and resumable answers

The system SHALL bound every result-bearing answer by default: a per-query result limit caps how many results are returned, and per-result content is capped to a content-size bound, each applied from a documented default when the caller requests no explicit bound.
A caller MAY request an explicit bound, and MAY request an unbounded answer explicitly, which the system SHALL honor.
Any truncation — of the result set or of a result's content — SHALL be disclosed in the answer.
A caller MAY request a positioned window into a result's content, and when the returned content is a proper part of the whole, the answer SHALL disclose which part it holds and the whole extent; a window requested beyond the content SHALL yield empty content with that disclosure, not a failure.
When a result set is truncated, the answer SHALL carry an opaque continuation token that deterministically resumes the remaining results, and a token presented against query parameters it was not issued for SHALL be rejected rather than silently resumed against the wrong results.

#### Scenario: Result set capped and truncation disclosed

- **GIVEN** a query whose results exceed the requested result limit
- **WHEN** the query runs at that limit
- **THEN** at most that many results are returned and the answer discloses that the result set was truncated

#### Scenario: A default bound applies when none is requested

- **GIVEN** a result-bearing query whose results exceed the documented default limit, with no explicit limit requested
- **WHEN** the query runs
- **THEN** at most the documented default number of results are returned and the answer discloses that the result set was truncated

#### Scenario: Windowed content is addressable and disclosed

- **GIVEN** a result whose content is longer than a requested content window into it
- **WHEN** the query runs for that window
- **THEN** exactly that window of the content is returned, and the answer discloses the window's position within the content and the content's whole extent

#### Scenario: Continuation resumes deterministically

- **GIVEN** a truncated result set and the continuation token from its answer
- **WHEN** the query is re-issued with that token
- **THEN** the next results resume from where the prior page ended, deterministically for identical inputs

#### Scenario: Content bounded with truncation disclosed

- **GIVEN** a result whose content exceeds the content-size bound
- **WHEN** the query runs
- **THEN** the content is capped to the bound and the answer discloses that the content was truncated

#### Scenario: Mismatched continuation token refused

- **GIVEN** a continuation token issued for one query
- **WHEN** it is presented with different query parameters
- **THEN** it is rejected as a usage error rather than resumed against the wrong results

#### Scenario: Impact detail rows are bounded and resumable

- **GIVEN** a subject whose direct dependents exceed the result limit
- **WHEN** the impact relation runs
- **THEN** at most the limit's rows are detailed, the truncation is disclosed with a continuation, and the beyond-bound aggregate and horizon disclosure accompany every page

### Requirement: Diff-seeded impact assessment

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

### Requirement: Heuristic-grade answers are structurally labeled

The system SHALL carry, in every `tests` answer — in the machine answer and in the human rendering alike — a structural marker identifying the answer as derived from convention-based test classification rather than from resolved semantic fact; each returned site SHALL carry, as provenance, the convention rule that classified its enclosing declaration; and the marker SHALL accompany, never replace, the provenance and freshness labeling every answer carries.
An empty `tests` answer asserts only that no convention-classified reference site was found — never that nothing tests the subject — and SHALL carry the marker like any other `tests` answer.
An ambiguous-reference or unresolved-subject answer terminates before classification is consulted and contains no classification-derived content, so it SHALL NOT carry the marker — the marker asserts a derivation, never merely the relation requested.
An answer of a relation derived only from resolved reference evidence SHALL NOT carry the heuristic-grade marker.

#### Scenario: Machine answer carries the marker

- **GIVEN** a `tests` query with one or more results
- **WHEN** the answer is returned as JSON
- **THEN** it carries a structural field marking the answer as convention-based classification

#### Scenario: Human render carries the marker

- **GIVEN** a `tests` query with one or more results
- **WHEN** the answer is rendered for a human
- **THEN** the rendering states that the results are convention-classified test code, not resolved semantic fact

#### Scenario: Each site carries its classification rule

- **GIVEN** a `tests` answer whose sites were classified under more than one convention rule
- **WHEN** the answer is returned as JSON
- **THEN** each site carries the convention rule that classified its enclosing declaration

#### Scenario: Resolved relations carry no heuristic marker

- **GIVEN** a `references` query against the same subject
- **WHEN** the answer is returned as JSON
- **THEN** it carries no heuristic-grade marker

#### Scenario: Unresolved subject carries no marker

- **GIVEN** a `tests` query whose subject resolves to no symbol
- **WHEN** the answer is returned as JSON
- **THEN** the typed absence carries no classification marker

#### Scenario: Ambiguous subject carries no marker

- **GIVEN** a `tests` query whose subject resolves ambiguously to several symbols
- **WHEN** the answer is returned as JSON
- **THEN** the ambiguous answer carries no classification marker

#### Scenario: Empty answer keeps the marker

- **GIVEN** a `tests` query whose subject has only production references
- **WHEN** the answer is returned as JSON
- **THEN** the empty answer carries the convention-based marker alongside its typed absence

#### Scenario: Empty human render states scoped absence

- **GIVEN** a `tests` query whose subject has only production references
- **WHEN** the answer is rendered for a human
- **THEN** the rendering presents the empty result as no convention-classified test reference found, not as proof that nothing tests the subject

#### Scenario: Heuristic marker composes with staleness

- **GIVEN** a `tests` query against an index whose sources changed since indexing
- **WHEN** the answer is returned
- **THEN** it carries both the staleness flag and the heuristic-grade marker, each independently

## Technical Notes

- **Implementation**: `src/query/`, `src/cli.rs`
- **Dependencies**: symbol-identity, semantic-engine, code-graph
