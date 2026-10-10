# Delta for code-graph

## MODIFIED Requirements

### Requirement: Clone-equivalence keys

> Previously: the token sequences disregarded comments and whitespace only, so a Python docstring counted as a literal value while a Rust doc comment did not.

For every corpus-contributing symbol that contains no other corpus-contributing symbol, the system SHALL persist two equivalence keys over the symbol's source: a formatting-insensitive key that two symbols share exactly when their token sequences are identical after comments, whitespace, and documentation are disregarded, and a substitution-insensitive key that two symbols share exactly when their token sequences are additionally identical under a consistent one-to-one substitution of identifiers and literal values.
Documentation here is the text the system reads as a symbol's own documentation for its interface tier: a Rust doc comment, and a Python docstring, which is the leading string statement of a function, class, or module body that Python reads as its docstring.
A string statement that is not in a docstring position SHALL remain in the token sequence as a literal value.
A symbol that contains other corpus-contributing symbols SHALL carry no equivalence key.
Keys SHALL be persisted with the build and wholly superseded by each subsequent build.

Serves: documentation-blind-clones

#### Scenario: Formatting variants share the formatting-insensitive key

- **GIVEN** two indexed functions whose sources differ only in whitespace and comments
- **WHEN** the index is built
- **THEN** the two functions carry the same formatting-insensitive key

#### Scenario: A consistently renamed copy shares only the substitution-insensitive key

- **GIVEN** two indexed functions identical except that one consistently renames the other's identifiers
- **WHEN** the index is built
- **THEN** the two functions carry the same substitution-insensitive key and different formatting-insensitive keys

#### Scenario: A literal-substituted copy shares only the substitution-insensitive key

- **GIVEN** two indexed functions identical except that one consistently replaces the other's literal values
- **WHEN** the index is built
- **THEN** the two functions carry the same substitution-insensitive key and different formatting-insensitive keys

#### Scenario: An inconsistent renaming shares neither key

- **GIVEN** two indexed functions alike except that one merges two distinct identifiers of the other into a single name
- **WHEN** the index is built
- **THEN** the two functions share neither equivalence key

#### Scenario: Unrelated code shares neither key

- **GIVEN** two indexed functions with different token sequences under every normalization
- **WHEN** the index is built
- **THEN** the two functions share neither equivalence key

#### Scenario: A container carries no equivalence key

- **GIVEN** an indexed type containing several methods
- **WHEN** the index is built
- **THEN** the type carries no equivalence key

#### Scenario: Keys are superseded by each build

- **GIVEN** an indexed leaf whose source then changes
- **WHEN** the store is rebuilt
- **THEN** the leaf's persisted keys are those of the new source and the prior build's keys are gone

#### Scenario: Python docstrings that differ share the formatting-insensitive key

- **GIVEN** two indexed Python functions identical except for the text of their docstrings
- **WHEN** the index is built
- **THEN** the two functions carry the same formatting-insensitive key

#### Scenario: A Python docstring present in only one copy is disregarded

- **GIVEN** two indexed Python functions identical except that only one has a docstring
- **WHEN** the index is built
- **THEN** the two functions carry the same formatting-insensitive key

#### Scenario: A renamed Python copy with a different docstring shares the substitution-insensitive key

- **GIVEN** two indexed Python test functions identical except for consistently renamed identifiers and a docstring present in only one
- **WHEN** the index is built
- **THEN** the two functions carry the same substitution-insensitive key and different formatting-insensitive keys

#### Scenario: Rust doc comments that differ share the formatting-insensitive key

- **GIVEN** two indexed Rust functions identical except for the text of their doc comments
- **WHEN** the index is built
- **THEN** the two functions carry the same formatting-insensitive key

#### Scenario: A string statement outside a docstring position stays a literal

- **GIVEN** two indexed Python functions identical except for the text of a bare string statement that follows the first statement of the body
- **WHEN** the index is built
- **THEN** the two functions carry the same substitution-insensitive key and different formatting-insensitive keys

## ADDED Requirements

### Requirement: Shared-fragment evidence persisted per build

For every corpus-contributing symbol with comparable content, the system SHALL persist with the build the evidence that the shared-fragment answer reads, derived from that comparable content.
A symbol that carries equivalence keys has as comparable content the documentation-disregarding token sequence the keys read.
A function or method that contains other corpus-contributing symbols has as comparable content the same documentation-disregarding token sequence over its own span, with the tokens of each contained corpus-contributing symbol replaced by one boundary that no run of shared tokens can include.
Any other symbol, such as a type or module that contains other corpus-contributing symbols, has no comparable content and SHALL contribute no evidence.
The evidence SHALL be wholly superseded by each subsequent build, a build that fails SHALL leave the prior build's evidence intact and authoritative, and a rebuild over unchanged sources SHALL yield identical evidence.
The build SHALL record with the evidence the guaranteed fragment length and the data-stretch length it was derived under, and the shared-fragment answer SHALL apply that recorded data-stretch length and apply and report that recorded guaranteed length.

Serves: shared-fragments

#### Scenario: A leaf contributes evidence

- **GIVEN** an indexed function longer than the guaranteed fragment length
- **WHEN** the index is built
- **THEN** the store holds shared-fragment evidence for the function

#### Scenario: A container contributes no evidence

- **GIVEN** an indexed type containing several methods
- **WHEN** the index is built
- **THEN** the store holds no shared-fragment evidence for the type itself

#### Scenario: A function that contains a nested function contributes evidence for its own code

- **GIVEN** an indexed function that defines a nested function, and whose own code outside the nested function is longer than the guaranteed fragment length
- **WHEN** the index is built
- **THEN** the store holds shared-fragment evidence for the function derived from its own code, and separate evidence for the nested function

#### Scenario: Rebuild over unchanged sources is idempotent

- **GIVEN** a store built from a set of sources
- **WHEN** the same sources are built into the same store again
- **THEN** the persisted shared-fragment evidence is identical to the first build's

#### Scenario: Evidence of a changed leaf is replaced

- **GIVEN** an indexed leaf whose source then changes
- **WHEN** the store is rebuilt
- **THEN** the leaf's evidence derives from the new source and no evidence from the prior source remains

#### Scenario: A failed build leaves prior evidence authoritative

- **GIVEN** a store holding a completed build's shared-fragment evidence
- **WHEN** a subsequent build fails before completing
- **THEN** the prior build's evidence remains retrievable, unchanged

#### Scenario: The answer applies the length the store records

- **GIVEN** a store whose recorded guaranteed fragment length differs from the length the binary builds with
- **WHEN** a shared-fragment answer is computed from that store
- **THEN** the answer applies and reports the recorded length
