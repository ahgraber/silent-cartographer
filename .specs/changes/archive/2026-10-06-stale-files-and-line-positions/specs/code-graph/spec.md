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

### Requirement: Recorded declarations carry their header end

For every symbol the index records with a definition span, the system SHALL record where the symbol's header ends, both as a byte offset and as a line and a column, and both forms SHALL denote the same position in the source text the build indexed.
A symbol's header SHALL be the part of its definition span that precedes the declaration's body, and SHALL start where the definition span starts.
A declaration with no body distinct from its form, such as a constant, a field, or a parameter, SHALL have a header equal to its whole definition span.
A document's own module, whose header does not appear in the source, SHALL have no header end, and neither SHALL a symbol without a definition span.
Line and column SHALL follow the same numbering as every other recorded location.

Serves: clickable-locations

#### Scenario: A function's header ends before its body

- **GIVEN** a function whose declaration form precedes a body
- **WHEN** the index is built
- **THEN** the function's header end is the position where its body begins, not the end of its definition span

#### Scenario: A header spans the lines its declaration form spans

- **GIVEN** a function whose parameter list runs over several lines, or a declaration preceded by decorators that its definition span includes
- **WHEN** the index is built
- **THEN** the header covers every line from the start of the definition span through the end of the declaration form

#### Scenario: A declaration without a body

- **GIVEN** a constant, a field, or a parameter that the index records with a definition span
- **WHEN** the index is built
- **THEN** its header end equals its definition span's end

#### Scenario: A document's own module has no header end

- **GIVEN** any indexed source file
- **WHEN** the index is built
- **THEN** the module the file defines carries no header end, while a module declared inside the file carries one

### Requirement: A build names sources that changed while it ran

After a build writes the store, the system SHALL discover the workspace's sources again and compare each with the text the build indexed.
A source discovered again with different text SHALL be named changed, a source discovered only now SHALL be named added, and an indexed source no longer discovered SHALL be named removed.
The build SHALL succeed whether or not any source is named, and both its human answer and its machine answer SHALL name every such source with its classification.
When a source cannot be read again for a reason other than no longer existing, the build SHALL fail, although the store is already written.
When no source is named, the machine answer SHALL carry an empty list.
A build that analyzes nothing because the stored index is already current SHALL carry an empty list.

Serves: build-names-concurrent-edits

#### Scenario: An edit during the build is named

- **GIVEN** a workspace whose analyzer rewrites one source file while it runs
- **WHEN** the workspace is built
- **THEN** the build succeeds, its answer names that file as changed, and `status` then reports the index stale for changed content

#### Scenario: A file created during the build is named

- **GIVEN** a workspace whose analyzer creates a new source file while it runs
- **WHEN** the workspace is built
- **THEN** the build succeeds and its answer names that file as added

#### Scenario: A file deleted during the build is named

- **GIVEN** a workspace whose analyzer deletes one indexed source file while it runs
- **WHEN** the workspace is built
- **THEN** the build succeeds and its answer names that file as removed

#### Scenario: A source that becomes unreadable fails the build

- **GIVEN** a workspace whose analyzer creates a source file that cannot be read while it runs
- **WHEN** the workspace is built
- **THEN** the build fails with an error that names that file, and the store holds the new index

#### Scenario: An undisturbed build names nothing

- **GIVEN** a workspace that no process edits during the build
- **WHEN** the workspace is built
- **THEN** the machine answer carries an empty list and the human answer names no file

#### Scenario: An already-current build names nothing

- **GIVEN** a workspace whose index is already current
- **WHEN** the workspace is built without the explicit rebuild option
- **THEN** no analysis runs and the machine answer carries an empty list
