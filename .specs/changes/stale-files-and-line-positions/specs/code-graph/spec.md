# Delta for code-graph

## ADDED Requirements

### Requirement: Recorded locations carry line and column

For every location the index records — a symbol's definition, a reference occurrence, and a join discrepancy — the system SHALL record its start and its end both as byte offsets and as a line and a column, and both forms SHALL denote the same positions in the source text the build indexed.
A line SHALL be numbered from 1, and a line SHALL end at a line feed, at a carriage return followed by a line feed, or at a carriage return not followed by a line feed.
A column SHALL be numbered from 1 and SHALL count Unicode characters from the start of the line, not bytes and not UTF-16 code units.
The end SHALL be the position immediately after the location's last character, as the end byte offset is.
A byte offset between the carriage return and the line feed of one line ending, which no line and column can name, SHALL have the line and column of that carriage return; every other byte offset and its line and column SHALL denote the same position.
A location whose byte offsets are absent SHALL have its line and column absent too.

Serves: clickable-locations

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
