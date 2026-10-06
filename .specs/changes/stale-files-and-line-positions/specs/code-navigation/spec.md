# Delta for code-navigation

## ADDED Requirements

### Requirement: Answers report locations as line and column

Every location in every answer — a result's location in `get`, `trace`, `find`, `search`, `similar`, and `impact`, a region `impact` could not map, and a discrepancy row in the index status report — SHALL carry its start and end line and column beside its byte offsets in the machine answer, and both forms SHALL denote the same positions.
The line and column of a location drawn from the index SHALL describe the text the index was built from, as its byte offsets do, including when that answer is marked stale.
The line and column of a region `impact` could not map SHALL describe the pre-change text that region's byte offsets describe, and SHALL be absent when that pre-change text could not be read.
For a query that reads no `--at` position, the machine answer SHALL be the same whichever position unit the caller selects.
Under the line position unit, which is the default, the human rendering SHALL name a location's start as `path:line:column`; under the byte position unit, it SHALL name the location as `path:start-end` in byte offsets.

Serves: clickable-locations

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

Serves: clickable-locations

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

Serves: status-names-what-changed

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

Serves: status-names-what-changed

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
