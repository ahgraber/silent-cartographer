# Command Surface Specification

## Purpose

Defines the CLI envelope every command shares: a closed, canonical flag vocabulary; a distinct-code exit taxonomy; teaching rejections that name valid alternatives; non-interactive operation; the stdout/stderr and `--json`/human-render output discipline; source-faithful, injection-safe content rendering under a color gate; and the operational and introspection commands (`doctor`, `cache`, `manifest`, `completions`) that ride the same contract.

## Requirements

### Requirement: Closed flag vocabulary

The system SHALL expose a fixed, closed set of canonical option flags shared across its commands, and SHALL NOT accept an alternate spelling of a canonical flag, so that the option surface an agent learns is stable and uniform.

#### Scenario: Canonical flag accepted

- **GIVEN** a command that offers structured output
- **WHEN** it is invoked with the canonical `--json` flag
- **THEN** the flag is accepted and structured output is produced

#### Scenario: Alias spelling rejected

- **GIVEN** a command that offers structured output
- **WHEN** it is invoked with a non-canonical spelling of that option, such as `--format=json`
- **THEN** the invocation is rejected as a usage error rather than silently honored

### Requirement: Exit-code taxonomy

The system SHALL signal each invocation's outcome through a closed exit-code taxonomy, using a distinct code per category, that distinguishes success — including a typed-empty result — from a usage error, an absent index, a store unusable because its recorded schema version differs, a target refused because the system cannot confirm it is a store the system created, and an indexer or setup failure.

#### Scenario: Success on a non-empty answer

- **GIVEN** an indexed workspace
- **WHEN** a query returns one or more results
- **THEN** the process exits with the success code

#### Scenario: A typed-empty answer is still success

- **GIVEN** an indexed workspace
- **WHEN** a query resolves correctly but stands in no instance of the requested relation
- **THEN** the process exits with the success code, not a failure code

#### Scenario: Usage error is distinct

- **GIVEN** any command
- **WHEN** it is invoked with an invalid flag or malformed argument
- **THEN** the process exits with the usage-error code, distinct from the success and failure codes

#### Scenario: Absent index is distinct

- **GIVEN** a directory with no built index
- **WHEN** a query command is invoked
- **THEN** the process exits with the no-index code, distinct from a usage error

#### Scenario: Incompatible store is distinct

- **GIVEN** a store the system created under a different schema version than the binary expects
- **WHEN** a query command runs against it
- **THEN** the process exits with the incompatible-store code, distinct from the no-index code

#### Scenario: Ownership refusal is distinct

- **GIVEN** a file at the index path that the system does not recognize as its own store
- **WHEN** a build, query, or reset command runs against it
- **THEN** the process exits with the unrecognized-store code, distinct from the absent-index and incompatible-store codes

#### Scenario: Indexer setup failure is distinct

- **GIVEN** a workspace whose required language indexer is not installed
- **WHEN** a command that needs it runs
- **THEN** the process exits with the indexer/setup-failure code, distinct from the other categories

### Requirement: Rejections name valid alternatives

The system SHALL validate an invocation before performing any side effect, and SHALL, when rejecting an input, emit a diagnostic that names the valid alternatives for that input — the accepted values of an enumerated option, or a corrected invocation form — so that the caller can self-correct from the message alone.

#### Scenario: Unknown enum value enumerates the valid set

- **GIVEN** a command with an enumerated option
- **WHEN** it is invoked with a value outside that enumeration
- **THEN** the diagnostic lists the accepted values for that option

#### Scenario: Validation precedes side effects

- **GIVEN** a command that would mutate stored state
- **WHEN** it is invoked with an invalid argument
- **THEN** no state is changed and the invocation is rejected as a usage error

### Requirement: Non-interactive operation

The system SHALL complete every command without interactive prompting, taking all input from arguments and flags, and SHALL treat a non-terminal environment as headless.

#### Scenario: Runs to completion without a terminal

- **GIVEN** a command invoked with standard input and output not attached to a terminal
- **WHEN** it runs
- **THEN** it completes without waiting for interactive input

### Requirement: Output stream discipline

The system SHALL write the machine-readable answer to standard output and every diagnostic to standard error; under `--json` the standard-output answer SHALL be the structured machine answer, and the default human rendering SHALL be a faithful projection of that same answer, presenting the same results in the same order.

#### Scenario: Answer and diagnostics are separated

- **GIVEN** a command that emits both an answer and a diagnostic
- **WHEN** it runs
- **THEN** the answer appears on standard output and the diagnostic appears on standard error

#### Scenario: Human render matches the JSON answer

- **GIVEN** a query with several results
- **WHEN** it is run once with `--json` and once with the default human rendering
- **THEN** both present the same results in the same order

### Requirement: Source-faithful content rendering

The system SHALL render a content-bearing detail — a signature, interface, or body — as its source text as it reads in the file, with terminal-control and bidirectional-control characters made visible as replacement characters, never executed, and SHALL emit color or styling only to a terminal by default — never to a redirected standard output unless the invocation explicitly forces styling — and never under `--json` regardless of any styling option.

#### Scenario: Body renders as source

- **GIVEN** a symbol whose body spans several lines
- **WHEN** it is retrieved at body detail and rendered for a human
- **THEN** the body is shown as multi-line source text, not as an escaped single-line scalar

#### Scenario: Embedded terminal controls are made visible, not executed

- **GIVEN** a symbol whose body embeds a terminal-control escape sequence
- **WHEN** it is retrieved at body detail and rendered for a human
- **THEN** the control characters appear as replacement characters while the surrounding source text and its own line endings are preserved, and the same body requested with `--json` round-trips byte-exactly

#### Scenario: No styling to a redirected stream

- **GIVEN** a content-bearing answer requested without an explicit styling override
- **WHEN** standard output is redirected to a file or pipe
- **THEN** the emitted text carries no color or styling control sequences

#### Scenario: Forced styling is honored on a redirected stream

- **GIVEN** a content-bearing answer requested with styling explicitly forced (`--color=always`)
- **WHEN** standard output is redirected to a file or pipe
- **THEN** styling appears on structural lines only, and the source text itself carries no styling control sequences

#### Scenario: No styling under JSON

- **GIVEN** a content-bearing answer
- **WHEN** it is requested with `--json`
- **THEN** the JSON content carries no color or styling control sequences

### Requirement: Indexer readiness report

The system SHALL provide a command that reports, for each required language indexer, whether it is present and its version, and an install hint when it is absent, and SHALL signal an indexer/setup failure through the exit-code taxonomy when a required indexer is missing.
Each indexer's readiness probe SHALL complete within a bounded time, and an indexer that is present but does not respond within that bound SHALL be reported as unresponsive — distinct from absent — with an investigation hint, and SHALL likewise signal the indexer/setup failure through the exit-code taxonomy.

#### Scenario: All indexers present

- **GIVEN** a host with every required language indexer installed
- **WHEN** the readiness command runs
- **THEN** each indexer is reported present with its version and the process exits with the success code

#### Scenario: A missing indexer is reported with a hint

- **GIVEN** a host missing a required language indexer
- **WHEN** the readiness command runs
- **THEN** that indexer is reported absent with an install hint and the process exits with the indexer/setup-failure code

#### Scenario: An unresponsive indexer is reported within a bounded time

- **GIVEN** an indexer executable that hangs when probed
- **WHEN** the readiness command runs
- **THEN** it reports the indexer unresponsive with an investigation hint within a bounded time and exits with the indexer/setup-failure code

### Requirement: Index reset

The system SHALL provide a command that removes the stored index for the workspace and reports the path affected; removal SHALL apply only to an index store the system recognizes as its own creation, and any other target — including a database another application created and versioned — SHALL be refused as a failure naming the manual alternative, leaving the target intact.
Removing an index that is already absent SHALL be a success, and an operating-system error encountered while removing it SHALL be reported as a failure naming the affected path.
The removal SHALL be reachable only through an explicitly named action of that command; an invocation naming the command without an action SHALL be rejected as a usage error that names the valid actions, and SHALL leave any store at the path intact.
When the store path is itself a symbolic link, the removal SHALL remove the link, SHALL leave the store the link names in place, and SHALL disclose that the link rather than the store was removed, so that success is never reported for an index that still exists.

#### Scenario: Existing index removed

- **GIVEN** a workspace with a stored index
- **WHEN** the removal action runs
- **THEN** the index is removed, the affected path is reported, and the process exits with the success code

#### Scenario: A non-index target is refused

- **GIVEN** a file at the index path that is not a recognizable index store
- **WHEN** the removal action runs
- **THEN** the file is left intact and the refusal names the manual removal alternative, exiting with a failure code

#### Scenario: A foreign versioned database is refused

- **GIVEN** a database file at the index path that another application created and stamped with its own version
- **WHEN** the removal action runs
- **THEN** the file is left intact and the refusal names the manual removal alternative, exiting with a failure code

#### Scenario: Nothing to remove is success

- **GIVEN** a workspace with no stored index
- **WHEN** the removal action runs
- **THEN** the process exits with the success code rather than a failure

#### Scenario: Removal error names the path

- **GIVEN** a stored index that cannot be removed because of an operating-system error
- **WHEN** the removal action runs
- **THEN** the failure is reported naming the affected path and the process exits with a failure code

#### Scenario: A symbolic link at the store path is removed and disclosed

- **GIVEN** a store path that is a symbolic link naming a recognizable index store elsewhere
- **WHEN** the removal action runs
- **THEN** the link is removed, the store it names is left in place, and the answer discloses that the link rather than the store was removed

#### Scenario: Naming no action removes nothing

- **GIVEN** a workspace with a stored index
- **WHEN** the index-store command runs with no action named
- **THEN** the invocation is rejected with the usage code, the rejection names the valid actions, and the index is still present

### Requirement: Index store location report

The system SHALL provide an action that reports the directory containing the index store at the discovered store path, and SHALL report it whether or not a store exists at that path, exiting with the success code in both cases.
The reported directory SHALL be derived from the store path as given, without resolving it against the workspace root, and a store path naming no parent directory SHALL report the working directory rather than an empty answer.
The machine-readable answer SHALL carry the store path itself alongside the directory.

#### Scenario: Directory reported for an existing store

- **GIVEN** a workspace with a stored index
- **WHEN** the location action runs
- **THEN** the directory containing the store is reported and the process exits with the success code

#### Scenario: Directory reported when no store exists

- **GIVEN** a workspace with no stored index
- **WHEN** the location action runs
- **THEN** the directory the store would occupy is reported and the process exits with the success code

#### Scenario: A store path without a parent reports the working directory

- **GIVEN** a store path that is a bare filename
- **WHEN** the location action runs
- **THEN** the reported directory is the working directory rather than an empty answer

#### Scenario: Machine answer carries the store path

- **GIVEN** any store path
- **WHEN** the location action runs with the machine-readable output requested
- **THEN** the answer carries both the directory and the store path

### Requirement: Index store size report

The system SHALL provide an action that reports the on-disk size of the index store as the total bytes of every file the removal action would delete, whenever the store path holds a store the system recognizes as its own, including a store recorded under a schema version the binary cannot read.
For any other target — an absent store, a file the system does not recognize as its own, or a path whose contents cannot be examined — the action SHALL NOT report a size, and SHALL exit with the success code rather than a failure.
When a store the system recognizes as its own cannot be measured, the action SHALL report its state without a size rather than report a size that understates what is at the path, and SHALL exit with the success code.
The machine-readable answer SHALL name the target's state in every case, using the same distinctions the structural surface index reports.

#### Scenario: Size of a recognized store

- **GIVEN** a workspace with a stored index the system recognizes as its own
- **WHEN** the size action runs
- **THEN** the store's byte count is reported and the process exits with the success code

#### Scenario: Auxiliary files count toward the total

- **GIVEN** a recognized store accompanied by the auxiliary files the removal action deletes alongside it
- **WHEN** the size action runs
- **THEN** the reported total covers the store and those auxiliary files together

#### Scenario: An incompatible store reports a size

- **GIVEN** a store the system created, recorded under a schema version the binary does not read
- **WHEN** the size action runs
- **THEN** its byte count is reported, the answer names the incompatible state, and the process exits with the success code

#### Scenario: A recognized store that cannot be measured reports no size

- **GIVEN** a store the system recognizes as its own whose size cannot be established, such as one accompanied by an auxiliary file that cannot be examined
- **WHEN** the size action runs
- **THEN** the answer names the recognized state, reports no byte count, and the process exits with the success code

#### Scenario: An absent store reports no size

- **GIVEN** a workspace with no stored index
- **WHEN** the size action runs
- **THEN** no byte count is reported, the answer names the absent state, and the process exits with the success code

#### Scenario: An unrecognized file reports no size

- **GIVEN** a file at the store path that the system does not recognize as its own store
- **WHEN** the size action runs
- **THEN** no byte count is reported, the answer names the unrecognized state, and the process exits with the success code

#### Scenario: An unexaminable path reports no size

- **GIVEN** a path at the store path whose contents cannot be read at all, such as a directory
- **WHEN** the size action runs
- **THEN** no byte count is reported, the answer names that state distinctly from both absent and unrecognized, and the process exits with the success code

### Requirement: Versioned structural surface index

The system SHALL provide a command that emits, as its machine-readable answer, a structural index of the CLI surface — a surface version, the commands with each command's valid argument and flag values and defaults, and the current index state — and the surface version SHALL change whenever the command or flag structure changes, so that a caller can detect that its understanding of the surface is out of date.
The reported index state SHALL distinguish an absent index, a file at the index path that is not recognized as the system's own store, a path at the index path whose contents cannot be examined, and a store the system recognizes as its own whose recorded schema version differs from the version the binary expects, without failing the command.

#### Scenario: Index enumerates the command surface

- **GIVEN** an installed build
- **WHEN** the surface-index command runs
- **THEN** its answer lists the commands with each command's valid argument and flag values and defaults

#### Scenario: Index carries a surface version

- **GIVEN** an installed build
- **WHEN** the surface-index command runs
- **THEN** its answer carries a surface version identifier

#### Scenario: Surface version distinguishes revisions

- **GIVEN** two builds whose command-or-flag structure differs
- **WHEN** the surface-index command runs against each
- **THEN** the two answers carry different surface versions

#### Scenario: Index state distinguishes unrecognized from absent

- **GIVEN** a file at the index path that the system does not recognize as its own store
- **WHEN** the surface-index command runs
- **THEN** the answer's index state reports the unrecognized condition, distinct from an absent index, and the command succeeds

#### Scenario: Index state distinguishes an unexaminable path

- **GIVEN** a path at the index path whose contents cannot be read at all, such as a directory
- **WHEN** the surface-index command runs
- **THEN** the answer's index state reports that condition, distinct from both an absent index and an unrecognized file, and the command succeeds

#### Scenario: Index state distinguishes an incompatible store

- **GIVEN** a store the system created at the index path, recorded under a different schema version than the binary expects
- **WHEN** the surface-index command runs
- **THEN** the answer's index state reports the incompatible condition, distinct from an absent index, and the command succeeds

### Requirement: Shell completion scripts

The system SHALL provide a command that emits, on standard output, a completion script for a named supported shell, derived from the same command definition the parser executes, so that the completed surface cannot drift from the real one, and SHALL reject an unsupported shell name as a usage error naming the supported shells.

#### Scenario: A supported shell emits a completion script

- **GIVEN** an installed build
- **WHEN** the completion-script command runs with a supported shell name (e.g. `zsh`)
- **THEN** a completion script for the current command surface lands on standard output

#### Scenario: An unknown shell is a usage error

- **GIVEN** an installed build
- **WHEN** the completion-script command runs with a shell name outside the supported set
- **THEN** the invocation is rejected as a usage error whose diagnostic lists the accepted shells

#### Scenario: The emitted script names the current commands

- **GIVEN** an installed build
- **WHEN** the completion-script command runs with a supported shell name
- **THEN** the emitted script names the current top-level commands

#### Scenario: An explicit `--json` is rejected

- **GIVEN** the completion-script command
- **WHEN** it is invoked with `--json` explicitly
- **THEN** the invocation is rejected as a usage error, since a completion script is not a machine answer

#### Scenario: Completion precedes any build

- **GIVEN** a directory with no built index
- **WHEN** the completion-script command runs with a supported shell name
- **THEN** the script is emitted and the process exits with the success code

### Requirement: Commit-hook installation

The system SHALL provide a command that installs a repository commit hook refreshing the index after each commit, so a caller keeps the index tracking the committed state without wiring that by hand.
Installation SHALL report the path written, SHALL make the written hook executable, and SHALL refuse rather than overwrite when a hook is already present at that path, naming what it found and leaving it intact.
Installation outside a git worktree SHALL be reported through the same typed environment failure the diff-seeded assessment raises for that condition.

The hook is the freshness discipline the diff-seeded assessment depends on: its pre-change side is a committed state, so an index refreshed at each commit is the state that assessment resolves against.

#### Scenario: Hook installed where none exists

- **GIVEN** a git worktree with no commit hook at the target path
- **WHEN** the hook-installation command runs
- **THEN** an executable hook that refreshes the index is written, its path is reported, and the process exits with the success code

#### Scenario: An existing hook is never overwritten

- **GIVEN** a git worktree that already has a hook at the target path
- **WHEN** the hook-installation command runs
- **THEN** the existing hook is left byte-for-byte intact, and the refusal names the path it found, exiting with a failure code

#### Scenario: Installing outside a git worktree is a typed environment failure

- **GIVEN** a directory that is not inside a git worktree
- **WHEN** the hook-installation command runs
- **THEN** the failure states that the directory is not a git worktree and exits through the environment/setup-failure code

#### Scenario: The hook is installed where this worktree resolves its hooks

- **GIVEN** a git worktree whose hook directory is not a `.git/hooks` directory beside the working tree — a linked worktree, or a repository with a relocated hook path
- **WHEN** the hook-installation command runs
- **THEN** the hook is written to the path that worktree actually resolves its hooks to, rather than to an assumed location

### Requirement: Chunk parameters on build

The `build` command SHALL accept the two chunk parameters — the chunk size and the overlap between adjacent chunks — as canonical flags of the closed option vocabulary, and SHALL apply a recommended default to each when it is not supplied, so that an operator can vary the retrieval tradeoff without rebuilding the tool.

The chunk size SHALL bound the whole text a chunk is embedded from, its passage header included, so that the size an operator supplies is the size the embedding model receives.

A supplied value SHALL be validated at the boundary: a chunk size that cannot admit content beside a passage's header, or an overlap not smaller than the chunk size, SHALL be rejected as a usage error rather than silently clamped.

#### Scenario: Defaults apply when unsupplied

- **GIVEN** a workspace to index
- **WHEN** `build` is invoked with neither parameter
- **THEN** the build completes and the store records the recommended defaults as the parameters in effect

#### Scenario: Supplied values take effect

- **GIVEN** a workspace to index
- **WHEN** `build` is invoked with an explicit chunk size and overlap
- **THEN** the build completes and the store records the supplied values as the parameters in effect

#### Scenario: No chunk exceeds the supplied size

- **GIVEN** a workspace holding a symbol whose content far exceeds the chunk size
- **WHEN** `build` is invoked with an explicit chunk size
- **THEN** every chunk the build embeds is within that size, header included

#### Scenario: An overlap that is not smaller than the chunk size is refused

- **GIVEN** a workspace to index
- **WHEN** `build` is invoked with an overlap equal to or greater than the chunk size
- **THEN** the invocation is rejected as a usage error and no build is performed

#### Scenario: A chunk size leaving no room for content is refused

- **GIVEN** a workspace to index
- **WHEN** `build` is invoked with a chunk size too small to admit content beside a passage's header
- **THEN** the invocation is rejected as a usage error and no build is performed

## Technical Notes

- **Implementation**: `src/cli.rs`, `src/main.rs`, `src/commands.rs`, `src/render.rs`, `src/exit.rs`, `src/manifest.rs`, `src/semantic/probe.rs`
- **Dependencies**: code-navigation (the query commands that inherit this envelope)
