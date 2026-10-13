# Code Navigation Specification

## Purpose

Defines the query surface over the code graph: resolving a symbol reference across its identity, qualified-name, and shortname forms; retrieving a symbol at a chosen detail; tracing named relations; and the calibrated output contract every answer carries.

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

The system SHALL support, on any relationship trace, a per-query detail level that projects each returned result at a chosen level — location, signature, interface, or body — with location as the default; a result that denotes a symbol SHALL project that symbol's own content, a result that denotes a reference site SHALL project the content of the declaration the site is attributed to, and the chosen detail SHALL NOT alter which results are returned or their order.

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
A continuation token presented after the seeding change has moved SHALL be rejected rather than resumed against a different change's results.

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
A changed region in a document the index does not hold SHALL be reported as unmappable rather than silently dropped; a region in a document the index holds, where the index has no declaration, SHALL NOT be reported unmappable, because an approximate answer already discloses that declarations may be missing.
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
An ambiguous-reference or unresolved-subject answer contains no classification-derived content, so it SHALL NOT carry the marker — the marker asserts a derivation, never merely the relation requested.
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

### Requirement: Ranked ordering of dependents

Under the default `ranked` ordering, the system SHALL order the detailed rows of every `dependents` trace and every `impact` answer by distance first, nearest first, and within equal distance by the dependent's codebase-wide structural importance, most important first.
The ordering SHALL NOT alter which symbols the answer contains, the depth bound, the beyond-bound aggregates, or the horizon disclosure.
When the result limit truncates the detailed rows, the rows returned SHALL be the first of the ordered sequence, so a bounded page holds the nearest, most important dependents the answer has to offer.

#### Scenario: Within a layer the widely-depended-upon dependent ranks first

- **GIVEN** an indexed symbol with two direct dependents, one that much of the codebase transitively depends on and one that nothing else depends on
- **WHEN** `trace` is invoked for it over the `dependents` relation under the `ranked` ordering
- **THEN** the widely-depended-upon dependent is returned before the other

#### Scenario: Distance outranks importance

- **GIVEN** an indexed symbol whose distance-2 reach includes a dependent more widely depended upon than any of its direct dependents
- **WHEN** `trace` is invoked for it over the `dependents` relation under the `ranked` ordering
- **THEN** every distance-1 row is returned before the first distance-2 row

#### Scenario: Impact answers order each layer by importance

- **GIVEN** a workspace whose diff changes several indexed symbols with dependents of differing codebase-wide importance at the same shortest distance
- **WHEN** `impact` is invoked under the `ranked` ordering
- **THEN** the detailed rows within each distance layer are ordered most important first

#### Scenario: A truncated page holds the head of the ordered sequence

- **GIVEN** a subject whose distance-1 dependents alone exceed the result limit
- **WHEN** `trace` is invoked over the `dependents` relation under the `ranked` ordering at that limit
- **THEN** the detailed rows are the most important distance-1 dependents, and the truncation is disclosed with a continuation

#### Scenario: Ordering leaves the aggregates untouched

- **GIVEN** a subject whose reach extends beyond the requested depth bound
- **WHEN** `trace` is invoked over the `dependents` relation under the `ranked` ordering
- **THEN** the beyond-bound aggregate counts and the horizon disclosure are identical to those of the same query under the `unranked` ordering

### Requirement: Dependents order selector

The system SHALL support, on `dependents` traces and `impact` assessments, a per-query order selector offering exactly two orderings: `ranked` (the default) — distance first, then structural importance within each layer — and `unranked` — derived only from the answer's own stable structural keys: distance, then dependency kind, then canonical identity.
The selector SHALL be accepted only where its orderings are defined — a `dependents` trace or an `impact` assessment — and an order requested anywhere else SHALL be rejected as a usage error before any traversal.
The selector SHALL NOT alter which symbols the answer contains.
A continuation token SHALL be rejected, rather than resumed, when it is presented with a different ordering or a different ranking-model version than the one it was issued under.

#### Scenario: No selector yields the ranked ordering

- **GIVEN** an indexed symbol with dependents of differing codebase-wide importance at the same distance
- **WHEN** `trace` is invoked over the `dependents` relation with no order requested
- **THEN** the rows within a distance layer are ordered most important first

#### Scenario: Unranked ordering on request

- **GIVEN** an indexed symbol with several dependents
- **WHEN** `trace` is invoked over the `dependents` relation with the `unranked` ordering requested
- **THEN** the rows are ordered by distance, then dependency kind, then canonical identity

#### Scenario: Both orderings return the same answer set

- **GIVEN** an indexed symbol with several dependents
- **WHEN** the same `dependents` trace is invoked once under each ordering
- **THEN** both answers contain exactly the same symbols with the same distances and kinds

#### Scenario: A continuation token binds its ordering

- **GIVEN** a truncated `dependents` answer under the `ranked` ordering and its continuation token
- **WHEN** the token is presented with the `unranked` ordering requested
- **THEN** it is rejected as a usage error rather than resumed

#### Scenario: A continuation token does not outlive its ranking model

- **GIVEN** a truncated `dependents` answer under the `ranked` ordering and its continuation token
- **WHEN** the token is presented after the ranking model's version has changed
- **THEN** it is rejected as a usage error rather than resumed against a differently-ordered sequence

#### Scenario: An order requested outside its relations is refused

- **GIVEN** an indexed symbol referenced in several places
- **WHEN** `trace` is invoked over the `references` relation with an order requested
- **THEN** the invocation is rejected as a usage error before any traversal, naming where the selector applies

### Requirement: Ordering disclosure

Every `dependents` trace and `impact` answer, a typed-empty answer included, SHALL disclose which ordering is in effect through a single structural field in the machine answer.
A `ranked` answer SHALL be presented, in the human rendering and the command's self-description alike, as ordered within each distance layer by a structural-importance heuristic rather than resolved semantic fact; an `unranked` answer SHALL NOT be presented as heuristic.
The disclosure SHALL accompany, never replace, the provenance and freshness labeling every answer carries.

#### Scenario: Machine answer discloses the ordering in effect

- **GIVEN** a `dependents` query with one or more results
- **WHEN** the answer is returned as JSON under the `ranked` ordering
- **THEN** it carries a single structural field identifying the ordering as ranked

#### Scenario: Human render states the heuristic

- **GIVEN** a `dependents` query with one or more results
- **WHEN** the answer is rendered for a human under the `ranked` ordering
- **THEN** the rendering states that rows within a distance layer are ordered by a structural-importance heuristic

#### Scenario: Unranked answers are not presented as heuristic

- **GIVEN** a `dependents` query with one or more results
- **WHEN** the answer is returned under the `unranked` ordering
- **THEN** the machine answer identifies the ordering as unranked and neither it nor the human rendering presents the ordering as heuristic

#### Scenario: An empty answer keeps the disclosure

- **GIVEN** a `dependents` query whose subject has no dependents
- **WHEN** the answer is returned as JSON under the `ranked` ordering
- **THEN** the typed-empty answer still carries the field identifying the ordering in effect

### Requirement: Search by meaning

The system SHALL provide a CLI command `search` that, given a natural-language query, returns corpus-contributing symbols ordered by estimated relevance to the query, most relevant first, where a symbol's name, its documentation, and its source content all serve as relevance evidence — including name words the source spells within one compound token — and sharing exact tokens with the query is not a precondition for a symbol to be returned.
A symbol whose passage is represented by several chunks SHALL be ranked by its best-matching chunk, so that content anywhere in a long symbol can surface it.
Among the candidates each ranking method considers, no symbol SHALL rank higher because more chunks represent it.
An answer MAY be drawn from a bounded set of candidates; every answer already states that its rows are candidates and not a complete list, and the bound never changes the order of the candidates the answer returns.
Each result row SHALL identify its symbol and location and carry content at a per-query detail level — location, signature, interface, or body — with signature as the default, and the chosen detail SHALL NOT alter which symbols are returned or their order.
Each corpus-contributing symbol SHALL appear at most once in a search answer, and a search against a store whose corpus is empty SHALL be a typed-empty answer, not a failure.

#### Scenario: Documentation words match without the name

- **GIVEN** an indexed function whose documentation describes its behavior
- **WHEN** `search` is invoked with words drawn from that documentation, none of which appear in the function's name
- **THEN** the function is among the returned rows

#### Scenario: Name words match as natural language

- **GIVEN** an indexed function whose compound name joins several words into one source token
- **WHEN** `search` is invoked with those words as separate natural-language words
- **THEN** the function is among the returned rows

#### Scenario: Content late in a long symbol is findable

- **GIVEN** an indexed symbol whose passage exceeds the chunk size, with distinctive content near its end
- **WHEN** `search` is invoked with words drawn from that content
- **THEN** the symbol is among the returned rows

#### Scenario: Rows default to signature detail

- **GIVEN** an indexed workspace
- **WHEN** `search` is invoked with no detail requested
- **THEN** each returned row carries the signature tier of its symbol

#### Scenario: Detail does not change the result set

- **GIVEN** an indexed workspace and a query with several relevant symbols
- **WHEN** the same search is invoked at two different detail levels
- **THEN** both answers return the same symbols in the same order

#### Scenario: A symbol appears at most once

- **GIVEN** an indexed function relevant to a query through both its name and its documentation
- **WHEN** `search` is invoked with that query
- **THEN** the function appears exactly once in the answer

#### Scenario: A multi-chunk symbol appears at most once

- **GIVEN** an indexed symbol whose passage is represented by several chunks, more than one of them relevant to a query
- **WHEN** `search` is invoked with that query
- **THEN** the symbol appears exactly once in the answer

#### Scenario: An empty corpus is typed absence

- **GIVEN** a built store whose corpus holds no passages
- **WHEN** `search` is invoked
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer

### Requirement: Similar-code lookup

The system SHALL provide a CLI command `similar` that, given a subject symbol supplied as a symbol reference or a source position — resolved under the symbol-reference resolution contract, a position resolving to the symbol enclosing it — returns other corpus-contributing symbols ordered by estimated similarity of their content to the subject's, most similar first.
Where either the subject's passage or a candidate's is represented by several chunks, similarity SHALL be their best-matching pair of chunks, so that two symbols sharing one region of content rank as similar.
Among the candidates each ranking method considers, neither SHALL rank higher because more chunks represent it.
An answer MAY be drawn from a bounded set of candidates; like a `search` answer, it states that its rows are candidates and not a complete list.
The subject SHALL NOT appear in its own answer.
Each result row SHALL identify its symbol and location and carry content at the same per-query detail axis as `search`, with signature as the default, and the chosen detail SHALL NOT alter which symbols are returned or their order.
An ambiguous subject reference SHALL yield the typed candidate set, and a corpus containing no symbol other than the subject SHALL yield a typed-empty answer.

#### Scenario: Neighbors ranked with the subject excluded

- **GIVEN** an indexed function and several other corpus-contributing symbols
- **WHEN** `similar` is invoked for the function
- **THEN** other symbols are returned ordered most similar first, and the subject function is not among them

#### Scenario: A multi-chunk subject excludes only itself

- **GIVEN** an indexed subject whose passage is represented by several chunks
- **WHEN** `similar` is invoked for that subject
- **THEN** no returned row is the subject

#### Scenario: Symbols sharing one region rank as similar

- **GIVEN** two indexed symbols whose passages exceed the chunk size and whose content coincides in one region only
- **WHEN** `similar` is invoked for one of them
- **THEN** the other is among the returned rows

#### Scenario: A position resolves the enclosing symbol as subject

- **GIVEN** a source position inside a method body
- **WHEN** `similar` is invoked for that position
- **THEN** the answer's subject is the method enclosing the position

#### Scenario: An ambiguous reference yields candidates

- **GIVEN** a short name shared by several indexed symbols
- **WHEN** `similar` is invoked with that name
- **THEN** a typed candidate set of the matching symbols is returned rather than an arbitrary one being chosen

#### Scenario: No other corpus symbol is typed absence

- **GIVEN** a built store whose corpus holds only the subject symbol
- **WHEN** `similar` is invoked for that symbol
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer

#### Scenario: Similar rows default to signature detail

- **GIVEN** an indexed workspace and a resolved subject
- **WHEN** `similar` is invoked with no detail requested
- **THEN** each returned row carries the signature tier of its symbol

#### Scenario: Similar detail does not change the result set

- **GIVEN** a subject with several corpus neighbors
- **WHEN** the same `similar` is invoked at two different detail levels
- **THEN** both answers return the same symbols in the same order

### Requirement: Clone certainty in similar answers

Within a `similar` answer whose subject carries equivalence keys, a returned symbol sharing the subject's formatting-insensitive key SHALL carry a typed exact-clone marker, and a returned symbol sharing the subject's substitution-insensitive key but not its formatting-insensitive key SHALL carry a typed variant-clone marker — asserting an identical token structure with only names and literal values substituted and documentation disregarded, never behavioral equivalence.
Rows carrying a clone marker SHALL precede rows carrying none, exact-clone rows SHALL precede variant-clone rows, and within each certainty class the estimated-similarity order SHALL apply.
The markers derive from deterministic equivalence and SHALL never be assigned from estimated similarity; when the subject carries no equivalence key, no row SHALL carry a clone marker.

#### Scenario: A verbatim copy is marked exact and ranks first

- **GIVEN** an indexed function and a copy of it differing only in whitespace and comments
- **WHEN** `similar` is invoked for the function
- **THEN** the copy carries the exact-clone marker and precedes every unmarked row

#### Scenario: A renamed copy is marked and ordered between

- **GIVEN** an indexed function, a whitespace-variant copy, and a consistently identifier-renamed copy
- **WHEN** `similar` is invoked for the function
- **THEN** the renamed copy carries the variant-clone marker, follows the exact-clone row, and precedes every unmarked row

#### Scenario: A literal-substituted copy is marked as a variant

- **GIVEN** an indexed function and a copy of it differing only in its literal values
- **WHEN** `similar` is invoked for the function
- **THEN** the copy carries the variant-clone marker, not the exact-clone marker

#### Scenario: An edited copy carries no marker

- **GIVEN** an indexed function and a copy of it with statements added
- **WHEN** `similar` is invoked for the function
- **THEN** the edited copy carries no clone marker and is ordered by estimated similarity alone

#### Scenario: A keyless subject yields no markers

- **GIVEN** an indexed type that carries no equivalence key
- **WHEN** `similar` is invoked for the type
- **THEN** no returned row carries a clone marker

#### Scenario: The estimated order applies within a certainty class

- **GIVEN** a subject with two variant clones, one nearer the subject's content than the other
- **WHEN** `similar` is invoked for the subject
- **THEN** the nearer variant precedes the farther within the variant class

#### Scenario: A copy with a different docstring is marked exact

- **GIVEN** an indexed Python function and a copy of it differing only in its docstring
- **WHEN** `similar` is invoked for the function
- **THEN** the copy carries the exact-clone marker

#### Scenario: A renamed test with a docstring is marked as a variant

- **GIVEN** two indexed Python test functions identical except for consistently renamed identifiers and a docstring present in only one
- **WHEN** `similar` is invoked for either test
- **THEN** the other test carries the variant-clone marker

### Requirement: Relation labels on similar rows

Every row of a `similar` answer SHALL carry the set of relation labels that hold between the row's symbol and the subject, and an empty set when none holds.
The label vocabulary is closed:

- `uses_subject`: the row's symbol has a `uses` edge to the subject.
- `used_by_subject`: the subject has a `uses` edge to the row's symbol.
- `tests_subject`: the row's symbol is classified test code and has a `uses` edge to the subject.
- `type_family`: the row's declaring type and the subject's declaring type differ, and either one declares the other as a supertype or both declare a common supertype defined in the workspace.
  A type's declaring type is the type itself, and a member's declaring type is the type that directly encloses it.

A `tests_subject` label SHALL carry, as provenance, the convention rule that classified the row's symbol as test code.
A label asserts the stated graph fact only, never that the row duplicates, overrides, or implements the subject.
The labels SHALL NOT alter which rows the answer contains or their order.
The human rendering SHALL show each row's labels.

#### Scenario: A callee is labeled used by the subject

- **GIVEN** an indexed function whose body calls a second function, and the second function among the subject's `similar` rows
- **WHEN** `similar` is invoked for the first function
- **THEN** the second function's row carries `used_by_subject`

#### Scenario: A caller is labeled as using the subject

- **GIVEN** an indexed facade function whose body calls the subject, and the facade among the subject's `similar` rows
- **WHEN** `similar` is invoked for the subject
- **THEN** the facade's row carries `uses_subject`

#### Scenario: A test that calls the subject carries both labels

- **GIVEN** an indexed test function classified test code whose body calls the subject, and the test among the subject's `similar` rows
- **WHEN** `similar` is invoked for the subject
- **THEN** the test's row carries `uses_subject` and `tests_subject`, and the `tests_subject` label names the convention rule that classified the test

#### Scenario: Sibling implementations are labeled type family

- **GIVEN** two indexed Python classes that both declare the same workspace base class, each with a method of the same name
- **WHEN** `similar` is invoked for one of the methods
- **THEN** the other method's row carries `type_family`

#### Scenario: A base method and its override are labeled type family

- **GIVEN** an indexed Rust trait defined in the workspace with a default method, and a type that implements the trait with its own method
- **WHEN** `similar` is invoked for the type's method
- **THEN** the trait's default method row carries `type_family`

#### Scenario: An external common supertype is not a type family

- **GIVEN** two indexed Python classes whose only common declared base is defined outside the workspace
- **WHEN** `similar` is invoked for a method of one class
- **THEN** a method of the other class carries no `type_family` label

#### Scenario: Methods of one type are not a type family

- **GIVEN** two indexed methods declared in the same class
- **WHEN** `similar` is invoked for one of them
- **THEN** the other method's row carries no `type_family` label

#### Scenario: An unrelated row carries an empty set

- **GIVEN** an indexed function with no edge to the subject and no type relation to it
- **WHEN** `similar` returns it as a row for the subject
- **THEN** the row carries an empty label set

#### Scenario: Labels do not change the answer

- **GIVEN** a subject with several similar rows, some of which carry labels
- **WHEN** the rows and their order are compared with the same answer computed without labels
- **THEN** the rows and their order are identical

### Requirement: Rows with chosen relation labels can be excluded

A caller MAY request that `similar` omit every row carrying any of one or more chosen relation labels.
An answer computed under an exclusion SHALL hold exactly the rows of the unexcluded answer that carry none of the chosen labels, in the same relative order, and SHALL apply the result limit and the continuation token after the omission.
The answer SHALL disclose the labels it excluded.
A continuation token issued under one exclusion SHALL be refused under a different exclusion.

#### Scenario: Excluded rows are omitted and the order is kept

- **GIVEN** a subject whose `similar` answer holds a test row carrying `tests_subject` between two unlabeled rows
- **WHEN** `similar` is invoked for the subject excluding `tests_subject`
- **THEN** the test row is absent and the two unlabeled rows keep their relative order

#### Scenario: The limit counts remaining rows

- **GIVEN** a subject whose first rows carry `used_by_subject`
- **WHEN** `similar` is invoked with a limit excluding `used_by_subject`
- **THEN** the answer holds the limit's number of rows, none of which carries the excluded label

#### Scenario: The exclusion is disclosed

- **GIVEN** a `similar` query excluding `type_family`
- **WHEN** the answer is returned as JSON
- **THEN** the answer names `type_family` as excluded

#### Scenario: A token under another exclusion is refused

- **GIVEN** a continuation token from a truncated `similar` answer excluding `uses_subject`
- **WHEN** the token is presented with no exclusion
- **THEN** it is rejected as a usage error

### Requirement: Similar for many subjects in one invocation

The system SHALL accept, in one `similar` invocation, a list of subject references read from a named file or from standard input, one reference per line, and SHALL return one per-subject answer for each listed reference, in list order.
Each per-subject answer SHALL be identical to the answer a single-subject `similar` invocation returns for the same reference under the same controls, including its typed outcome for an ambiguous or unresolved reference, its rows, order, markers, labels, truncation disclosure, and continuation token.
An ambiguous, unresolved, or keyless reference SHALL NOT fail the invocation.
An empty list SHALL yield a typed-empty answer.
A list that cannot be read SHALL fail as a usage error naming the source.

#### Scenario: Each subject gets its own answer in order

- **GIVEN** a list of three indexed symbol references
- **WHEN** `similar` is invoked with the list
- **THEN** the answer holds three per-subject answers in list order

#### Scenario: A per-subject answer equals the single answer

- **GIVEN** an indexed subject and a list holding its reference
- **WHEN** `similar` is invoked with the list and, separately, with the reference alone under the same controls
- **THEN** the per-subject answer and the single answer are identical

#### Scenario: An unresolved reference does not fail the batch

- **GIVEN** a list holding one indexed reference and one reference that resolves to no symbol
- **WHEN** `similar` is invoked with the list
- **THEN** the invocation succeeds, the first answer holds rows, and the second is the typed absence

#### Scenario: An ambiguous reference yields its candidates

- **GIVEN** a list holding a short name shared by several indexed symbols
- **WHEN** `similar` is invoked with the list
- **THEN** that per-subject answer is the typed candidate set

#### Scenario: A per-subject token resumes a single query

- **GIVEN** a per-subject answer that is truncated and carries a continuation token
- **WHEN** a single-subject `similar` for that reference is invoked with the token
- **THEN** the next rows are returned

#### Scenario: Standard input supplies the list

- **GIVEN** two indexed references written to standard input
- **WHEN** `similar` is invoked reading the list from standard input
- **THEN** the answer holds two per-subject answers

#### Scenario: An empty list is typed absence

- **GIVEN** an empty list
- **WHEN** `similar` is invoked with it
- **THEN** an empty set is returned as a definite "none", distinct from a failed answer

#### Scenario: An unreadable list is a usage error

- **GIVEN** a list path that does not exist
- **WHEN** `similar` is invoked with it
- **THEN** the invocation fails as a usage error naming the path

### Requirement: Mutual nearest pairs

The system SHALL provide, through `similar`, a corpus-wide answer listing every unordered pair of corpus-contributing symbols in which each symbol is the first row of the other's `similar` answer under the same relation-label exclusion.
Each pair SHALL appear once, SHALL carry the clone-certainty marker and the relation labels that the first row of each member's answer carries, and SHALL carry no similarity score.
Pairs SHALL be ordered deterministically by the canonical identity of the pair's lower member.
The answer SHALL be bounded and resumable under the same contract as every result-bearing answer, and it is a `similar` answer under the semantic-answer labeling contract.

#### Scenario: A mutual pair is listed once

- **GIVEN** two indexed functions, each the first row of the other's `similar` answer
- **WHEN** mutual nearest pairs are requested
- **THEN** the pair appears exactly once

#### Scenario: A one-sided nearest neighbor is not a pair

- **GIVEN** an indexed function whose first `similar` row is a second function, whose own first row is a third function
- **WHEN** mutual nearest pairs are requested
- **THEN** no pair joins the first and the second function

#### Scenario: A pair carries its marker and labels

- **GIVEN** two indexed functions that are exact clones and each other's first row
- **WHEN** mutual nearest pairs are requested
- **THEN** the pair carries the exact-clone marker and the relation labels between its members

#### Scenario: An exclusion changes the first rows

- **GIVEN** a test function whose first `similar` row is its subject, and the subject's first row is the test
- **WHEN** mutual nearest pairs are requested excluding `tests_subject`
- **THEN** that pair is not listed

#### Scenario: Pages resume deterministically

- **GIVEN** a corpus with more mutual pairs than the result limit
- **WHEN** the answer is paged to the end through its continuation tokens
- **THEN** the concatenated pages equal the unbounded answer

#### Scenario: The answer is labeled as estimation

- **GIVEN** a mutual nearest pairs answer
- **WHEN** it is returned as JSON
- **THEN** it carries the estimation marker and the semantic-index identity

### Requirement: Corpus clone groups

The system SHALL provide a CLI command `clones` that, given no subject, lists every group of two or more corpus-contributing symbols that share a substitution-insensitive key, and within each group identifies which members share a formatting-insensitive key with each other.
Every symbol carrying an equivalence key shared with at least one other symbol SHALL appear in exactly one group.
A group SHALL be identified as exact when all its members share one formatting-insensitive key, and as variant otherwise.
Groups SHALL be ordered deterministically, groups of longer token sequences before groups of shorter ones.
The answer SHALL be bounded and resumable, the limit counting groups, and a corpus with no shared key SHALL yield a typed-empty answer.

#### Scenario: Exact copies form an exact group

- **GIVEN** three indexed functions differing only in whitespace, comments, and documentation
- **WHEN** `clones` is invoked with no subject
- **THEN** one exact group holds the three functions

#### Scenario: A variant group shows its exact subsets

- **GIVEN** two exact copies of an indexed function and a consistently renamed third copy
- **WHEN** `clones` is invoked with no subject
- **THEN** one variant group holds all three, and the two exact copies are identified as sharing a formatting-insensitive key

#### Scenario: A symbol with no clone is not listed

- **GIVEN** an indexed function whose keys no other symbol shares
- **WHEN** `clones` is invoked with no subject
- **THEN** the function appears in no group

#### Scenario: Longer clones are listed first

- **GIVEN** a clone group of short functions and a clone group of long functions
- **WHEN** `clones` is invoked with no subject
- **THEN** the group of long functions precedes the group of short functions

#### Scenario: No shared key is typed absence

- **GIVEN** an indexed workspace in which no two symbols share an equivalence key
- **WHEN** `clones` is invoked with no subject
- **THEN** an empty set is returned as a definite "none", distinct from a failed answer

#### Scenario: Groups page deterministically

- **GIVEN** a corpus with more clone groups than the result limit
- **WHEN** the answer is paged to the end through its continuation tokens
- **THEN** the concatenated pages equal the unbounded answer

### Requirement: Shared fragments of a subject

Given a subject symbol supplied as a symbol reference or a source position, resolved under the symbol-reference resolution contract, `clones` SHALL return the subject's whole-symbol clones and its shared fragments.
A whole-symbol clone is another symbol sharing the subject's substitution-insensitive key, marked exact or variant as in `similar`.
A shared fragment is a pair of token runs, one in the comparable content of the subject and one in the comparable content of another symbol, as the shared-fragment evidence requirement defines comparable content, reported as a location in each symbol.
A data token is a literal value, a sign directly before a numeric literal, a punctuation token, or a token that the language backend classifies as part of a record literal's structure: a field or key name, or the name or keyword that opens a record literal.
A data stretch is a maximal stretch of consecutive data tokens that the two symbols hold at corresponding positions and that is at least the documented data-stretch length, measured over the whole stretch, not only the part a run includes.
In every reported pair of runs, each token SHALL have the same data role as its counterpart, and the pair SHALL be identical, marked exact, or identical under a consistent one-to-one substitution of identifiers and literal values within the runs, marked variant, after documentation is disregarded, except that every token of a data stretch that a run includes SHALL be identical in both runs.
A backend that classifies no record-literal structure satisfies this requirement with literal values and punctuation as its only data tokens.
No reported run SHALL include the boundary of a contained corpus-contributing symbol.
No reported run SHALL be shorter than the documented guaranteed fragment length.
The comparison of the subject with any one other symbol SHALL be bounded by a documented limit on comparison work.
Every pair of runs that meets that definition and is at least the guaranteed length SHALL be contained in a reported pair, except where the comparison of the two symbols reached the limit.
A symbol whose comparison with the subject reached the limit SHALL have no fragment rows, and the answer SHALL name it as not compared completely.
No reported pair SHALL be contained in another reported pair for the same two symbols, and a symbol reported as a whole-symbol clone SHALL have no fragment rows.
The subject SHALL NOT appear in its own answer.
A subject with no comparable content SHALL yield a typed answer stating that the subject has no comparable content, distinct from an answer that found no clone.
An ambiguous subject reference SHALL yield the typed candidate set.
The answer SHALL be bounded and resumable, ordered deterministically with longer runs first.

#### Scenario: Shared lines inside a longer method are found

- **GIVEN** an indexed long method that contains a block of statements copied from a shorter function, the block longer than the guaranteed length
- **WHEN** `clones` is invoked for the shorter function
- **THEN** the answer reports a fragment whose run in the method covers the copied block

#### Scenario: A renamed block is a variant fragment

- **GIVEN** an indexed function containing a block that another function holds with its identifiers consistently renamed
- **WHEN** `clones` is invoked for the first function
- **THEN** the fragment is reported and marked variant

#### Scenario: An inconsistently renamed block is not reported as one run

- **GIVEN** two indexed functions whose shared block merges two distinct identifiers of one into a single name in the other, and no consistent run of the guaranteed length
- **WHEN** `clones` is invoked for one of them
- **THEN** no fragment is reported for that block

#### Scenario: A block shorter than the guaranteed length is not reported

- **GIVEN** two indexed functions whose only shared run is shorter than the guaranteed length
- **WHEN** `clones` is invoked for one of them
- **THEN** no fragment row names the other function

#### Scenario: A whole-symbol clone has no fragment rows

- **GIVEN** an indexed function and an exact copy of it
- **WHEN** `clones` is invoked for the function
- **THEN** the copy is reported as an exact whole-symbol clone and has no fragment rows

#### Scenario: A docstring does not break a run

- **GIVEN** two indexed Python functions that share a block of statements and differ in their docstrings
- **WHEN** `clones` is invoked for one of them
- **THEN** the shared block is reported as an exact fragment

#### Scenario: A container subject has no comparable content

- **GIVEN** an indexed type containing several methods
- **WHEN** `clones` is invoked for the type
- **THEN** the answer states that the subject has no comparable content, distinct from a typed-empty answer

#### Scenario: Shared lines in the own code of a function with a nested function are found

- **GIVEN** an indexed function that defines a nested function, and whose own code outside the nested function contains a block copied from a second function, the block longer than the guaranteed length
- **WHEN** `clones` is invoked for the second function
- **THEN** the answer reports a fragment whose run in the first function covers the copied block

#### Scenario: A run does not include a nested symbol

- **GIVEN** two indexed functions that share the same statements, where one of them defines a nested function between two of those statements
- **WHEN** `clones` is invoked for either function
- **THEN** no reported run in the function with the nested function covers the nested function's tokens

#### Scenario: Different tables of literal values share no fragment

- **GIVEN** two indexed symbols that each hold a long table of distinct literal values, the tables holding different values
- **WHEN** `clones` is invoked for one of them
- **THEN** no fragment row joins the two tables

#### Scenario: Different table values do not extend a fragment

- **GIVEN** two indexed symbols that share one statement shorter than the guaranteed length, each statement placed between two long tables whose values differ between the symbols
- **WHEN** `clones` is invoked for one of them
- **THEN** no fragment row joins the two symbols

#### Scenario: Different tables of negative numbers share no fragment

- **GIVEN** two indexed symbols that each hold a long table of distinct negative numbers, the tables holding different values
- **WHEN** `clones` is invoked for one of them
- **THEN** no fragment row joins the two tables

#### Scenario: A copied table is an exact fragment

- **GIVEN** an indexed function that contains a long table of literal values copied from a second function
- **WHEN** `clones` is invoked for the second function
- **THEN** the answer reports an exact fragment whose run in the first function covers the table

#### Scenario: Different tables of records share no fragment

- **GIVEN** two indexed symbols that each hold a long table of record literals with the same field names and different values, in a language whose backend classifies record-literal structure
- **WHEN** `clones` is invoked for one of them
- **THEN** no fragment row joins the two tables

#### Scenario: A changed constant in code remains a variant

- **GIVEN** an indexed function containing a block of statements that another function holds with one numeric constant changed
- **WHEN** `clones` is invoked for the first function
- **THEN** the block is reported as a variant fragment

#### Scenario: A repetitive pair is named, not reported in part

- **GIVEN** an indexed subject and another symbol whose comparison exceeds the documented work limit, such as two long tables of distinct literal values
- **WHEN** `clones` is invoked for the subject
- **THEN** the answer names the other symbol as not compared completely, reports no fragment row for it, and reports the rows of every other symbol

#### Scenario: No shared run is typed absence

- **GIVEN** an indexed function that shares no run of the guaranteed length and no key with any symbol
- **WHEN** `clones` is invoked for the function
- **THEN** an empty set is returned as a definite "none", distinct from a failed answer

#### Scenario: A position resolves the enclosing symbol

- **GIVEN** a source position inside a method body
- **WHEN** `clones` is invoked for that position
- **THEN** the answer's subject is the method enclosing the position

#### Scenario: Rust fragments are found

- **GIVEN** two indexed Rust functions that share a block of statements longer than the guaranteed length
- **WHEN** `clones` is invoked for one of them
- **THEN** the shared block is reported as a fragment

### Requirement: Clone answers are labeled as token equivalence

Every `clones` answer, a typed-empty answer included, SHALL carry, in the machine answer and the human rendering alike, a structural marker identifying its rows as deterministic token equivalence, never behavioral equivalence, and SHALL carry the guaranteed fragment length in effect.
A `clones` answer SHALL NOT carry the estimation marker or the semantic-index identity, and SHALL carry no similarity score.
The one exception is an ambiguous-reference or unresolved-subject answer, which SHALL NOT carry the token-equivalence marker.
The marker SHALL accompany, never replace, the provenance and freshness labeling every answer carries.
The machine answer and the human rendering SHALL both name every symbol not compared completely.

#### Scenario: The machine answer carries the marker and the length

- **GIVEN** a `clones` query with one or more rows
- **WHEN** the answer is returned as JSON
- **THEN** it carries the token-equivalence marker and the guaranteed fragment length, and no estimation marker

#### Scenario: The human render states token equivalence

- **GIVEN** a `clones` query with one or more rows
- **WHEN** the answer is rendered for a human
- **THEN** the rendering states that the rows are identical token structure, not proof of identical behavior

#### Scenario: An empty answer keeps the marker

- **GIVEN** a `clones` query against a corpus with no shared key
- **WHEN** the answer is returned as JSON
- **THEN** the typed-empty answer carries the token-equivalence marker

#### Scenario: An ambiguous subject carries no marker

- **GIVEN** a `clones` subject reference that resolves ambiguously to several symbols
- **WHEN** the answer is returned as JSON
- **THEN** the candidate-set answer carries no token-equivalence marker

#### Scenario: The marker composes with staleness

- **GIVEN** a `clones` query against an index whose sources changed since indexing
- **WHEN** the answer is returned
- **THEN** it carries both the staleness flag and the token-equivalence marker

### Requirement: Semantic answers are structurally labeled

Every `search` and `similar` answer — a typed-empty answer included — SHALL carry, in the machine answer and the human rendering alike, a structural marker identifying the ranking as model-derived estimation rather than resolved semantic fact, and SHALL carry the store's recorded semantic-index identity as provenance.
The machine answer SHALL carry the recorded identity whole — it is the consumer's attribution of a ranking to the regime that produced it; the human rendering MAY present the identity in condensed form.
An ambiguous-reference or unresolved-subject answer contains no estimation-derived content, so it SHALL NOT carry the marker or the semantic-index provenance — the marker asserts a derivation, never merely the command invoked.
The human rendering and the commands' self-descriptions SHALL present the results as the nearest candidates the index holds — never as the complete set of relevant code, and an empty or truncated answer never as evidence that no relevant code exists.
A clone-certainty marker derives from deterministic equivalence and SHALL NOT be presented as part of the estimated ranking's heuristic grade.
The marker SHALL accompany, never replace, the provenance, freshness, and staleness labeling every answer carries.

#### Scenario: Machine answer carries the marker and provenance

- **GIVEN** a `search` query with one or more results
- **WHEN** the answer is returned as JSON
- **THEN** it carries a structural field marking the ranking as model-derived estimation and the semantic-index identity as provenance

#### Scenario: Human render frames results as candidates

- **GIVEN** a `search` query with one or more results
- **WHEN** the answer is rendered for a human
- **THEN** the rendering presents the rows as the nearest candidates by estimated relevance, not as the complete set of matches

#### Scenario: An empty answer keeps the marker and scoped absence

- **GIVEN** a `search` against a built store whose corpus holds no passages
- **WHEN** the answer is returned
- **THEN** the typed-empty answer carries the marker, and the human rendering does not present the absence as proof that no relevant code exists

#### Scenario: The marker composes with staleness

- **GIVEN** a `similar` query against an index whose sources changed since indexing
- **WHEN** the answer is returned
- **THEN** it carries both the staleness flag and the estimation marker, each independently

#### Scenario: Clone markers are presented as fact

- **GIVEN** a `similar` answer containing a row with the exact-clone marker
- **WHEN** the answer is rendered for a human
- **THEN** the clone marker is presented as deterministic equivalence, distinct from the estimated ranking

#### Scenario: An ambiguous subject carries no marker

- **GIVEN** a `similar` subject reference that resolves ambiguously to several symbols
- **WHEN** the answer is returned as JSON
- **THEN** the candidate-set answer carries neither the estimation marker nor the semantic-index provenance

#### Scenario: An unresolved subject carries no marker

- **GIVEN** a `similar` subject reference that resolves to no symbol
- **WHEN** the answer is returned as JSON
- **THEN** the typed absence carries neither the estimation marker nor the semantic-index provenance

### Requirement: A stale answer names its refresh command

Every answer marked stale SHALL carry, in the machine answer and in the human rendering alike, a runnable command that refreshes the index the answer came from, naming that index's location, workspace identity, and workspace root.
Running the command SHALL make the same query's answer fresh when the sources, analyzer, and environment are unchanged in the meantime.
An answer marked fresh SHALL NOT carry a refresh command.
When the index does not record its workspace root, the stale answer SHALL say that the root is unknown instead of naming a root.
The index status report SHALL carry the same refresh command when it reports the index stale, and SHALL NOT carry one when it reports the index fresh.

#### Scenario: Stale after changed sources

- **GIVEN** an index built before a source file changed, as after `git reset --hard` to another commit
- **WHEN** a query is answered from it
- **THEN** the answer is marked stale and carries a refresh command that names the index location, workspace identity, and workspace root

#### Scenario: Stale after a changed analyzer

- **GIVEN** an index recorded under one analyzer version while another version is in effect
- **WHEN** a query is answered from it
- **THEN** the answer is marked stale and carries a refresh command

#### Scenario: Stale after a changed environment

- **GIVEN** an index recorded under one interpreter environment while a different environment is in effect for the query
- **WHEN** a query is answered from it and the refresh command it carries is run
- **THEN** the same query, repeated under the same environment, is answered fresh

#### Scenario: Running the refresh command makes the answer fresh

- **GIVEN** a stale answer and its refresh command
- **WHEN** the command is run and the same query is repeated
- **THEN** the repeated answer is marked fresh

#### Scenario: A fresh answer carries no refresh command

- **GIVEN** an index whose sources and analyzer are unchanged since indexing
- **WHEN** a query is answered from it
- **THEN** the answer carries no refresh command

#### Scenario: The status report names the refresh command

- **GIVEN** an index built before a source file changed
- **WHEN** the index status is reported, in the machine answer and in the human rendering
- **THEN** both report the index stale and carry the refresh command, and running the command makes the status report fresh

#### Scenario: The human rendering names the refresh command

- **GIVEN** a stale answer
- **WHEN** it is rendered for a human
- **THEN** the rendering includes the refresh command

### Requirement: Freshness does not depend on how the workspace root is named

An index SHALL answer fresh to a query in its workspace when the sources, analyzer, and interpreter environment are unchanged since the index was built, whether the build named the workspace root by an absolute path, by a relative path, or as the invocation directory.
The recorded environment and the environment in effect for a query SHALL be compared as the same directory, not as the same spelling of a path.

#### Scenario: An index built by the refresh hooks answers fresh

- **GIVEN** a Python workspace whose environment is found through a `.venv` directory, with no activated environment, and an index built with the workspace root given as an absolute path, as the refresh hooks build it
- **WHEN** a query runs from the workspace directory
- **THEN** the answer is marked fresh

#### Scenario: An index built from the workspace directory answers fresh

- **GIVEN** the same workspace, with an index built with the workspace root given as `.`
- **WHEN** a query runs from the workspace directory
- **THEN** the answer is marked fresh

### Requirement: Answers report locations as line and column

Every location in every answer — a result's location in `get`, `trace`, `find`, `search`, `similar`, and `impact`, a region `impact` could not map, and a discrepancy row in the index status report — SHALL carry its start and end line and column beside its byte offsets in the machine answer, and both forms SHALL denote the same positions.
The line and column of a location drawn from the index SHALL describe the text the index was built from, as its byte offsets do, including when that answer is marked stale.
The line and column of a region `impact` could not map SHALL describe the pre-change text that region's byte offsets describe, and SHALL be absent when that pre-change text could not be read.
For a query that reads no `--at` position, the machine answer SHALL be the same whichever position unit the caller selects.
Under the line position unit, which is the default, the human rendering SHALL name a location's start as `path:line:column`; under the byte position unit, it SHALL name the location as `path:start-end` in byte offsets.

#### Scenario: A result location carries both forms

- **GIVEN** an indexed symbol
- **WHEN** `find` returns it with `--json`
- **THEN** its location carries byte offsets and a start and end line and column, and both denote the same positions

#### Scenario: The default human rendering names line and column

- **GIVEN** an indexed symbol defined at line 12, column 5
- **WHEN** `get` returns it at location detail without `--json`
- **THEN** the rendering names the location as `path:12:5`

#### Scenario: The byte unit renders byte offsets

- **GIVEN** an indexed symbol
- **WHEN** `get` returns it at location detail with `--position byte` and without `--json`
- **THEN** the rendering names the location by its start and end byte offsets

#### Scenario: The position unit does not change the machine answer

- **GIVEN** an indexed symbol
- **WHEN** `trace` returns it with `--json`, once under each position unit
- **THEN** the two machine answers are identical

#### Scenario: A stale answer's lines describe the indexed text

- **GIVEN** an index built before lines were inserted above a symbol's definition
- **WHEN** a query returns that symbol
- **THEN** the answer is marked stale, and its line and column equal the definition's position in the indexed text, not in the edited file

#### Scenario: An unmappable impact region carries line and column

- **GIVEN** a change to a file the index does not hold
- **WHEN** `impact` reports the changed region as unmappable with `--json`
- **THEN** the region carries the line and column that its byte offsets denote in the pre-change text

#### Scenario: An unreadable pre-change file carries no line and column

- **GIVEN** a change to a file whose pre-change content cannot be read
- **WHEN** `impact` reports the file as unmappable with `--json`
- **THEN** the region carries no line and no column

#### Scenario: A status discrepancy row carries line and column

- **GIVEN** an index with a persisted join discrepancy whose coordinates normalized
- **WHEN** the index status is reported with `--discrepancies --all --json`
- **THEN** the discrepancy row carries a line and column beside its byte offsets

### Requirement: Source positions accept line and column

Under the line position unit, which is the default, a source position given to `get --at` or `similar --at` SHALL be accepted as `path:line:column` or as `path:line`; under the byte position unit, it SHALL be accepted as `path:byte_offset`.
A position SHALL be read against the text the index was built from, as a location in an answer is.
A `path:line:column` position SHALL resolve to the innermost symbol that encloses it, as a byte offset at the same position does.
A column past the end of its line SHALL denote the end of that line.
A `path:line` position SHALL resolve to the outermost symbols whose header covers at least one position of that line; when no header covers the line, it SHALL resolve to the innermost symbol that encloses the line's first column.
A header or span that ends at the line's first column covers none of that line.
When more than one symbol qualifies and none encloses another, the answer SHALL be the typed candidate set of those symbols.
A line past the end of the indexed file SHALL resolve to no symbol, and the answer SHALL be typed absence.
A line or column of zero SHALL be refused as a usage error that names the accepted forms.

#### Scenario: A line and column inside a method body

- **GIVEN** a line and column inside a method body
- **WHEN** `get --at path:line:column` runs
- **THEN** the method is returned

#### Scenario: A line holding a declaration header

- **GIVEN** the line on which a method's declaration starts, inside a type
- **WHEN** `get --at path:line` runs
- **THEN** the method is returned, not the type that encloses it

#### Scenario: A header line that declares parameters

- **GIVEN** the line on which a method's declaration starts, where that line also declares the method's parameters and the index records the parameters as symbols
- **WHEN** `get --at path:line` runs
- **THEN** the method is returned, not a parameter

#### Scenario: Every line of a multi-line header

- **GIVEN** a declaration whose header runs over several lines, through decorators or a parameter list
- **WHEN** `get --at path:line` names any line of that header
- **THEN** the declaration is returned

#### Scenario: A one-line declaration with members

- **GIVEN** a line holding a type declaration together with its members
- **WHEN** `get --at path:line` runs
- **THEN** the type is returned, not its members

#### Scenario: A line declaring a member

- **GIVEN** a line inside a multi-line type that declares one member of that type
- **WHEN** `get --at path:line` runs
- **THEN** the member is returned

#### Scenario: A line inside a body

- **GIVEN** a line inside a function body that holds no nested declaration
- **WHEN** `get --at path:line` runs
- **THEN** the function is returned

#### Scenario: A line holding two sibling declarations

- **GIVEN** a line on which two declarations start, neither enclosing the other
- **WHEN** `get --at path:line` runs
- **THEN** the answer is the typed candidate set of the two declarations

#### Scenario: A span ending at the line's first column does not cover it

- **GIVEN** a declaration whose span ends at column 1 of the line on which the next declaration starts
- **WHEN** `get --at path:line` names that line
- **THEN** the next declaration is returned, not the one that ended

#### Scenario: A column past the end of its line

- **GIVEN** a middle line of a multi-line function, and a second line on which one declaration ends and another starts
- **WHEN** `get --at path:line:column` names each line with a column past its end
- **THEN** the first returns the function, and the second returns the symbol that encloses the end of that line

#### Scenario: A line past the end of the file

- **GIVEN** an indexed file with fewer lines than a requested line
- **WHEN** `get --at path:line` names that line
- **THEN** the answer is typed absence

#### Scenario: A line position on a Python method

- **GIVEN** a line inside a Python method body
- **WHEN** `similar --at path:line` runs
- **THEN** the method is taken as the subject

#### Scenario: The byte unit reads a byte offset

- **GIVEN** a byte offset inside a method body
- **WHEN** `get --at path:byte_offset --position byte` runs
- **THEN** the method is returned

#### Scenario: A stale position reads the indexed text

- **GIVEN** an index built before lines were inserted above a method
- **WHEN** `get --at` names the method's line in the indexed text
- **THEN** the method is returned and the answer is marked stale

#### Scenario: A zero line is a usage error

- **GIVEN** any index
- **WHEN** `get --at path:0` runs
- **THEN** the command fails with a usage error that names the accepted position forms

### Requirement: The index status report names changed files

Whenever the source files differ from the content the index was built from, the index status report SHALL carry the count of changed, added, and removed files.
When the file detail is requested, with `status --detail files` or `status --detail`, the report SHALL name each differing file as changed, added, or removed, in the machine answer and in the human rendering alike.
The source files are those the index's recorded language backend discovers in the workspace.
A changed file is one the index holds whose content differs; an added file is a source file the index does not hold; a removed file is one the index holds that is no longer a source file.
The report SHALL report the index stale for changed content exactly when at least one file differs, and SHALL NOT name a file that is neither a source file nor held by the index.
The freshness, the counts, and the file list in one report SHALL describe the same observation of the source files.
The file list SHALL be bounded and resumable as a result-bearing answer is, and a continuation token SHALL be refused when the source files' paths or content differ from those the token's page was drawn from.

#### Scenario: A changed file is named

- **GIVEN** an index built before one source file was edited
- **WHEN** the index status is reported with `--detail files`
- **THEN** the report names that file as changed and reports the index stale for changed content

#### Scenario: An added file is named

- **GIVEN** an index built before a new source file was created
- **WHEN** the index status is reported with `--detail files`
- **THEN** the report names that file as added

#### Scenario: A removed file is named

- **GIVEN** an index built before a source file was deleted
- **WHEN** the index status is reported with `--detail files`
- **THEN** the report names that file as removed

#### Scenario: Counts appear without the file detail

- **GIVEN** an index built before two source files were edited and one was created
- **WHEN** the index status is reported without `--detail`
- **THEN** the report carries two changed files, one added file, and no removed file, and names no file

#### Scenario: A bare detail flag names the files

- **GIVEN** an index built before one source file was edited
- **WHEN** the index status is reported with `--detail` and no value
- **THEN** the report is the same as with `--detail files`

#### Scenario: A fresh index names no file

- **GIVEN** an index whose sources are unchanged since it was built
- **WHEN** the index status is reported with `--detail files`
- **THEN** the report names no file and does not report the index stale for changed content

#### Scenario: A file that is not a source file is not named

- **GIVEN** an index built before a file that is not a source file, such as a README, was edited
- **WHEN** the index status is reported with `--detail files`
- **THEN** the report names no file and reports the index fresh

#### Scenario: The human rendering names the files

- **GIVEN** an index built before one source file was edited and another was deleted
- **WHEN** the index status is reported with `--detail files` and without `--json`
- **THEN** the rendering names the edited file as changed and the deleted file as removed

#### Scenario: The file list is bounded and resumable

- **GIVEN** an index built before more source files were edited than a requested limit
- **WHEN** the index status is reported with `--detail files` at that limit, and then with the continuation token from its answer
- **THEN** the first report names at most that many files and discloses the truncation, and the second names the files that follow

#### Scenario: A token is refused after further edits

- **GIVEN** a continuation token from a truncated file list
- **WHEN** another source file's content is changed and the token is presented
- **THEN** the token is refused as a usage error

### Requirement: The index status report names every staleness reason

The index status report SHALL name every reason the index is stale — changed content, a changed analyzer, and a changed environment — each exactly once, in the machine answer and in the human rendering alike, and SHALL name no reason when the index is fresh.
The single freshness value the report carries SHALL be one of the reasons it names.

#### Scenario: Content and analyzer both changed

- **GIVEN** an index built before a source file was edited, recorded under an analyzer version other than the one in effect
- **WHEN** the index status is reported
- **THEN** the report names both changed content and a changed analyzer

#### Scenario: Only the environment changed

- **GIVEN** an index whose sources and analyzer are unchanged, recorded under an interpreter environment other than the one in effect
- **WHEN** the index status is reported
- **THEN** the report names only a changed environment

#### Scenario: A fresh index names no reason

- **GIVEN** an index whose sources, analyzer, and environment are unchanged
- **WHEN** the index status is reported
- **THEN** the report names no staleness reason

## Technical Notes

- **Implementation**: `src/query/`, `src/cli.rs`
- **Dependencies**: symbol-identity, semantic-engine, code-graph
