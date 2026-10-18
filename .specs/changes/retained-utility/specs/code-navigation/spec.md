# Delta for Code Navigation

## ADDED Requirements

### Requirement: Relations a structural index cannot answer are unavailable

On a structural index, a `trace` over `references`, `importers`, `implementers`, `implements`, `tests`, or `dependents` whose subject resolves to one symbol SHALL return an answer whose outcome is `unavailable`, distinct from every other outcome.
The answer SHALL state that the index is structural and that the relation needs a semantic index, and SHALL carry no results and no continuation token.
A `references` answer whose outcome is `unavailable` SHALL still carry the unresolved-call group that `References answers disclose unresolved calls that share the subject's name` defines.
A subject that resolves to several symbols or to none SHALL keep its candidate-set or absent outcome.
The human rendering SHALL state that the relation is unavailable on a structural index, and SHALL NOT state that the relation holds no instance.

Serves: calibrated-capability-disclosure, structural-navigation-without-semantic-indexer

#### Scenario: Dependents of a Bash function are unavailable

- **GIVEN** a structural Bash index holding the function `deploy`, which another function calls
- **WHEN** `trace` is invoked for `deploy` over the `dependents` relation
- **THEN** the answer's outcome is `unavailable`, it carries no results, and it states that `dependents` needs a semantic index

#### Scenario: References of a Bash function list the calls that name it

- **GIVEN** a structural Bash index holding the function `deploy`, and three commands named `deploy` in other files
- **WHEN** `trace` is invoked for `deploy` over the `references` relation
- **THEN** the answer's outcome is `unavailable`, and its unresolved-call group lists the three calls with their locations and enclosing declarations

#### Scenario: Implementers on a structural Python index are unavailable

- **GIVEN** a structural Python index holding the class `User`, which another class names as its base
- **WHEN** `trace` is invoked for `User` over the `implementers` relation
- **THEN** the answer's outcome is `unavailable`

#### Scenario: An ambiguous subject on a structural index is still a candidate set

- **GIVEN** a structural Bash index whose files `a.sh` and `b.sh` each define the function `log`
- **WHEN** `trace` is invoked for `log` over the `dependents` relation
- **THEN** the answer is the typed candidate set of the two functions, not `unavailable`

#### Scenario: The human rendering does not claim an empty relation

- **GIVEN** a structural index holding a function
- **WHEN** `trace` over the `importers` relation is rendered for a human
- **THEN** the rendering states that the relation is unavailable on a structural index, and does not state that the relation holds no instance

### Requirement: A structural index answers the structural queries

On a structural index, `get`, `find`, `search`, `similar`, `clones`, and `trace` over `containers` and `contains` SHALL answer under the same requirements as on a semantic index.
A reference equal to a declaration's whole name SHALL resolve to that declaration, also when the name contains `::`.
A `similar` row on a structural index SHALL carry no relation label that derives from references, dependency edges, type-hierarchy edges, or test classification.

Serves: structural-navigation-without-semantic-indexer

#### Scenario: A Bash function is retrieved with its body

- **GIVEN** a structural Bash index holding the function `deploy`
- **WHEN** `get` is invoked for `deploy` at body detail
- **THEN** the function's location and source text are returned

#### Scenario: A file's functions are its contents

- **GIVEN** a structural Bash index whose file `lib.sh` defines `log` and `die`
- **WHEN** `trace` is invoked for the file over the `contains` relation
- **THEN** exactly `log` and `die` are returned

#### Scenario: A name fragment finds structural declarations

- **GIVEN** a structural Bash index holding the functions `deploy_staging` and `deploy_prod`
- **WHEN** `find` is invoked with the fragment `deploy`
- **THEN** both functions are returned

#### Scenario: A Bash name containing the separator resolves by its whole name

- **GIVEN** a structural Bash index whose file defines the top-level function `lib::log`, and the function `lib` with the function `log` nested inside it
- **WHEN** `get` is invoked for `lib::log`
- **THEN** the answer is the typed candidate set holding the top-level `lib::log` and the nested `log`

#### Scenario: Similar rows carry no relation labels

- **GIVEN** a structural Python index holding two near-identical functions, one of which calls the other
- **WHEN** `similar` is invoked for one of them
- **THEN** the other is returned with no relation label

### Requirement: Impact on a structural index reports its dependents as unavailable

On a structural index, an `impact` answer whose change touches an indexed declaration SHALL list as its seeds the declarations the change touches, under the same seed selection as on a semantic index, SHALL carry no dependents, and SHALL state that the dependents are unavailable because they need a semantic index.
Its human rendering SHALL list the seeds and state that the dependents are unavailable, and SHALL NOT state a reach, a horizon, or that no dependent was recorded.
A change that touches no indexed declaration SHALL remain a typed-empty answer, because the syntax tree establishes it.
An approximate structural answer's recovery procedure SHALL build its index with the language and tier of the index the answer came from.

Serves: calibrated-capability-disclosure, structural-navigation-without-semantic-indexer

#### Scenario: A changed Bash function is a seed with unavailable dependents

- **GIVEN** a structural Bash index, and a diff that changes the body of the function `deploy`
- **WHEN** `impact` is invoked
- **THEN** the answer lists `deploy` as a seed, carries no dependents, and states that dependents need a semantic index

#### Scenario: The human rendering claims no reach

- **GIVEN** a structural Bash index, and a diff that changes the body of the function `deploy`
- **WHEN** the `impact` answer is rendered for a human
- **THEN** the rendering lists `deploy`, states that its dependents are unavailable, and states no reach or horizon

#### Scenario: A comment-only change is still a definite none

- **GIVEN** a structural Bash index, and a diff that changes only a comment outside every function
- **WHEN** `impact` is invoked
- **THEN** the answer is typed-empty, as on a semantic index

#### Scenario: A deleted extensionless script is a seed

- **GIVEN** a structural Bash index holding the extensionless script `bin/deploy`, whose shebang runs `bash`, and a diff that deletes it
- **WHEN** `impact` is invoked
- **THEN** the functions `bin/deploy` defined are listed as seeds

#### Scenario: The recovery procedure rebuilds the same tier and language

- **GIVEN** an approximate `impact` answer from a structural Python index, and from a Bash index selected explicitly in a Rust workspace
- **WHEN** each answer's recovery procedure is followed
- **THEN** the first builds a structural Python index and the second a Bash index

### Requirement: Negative answers of a structural index disclose syntax errors

On a structural index, an answer whose outcome is absent or empty, and a `find` answer with no results, SHALL carry the number of indexed files whose syntax tree holds a syntax error, when that number is not zero.
Its human rendering SHALL state that number, and that a declaration in those files may be missing from the index.

Serves: calibrated-capability-disclosure

#### Scenario: A lost declaration is not a confident absence

- **GIVEN** a Bash file holding `foo#0() { :; }`, which Bash accepts and the grammar parses as a syntax error, indexed structurally
- **WHEN** `find` is invoked with the fragment `foo`
- **THEN** the answer carries no results and the count of one file with a syntax error, and its rendering states that a declaration in that file may be missing

#### Scenario: A clean index adds no disclosure

- **GIVEN** a structural Bash index whose files hold no syntax error
- **WHEN** `get` is invoked for a name no declaration has
- **THEN** the answer is absent and carries no syntax-error count

### Requirement: An identity of the other tier is named as such

When a reference supplied to a query is a structural identity and the index is semantic, or is a semantic identity and the index is structural, the answer SHALL report that the reference denotes no symbol, and SHALL state the kind of identity received and the tier of the index.

Serves: calibrated-capability-disclosure

#### Scenario: A structural identity against a semantic index

- **GIVEN** a semantic Python index, and the structural identity of a declaration from an earlier structural index of the same workspace
- **WHEN** the identity is supplied to `get`
- **THEN** the answer reports that the reference denotes no symbol, and states that it received a structural identity and that the index is semantic

#### Scenario: A semantic identity against a structural index

- **GIVEN** a structural Python index, and the semantic identity of a declaration from an earlier semantic index of the same workspace
- **WHEN** the identity is supplied to `get`
- **THEN** the answer reports that the reference denotes no symbol, and states that it received a semantic identity and that the index is structural

### Requirement: The index status report states the index tier

The index status report SHALL state whether the index is structural or semantic, in the machine answer and in the human rendering alike.
For a structural index, it SHALL name the relations that the index does not answer, and the indexed files whose syntax tree holds a syntax error.

Serves: calibrated-capability-disclosure

#### Scenario: A structural index reports its tier and missing relations

- **GIVEN** a structural Bash index
- **WHEN** the index status is reported
- **THEN** the report states that the index is structural and names `references`, `importers`, `implementers`, `implements`, `tests`, and `dependents` as relations it does not answer

#### Scenario: A structural index names its files with syntax errors

- **GIVEN** a structural Bash index with one file whose syntax tree holds a syntax error
- **WHEN** the index status is reported
- **THEN** the report names that file

#### Scenario: A semantic index reports its tier

- **GIVEN** a semantic Rust index
- **WHEN** the index status is reported
- **THEN** the report states that the index is semantic and names no unanswered relation

## MODIFIED Requirements

### Requirement: Calibrated output contract

The system SHALL return every query result with its provenance, its freshness, and the tier of the index that produced it, structural or semantic, SHALL identify each returned symbol by both its stable canonical identity and a human-readable name, SHALL represent an empty result as typed absence distinct from an unavailable or failed answer, SHALL order results deterministically for identical inputs, and SHALL offer the result as structured JSON.
The human rendering of an answer from a structural index SHALL state that the index is structural.

> Previously: an answer did not carry a tier, because every index was semantic.

Serves: calibrated-capability-disclosure

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

#### Scenario: Each answer carries its tier

- **GIVEN** a semantic Rust index and a structural Bash index
- **WHEN** `find` is answered against each as JSON
- **THEN** the first answer carries the semantic tier and the second the structural tier

#### Scenario: A structural answer says so to a human

- **GIVEN** a structural Bash index
- **WHEN** a `get` answer is rendered for a human
- **THEN** the rendering states that the index is structural

#### Scenario: A fresh structural index is fresh with a semantic indexer installed

- **GIVEN** a structural Python index whose sources are unchanged, on a host where scip-python and a Python environment are available
- **WHEN** a query result is returned
- **THEN** it is marked fresh

### Requirement: Relationship trace

> Previously: every index was semantic, so every relation answered from the index.
> This block is based on the text that `method-overrides` syncs into the baseline; rebase it on the synced baseline before this change syncs.

The system SHALL provide a CLI command `trace` that, given a subject symbol and a relation kind, returns the symbols standing in that relation to the subject.
Relation kinds follow a shared-root directional convention (e.g. `containers`/`contains`), and the supported relations are `containers` (the declaration that directly encloses a subject), `contains` (the symbols a subject directly contains), `references` (the sites that reference a subject, which for a type subject are its type-occurrences), `dependents` (the symbols that depend on the subject directly or transitively, subject to a depth bound), `importers` (the modules that import the subject), `implementers` (for a type subject, the types that declare the subject as a supertype — a trait's implementors or a base type's subtypes; for a method subject, the methods that directly override it), `implements` (for a type subject, the supertypes it directly declares; for a method subject, the methods it directly overrides), and `tests` (the reference sites of a subject whose enclosing declaration is classified test code — the answer to "what test code exercises this symbol").
The command's self-description SHALL present `dependents` as impact assessment — the answer to "what could break if this symbol changes" — and SHALL present `tests` as convention-based classification rather than resolved semantic fact.
An empty answer of a structural relation (`containers`, `contains`, `implementers`, `implements`) is a definite "none"; an empty answer of a relation derived from references is governed by "Empty answers of reference-derived relations claim no proof".
On a structural index, `Relations a structural index cannot answer are unavailable` governs `references`, `importers`, `implementers`, `implements`, `tests`, and `dependents`, and `containers` and `contains` answer as this requirement states.

Serves: calibrated-capability-disclosure

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

#### Scenario: Trace implementers of a method

- **GIVEN** an indexed Python method overridden by methods of two subclasses, one of which is itself overridden by a method of a further subclass
- **WHEN** `trace` is invoked for it over the `implementers` relation
- **THEN** exactly the two directly overriding methods are returned

#### Scenario: Trace implementers of a Rust trait method

- **GIVEN** an indexed Rust trait method implemented by two impls
- **WHEN** `trace` is invoked for it over the `implementers` relation
- **THEN** exactly the two impl methods are returned

#### Scenario: Trace implements of a method

- **GIVEN** an indexed Python method of a class with two declared bases that each declare a method of the same name
- **WHEN** `trace` is invoked for it over the `implements` relation
- **THEN** exactly the two base methods are returned

#### Scenario: Trace implements of a type

- **GIVEN** an indexed Rust type that implements two workspace traits
- **WHEN** `trace` is invoked for it over the `implements` relation
- **THEN** exactly the two traits are returned

#### Scenario: Trace implements of a Rust impl method

- **GIVEN** an indexed Rust impl method that implements a trait item
- **WHEN** `trace` is invoked for it over the `implements` relation
- **THEN** exactly that trait item is returned

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
- **THEN** an empty set is returned as typed absence, distinct from an unavailable or failed answer

#### Scenario: An empty structural relation is a definite none

- **GIVEN** an indexed symbol that directly contains no declaration
- **WHEN** `trace` is invoked for it over the `contains` relation
- **THEN** an empty set is returned as a definite "none"

#### Scenario: An empty implements answer is a definite none

- **GIVEN** an indexed method that overrides no method
- **WHEN** `trace` is invoked for it over the `implements` relation
- **THEN** an empty set is returned as a definite "none"

#### Scenario: A trace over a structural index answers or is unavailable

- **GIVEN** a structural Python index holding a class with a method
- **WHEN** `trace` is invoked for the class over the `contains` relation and over the `implements` relation
- **THEN** the first answer returns the method, and the second answer's outcome is `unavailable`

### Requirement: References answers disclose unresolved calls that share the subject's name

> Previously: the called name matched was the last component of the subject's name for every subject.
> This block is based on the text that `unresolved-calls` syncs into the baseline; rebase it on the synced baseline before this change syncs.

Every `references` answer whose subject resolves to one symbol SHALL carry, separate from its results, an unresolved-call group: the recorded unresolved calls whose called name equals the subject's own name: the last component of its name for a Rust or Python subject, and its whole function name for a Bash subject, which may contain `::`.
The group SHALL carry the called name, the total number of matching calls, and the calls, each with its location as line and column, its receiver text and whether that text was shortened when it has a receiver, and its enclosing declaration, or its module or file.
A group with no matching calls SHALL carry a total of zero and no calls, so that a caller can tell "no call shares the name" from an answer that does not report the group.
The group SHALL list at most the answer's result limit of calls, in the same order for identical inputs, SHALL disclose when it lists fewer calls than its total, and SHALL list every matching call when the caller requests an unbounded answer.
The group is not a paged result set: it SHALL carry no continuation token, and every page of a paged `references` answer SHALL carry the same group.
The group's calls SHALL NOT be counted among the answer's results or in its paging disclosure, and the group SHALL NOT add the heuristic-grade marker to the answer.
An ambiguous-reference or unresolved-subject answer SHALL NOT carry the group.
The human rendering SHALL present the group after the results, and SHALL label its calls as calls with no resolved target that may or may not call the subject.

Serves: structural-navigation-without-semantic-indexer

#### Scenario: Resolved references and unresolved calls appear side by side

- **GIVEN** an indexed Python method with resolved references, whose name is also the called name of recorded unresolved calls
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** the answer's results are the resolved references only, and the group lists the unresolved calls with their receiver text, locations, and enclosing declarations

#### Scenario: A subject with no matching call carries an empty group

- **GIVEN** an indexed symbol whose name is the called name of no recorded unresolved call
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** the answer carries a group with a total of zero and no calls

#### Scenario: A module-level call names its module

- **GIVEN** an indexed method whose name is the called name of an unresolved call at module scope
- **WHEN** `trace` is invoked for it over the `references` relation and the answer is returned as JSON
- **THEN** the group's entry for that call names the module as its enclosing declaration

#### Scenario: A Rust subject carries the group

- **GIVEN** an indexed Rust method whose name is the called name of an unresolved call recorded in code the analyzer did not analyze
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** the group lists that call

#### Scenario: The group is capped at the result limit

- **GIVEN** an indexed symbol whose name is the called name of more recorded unresolved calls than the requested result limit
- **WHEN** `trace` is invoked for it over the `references` relation at that limit
- **THEN** the group lists exactly the limit's number of calls, carries the full total, discloses that it lists fewer calls than its total, and carries no continuation token

#### Scenario: An unbounded answer lists every unresolved call

- **GIVEN** an indexed symbol whose name is the called name of more recorded unresolved calls than the default result limit
- **WHEN** `trace` is invoked for it over the `references` relation with an unbounded answer requested
- **THEN** the group lists every matching call and discloses no shortfall

#### Scenario: Every page carries the same group

- **GIVEN** an indexed symbol whose resolved references exceed the result limit and whose name is the called name of recorded unresolved calls
- **WHEN** the first page and the page its continuation token resumes are requested
- **THEN** both pages carry the same group

#### Scenario: The group does not change the paging counts

- **GIVEN** an indexed symbol whose resolved references exceed the result limit and whose name is the called name of recorded unresolved calls
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** the paging disclosure's returned and total counts are the counts of resolved references alone

#### Scenario: The group carries no heuristic-grade marker

- **GIVEN** a `references` answer whose group lists one or more calls
- **WHEN** the answer is returned as JSON
- **THEN** it carries no heuristic-grade marker

#### Scenario: An ambiguous subject carries no group

- **GIVEN** a short name shared by several indexed symbols
- **WHEN** `trace` is invoked with it over the `references` relation
- **THEN** the typed candidate set carries no unresolved-call group

#### Scenario: The group renders after the results with its label

- **GIVEN** a `references` answer with resolved references and a group that lists one or more calls
- **WHEN** the answer is rendered for a human
- **THEN** the group appears after the results, and its calls are labeled as calls with no resolved target that may or may not call the subject

#### Scenario: A Bash name containing the separator matches as a whole

- **GIVEN** a structural Bash index holding the function `lib::log`, and commands named `lib::log` and `log` elsewhere
- **WHEN** `trace` is invoked for `lib::log` over the `references` relation
- **THEN** the group lists the command named `lib::log` and not the command named `log`

### Requirement: Empty answers of reference-derived relations claim no proof

> Previously: the requirement did not distinguish the tier of the index.
> This block is based on the text that `unresolved-calls` syncs into the baseline; rebase it on the synced baseline before this change syncs.

An empty answer of `references`, `dependents`, or `importers`, and an `impact` answer from a semantic index whose seeds have no resolved dependent, assert only that the index recorded no resolved instance of the relation.
Such an answer SHALL keep its typed outcome, distinct from an unavailable or failed answer, and SHALL NOT be described, in its human rendering, in the command's self-description, or in the MCP tool description, as a definite or complete empty set.
Its human rendering SHALL state that no resolved instance was recorded; for a `references` answer, SHALL state the totals of its unresolved-call and unconfirmed-use groups; and SHALL state that calls made by frameworks, implicit calls such as constructors and operators, and lookups by string are not seen.
The empty answers of `containers`, `contains`, and `implementers`, and an `impact` answer whose change touches no indexed symbol, are outside this requirement: each states a fact the syntax tree establishes.
On a structural index, these relations and the dependents of an `impact` answer are unavailable rather than empty, as `Relations a structural index cannot answer are unavailable` and `Impact on a structural index reports its dependents as unavailable` require.

Serves: calibrated-capability-disclosure

#### Scenario: An empty references answer states what it cannot see

- **GIVEN** an indexed Python function with no resolved reference, no unresolved call that shares its name, and no refused use, such as a view function registered with a route decorator
- **WHEN** `trace` is invoked for it over the `references` relation and the answer is rendered for a human
- **THEN** the answer has the empty outcome, and the rendering states that no resolved reference was recorded, that no unresolved call and no unconfirmed use was found, and that calls by frameworks, implicit calls, and lookups by string are not seen, and does not call the result definite

#### Scenario: An empty references answer with unresolved calls gives their total

- **GIVEN** an indexed Python method with no resolved reference, whose name is the called name of recorded unresolved calls
- **WHEN** `trace` is invoked for it over the `references` relation and the answer is rendered for a human
- **THEN** the answer has the empty outcome, and the rendering states the total of the unresolved-call group and does not call the result definite

#### Scenario: An empty dependents answer states what it cannot see

- **GIVEN** an indexed function on which no indexed symbol depends
- **WHEN** `trace` is invoked for it over the `dependents` relation and the answer is rendered for a human
- **THEN** the rendering states that no resolved dependent was recorded and that calls by frameworks, implicit calls, and lookups by string are not seen, and does not call the result definite

#### Scenario: An empty importers answer states what it cannot see

- **GIVEN** an indexed module that no indexed module imports
- **WHEN** `trace` is invoked for it over the `importers` relation and the answer is rendered for a human
- **THEN** the rendering states that no resolved importer was recorded and does not call the result definite

#### Scenario: An impact answer with no dependents states what it cannot see

- **GIVEN** a workspace whose diff changes an indexed function on which no indexed symbol depends
- **WHEN** `impact` is invoked and the answer is rendered for a human
- **THEN** the rendering lists the changed function as a seed, states that no resolved dependent was recorded and that calls by frameworks, implicit calls, and lookups by string are not seen, and does not call the result definite

#### Scenario: An impact answer that touches no indexed symbol stays definite

- **GIVEN** a workspace whose diff touches only comments, blank lines, or files the graph does not track
- **WHEN** `impact` is invoked and the answer is rendered for a human
- **THEN** the rendering states that the change touches nothing the graph tracks, as a definite none

#### Scenario: Structural relations keep the definite wording

- **GIVEN** an indexed function that contains no declaration
- **WHEN** `trace` is invoked for it over the `contains` relation and the answer is rendered for a human
- **THEN** the rendering calls the result a definite empty set

#### Scenario: A structural index answers unavailable, not empty

- **GIVEN** a structural Bash index holding a function that no command names
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** the answer's outcome is `unavailable`, not empty

### Requirement: Diff-seeded impact assessment

The system SHALL provide a CLI command `impact` that assesses the reverse-reachability impact of a change without the caller naming a symbol: it derives a set of seed symbols from a git diff, and, on a semantic index, returns the union of those seeds' `dependents` as a depth-bounded impact answer carrying the same honest-horizon disclosure a `dependents` trace carries.
On a semantic index, the impact set SHALL be exactly the reverse-reachability closure of the seed set — no forward or callee-direction reach — and a change that touches no indexed symbol SHALL be a typed-empty successful answer, distinct from an unavailable or failed one.
On a structural index, the seeds are derived in the same way, and `Impact on a structural index reports its dependents as unavailable` governs the answer.

> Previously: every index was semantic, so every impact answer carried the reverse-reachability closure.

Serves: calibrated-capability-disclosure

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

#### Scenario: A structural index derives seeds without reach

- **GIVEN** a structural Bash index, and a diff that changes the function `deploy`, which another function calls
- **WHEN** `impact` is invoked
- **THEN** `deploy` is a seed, and the answer carries no dependents and no horizon disclosure

### Requirement: References answers disclose override relations

> Previously: every index was semantic, so every method subject carried the disclosure.
> This block is based on the text that `method-overrides` syncs into the baseline; rebase it on the synced baseline before this change syncs.

Every `references` answer from a semantic index whose subject resolves to one method SHALL carry, separate from its results, an override disclosure with two parts.
The first part SHALL describe the methods that override the subject, directly or through other overrides.
The second part SHALL describe the methods the subject overrides, directly or through other overrides.
Each part SHALL carry the number of those methods and the total number of their resolved references, and a part with no methods SHALL carry zero for both, so that a caller can tell "not overridden" from an answer that does not report the disclosure.
Each part whose number of methods is not zero SHALL carry the `trace` command that lists the directly related methods (`implementers` for the first part, `implements` for the second), quoted so that it runs as given, and naming the same index store and workspace as the query that produced the answer.
The disclosure SHALL NOT be counted among the answer's results or in its paging disclosure, and every page of a paged answer SHALL carry the same disclosure.
An answer whose subject is not a method, and an ambiguous-reference or unresolved-subject answer, SHALL NOT carry the disclosure.
The human rendering SHALL present each part whose number of methods is not zero on its own line, stating the number of methods, that the number includes overrides of overrides, the total of their resolved references, and the part's command, described as listing the direct relations only.
The scoped line of an empty `references` answer SHALL also state each part whose number of methods is not zero.
A `references` answer from a structural index SHALL NOT carry the disclosure, because override relations are unavailable on it, and a zero count would claim that the method is not overridden.

Serves: calibrated-capability-disclosure

#### Scenario: An overridden method discloses its overrides

- **GIVEN** an indexed Python method with no resolved reference, overridden by two methods that have resolved references
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** the first part carries two methods and the total of their resolved references, and the second part carries zero methods and zero references

#### Scenario: Overrides of overrides are counted

- **GIVEN** an indexed method `A.m` overridden by `B.m`, which is overridden by `C.m`
- **WHEN** `trace` is invoked for `A.m` over the `references` relation
- **THEN** the first part counts both `B.m` and `C.m`

#### Scenario: An overriding method discloses what it overrides

- **GIVEN** an indexed Python method with no resolved reference that overrides a base method with resolved references
- **WHEN** `trace` is invoked for it over the `references` relation and the answer is rendered for a human
- **THEN** the second part carries one method and that method's number of resolved references, and the empty line and the part's line both state that relation

#### Scenario: The part's command runs against the same store

- **GIVEN** an index built into a store at a path other than the default, holding an overridden method whose identity contains characters a shell treats specially
- **WHEN** `trace` is invoked for it over the `references` relation with that store, and the first part's command is run as given
- **THEN** the command lists the method's direct overrides from that store

#### Scenario: The rendering separates the transitive count from the direct listing

- **GIVEN** an indexed method `A.m` overridden by `B.m`, which is overridden by `C.m`
- **WHEN** `trace` is invoked for `A.m` over the `references` relation and the answer is rendered for a human
- **THEN** the first part's line states two methods including overrides of overrides, and describes its command as listing the direct overrides

#### Scenario: A Rust impl method discloses the trait item it implements

- **GIVEN** an indexed Rust impl method that implements a trait item with resolved references
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** the second part carries one method and that trait item's number of resolved references

#### Scenario: A method with no override relation carries zeros

- **GIVEN** an indexed method that overrides nothing and that nothing overrides
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** both parts carry zero methods and zero references, and the human rendering shows no override line

#### Scenario: A type subject carries no override disclosure

- **GIVEN** an indexed class with subclasses
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** the answer carries no override disclosure

#### Scenario: Every page carries the same override disclosure

- **GIVEN** an indexed overridden method whose resolved references exceed the result limit
- **WHEN** the first page and the page its continuation token resumes are requested
- **THEN** both pages carry the same disclosure, and the paging disclosure counts resolved references of the subject alone

#### Scenario: A structural references answer carries no override disclosure

- **GIVEN** a structural Python index whose class `Admin(User)` and its base `User` both declare `save`
- **WHEN** `trace` is invoked for `User.save` over the `references` relation
- **THEN** the answer carries no override disclosure
