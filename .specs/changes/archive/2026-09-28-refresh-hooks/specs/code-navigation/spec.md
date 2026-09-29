# Delta for code-navigation

## ADDED Requirements

### Requirement: A stale answer names its refresh command

Every answer marked stale SHALL carry, in the machine answer and in the human rendering alike, a runnable command that refreshes the index the answer came from, naming that index's location, workspace identity, and workspace root.
Running the command SHALL make the same query's answer fresh when the sources, analyzer, and environment are unchanged in the meantime.
An answer marked fresh SHALL NOT carry a refresh command.
When the index does not record its workspace root, the stale answer SHALL say that the root is unknown instead of naming a root.
The index status report SHALL carry the same refresh command when it reports the index stale, and SHALL NOT carry one when it reports the index fresh.

Serves: missing-refresh-is-visible

#### Scenario: Stale after changed sources

- **GIVEN** an index built before a source file changed, as after `git reset --hard` to another commit
- **WHEN** a query is answered from it
- **THEN** the answer is marked stale and carries a refresh command that names the index location, workspace identity, and workspace root

#### Scenario: Stale after a changed analyzer

- **GIVEN** an index recorded under one analyzer version while another version is in effect
- **WHEN** a query is answered from it
- **THEN** the answer is marked stale and carries a refresh command

#### Scenario: Stale after a changed environment

- **GIVEN** an index recorded under one interpreter environment while a different environment is in effect for the query
- **WHEN** a query is answered from it and the refresh command it carries is run
- **THEN** the same query, repeated under the same environment, is answered fresh

#### Scenario: Running the refresh command makes the answer fresh

- **GIVEN** a stale answer and its refresh command
- **WHEN** the command is run and the same query is repeated
- **THEN** the repeated answer is marked fresh

#### Scenario: A fresh answer carries no refresh command

- **GIVEN** an index whose sources and analyzer are unchanged since indexing
- **WHEN** a query is answered from it
- **THEN** the answer carries no refresh command

#### Scenario: The status report names the refresh command

- **GIVEN** an index built before a source file changed
- **WHEN** the index status is reported, in the machine answer and in the human rendering
- **THEN** both report the index stale and carry the refresh command, and running the command makes the status report fresh

#### Scenario: The human rendering names the refresh command

- **GIVEN** a stale answer
- **WHEN** it is rendered for a human
- **THEN** the rendering includes the refresh command

### Requirement: Freshness does not depend on how the workspace root is named

An index SHALL answer fresh to a query in its workspace when the sources, analyzer, and interpreter environment are unchanged since the index was built, whether the build named the workspace root by an absolute path, by a relative path, or as the invocation directory.
The recorded environment and the environment in effect for a query SHALL be compared as the same directory, not as the same spelling of a path.

Serves: index-follows-head, missing-refresh-is-visible

#### Scenario: An index built by the refresh hooks answers fresh

- **GIVEN** a Python workspace whose environment is found through a `.venv` directory, with no activated environment, and an index built with the workspace root given as an absolute path, as the refresh hooks build it
- **WHEN** a query runs from the workspace directory
- **THEN** the answer is marked fresh

#### Scenario: An index built from the workspace directory answers fresh

- **GIVEN** the same workspace, with an index built with the workspace root given as `.`
- **WHEN** a query runs from the workspace directory
- **THEN** the answer is marked fresh
