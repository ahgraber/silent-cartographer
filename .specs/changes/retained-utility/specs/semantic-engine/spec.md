# Delta for Semantic Engine

## MODIFIED Requirements

### Requirement: Unavailable backend is a typed refusal, never a partial index

When a backend's external toolchain is unavailable, or the project's environment cannot be resolved well enough to produce a faithful index, indexing SHALL fail with a typed, actionable error naming what is missing, and SHALL NOT produce or persist a partial or degraded index in its place.
The error SHALL also name how to request a structural index, and state that a structural index does not answer references or dependents, and that its impact answer lists the touched declarations without their dependents.
Indexing that fails this way SHALL leave any existing store unchanged.
A structural index that the caller requested or confirmed is not a degraded index in the failed index's place; `A Rust or Python build is structural only on request` governs it.

> Previously: the error named only what was missing, because no other kind of index existed.

Serves: consent-before-structural-index

#### Scenario: Missing indexer tool refuses with guidance

- **GIVEN** a project whose backend indexer tool is not present in the environment, with no structural index requested with the flag, confirmed, or stored for the workspace
- **WHEN** a build is attempted
- **THEN** the build fails with a typed error naming the missing tool and the request for a structural index, and any existing store is left untouched

#### Scenario: Unresolvable environment refuses with guidance

- **GIVEN** a Python project whose interpreter environment cannot be resolved, with no structural index requested with the flag, confirmed, or stored for the workspace
- **WHEN** a build is attempted
- **THEN** the build fails with a typed error naming the environment problem and the request for a structural index, and no index is persisted from the failed attempt

### Requirement: A workspace file the indexer rejects is a typed refusal

When the selected Python indexer rejects the workspace because a project file has no importable module name, indexing SHALL fail with a typed, actionable error that names the rejected file and the configuration remedy for its cause, and SHALL NOT produce or persist a partial index in its place.
The error SHALL also name how to request a structural index.
The system SHALL NOT exclude the rejected file on the user's behalf.

> Previously: the error named only the configuration remedy.

Serves: consent-before-structural-index

#### Scenario: A file carrying inline script metadata is refused with the script remedy

- **GIVEN** a Python workspace holding a file that begins with PEP 723 inline script metadata, built with the ty-scip indexer, with no structural index requested with the flag, confirmed, or stored for the workspace
- **WHEN** a build is attempted
- **THEN** the build fails with a typed error naming that file, the setting that excludes script files, and the request for a structural index, and no index is persisted from the attempt

#### Scenario: A file whose path is not an importable name is refused with the exclude remedy

- **GIVEN** a Python workspace holding a source file under a directory whose name is not a valid Python identifier, built with the ty-scip indexer, with no structural index requested with the flag, confirmed, or stored for the workspace
- **WHEN** a build is attempted
- **THEN** the build fails with a typed error naming that file, the exclude setting, and the request for a structural index, and no index is persisted from the attempt

#### Scenario: A refused build leaves the existing store untouched

- **GIVEN** a workspace with an existing store, and a new file the ty-scip indexer rejects, with no structural index requested with the flag, confirmed, or stored for the workspace
- **WHEN** a build is attempted
- **THEN** the build fails with the typed error and the existing store is unchanged
