# Delta for Command Surface Specification

## ADDED Requirements

### Requirement: Messages say what happened, why, what it means, and what to do

Every warning, refusal, and error the system reports SHALL state what failed or was skipped, and name the thing it concerns.
It SHALL state the cause when the system knows the cause, and SHALL NOT state a cause the system did not observe.
It SHALL state what the event means for the answer or the store: what is missing, what may be wrong, or what is left unchanged.
It SHALL name the next step when one exists: a command, a flag, or a setting.
A usage error is the one exception: it SHALL name the accepted input, and need not state a cause or what the error means.

Serves: actionable-messages

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

## MODIFIED Requirements

### Requirement: Shell completion scripts

> Previously: the script was described as derived from the parser's command definition; the behavior is unchanged.

The system SHALL provide a command that emits, on standard output, a completion script for a named supported shell that offers exactly the commands and flags the command line accepts, and SHALL reject an unsupported shell name as a usage error naming the supported shells.

Serves: complete-at-the-shell

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

> Previously: the refusal outside a git worktree was stated as the same failure another command raises, and a paragraph explained why the hooks exist; the behavior is unchanged.

The system SHALL provide a command that installs, for a workspace, repository hooks that refresh that workspace's index when a git operation changes the checked-out commit, so a caller keeps the index tracking the checked-out state without wiring that by hand.
Installation SHALL write the hooks to the paths the worktree resolves its hooks to, SHALL make each written hook executable, and SHALL report each path written.
One set of hooks SHALL refresh every workspace installed into it: installing a second workspace SHALL keep the first workspace refreshed, and installing a workspace that is already installed SHALL leave every hook file unchanged.
When a refresh hook for one event is missing and the remaining refresh hooks agree on the installed workspaces, installation SHALL restore the missing hook for all of those workspaces.
When the present refresh hooks disagree on the installed workspaces, installation SHALL refuse before changing any file and SHALL name the hook files that disagree, unless they differ only by the workspace being installed; then installation SHALL complete, so a retry finishes an installation that failed partway.
Installation SHALL NOT write git configuration.
Installation SHALL refuse, before changing any file, a workspace root, index path, or workspace identity that contains a line break, because the hook file cannot carry it.
Installation outside a git worktree SHALL fail with the environment/setup-failure exit code and state that the directory is not a git worktree.

Serves: index-follows-head, hooks-coexist, missing-refresh-is-visible

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

> Previously: only a scenario stated that an amended commit causes exactly one refresh.

After a git operation that changes the checked-out commit completes — a commit, an amended commit, a completed rebase, a branch checkout, or a merge or pull — the installed hooks SHALL have refreshed each installed workspace's index, so the index matches the checked-out state.
An amended commit SHALL cause exactly one refresh.
A rebase with git's default backend SHALL cause at most one refresh, after the rebase completes, whatever the number of commits it replays.
A rebase with git's apply backend SHALL cause at most two refreshes, whatever the number of commits it replays.
The hooks SHALL NOT refresh the index for a commit that a rebase in progress creates, SHALL NOT refresh it for a checkout that restores files without changing the checked-out commit, and SHALL NOT refresh it for a squash merge, which leaves the checked-out commit unchanged.
A refresh SHALL NOT change the outcome or the exit status of the git operation that triggered it, including when the refresh fails.

Serves: rebase-builds-once, index-follows-head

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
