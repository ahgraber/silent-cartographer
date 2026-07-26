# Command Surface — Delta: impact

## ADDED

### Requirement: Commit-hook installation

Serves: honest-diff-reach

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
