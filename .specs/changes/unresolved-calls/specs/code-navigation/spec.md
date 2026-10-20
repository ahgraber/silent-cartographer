# Delta for Code Navigation

## ADDED Requirements

### Requirement: References answers disclose unresolved calls that share the subject's name

Every `references` answer whose subject resolves to one symbol SHALL carry, separate from its results, an unresolved-call group: the recorded unresolved calls whose called name equals the last component of the subject's name.
The group SHALL carry the called name, the total number of matching calls, and the calls, each with its location as line and column, its receiver text and whether that text was shortened when it has a receiver, and its enclosing declaration or module.
A group with no matching calls SHALL carry a total of zero and no calls, so that a caller can tell "no call shares the name" from an answer that does not report the group.
The group SHALL list at most the answer's result limit of calls, in the same order for identical inputs, SHALL disclose when it lists fewer calls than its total, and SHALL list every matching call when the caller requests an unbounded answer.
The group is not a paged result set: it SHALL carry no continuation token, and every page of a paged `references` answer SHALL carry the same group.
The group's calls SHALL NOT be counted among the answer's results or in its paging disclosure, and the group SHALL NOT add the heuristic-grade marker to the answer.
An ambiguous-reference or unresolved-subject answer SHALL NOT carry the group.
The human rendering SHALL present the group after the results, and SHALL label its calls as calls with no resolved target that may or may not call the subject.

Serves: references-name-what-they-cannot-see

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

### Requirement: References answers disclose unconfirmed uses of the subject

Every `references` answer whose subject resolves to one symbol SHALL carry, separate from its results, an unconfirmed-use group: the refused reference occurrences whose recorded symbols include the subject.
The group SHALL carry the total number of such uses and the uses, each with its document, its location as line and column when the refusal recorded one, its refusal kind, and the detail of that refusal: for a duplicate-ambiguous use, the symbols it may denote and their number; for any other refusal, the source text found at its position when there was any.
A use whose refusal recorded no location, or no source text, SHALL carry typed absence for it, never a fabricated value.
A use SHALL list at most a documented number of the symbols it may denote, and at most a documented length of found source text, and SHALL disclose when either is shortened.
A group with no such uses SHALL carry a total of zero and no uses.
The group SHALL list at most the answer's result limit of uses, in the same order for identical inputs, SHALL disclose when it lists fewer uses than its total, and SHALL list every use when the caller requests an unbounded answer.
The group is not a paged result set: it SHALL carry no continuation token, and every page of a paged `references` answer SHALL carry the same group.
The group's uses SHALL NOT be counted among the answer's results or in its paging disclosure.
An ambiguous-reference or unresolved-subject answer SHALL NOT carry the group.
The human rendering SHALL present the group after the unresolved-call group, and SHALL label its uses as uses that may refer to the subject but are not counted, each with its reason.

Serves: references-name-what-they-cannot-see

#### Scenario: A use ambiguous between duplicates is disclosed on each duplicate

- **GIVEN** a reference recorded duplicate-ambiguous between three definitions that share one descriptor
- **WHEN** `trace` is invoked over the `references` relation for each of the three definitions
- **THEN** each answer's unconfirmed-use group lists that use with all three definitions as the symbols it may denote

#### Scenario: A use refused for mismatched text is disclosed with the text found

- **GIVEN** a reference occurrence of an indexed symbol, refused because the source text at its position is not the symbol's name
- **WHEN** `trace` is invoked for that symbol over the `references` relation
- **THEN** the unconfirmed-use group lists that use with the source text found at its position

#### Scenario: A refused use with no location is listed with typed absence

- **GIVEN** a reference occurrence of an indexed symbol whose range lies beyond the end of its document, so the join recorded it with no location and no source text
- **WHEN** `trace` is invoked for that symbol over the `references` relation and the answer is returned as JSON and rendered for a human
- **THEN** the unconfirmed-use group lists that use with its document and refusal kind, its location and source text are typed absence, and the rendering states that its position is unknown

#### Scenario: A use ambiguous among many duplicates lists a bounded candidate set

- **GIVEN** a reference recorded duplicate-ambiguous among more definitions than the documented candidate bound
- **WHEN** `trace` is invoked for one of them over the `references` relation
- **THEN** the use lists exactly the bound's number of candidates, carries the full number, and discloses that the list is shortened

#### Scenario: A subject with no refused use carries an empty group

- **GIVEN** an indexed symbol that no refused reference occurrence may denote
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** the answer carries an unconfirmed-use group with a total of zero and no uses

#### Scenario: The unconfirmed-use group is capped at the result limit

- **GIVEN** an indexed symbol that more refused reference occurrences may denote than the requested result limit
- **WHEN** `trace` is invoked for it over the `references` relation at that limit
- **THEN** the group lists exactly the limit's number of uses, carries the full total, discloses that it lists fewer uses than its total, and carries no continuation token

#### Scenario: The unconfirmed-use group renders with its reasons

- **GIVEN** a `references` answer whose unconfirmed-use group lists a duplicate-ambiguous use and a use refused for mismatched text
- **WHEN** the answer is rendered for a human
- **THEN** the group appears after the unresolved-call group, the first use names the definitions it could mean, and the second names the source text found at its position

### Requirement: Empty answers of reference-derived relations claim no proof

An empty answer of `references`, `dependents`, `importers`, or `tests`, and an `impact` answer whose seeds have no resolved dependent, assert only that the index recorded no resolved instance of the relation.
Such an answer SHALL keep its typed outcome, distinct from an unavailable or failed answer, and SHALL NOT be described, in its human rendering, in the command's self-description, or in the MCP tool description, as a definite or complete empty set.
Its human rendering SHALL state that no resolved instance was recorded, or for a `tests` answer that no convention-classified test reference was found; for a `references` answer, SHALL state the totals of its unresolved-call and unconfirmed-use groups; and SHALL state that calls made by frameworks, implicit calls such as constructors and operators, and lookups by string are not seen.
The empty answers of `containers`, `contains`, and `implementers`, and an `impact` answer whose change touches no indexed symbol, are outside this requirement: each states a fact the syntax tree establishes.

Serves: empty-answers-claim-no-proof

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

#### Scenario: An empty tests answer states what it cannot see

- **GIVEN** an indexed function referenced only from production code
- **WHEN** `trace` is invoked for it over the `tests` relation and the answer is rendered for a human
- **THEN** the rendering states that no convention-classified test reference was found and that calls by frameworks, implicit calls, and lookups by string are not seen, and does not call the result definite

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

### Requirement: Unresolved calls stay out of every relation

The system SHALL NOT count a recorded unresolved call or a refused reference occurrence as a result of any relation: not as a reference, a dependent, an impact row, a test site, an importer, or an implementer.
No answer other than a `references` answer SHALL carry the unresolved-call group or the unconfirmed-use group.

Serves: references-name-what-they-cannot-see

#### Scenario: Dependents ignore unresolved calls

- **GIVEN** an indexed symbol whose only callers reach it through unresolved calls
- **WHEN** `trace` is invoked for it over the `dependents` relation
- **THEN** the answer is empty and carries neither group

#### Scenario: Tests ignore unresolved calls

- **GIVEN** an indexed symbol whose name is the called name of an unresolved call inside a test-classified function
- **WHEN** `trace` is invoked for it over the `tests` relation
- **THEN** that call is not returned as a test site

#### Scenario: Dependents ignore refused uses

- **GIVEN** an indexed symbol whose only use is a refused reference occurrence
- **WHEN** `trace` is invoked for it over the `dependents` relation
- **THEN** the answer is empty

## MODIFIED Requirements

### Requirement: Relationship trace

> Previously: every empty relation was described as a definite "none"; an empty answer of a relation derived from references now asserts only that no resolved instance was recorded.

The system SHALL provide a CLI command `trace` that, given a subject symbol and a relation kind, returns the symbols standing in that relation to the subject.
Relation kinds follow a shared-root directional convention (e.g. `containers`/`contains`), and the supported relations are `containers` (the declaration that directly encloses a subject), `contains` (the symbols a subject directly contains), `references` (the sites that reference a subject, which for a type subject are its type-occurrences), `dependents` (the symbols that depend on the subject directly or transitively, subject to a depth bound), `importers` (the modules that import the subject), `implementers` (the types that declare the subject as a supertype — a trait's implementors or a base type's subtypes), and `tests` (the reference sites of a subject whose enclosing declaration is classified test code — the answer to "what test code exercises this symbol").
The command's self-description SHALL present `dependents` as impact assessment — the answer to "what could break if this symbol changes" — and SHALL present `tests` as convention-based classification rather than resolved semantic fact.
An empty answer of a structural relation (`containers`, `contains`, `implementers`) is a definite "none"; an empty answer of a relation derived from references is governed by "Empty answers of reference-derived relations claim no proof".

Serves: empty-answers-claim-no-proof

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
- **THEN** an empty set is returned as typed absence, distinct from an unavailable or failed answer

#### Scenario: An empty structural relation is a definite none

- **GIVEN** an indexed symbol that directly contains no declaration
- **WHEN** `trace` is invoked for it over the `contains` relation
- **THEN** an empty set is returned as a definite "none"
