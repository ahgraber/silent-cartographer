# Delta for mcp-surface

## ADDED Requirements

### Requirement: Tools mirror query commands

The server SHALL expose exactly one tool per query command it covers — `get`, `trace`, `find`, `search`, `similar`, and `impact` — named for that command, and SHALL NOT expose a tool that answers a question no single command answers.
Each tool SHALL accept the bounding and projection controls its mirrored command accepts, so a caller governs result size through the tool rather than by post-processing the answer.

Serves: tools-not-shell

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

### Requirement: The server states when to prefer it

The server SHALL provide instructions, to a client of either protocol era, that state when to prefer these tools over textual search, what the graph covers and does not, and how to recover from each index-state failure a tool can report.

Serves: tools-not-shell

#### Scenario: Instructions reach a client of either era

- **GIVEN** a running server and a client of either protocol era
- **WHEN** it connects
- **THEN** it receives the server's instructions

#### Scenario: Instructions state the coverage boundary and the recoveries

- **GIVEN** the server's instructions
- **WHEN** they are read
- **THEN** they name each tool, state what the graph covers and when to use textual search instead, and name the recovery for each index-state failure category

### Requirement: Answers pass through unmodified

For any tool invocation that its mirrored command answers, the tool result SHALL be that command's structured answer with no field added, removed, renamed, or reordered, and any content the answer carries SHALL round-trip byte-exactly.

Serves: faithful-answers

#### Scenario: A result-bearing answer is unaltered

- **GIVEN** a query that resolves to one or more results
- **WHEN** it is invoked as a tool and separately as its mirrored command
- **THEN** the two structured answers are equal

#### Scenario: A typed-empty answer survives as typed absence

- **GIVEN** a subject that stands in no instance of the requested relation
- **WHEN** the relation is traced through its tool
- **THEN** the result is the command's typed-empty answer, distinguishable from an answer that carries results and from an error

#### Scenario: An ambiguous reference returns its candidate set

- **GIVEN** a symbol reference that denotes more than one symbol
- **WHEN** it is retrieved through its tool
- **THEN** the result is the command's typed candidate set rather than an arbitrary single symbol or an error

#### Scenario: Calibration labels reach the caller

- **GIVEN** a query whose answer is heuristic-grade or derives from an out-of-date index
- **WHEN** it is invoked as a tool
- **THEN** the result carries the same provenance, freshness, and classification labels the command's answer carries

#### Scenario: Body content round-trips byte-exactly

- **GIVEN** a symbol whose body embeds a terminal-control escape sequence
- **WHEN** it is retrieved at body detail through its tool
- **THEN** the returned content is byte-identical to the command's structured answer for the same request

### Requirement: Command failures surface as typed tool errors

Every non-success outcome of a mirrored command SHALL surface as a tool error that distinguishes the command's exit category and preserves the command's own diagnostic text, so a caller recovers from the error alone.
A successful outcome SHALL NOT surface as an error, a typed-empty answer included.

Serves: faithful-answers

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

### Requirement: Every invocation resolves to one workspace

Every tool invocation SHALL resolve to exactly one workspace root — the root named on the invocation when one is given, otherwise the server's launch-time default — and its answer SHALL derive from the index store belonging to that root.
The launch-time default SHALL be the root the server was explicitly configured with, or, absent that configuration, the directory the server process itself resolves to.
A configured root that does not resolve to an existing directory SHALL be refused rather than silently replaced by a fallback.

Serves: project-scoped-answers

#### Scenario: A named root overrides the default

- **GIVEN** a server whose launch-time default is one workspace
- **WHEN** a tool is invoked naming a different workspace root
- **THEN** the answer derives from the named root's store, not the default's

#### Scenario: An unnamed invocation uses the configured default

- **GIVEN** a server explicitly configured with a workspace root
- **WHEN** a tool is invoked without naming a root
- **THEN** the answer derives from the configured root's store

#### Scenario: An unconfigured server falls back to its own directory

- **GIVEN** a server started with no explicit workspace configuration
- **WHEN** a tool is invoked without naming a root
- **THEN** the answer derives from the store belonging to the directory the server process resolves to

#### Scenario: An unresolvable root is refused

- **GIVEN** a server configured with a workspace root that is not an existing directory
- **WHEN** the server starts
- **THEN** it refuses to serve, naming the root it could not resolve, rather than falling back to another directory

### Requirement: Concurrent workspaces stay isolated

Concurrent tool invocations that resolve to distinct workspace roots SHALL each derive their answer from their own root's store, and no invocation SHALL observe another's store.

Serves: project-scoped-answers

#### Scenario: Concurrent invocations across projects do not cross

- **GIVEN** two indexed workspaces holding different symbols
- **WHEN** tools resolving to each root are invoked concurrently
- **THEN** each answer contains only symbols from its own workspace

#### Scenario: A store describing another workspace is disclosed

- **GIVEN** a resolved root whose store records a different workspace root than the one resolved
- **WHEN** a tool is invoked against it
- **THEN** the answer carries the command's workspace-relationship disclosure rather than presenting as a matched answer

### Requirement: The server refuses a surface it was not built against

The server SHALL confirm at startup that the installed binary's reported command-surface version equals the version the server was built against, and SHALL refuse to serve on any difference.
The refusal SHALL name both versions and state the version-alignment remediation, and SHALL be distinguishable from every index-state failure, whose remediation is a rebuild.

Serves: surface-truth

#### Scenario: A matching surface version serves

- **GIVEN** an installed binary whose reported surface version equals the version the server was built against
- **WHEN** the server starts
- **THEN** it serves its tools

#### Scenario: A differing surface version refuses

- **GIVEN** an installed binary whose reported surface version differs from the version the server was built against
- **WHEN** the server starts
- **THEN** it refuses to serve, naming both versions and the version-alignment remediation

#### Scenario: An unavailable binary is a distinct startup failure

- **GIVEN** a host where the binary cannot be found or cannot be run
- **WHEN** the server starts
- **THEN** it refuses to serve with a failure naming the binary, distinct from a surface-version difference

### Requirement: The server runs the binary it verified

Every invocation SHALL execute the same binary the startup check verified, whatever workspace the invocation resolves to.

Serves: surface-truth

#### Scenario: The verified binary is run from any workspace

- **GIVEN** a server whose binary was located through a path relative to the directory it started in
- **WHEN** a tool is invoked against a workspace containing a different executable at that same relative path
- **THEN** the answer comes from the binary the startup check verified, not from the one in the workspace

### Requirement: Tool schemas match the installed surface

For every tool the server exposes, each parameter SHALL correspond to an option the installed binary's surface index reports for the mirrored command, each enumerated value the parameter accepts SHALL lie within that option's reported valid set, and each default a tool's description states SHALL equal the reported default.

Serves: surface-truth

#### Scenario: Every parameter names a real option

- **GIVEN** the exposed tools and the installed binary's surface index
- **WHEN** each tool's parameters are compared against the index
- **THEN** every parameter corresponds to an option the index reports for that tool's mirrored command

#### Scenario: Every accepted value is in the reported set

- **GIVEN** a tool parameter that accepts an enumerated set of values
- **WHEN** that set is compared against the index's reported valid values for the corresponding option
- **THEN** no accepted value lies outside the reported set

#### Scenario: Every stated default matches the reported default

- **GIVEN** a tool whose description states a default for one of its parameters
- **WHEN** that stated default is compared against the index's reported default
- **THEN** the two are equal

### Requirement: Lifecycle tools require an explicit acknowledgment

The `build` and `hooks install` tools SHALL NOT perform their operation unless the invocation explicitly sets that tool's acknowledgment parameter.
An unacknowledged invocation SHALL leave every store, file, and repository it would have touched unchanged, and SHALL be refused with a diagnostic stating the operation's cost and how to proceed.

Serves: self-service-setup

#### Scenario: An unacknowledged build changes nothing

- **GIVEN** a workspace with no built index
- **WHEN** the build tool is invoked without its acknowledgment parameter set
- **THEN** no store is created and the refusal states the cost of building and how to proceed

#### Scenario: An acknowledged build proceeds

- **GIVEN** a workspace with no built index
- **WHEN** the build tool is invoked with its acknowledgment parameter set
- **THEN** the index is built and the tool returns the command's answer

#### Scenario: An unacknowledged hook installation writes nothing

- **GIVEN** a git worktree with no commit hook at the target path
- **WHEN** the hook-installation tool is invoked without its acknowledgment parameter set
- **THEN** no file is written and the refusal states what the operation would write and how to proceed

#### Scenario: An acknowledged hook installation proceeds

- **GIVEN** a git worktree with no commit hook at the target path
- **WHEN** the hook-installation tool is invoked with its acknowledgment parameter set
- **THEN** the hook is installed and the tool returns the command's answer

### Requirement: A lifecycle operation proceeds only on confirmed consent

A lifecycle tool SHALL obtain the caller's confirmation before performing its operation, through the mechanism the negotiated protocol era defines, and SHALL leave every store, file, and repository unchanged when that confirmation is declined or cancelled.
When the connected client offers no elicitation capability, the tool SHALL NOT perform its operation and SHALL NOT issue a request the client cannot answer; it SHALL refuse, state that the client cannot be asked to confirm, and name the command that performs the operation.

Serves: self-service-setup, every-client

#### Scenario: A handshake-era client is asked and confirms

- **GIVEN** a handshake-era client offering an elicitation capability
- **WHEN** an acknowledged lifecycle tool is invoked and the caller confirms
- **THEN** the operation is performed and the tool returns the command's answer

#### Scenario: A modern client is asked and confirms

- **GIVEN** a client on the 2026-07-28 revision offering an elicitation capability
- **WHEN** an acknowledged lifecycle tool is invoked and the caller confirms
- **THEN** the operation is performed and the tool returns the command's answer

#### Scenario: A declined confirmation performs nothing

- **GIVEN** a client offering an elicitation capability
- **WHEN** an acknowledged lifecycle tool is invoked and the caller declines
- **THEN** no store, file, or repository is changed and the tool reports the operation as not performed

#### Scenario: A client that cannot be asked is refused with its remedy

- **GIVEN** a client offering no elicitation capability
- **WHEN** an acknowledged lifecycle tool is invoked
- **THEN** no store, file, or repository is changed, no unanswerable request is issued, and the refusal states that the client cannot be asked to confirm and names the command that performs the operation

#### Scenario: The acknowledgment is checked before anything is asked

- **GIVEN** a client offering an elicitation capability
- **WHEN** a lifecycle tool is invoked without its acknowledgment parameter set
- **THEN** the invocation is refused without a confirmation request being issued

### Requirement: One server serves both protocol eras

The server SHALL serve clients negotiating the handshake era and clients negotiating the 2026-07-28 revision from a single running instance, and for any query, the answer a tool returns SHALL NOT differ by the era the connection negotiated.

Serves: every-client

#### Scenario: A handshake-era client is served

- **GIVEN** a running server and a client negotiating the handshake era
- **WHEN** it lists and invokes a query tool
- **THEN** the tool is listed and returns its answer

#### Scenario: A modern client is served

- **GIVEN** a running server and a client negotiating the 2026-07-28 revision
- **WHEN** it lists and invokes a query tool
- **THEN** the tool is listed and returns its answer

#### Scenario: The answer does not vary by era

- **GIVEN** one indexed workspace and one query
- **WHEN** the query is invoked by a client of each era against the same server
- **THEN** both results are equal

### Requirement: Every tool runs to completion within its invocation

Every tool SHALL run to completion within the invocation that called it, and SHALL NOT require the client to have negotiated any extension in order to receive its answer.

Serves: every-client

#### Scenario: A confirmed build answers within the invocation

- **GIVEN** a client that has negotiated no extension beyond the base protocol
- **WHEN** it invokes a confirmed build
- **THEN** the invocation returns the command's answer rather than an error reporting a missing capability

### Requirement: An abandoned invocation leaves nothing running

When an invocation is cancelled or abandoned before its command finishes, the server SHALL stop that command rather than let it run on, and SHALL NOT leave any store, file, or repository being written after the caller has stopped waiting.

Serves: self-service-setup

#### Scenario: A cancelled build stops writing

- **GIVEN** an invocation whose command has begun rewriting the index store
- **WHEN** the invocation is cancelled before the command finishes
- **THEN** the command is stopped, and the store it was writing does not appear afterwards
