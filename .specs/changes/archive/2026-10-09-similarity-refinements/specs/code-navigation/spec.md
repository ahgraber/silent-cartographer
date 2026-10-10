# Delta for code-navigation

## MODIFIED Requirements

### Requirement: Clone certainty in similar answers

> Previously: the variant-clone marker asserted identical token structure with names and literal values substituted, and documentation counted as structure.

Within a `similar` answer whose subject carries equivalence keys, a returned symbol sharing the subject's formatting-insensitive key SHALL carry a typed exact-clone marker, and a returned symbol sharing the subject's substitution-insensitive key but not its formatting-insensitive key SHALL carry a typed variant-clone marker — asserting an identical token structure with only names and literal values substituted and documentation disregarded, never behavioral equivalence.
Rows carrying a clone marker SHALL precede rows carrying none, exact-clone rows SHALL precede variant-clone rows, and within each certainty class the estimated-similarity order SHALL apply.
The markers derive from deterministic equivalence and SHALL never be assigned from estimated similarity; when the subject carries no equivalence key, no row SHALL carry a clone marker.

Serves: documentation-blind-clones

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

## ADDED Requirements

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

Serves: similar-rows-name-their-relation

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

Serves: similar-rows-name-their-relation

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

Serves: duplication-audit-in-one-run

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

Serves: duplication-audit-in-one-run

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

Serves: duplication-audit-in-one-run

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

Serves: shared-fragments

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

Every `clones` answer that reaches the clone evidence, a typed-empty answer included, SHALL carry, in the machine answer and the human rendering alike, a structural marker identifying its rows as deterministic token equivalence, never behavioral equivalence, and SHALL carry the guaranteed fragment length in effect.
A `clones` answer SHALL NOT carry the estimation marker or the semantic-index identity, and SHALL carry no similarity score.
An ambiguous-reference or unresolved-subject answer terminates before the clone evidence is read, so it SHALL NOT carry the token-equivalence marker.
The marker SHALL accompany, never replace, the provenance and freshness labeling every answer carries.
The machine answer and the human rendering SHALL both name every symbol not compared completely.

Serves: duplication-audit-in-one-run, shared-fragments

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
