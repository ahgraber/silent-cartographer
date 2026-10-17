# Delta for Code Navigation

## MODIFIED Requirements

### Requirement: Relationship trace

> Previously: `implementers` accepted only a type subject, and no relation returned what a subject implements.
> This block is based on the text that `unresolved-calls` syncs into the baseline; rebase it on the synced baseline before this change syncs.

The system SHALL provide a CLI command `trace` that, given a subject symbol and a relation kind, returns the symbols standing in that relation to the subject.
Relation kinds follow a shared-root directional convention (e.g. `containers`/`contains`), and the supported relations are `containers` (the declaration that directly encloses a subject), `contains` (the symbols a subject directly contains), `references` (the sites that reference a subject, which for a type subject are its type-occurrences), `dependents` (the symbols that depend on the subject directly or transitively, subject to a depth bound), `importers` (the modules that import the subject), `implementers` (for a type subject, the types that declare the subject as a supertype — a trait's implementors or a base type's subtypes; for a method subject, the methods that directly override it), `implements` (for a type subject, the supertypes it directly declares; for a method subject, the methods it directly overrides), and `tests` (the reference sites of a subject whose enclosing declaration is classified test code — the answer to "what test code exercises this symbol").
The command's self-description SHALL present `dependents` as impact assessment — the answer to "what could break if this symbol changes" — and SHALL present `tests` as convention-based classification rather than resolved semantic fact.
An empty answer of a structural relation (`containers`, `contains`, `implementers`, `implements`) is a definite "none"; an empty answer of a relation derived from references is governed by "Empty answers of reference-derived relations claim no proof".

Serves: overrides-are-listed

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

### Requirement: Ranked ordering of dependents

> Previously: the requirement did not say which dependency edges structural importance counts, and the importance computation did not count `overrides` edges.

Under the default `ranked` ordering, the system SHALL order the detailed rows of every `dependents` trace and every `impact` answer by distance first, nearest first, and within equal distance by the dependent's codebase-wide structural importance, most important first.
Structural importance SHALL count every dependency edge kind the dependents traversal follows, `overrides` included, so that a method's importance includes the importance of the methods that override it, and an overriding method's importance does not include the importance of the method it overrides.
The ordering SHALL NOT alter which symbols the answer contains, the depth bound, the beyond-bound aggregates, or the horizon disclosure.
When the result limit truncates the detailed rows, the rows returned SHALL be the first of the ordered sequence, so a bounded page holds the nearest, most important dependents the answer has to offer.

Serves: base-change-reaches-overrides

#### Scenario: Within a layer the widely-depended-upon dependent ranks first

- **GIVEN** an indexed symbol with two direct dependents, one that much of the codebase transitively depends on and one that nothing else depends on
- **WHEN** `trace` is invoked for it over the `dependents` relation under the `ranked` ordering
- **THEN** the widely-depended-upon dependent is returned before the other

#### Scenario: An overridden method ranks by its overrides' dependents

- **GIVEN** an indexed symbol with two direct dependents: a base method with one direct caller, overridden by one method that has six callers, and a function with three direct callers; and no other dependency edge leaves the overriding method, the callers, or the function
- **WHEN** `trace` is invoked for the symbol over the `dependents` relation under the `ranked` ordering
- **THEN** the base method is returned before the function

#### Scenario: An override does not inherit its base's importance

- **GIVEN** an indexed symbol with two direct dependents: an overriding method with no callers whose base method is widely depended upon, and a function with one caller
- **WHEN** `trace` is invoked for the symbol over the `dependents` relation under the `ranked` ordering
- **THEN** the function is returned before the overriding method

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

## ADDED Requirements

### Requirement: References answers disclose override relations

Every `references` answer whose subject resolves to one method SHALL carry, separate from its results, an override disclosure with two parts.
The first part SHALL describe the methods that override the subject, directly or through other overrides.
The second part SHALL describe the methods the subject overrides, directly or through other overrides.
Each part SHALL carry the number of those methods and the total number of their resolved references, and a part with no methods SHALL carry zero for both, so that a caller can tell "not overridden" from an answer that does not report the disclosure.
Each part whose number of methods is not zero SHALL carry the `trace` command that lists the directly related methods (`implementers` for the first part, `implements` for the second), quoted so that it runs as given, and naming the same index store and workspace as the query that produced the answer.
The disclosure SHALL NOT be counted among the answer's results or in its paging disclosure, and every page of a paged answer SHALL carry the same disclosure.
An answer whose subject is not a method, and an ambiguous-reference or unresolved-subject answer, SHALL NOT carry the disclosure.
The human rendering SHALL present each part whose number of methods is not zero on its own line, stating the number of methods, that the number includes overrides of overrides, the total of their resolved references, and the part's command, described as listing the direct relations only.
The scoped line of an empty `references` answer SHALL also state each part whose number of methods is not zero.

Serves: overridden-method-is-not-reported-dead

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
