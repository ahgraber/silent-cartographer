# Delta for MCP Surface

## MODIFIED Requirements

### Requirement: Command failures surface as typed tool errors

Every non-success outcome of a mirrored command SHALL surface as a tool error that distinguishes the command's exit category and preserves the command's own diagnostic text, so a caller recovers from the error alone.
A successful outcome SHALL NOT surface as an error, a typed-empty answer included.
An answer the index's tier cannot give, one whose outcome is `unavailable` or an `impact` answer whose dependents are unavailable, SHALL surface as a tool result carrying the command's answer unmodified, not as an error, so the caller receives the answer's reason, its seeds, and its unresolved-call group.

> Previously: every non-success exit code surfaced as an error, and no answer exited with a non-success code.

Serves: calibrated-capability-disclosure

#### Scenario: An absent index is a distinct error naming its recovery

- **GIVEN** a workspace with no built index
- **WHEN** a query tool is invoked against it
- **THEN** the error identifies the absent-index category and carries the diagnostic naming the rebuild

#### Scenario: An incompatible store is distinct from an absent index

- **GIVEN** a store recorded under a different schema version than the installed binary expects
- **WHEN** a query tool is invoked against it
- **THEN** the error identifies the incompatible-store category, distinct from the absent-index category

#### Scenario: An unrecognized store is distinct

- **GIVEN** a file at the index path that the binary does not recognize as its own store
- **WHEN** a query tool is invoked against it
- **THEN** the error identifies the unrecognized-store category, distinct from the absent-index and incompatible-store categories

#### Scenario: A rejected invocation is a usage error

- **GIVEN** a tool invoked with a value outside an enumerated parameter's accepted set
- **WHEN** the invocation is made
- **THEN** the error identifies the usage category and names the accepted values

#### Scenario: A missing language indexer is distinct

- **GIVEN** a workspace whose required language indexer is not installed
- **WHEN** a tool that needs it is invoked
- **THEN** the error identifies the indexer-or-setup category, distinct from every index-state category

#### Scenario: A typed-empty answer is not an error

- **GIVEN** a query that resolves correctly and stands in no instance of the requested relation
- **WHEN** it is invoked as a tool
- **THEN** the tool returns a result rather than raising an error

#### Scenario: An unavailable answer is a result

- **GIVEN** a structural index holding a function that other commands call
- **WHEN** the `trace` tool is invoked for it over the `references` relation
- **THEN** the tool returns the command's `unavailable` answer as a result, with its unresolved-call group, rather than raising an error
