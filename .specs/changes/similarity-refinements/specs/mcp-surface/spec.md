# Delta for mcp-surface

## MODIFIED Requirements

### Requirement: Tools mirror query commands

> Previously: the covered query commands were `get`, `trace`, `find`, `search`, `similar`, and `impact`.

The server SHALL expose exactly one tool per query command it covers — `get`, `trace`, `find`, `search`, `similar`, `clones`, and `impact` — named for that command, and SHALL NOT expose a tool that answers a question no single command answers.
Each tool SHALL accept the bounding and projection controls its mirrored command accepts, so a caller governs result size through the tool rather than by post-processing the answer.

Serves: duplication-audit-in-one-run, shared-fragments, similar-rows-name-their-relation

#### Scenario: Tool list enumerates the mirrored commands

- **GIVEN** a running server
- **WHEN** a client lists the available tools
- **THEN** the list names one tool per covered query command and nothing else beyond the lifecycle tools

#### Scenario: An uncovered command has no tool

- **GIVEN** a running server
- **WHEN** a client lists the available tools
- **THEN** no tool corresponds to a command that removes the stored index

#### Scenario: Bounding controls reach the command

- **GIVEN** a workspace whose index holds more matches than a requested cap
- **WHEN** a tool is invoked with that cap
- **THEN** the answer carries no more results than the cap and discloses that the result set was truncated

#### Scenario: The clones tool is listed

- **GIVEN** a running server
- **WHEN** a client lists the available tools
- **THEN** the list names a `clones` tool

#### Scenario: Many subjects reach similar in one tool call

- **GIVEN** a workspace with three indexed symbols
- **WHEN** the `similar` tool is invoked once with the three references as its subject list
- **THEN** the answer holds three per-subject answers in list order

#### Scenario: Exclusions reach similar

- **GIVEN** a subject whose `similar` answer holds a row carrying `tests_subject`
- **WHEN** the `similar` tool is invoked for the subject excluding `tests_subject`
- **THEN** the answer holds no row carrying `tests_subject`
