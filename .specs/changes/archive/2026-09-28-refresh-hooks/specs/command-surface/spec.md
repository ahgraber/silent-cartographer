# Delta for command-surface

## ADDED Requirements

### Requirement: Refresh-hook installation

The system SHALL provide a command that installs, for a workspace, repository hooks that refresh that workspace's index when a git operation changes the checked-out commit, so a caller keeps the index tracking the checked-out state without wiring that by hand.
Installation SHALL write the hooks to the paths the worktree resolves its hooks to, SHALL make each written hook executable, and SHALL report each path written.
One set of hooks SHALL refresh every workspace installed into it: installing a second workspace SHALL keep the first workspace refreshed, and installing a workspace that is already installed SHALL leave every hook file unchanged.
When a refresh hook for one event is missing and the remaining refresh hooks agree on the installed workspaces, installation SHALL restore the missing hook for all of those workspaces.
When the present refresh hooks disagree on the installed workspaces, installation SHALL refuse before changing any file and SHALL name the hook files that disagree, unless they differ only by the workspace being installed; then installation SHALL complete, so a retry finishes an installation that failed partway.
Installation SHALL NOT write git configuration.
Installation SHALL refuse, before changing any file, a workspace root, index path, or workspace identity that contains a line break, because the hook file cannot carry it.
Installation outside a git worktree SHALL be reported through the same typed environment failure the diff-seeded assessment raises for that condition.

The hooks are the freshness discipline the diff-seeded assessment depends on: its pre-change side is a committed state, so an index refreshed whenever the checked-out commit changes is the state that assessment resolves against.

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

After a git operation that changes the checked-out commit completes — a commit, an amended commit, a completed rebase, a branch checkout, or a merge or pull — the installed hooks SHALL have refreshed each installed workspace's index, so the index matches the checked-out state.
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

### Requirement: Refresh-hook event action

The hooks command SHALL provide an action that applies one event's refresh rules to one workspace and refreshes its index when the rules call for it; the installed hooks call this action once per installed workspace.
The action SHALL exit with the success code whether or not the refresh runs or fails, SHALL write a failed refresh to standard error naming the event and the workspace, and SHALL write nothing to standard output when the rules call for no refresh.
An event, a workspace root, or trailing git arguments given to any other action of the hooks command, and the overwrite flag given to any action other than installation, SHALL be rejected as a usage error, before any file changes, naming the action the argument belongs to.

Serves: rebase-builds-once, index-follows-head

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

Serves: hooks-coexist

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

Serves: hooks-coexist, missing-refresh-is-visible

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

Serves: hooks-coexist

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

Serves: hooks-coexist

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

Serves: missing-refresh-is-visible

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

## REMOVED Requirements

### Requirement: Commit-hook installation

Removed because: installation now covers every event that changes the checked-out commit and keeps existing hooks instead of refusing.
**Refresh-hook installation**, **Refresh timing**, **Existing hooks keep running**, **Kept-hook collision**, and **Hook replacement on request** replace it.
