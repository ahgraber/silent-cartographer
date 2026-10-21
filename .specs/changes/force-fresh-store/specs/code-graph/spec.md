# Delta for Code Graph

## ADDED Requirements

### Requirement: A forced build replaces the store whole

A forced build SHALL produce its store from empty: it SHALL create every table of the store anew and SHALL carry nothing over from the store it replaces.
The new store SHALL take the place of the existing store only when the forced build succeeds.
A forced build that fails or is stopped SHALL leave the existing store unchanged and readable, and SHALL leave no partial store at the store path.
When the store path is a symbolic link, a forced build SHALL replace the store the link names and SHALL keep the link.
The answer of a forced build that replaced a store SHALL name the path of the store it replaced.
Before it analyzes the workspace, a forced build over an existing store SHALL check that the file system holding the store has free space of at least the existing store's size plus a documented margin; when it does not, the build SHALL fail without changing the store, and its error SHALL name the space required, the space available, and the store path.

Serves: force-starts-over, a-failed-force-keeps-the-index

#### Scenario: A forced build over a current store starts from empty

- **GIVEN** a store whose index is current for the workspace
- **WHEN** a confirmed forced build runs
- **THEN** the workspace is analyzed, the new store holds exactly the rows the build derived, and the answer names the store it replaced

#### Scenario: A forced build replaces a store whose tables differ

- **GIVEN** a store the system created, recorded under the schema version the binary writes, whose tables lack a column the binary writes
- **WHEN** a confirmed forced build runs
- **THEN** the build succeeds and the store has every table and column the binary writes

#### Scenario: A forced build with no store creates one

- **GIVEN** a workspace with no store
- **WHEN** a confirmed forced build runs
- **THEN** a store is created and the answer names no replaced store

#### Scenario: A failed forced build keeps the existing store

- **GIVEN** a store holding a built index
- **WHEN** a confirmed forced build fails after analysis has started
- **THEN** the existing store is unchanged, queries against it still answer, and no partial store remains at the store path

#### Scenario: A forced build without room for a second store fails first

- **GIVEN** a store holding a built index, on a file system whose free space is less than the store's size plus the documented margin
- **WHEN** a confirmed forced build runs
- **THEN** the build fails before the workspace is analyzed, the error names the space required, the space available, and the store path, and the store is unchanged

#### Scenario: A forced build through a symbolic link keeps the link

- **GIVEN** a store path that is a symbolic link naming a store the system created
- **WHEN** a confirmed forced build runs
- **THEN** the store path is still a symbolic link naming the same location, that location holds the new store, and the answer names that location as the replaced store

#### Scenario: A forced build shrinks a store left large by an earlier build

- **GIVEN** a store whose file grew while it held the index of a larger workspace, now rebuilt for a smaller one
- **WHEN** a confirmed forced build runs
- **THEN** the store file is no larger than a store built from empty for the smaller workspace

### Requirement: Leftover temporary stores are removed

A build SHALL remove each temporary store that an earlier forced build left beside the store path, when the system recognizes the file as a store it created and the build that wrote it is no longer running.
A build SHALL NOT remove a temporary store whose build is still running, nor any file the system does not recognize as its own store, whatever its name.
The build's answer SHALL name each temporary store it removed.

Serves: force-starts-over

#### Scenario: A stopped forced build's temporary store is removed

- **GIVEN** a temporary store beside the store path, left by a forced build that was stopped
- **WHEN** a build runs
- **THEN** the temporary store is removed and the answer names it

#### Scenario: A running build's temporary store is kept

- **GIVEN** a temporary store beside the store path that a forced build still running is writing
- **WHEN** another build runs
- **THEN** that temporary store is not removed

#### Scenario: An unrecognized file with a temporary-store name is kept

- **GIVEN** a file beside the store path whose name matches a temporary store's but that the system does not recognize as its own store
- **WHEN** a build runs
- **THEN** the file is not removed
