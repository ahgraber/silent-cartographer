# Delta for Code Graph

## ADDED Requirements

### Requirement: Unresolved calls are recorded

For every build, the system SHALL record each call in an indexed document whose called name has no semantic occurrence at the called name's location, and SHALL NOT record a call whose called name has a semantic occurrence there, whatever symbol that occurrence names, including a symbol local to the document such as a parameter.
A call's called name is the last name its callee is written with: `name` in Python `name(...)` and `receiver.name(...)`, and in Rust `name(...)`, `receiver.name(...)`, and `path::name(...)`.
A call whose callee ends in no name, such as a call of a subscript or of a parenthesized expression, SHALL NOT be recorded.
Each recorded call SHALL carry its called name, the location of the called name, the source text of its receiver when the callee has one (the expression before the last `.` or `::`), and its enclosing declaration, attributed under the same rule as a reference occurrence, or its module when no narrower declaration encloses it.
Receiver text longer than a documented bound SHALL be recorded as its leading part with a marker that it was shortened.
A call written inside the arguments of a Rust macro invocation is outside this requirement.

Serves: references-name-what-they-cannot-see

#### Scenario: A Python attribute call the indexer cannot resolve is recorded

- **GIVEN** a Python test function whose parameter has no annotation and whose body calls `client.fetch()`, indexed by each supported Python indexer in turn
- **WHEN** the index is built
- **THEN** the call is recorded with the called name `fetch`, the receiver text `client`, the location of `fetch`, and the test function as its enclosing declaration

#### Scenario: A Python plain call with no occurrence is recorded without a receiver

- **GIVEN** a Python module that calls a function imported from a third-party package, indexed by an indexer that records no occurrence for that call
- **WHEN** the index is built
- **THEN** the call is recorded with its called name and no receiver text

#### Scenario: A Rust call in code the analyzer does not analyze is recorded

- **GIVEN** a Rust function whose body calls `value.render()` under a `#[cfg(...)]` condition that is false for the build host
- **WHEN** the index is built
- **THEN** the call is recorded with the called name `render` and the receiver text `value`

#### Scenario: A Rust path call carries its path as receiver

- **GIVEN** a Rust call `Widget::build()` whose called name has no semantic occurrence
- **WHEN** the index is built
- **THEN** the call is recorded with the called name `build` and the receiver text `Widget`

#### Scenario: A resolved call is not recorded

- **GIVEN** a Python call and a Rust call whose called names each have a semantic occurrence
- **WHEN** the index is built
- **THEN** neither call is recorded as unresolved

#### Scenario: A call resolved to a local binding is not recorded

- **GIVEN** a Python function that calls its parameter `callback()`, indexed by each supported Python indexer in turn, and a Rust function that calls its closure parameter `callback()`, where each indexer records an occurrence of the local parameter at the called name
- **WHEN** the index is built
- **THEN** neither call is recorded as unresolved

#### Scenario: A module-level call attributes to the module

- **GIVEN** an unresolved call at module scope, enclosed by no narrower declaration
- **WHEN** the index is built
- **THEN** the recorded call's enclosing declaration is the module

#### Scenario: A long receiver is shortened with a marker

- **GIVEN** an unresolved call whose receiver is a call spanning many lines, longer than the documented bound
- **WHEN** the index is built
- **THEN** the recorded receiver text is the receiver's leading part, no longer than the bound, and is marked as shortened

## MODIFIED Requirements

### Requirement: Join discrepancies are inspectable

> Previously: a persisted discrepancy did not record which persisted symbols the refused occurrence may denote.

For every semantic occurrence whose join outcome is not aligned, the system SHALL persist the discrepancy's location, outcome kind, expected symbol name, and the source text found at that location, wholly superseded by each subsequent build; SHALL make that detail retrievable through the CLI; the default listing SHALL be a bounded summary whose aggregation covers the full discrepancy set and SHALL state when entries are withheld by the bound; and an explicit request SHALL return the complete detail.
For a refused reference occurrence, the system SHALL also persist the persisted symbols it may denote: every duplicate of its descriptor when it was recorded duplicate-ambiguous, and otherwise the one persisted symbol the occurrence names, if there is one.

Serves: references-name-what-they-cannot-see

#### Scenario: Discrepancy detail persisted and retrievable

- **GIVEN** a build that produces at least one non-aligned occurrence
- **WHEN** the build completes
- **THEN** the occurrence's location, outcome kind, expected symbol name, and found source text are retrievable

#### Scenario: A duplicate-ambiguous reference records every duplicate

- **GIVEN** a reference recorded duplicate-ambiguous between three definitions that share one descriptor
- **WHEN** the build completes
- **THEN** its discrepancy records all three definitions as the symbols it may denote

#### Scenario: A text-mismatch reference records the symbol it names

- **GIVEN** a reference occurrence of a persisted symbol, refused because the source text at its position is not the symbol's name
- **WHEN** the build completes
- **THEN** its discrepancy records that symbol as the one it may denote

#### Scenario: A refused occurrence of no persisted symbol records none

- **GIVEN** a refused reference occurrence whose symbol is not persisted
- **WHEN** the build completes
- **THEN** its discrepancy records no symbol it may denote

#### Scenario: Truncation is typed and the summary covers the full set

- **GIVEN** more persisted discrepancies than the bounded listing displays
- **WHEN** the default listing is requested
- **THEN** the listing states that it is truncated and its aggregate summary reflects every persisted discrepancy, not only the displayed entries

#### Scenario: Explicit request returns everything

- **GIVEN** more persisted discrepancies than the bounded listing displays
- **WHEN** the complete detail is explicitly requested
- **THEN** every persisted discrepancy is returned

#### Scenario: Discrepancies are superseded per build

- **GIVEN** discrepancies persisted by a prior build
- **WHEN** a new build completes
- **THEN** only the new build's discrepancies are retrievable

### Requirement: Recorded locations carry line and column

> Previously: the requirement listed a symbol's definition, a reference occurrence, and a join discrepancy, and no unresolved call.

For every location the index records — a symbol's definition, a reference occurrence, a join discrepancy, and an unresolved call — the system SHALL record its start and its end both as byte offsets and as a line and a column, and both forms SHALL denote the same positions in the source text the build indexed.
A line SHALL be numbered from 1, and a line SHALL end at a line feed, at a carriage return followed by a line feed, or at a carriage return not followed by a line feed.
A column SHALL be numbered from 1 and SHALL count Unicode characters from the start of the line, not bytes and not UTF-16 code units.
The end SHALL be the position immediately after the location's last character, as the end byte offset is.
A byte offset between the carriage return and the line feed of one line ending, which no line and column can name, SHALL have the line and column of that carriage return; every other byte offset and its line and column SHALL denote the same position.
A location whose byte offsets are absent SHALL have its line and column absent too.

Serves: references-name-what-they-cannot-see

#### Scenario: A location in ASCII text

- **GIVEN** a source file whose third line is `fn target() {}` with no indentation
- **WHEN** the index is built
- **THEN** the definition of `target` starts at line 3, column 1 and ends at line 3, column 15, and its byte offsets denote the same positions

#### Scenario: Columns count characters, not bytes

- **GIVEN** a source line in which a character encoded in more than one byte, such as `é`, precedes a reference occurrence
- **WHEN** the index is built
- **THEN** the occurrence's start column counts that character once

#### Scenario: Columns count characters, not UTF-16 code units

- **GIVEN** a source line in which a character outside the Basic Multilingual Plane, such as an emoji, precedes a reference occurrence
- **WHEN** the index is built
- **THEN** the occurrence's start column counts that character once

#### Scenario: Each line ending starts a new line

- **GIVEN** three source files that separate their lines with line feeds, with carriage return and line feed pairs, and with lone carriage returns
- **WHEN** the index is built
- **THEN** a definition on the fifth line of each file is recorded at line 5, and the carriage return and line feed pair counts as one line ending

#### Scenario: A position inside a line ending

- **GIVEN** a location whose byte offset falls between the carriage return and the line feed of a line ending
- **WHEN** the index is built
- **THEN** that position is recorded at the line and column of the carriage return

#### Scenario: A location that spans lines

- **GIVEN** a function whose definition span runs from its first line to its closing brace several lines later
- **WHEN** the index is built
- **THEN** the definition's end line is the line of the closing brace and its end column is the column after the brace

#### Scenario: A join discrepancy carries line and column

- **GIVEN** a semantic occurrence that the join refuses, at coordinates that normalize to a byte range
- **WHEN** the index is built
- **THEN** the persisted discrepancy carries the line and column of that byte range

#### Scenario: Absent coordinates stay absent

- **GIVEN** a semantic occurrence that the join refuses, at coordinates that cannot normalize to a byte range
- **WHEN** the index is built
- **THEN** the persisted discrepancy carries neither byte offsets nor a line and column

#### Scenario: An unresolved call carries line and column

- **GIVEN** an unresolved call whose line holds a character encoded in more than one byte before the called name
- **WHEN** the index is built
- **THEN** the recorded call's start column counts that character once, and its byte offsets denote the same position
