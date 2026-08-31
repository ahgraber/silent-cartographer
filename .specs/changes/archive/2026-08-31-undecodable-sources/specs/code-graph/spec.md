# Delta for Code Graph

## ADDED Requirements

### Requirement: Source discovery excludes undecodable files

Source discovery SHALL exclude every discovered file whose bytes are not valid UTF-8 or whose path is not valid Unicode, SHALL report each exclusion in a diagnostic identifying that file, and SHALL complete over the files that remain.
An excluded file SHALL contribute nothing to the syntax layer, and a semantic occurrence the analyzer reports in it SHALL be recorded under the semantic-only refusal outcome rather than aligned or dropped.
A symbol every one of whose occurrences lands in an excluded file SHALL be reported absent by every query, never as a found result carrying no content.
The discovered set SHALL be a function of the workspace alone and not of the operation asking for it, so a workspace holding an undecodable file presents the same set to a build as to a currency check.
A discovered file that cannot be read for any reason other than its encoding SHALL fail the invocation.

Serves: workspace-survives-undecodable-file, exclusion-is-disclosed

#### Scenario: An undecodable file does not stop the build

- **GIVEN** a workspace holding one source file whose bytes are not valid UTF-8 alongside decodable sources
- **WHEN** the index is built
- **THEN** the build completes and the index holds no symbol from the undecodable file

#### Scenario: Symbols from the other files are unaffected

- **GIVEN** the same workspace
- **WHEN** a symbol defined in a decodable file is requested
- **THEN** it is returned exactly as it would be from a workspace holding no undecodable file

#### Scenario: The exclusion names the file

- **GIVEN** the same workspace
- **WHEN** the index is built
- **THEN** a diagnostic names the excluded file, and the invocation still reports success

#### Scenario: Semantic coverage of an excluded file is refused, not aligned

- **GIVEN** a workspace whose analyzer reports occurrences in a file source discovery excluded
- **WHEN** the index is built
- **THEN** each of those occurrences is counted under the semantic-only refusal outcome, and the alignment accounting still sums to the total occurrences processed

#### Scenario: A symbol found only in an excluded file is reported absent, not empty

- **GIVEN** a workspace whose analyzer reports a symbol whose every occurrence is in a file source discovery excluded
- **WHEN** that symbol is requested by name
- **THEN** it is reported absent, not returned as a found result with no content

#### Scenario: The currency check sees the same discovered set

- **GIVEN** an index built over a workspace holding an undecodable file, with nothing since changed
- **WHEN** a build runs again
- **THEN** it reports the index already current and does no work

#### Scenario: An unreadable file still fails the invocation

- **GIVEN** a workspace holding a discovered source file that cannot be read for a reason other than its encoding
- **WHEN** the index is built
- **THEN** the build fails and names the file, rather than excluding it

#### Scenario: A file whose path is not valid Unicode is excluded, not fatal

- **GIVEN** a workspace holding a discovered source file whose path is not valid Unicode, alongside decodable sources
- **WHEN** the index is built
- **THEN** the build completes, the non-Unicode-path file is excluded, and its decodable siblings are indexed
