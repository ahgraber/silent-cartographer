# Command Surface Specification

## Purpose

Defines the CLI envelope every command shares: a closed, canonical flag vocabulary; a distinct-code exit taxonomy; teaching rejections that name valid alternatives; non-interactive operation; the stdout/stderr and `--json`/human-render output discipline; source-faithful, injection-safe content rendering under a color gate; and the operational and introspection commands (`doctor`, `cache`, `manifest`, `completions`) that ride the same contract.

External design references for this capability are listed under `command-surface` in [`REFERENCES.md`](../../REFERENCES.md).

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

The system SHALL provide a command that emits, on standard output, a completion script for a named supported shell that offers exactly the commands and flags the command line accepts, and SHALL reject an unsupported shell name as a usage error naming the supported shells.

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

### Requirement: Refresh-hook installation

The system SHALL provide a command that installs, for a workspace, repository hooks that refresh that workspace's index when a git operation changes the checked-out commit, so a caller keeps the index tracking the checked-out state without wiring that by hand.
Installation SHALL write the hooks to the paths the worktree resolves its hooks to, SHALL make each written hook executable, and SHALL report each path written.
One set of hooks SHALL refresh every workspace installed into it: installing a second workspace SHALL keep the first workspace refreshed, and installing a workspace that is already installed SHALL leave every hook file unchanged.
When a refresh hook for one event is missing and the remaining refresh hooks agree on the installed workspaces, installation SHALL restore the missing hook for all of those workspaces.
When the present refresh hooks disagree on the installed workspaces, installation SHALL refuse before changing any file and SHALL name the hook files that disagree, unless they differ only by the workspace being installed; then installation SHALL complete, so a retry finishes an installation that failed partway.
Installation SHALL NOT write git configuration.
Installation SHALL refuse, before changing any file, a workspace root, index path, or workspace identity that contains a line break, because the hook file cannot carry it.
Installation outside a git worktree SHALL fail with the environment/setup-failure exit code and state that the directory is not a git worktree.

#### Scenario: Hooks installed where none exist

- **GIVEN** a git worktree with no hook files
- **WHEN** the hook-installation command runs for a workspace
- **THEN** an executable hook is written for each refresh event, each path is reported, and the process exits with the success code

#### Scenario: A second workspace joins the installed hooks

- **GIVEN** a git worktree whose hooks are installed for one workspace
- **WHEN** the hook-installation command runs for a second workspace in the same worktree
- **THEN** a commit refreshes both workspaces' indexes

#### Scenario: Reinstalling a workspace changes nothing

- **GIVEN** a git worktree whose hooks are installed for a workspace
- **WHEN** the hook-installation command runs again for the same workspace
- **THEN** every hook file is byte-for-byte unchanged and the process exits with the success code

#### Scenario: A removed refresh hook is restored for every workspace

- **GIVEN** refresh hooks installed for two workspaces, and one event's refresh hook deleted by another tool
- **WHEN** the hook-installation command runs for one of the workspaces
- **THEN** that event refreshes both workspaces' indexes again

#### Scenario: A retry completes an installation that failed partway

- **GIVEN** refresh hooks installed for one workspace, and an installation of a second workspace that added it to some refresh hooks and then failed
- **WHEN** the hook-installation command runs again for the second workspace
- **THEN** every refresh hook names both workspaces and the process exits with the success code

#### Scenario: Disagreeing refresh hooks are refused

- **GIVEN** refresh hooks for different events that name different sets of workspaces
- **WHEN** the hook-installation command runs
- **THEN** no hook file changes, and the refusal names the hook files that disagree

#### Scenario: Installation leaves git configuration unchanged

- **GIVEN** a git worktree
- **WHEN** the hook-installation command runs
- **THEN** the repository's git configuration is byte-for-byte unchanged

#### Scenario: A workspace path with a line break is refused

- **GIVEN** a workspace whose root path contains a line break
- **WHEN** the hook-installation command runs
- **THEN** no hook file changes, and the refusal states that the path contains a line break

#### Scenario: Installing outside a git worktree is a typed environment failure

- **GIVEN** a directory that is not inside a git worktree
- **WHEN** the hook-installation command runs
- **THEN** the failure states that the directory is not a git worktree and exits through the environment/setup-failure code

#### Scenario: The hooks are installed where this worktree resolves its hooks

- **GIVEN** a git worktree whose hook directory is not a `.git/hooks` directory beside the working tree — a linked worktree, or a repository with a relocated hook path
- **WHEN** the hook-installation command runs
- **THEN** the hooks are written to the paths that worktree actually resolves its hooks to, rather than to an assumed location

### Requirement: Refresh timing

After a git operation that changes the checked-out commit completes — a commit, an amended commit, a completed rebase, a branch checkout, or a merge or pull — the installed hooks SHALL have refreshed each installed workspace's index, so the index matches the checked-out state.
An amended commit SHALL cause exactly one refresh.
A rebase with git's default backend SHALL cause at most one refresh, after the rebase completes, whatever the number of commits it replays.
A rebase with git's apply backend SHALL cause at most two refreshes, whatever the number of commits it replays.
The hooks SHALL NOT refresh the index for a commit that a rebase in progress creates, SHALL NOT refresh it for a checkout that restores files without changing the checked-out commit, and SHALL NOT refresh it for a squash merge, which leaves the checked-out commit unchanged.
A refresh SHALL NOT change the outcome or the exit status of the git operation that triggered it, including when the refresh fails.

#### Scenario: A commit refreshes the index

- **GIVEN** a workspace with installed hooks and an index that matches the checked-out state
- **WHEN** a source change is committed
- **THEN** the index matches the new commit

#### Scenario: An amended commit refreshes the index once

- **GIVEN** a workspace with installed hooks
- **WHEN** the last commit is amended with a source change
- **THEN** the index matches the amended commit and exactly one refresh ran

#### Scenario: A rebase that replays several commits refreshes once

- **GIVEN** a workspace with installed hooks and a branch with several commits to replay onto another branch
- **WHEN** the branch is rebased with the default backend and the rebase completes
- **THEN** exactly one refresh ran and the index matches the rebased branch

#### Scenario: A rebase with nothing to replay refreshes once

- **GIVEN** a workspace with installed hooks and a branch whose commits are all contained in the branch it is rebased onto
- **WHEN** the branch is rebased with the default backend
- **THEN** exactly one refresh ran and the index matches the new checked-out commit

#### Scenario: An apply-backend rebase refreshes at most twice

- **GIVEN** a workspace with installed hooks and a branch with several commits to replay onto another branch
- **WHEN** the branch is rebased with the apply backend, selected by the `--apply` flag or by the `rebase.backend` setting
- **THEN** at most two refreshes ran and the index matches the rebased branch

#### Scenario: A paused rebase does not refresh

- **GIVEN** a workspace with installed hooks and a rebase stopped on a conflict
- **WHEN** the conflict is resolved and committed while the rebase is still in progress
- **THEN** no refresh runs until the rebase completes

#### Scenario: An aborted rebase leaves the index at the original commit

- **GIVEN** a workspace with installed hooks and an index that matches the checked-out state
- **WHEN** a rebase starts, stops on a conflict, and is aborted
- **THEN** no refresh ran and the index still matches the checked-out commit

#### Scenario: A branch checkout refreshes the index

- **GIVEN** a workspace with installed hooks and two branches whose sources differ
- **WHEN** the other branch is checked out
- **THEN** the index matches the newly checked-out branch

#### Scenario: A merge refreshes the index

- **GIVEN** a workspace with installed hooks
- **WHEN** a pull or merge moves the checked-out commit, by fast-forward or by a merge commit
- **THEN** the index matches the resulting commit

#### Scenario: A squash merge does not refresh

- **GIVEN** a workspace with installed hooks
- **WHEN** another branch is merged with `--squash`
- **THEN** no refresh runs until the squashed result is committed

#### Scenario: A file checkout does not refresh

- **GIVEN** a workspace with installed hooks
- **WHEN** a file is restored from another commit without changing the checked-out commit
- **THEN** no refresh runs

#### Scenario: A failed refresh does not fail a branch checkout

- **GIVEN** a workspace with installed hooks and a refresh that fails
- **WHEN** another branch is checked out
- **THEN** the checkout exits with the success code and the refresh failure is reported on standard error

### Requirement: Refresh-hook event action

The hooks command SHALL provide an action that applies one event's refresh rules to one workspace and refreshes its index when the rules call for it; the installed hooks call this action once per installed workspace.
The action SHALL exit with the success code whether or not the refresh runs or fails, SHALL write a failed refresh to standard error naming the event and the workspace, and SHALL write nothing to standard output when the rules call for no refresh.
An event, a workspace root, or trailing git arguments given to any other action of the hooks command, and the overwrite flag given to any action other than installation, SHALL be rejected as a usage error, before any file changes, naming the action the argument belongs to.

#### Scenario: An event that calls for no refresh prints nothing

- **GIVEN** a workspace with an index
- **WHEN** the event action runs for a file checkout
- **THEN** standard output is empty and the process exits with the success code

#### Scenario: An argument given to the wrong action is a usage error

- **GIVEN** a git worktree
- **WHEN** the hook-installation command runs with an event name, or the removal command runs with the overwrite flag
- **THEN** no hook file changes, the process exits with the usage-error code, and the diagnostic names the action the argument belongs to

### Requirement: Existing hooks keep running

When the hook-installation command finds a hook file that it did not write, at an event's path or at that event's legacy path (`<event>.legacy`), it SHALL keep that hook.
A kept hook that git would have run — an executable file, or a symbolic link to one — SHALL continue to run on its event with the arguments and standard input git gives that event, and the git operation SHALL exit with the status it would have had without the refresh hooks.
A kept file that git would not have run SHALL be preserved unchanged and SHALL NOT be run.
A kept symbolic link SHALL be preserved as a link to the same target.
For any order of installing and removing the refresh hooks and a supported hook manager, each hook that existed before either SHALL run on its event while it remains installed, and SHALL be back at the path it was found at once both are removed.
A forced reinstall of the hook manager that deletes the refresh hook is the exception: until the hook-installation command runs again, neither the refresh nor the kept hook runs on that event, the kept hook stays in the hooks directory, and the readiness command reports the event as not installed.
While a supported hook manager and the refresh hooks are both installed, each of the hook manager's hooks SHALL run once per event, and no output SHALL report a failure that did not occur.
A supported hook manager is one whose generated hook file matches a form the system recognizes; prek is supported for the shim forms the system's tests capture.

#### Scenario: A supported hook manager installed first keeps running

- **GIVEN** a git worktree where prek already owns an event's hook file
- **WHEN** the hook-installation command runs and the event occurs
- **THEN** prek's hooks run once, the index is refreshed, and prek's hook file is unchanged

#### Scenario: A supported hook manager installed later keeps both running

- **GIVEN** a git worktree with installed refresh hooks
- **WHEN** prek is installed and the event occurs
- **THEN** prek's hooks run once and the index is refreshed

#### Scenario: A supported hook manager reinstalled runs once

- **GIVEN** a git worktree with prek and the refresh hooks both installed, in either order
- **WHEN** prek is installed again and the event occurs
- **THEN** each of prek's hooks runs once, the index is refreshed, and no output reports a failure

#### Scenario: A pre-existing hook survives the tools installed around it

- **GIVEN** an executable hook H at an event's path before either prek or the refresh hooks are installed
- **WHEN** prek and the refresh hooks are installed in either order, prek is installed again, and both are then removed in either order
- **THEN** H runs on every event while installed, and H is back at the event path, byte-for-byte as it was found, after both are removed

#### Scenario: An unrecognized hook at the event path is chained

- **GIVEN** an event's path holding a hook that is not a recognized hook-manager file, and a separate file at that event's legacy path
- **WHEN** the hook-installation command runs and the event occurs
- **THEN** the hook at the event path runs, the index is refreshed, and the file at the legacy path is unchanged

#### Scenario: A kept hook receives the event's standard input

- **GIVEN** an existing `post-rewrite` hook that records its standard input, kept by the hook-installation command
- **WHEN** a rebase completes
- **THEN** the kept hook records the list of rewritten commits that git supplied

#### Scenario: A kept hook's failure still fails the checkout

- **GIVEN** an existing `post-checkout` hook that exits with a failure status, kept by the hook-installation command
- **WHEN** another branch is checked out
- **THEN** the checkout exits with a failure status and the index is refreshed

#### Scenario: A non-executable kept file is not run

- **GIVEN** a non-executable file at an event's path
- **WHEN** the hook-installation command runs and the event occurs
- **THEN** the file is not run, the index is refreshed, and removing the refresh hooks puts the file back unchanged

#### Scenario: A kept symbolic link keeps its target

- **GIVEN** an event's path holding a relative symbolic link to an executable hook
- **WHEN** the hook-installation command runs and the event occurs
- **THEN** the linked hook runs, and the kept link points to the same target

### Requirement: Kept-hook collision

When a hook the command kept earlier remains in the hooks directory but no refresh hook is installed for its event, the hook-installation command SHALL NOT replace it with a different hook.
It SHALL proceed only when the hook it would now keep is identical to the remaining one — two regular files with the same bytes, or two symbolic links with the same target — and otherwise SHALL refuse before changing any file, naming the remaining hook's path and stating that it must be removed or renamed before installing.

#### Scenario: An identical remaining hook is replaced silently

- **GIVEN** a kept hook that remains without a refresh hook, byte-for-byte identical to the unrecognized hook now at its event's path
- **WHEN** the hook-installation command runs and the event occurs
- **THEN** the hook runs once, the index is refreshed, and the process exits with the success code

#### Scenario: A different remaining hook stops installation

- **GIVEN** a remaining kept hook whose content differs from the hook the command would now keep
- **WHEN** the hook-installation command runs
- **THEN** no file changes, and the refusal names the remaining hook's path

#### Scenario: Symbolic links with different targets are different

- **GIVEN** a remaining kept hook that is a symbolic link, and a hook to keep that is a symbolic link to a different file with the same bytes
- **WHEN** the hook-installation command runs
- **THEN** no file changes, and the refusal names the remaining hook's path

### Requirement: Hook replacement on request

The hook-installation command SHALL accept an overwrite flag.
With the flag, an existing hook file that the command would otherwise keep SHALL be deleted and SHALL NOT run on later events.
Without the flag, no existing hook file SHALL be removed.

#### Scenario: Overwrite replaces an existing hook

- **GIVEN** a git worktree with an existing hook file at an event's path
- **WHEN** the hook-installation command runs with the overwrite flag and the event occurs
- **THEN** the index is refreshed and the replaced hook does not run

### Requirement: Refresh-hook removal

The system SHALL provide a command that removes a workspace from the installed refresh hooks, after which events SHALL NOT refresh that workspace's index.
Other installed workspaces SHALL keep being refreshed.
Removal SHALL remove the workspace from every refresh hook that names it, including when the refresh hooks disagree.
When no workspace remains installed, the command SHALL remove every hook file the installation wrote and SHALL put each hook it kept back at the path the refresh hook occupied, byte-for-byte as it was found.
The command SHALL NOT delete a hook file the installation did not write.
Removing a workspace that is not installed SHALL leave every hook file unchanged and SHALL exit with the success code.
Removal SHALL NOT write git configuration.

#### Scenario: Removing one of two workspaces

- **GIVEN** refresh hooks installed for two workspaces
- **WHEN** the removal command runs for one of them and a commit is made
- **THEN** only the other workspace's index is refreshed

#### Scenario: Removing the last workspace restores a kept hook

- **GIVEN** refresh hooks installed for one workspace over an existing hook file
- **WHEN** the removal command runs for that workspace
- **THEN** no refresh hook file remains and the kept hook is back at its event path, byte-for-byte as it was found

#### Scenario: Removing the last workspace with no kept hook

- **GIVEN** refresh hooks installed for one workspace where no hook existed before
- **WHEN** the removal command runs for that workspace
- **THEN** no refresh hook file remains in the hooks directory

#### Scenario: Removing while a supported hook manager is installed

- **GIVEN** prek and the refresh hooks both installed
- **WHEN** the removal command runs for the last workspace and the event occurs
- **THEN** prek's hooks run once, no refresh runs, and no output reports a failure

#### Scenario: Removing from disagreeing refresh hooks

- **GIVEN** refresh hooks for different events that name different sets of workspaces, one of which is the workspace being removed
- **WHEN** the removal command runs for that workspace
- **THEN** no refresh hook names that workspace afterward

#### Scenario: Removing a workspace that is not installed

- **GIVEN** a git worktree whose refresh hooks do not include a workspace
- **WHEN** the removal command runs for that workspace
- **THEN** every hook file is byte-for-byte unchanged and the process exits with the success code

### Requirement: Refresh-hook status report

The readiness command SHALL report, for each refresh event of the git worktree it runs in, whether a refresh hook will run for that event and which workspaces it refreshes.
A refresh hook SHALL be reported as installed only when git will run it: it is executable and at the event's path, or it is executable and at the event's legacy path under an executable supported hook manager's file.
A refresh hook whose content differs from what installation writes for its workspaces SHALL be reported as modified, SHALL NOT be reported as refreshing its workspaces, because the edit can stop the refresh lines from running, and SHALL state that installation and removal replace the file and that the user's own commands belong in the kept hook.
A refresh hook at the legacy path under any other file SHALL be reported as present with its activation unverified, and SHALL NOT be reported as refreshing its workspaces.
A refresh hook that is absent, including one that another tool removed, SHALL be reported as not installed with the action that fixes it: the installation command, or, when a kept hook remains without a refresh hook, the remaining hook's path and the need to remove or rename it.
Refresh hooks that disagree on the installed workspaces SHALL be reported with each file's workspaces.
Hook status SHALL NOT change the readiness command's exit code, and outside a git worktree the report SHALL state that hook status does not apply.

#### Scenario: Installed hooks are reported

- **GIVEN** a git worktree with refresh hooks installed for a workspace
- **WHEN** the readiness command runs
- **THEN** each refresh event is reported installed with that workspace

#### Scenario: Hooks under a supported hook manager are reported installed

- **GIVEN** refresh hooks installed at the legacy path under prek's hook file
- **WHEN** the readiness command runs
- **THEN** each refresh event is reported installed

#### Scenario: Hooks under an unrecognized file are reported unverified

- **GIVEN** a refresh hook at an event's legacy path, under a file at the event's path that is not a recognized hook-manager file
- **WHEN** the readiness command runs
- **THEN** that event is reported present with activation unverified, and its workspaces are not reported as refreshed

#### Scenario: A removed hook is reported with its fix

- **GIVEN** refresh hooks whose file for one event another tool removed (for example, by a forced hook-manager reinstall)
- **WHEN** the readiness command runs
- **THEN** that event is reported not installed with the installation command, and the exit code is the one the indexer readiness alone determines

#### Scenario: A remaining kept hook is reported with its path

- **GIVEN** a kept hook that remains without a refresh hook, and that differs from the hook now at its event's path
- **WHEN** the readiness command runs
- **THEN** that event is reported not installed, naming the remaining hook's path and the need to remove or rename it

#### Scenario: A non-executable hook is not reported installed

- **GIVEN** refresh hooks installed for a workspace, and one event's refresh hook no longer executable
- **WHEN** the readiness command runs
- **THEN** that event is not reported installed

#### Scenario: A hand-edited hook is reported modified

- **GIVEN** refresh hooks installed for a workspace, and a command added by hand to one event's refresh hook
- **WHEN** the readiness command runs
- **THEN** that event is reported modified, its workspaces are not reported as refreshed, and the kept hook is named as the place for the user's own commands

#### Scenario: Disagreeing hooks are reported

- **GIVEN** refresh hooks for different events that name different sets of workspaces
- **WHEN** the readiness command runs
- **THEN** each event is reported with the workspaces its file names

#### Scenario: Outside a git worktree

- **GIVEN** a directory that is not inside a git worktree
- **WHEN** the readiness command runs
- **THEN** the report states that hook status does not apply and the exit code is the one the indexer readiness alone determines

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

### Requirement: Messages say what happened, why, what it means, and what to do

Every warning, refusal, and error the system reports SHALL state what failed or was skipped, and name the thing it concerns.
It SHALL state the cause when the system knows the cause, and SHALL NOT state a cause the system did not observe.
It SHALL state what the event means for the answer or the store: what is missing, what may be wrong, or what is left unchanged.
It SHALL name the next step when one exists: a command, a flag, or a setting.
A usage error is the one exception: it SHALL name the accepted input, and need not state a cause or what the error means.

#### Scenario: A skipped file is reported with its effect and remedy

- **GIVEN** a workspace that holds a source file whose content is not valid UTF-8
- **WHEN** the workspace is built
- **THEN** a warning names the file, states that it was skipped because it is not valid UTF-8, states that no answer shows the code it holds, and says to re-save the file as UTF-8

#### Scenario: A missing indexer is reported with its install step

- **GIVEN** a Rust workspace on a host where `rust-analyzer` cannot be run
- **WHEN** the workspace is built
- **THEN** the error names `rust-analyzer`, states why it could not be run, states that no index was built, and names the install command and the flag that selects another `rust-analyzer`

#### Scenario: A stale label claims no cause the system did not observe

- **GIVEN** an index recorded with a declared interpreter environment, on a host where that environment cannot be checked
- **WHEN** a query runs
- **THEN** the answer is labeled stale, and the label does not state that the environment changed

#### Scenario: A usage error names the accepted input

- **GIVEN** the refresh-hook action invoked with no event
- **WHEN** the command runs
- **THEN** the error names the accepted events

## Technical Notes

- **Implementation**: `src/cli.rs`, `src/main.rs`, `src/commands.rs`, `src/render.rs`, `src/exit.rs`, `src/manifest.rs`, `src/semantic/probe.rs`
- **Dependencies**: code-navigation (the query commands that inherit this envelope)
