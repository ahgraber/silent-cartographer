# Delta for Code Graph

## ADDED Requirements

### Requirement: Store ownership recognition

The system SHALL mark every index store it creates so that the store is recognizable as the system's own artifact independently of the store's schema version, and SHALL NOT write to, alter, or delete a file it does not recognize as a store it created.
An operation refused for lack of recognition SHALL leave the target file unchanged, and its error SHALL be typed — never a storage-level failure — naming the path and stating the recovery for a genuine but unrecognizable index (rebuild) separately from the recovery for an unrelated file (correct the path), never an unconditional instruction to delete.
A path whose contents cannot be examined at all SHALL be refused in the same category rather than read as recognized, as absent, or as a bare storage failure; its error SHALL name the path and the reason the examination failed, and SHALL NOT assert that the target is, or is not, a store the system created.
Creating a store SHALL NOT overwrite a file already occupying the path it builds at; that refusal SHALL leave the file unchanged and SHALL state removal only as conditional on the file being the system's own leftover from an interrupted build.

Serves: own-store-only

#### Scenario: Foreign database refused by build

- **GIVEN** a database file at the store path that the system did not create, holding its own data
- **WHEN** the index is built
- **THEN** the build refuses with the typed ownership error and the file's contents are unchanged

#### Scenario: Version-coincident foreign database refused by build

- **GIVEN** a database file the system did not create whose version stamp happens to equal the schema version the binary writes
- **WHEN** the index is built
- **THEN** the build refuses with the typed ownership error and the file's contents are unchanged

#### Scenario: Version-coincident foreign database refused by query

- **GIVEN** a database file the system did not create whose version stamp happens to equal the schema version the binary expects
- **WHEN** any query or status request runs against it
- **THEN** the request refuses with the typed ownership error and nothing is written into the file

#### Scenario: Non-database file refused

- **GIVEN** a file at the store path that is not a database at all
- **WHEN** the index is built or queried
- **THEN** the operation refuses with the typed ownership error, not a storage-level failure, and the file is unchanged

#### Scenario: Unmarked legacy store refused with rebuild guidance

- **GIVEN** an index store created by a system version that predates ownership marking
- **WHEN** any command that opens the store runs
- **THEN** the operation refuses, the error's rebuild branch names the recovery, and the file is unchanged

#### Scenario: Redirected path to a foreign database refused

- **GIVEN** a store path that resolves through a symlink to a database the system did not create
- **WHEN** the index is built
- **THEN** the build refuses with the typed ownership error and the link target is unchanged

#### Scenario: Recognized store operates normally

- **GIVEN** a store the system created carrying the expected schema version
- **WHEN** the index is built or queried
- **THEN** the operation proceeds with no ownership error

#### Scenario: Unexaminable path refused without an ownership claim

- **GIVEN** a store path naming something whose contents cannot be read at all, such as a directory
- **WHEN** the index is built or queried
- **THEN** the operation refuses in the ownership category, names the path and why the examination failed, claims neither that the target is nor that it is not the system's own store, and leaves the target unchanged

#### Scenario: An occupied build path is not overwritten

- **GIVEN** a file already occupying the path a new store would be built at
- **WHEN** the index is built
- **THEN** the build refuses in the ownership category, the occupying file is unchanged, and the refusal states removal only as conditional on the file being an interrupted build's leftover

### Requirement: Read operations create no store

For any query or status request naming a store path where no file exists, the system SHALL refuse with an error naming the build action as the remedy, and SHALL NOT create a file at that path.

Serves: own-store-only

#### Scenario: Query against a missing store creates nothing

- **GIVEN** a store path at which no file exists
- **WHEN** a query runs against it
- **THEN** the refusal names the build action as the remedy and no file exists at the path afterward

#### Scenario: Status against a missing store creates nothing

- **GIVEN** a store path at which no file exists
- **WHEN** a status request runs against it
- **THEN** the refusal names the build action as the remedy and no file exists at the path afterward

### Requirement: Stores record and disclose their workspace

The system SHALL record in every store it creates the identity of the workspace the store describes; every answer derived from a store whose recorded workspace identity differs from the workspace being queried SHALL carry a workspace-mismatch marker — in the machine answer and the human rendering alike — distinct from and composing with staleness; an answer from a store whose recorded identity matches SHALL carry no such marker; an answer for which the comparison cannot be evaluated SHALL disclose the workspace relationship as unknown, never presenting it as matched; and a build over a recognized store carrying the current schema version and recorded for a different workspace SHALL disclose both identities, proceed, and record the workspace the store now describes.

Serves: wrong-index-disclosed

#### Scenario: Matching workspace carries no marker

- **GIVEN** a store built from the workspace being queried
- **WHEN** any query runs
- **THEN** the answer carries no workspace-mismatch marker

#### Scenario: Different workspace carries the marker

- **GIVEN** a store recorded for one workspace
- **WHEN** a query runs against it from a different workspace
- **THEN** the answer carries the workspace-mismatch marker in the machine answer and the human rendering

#### Scenario: Mismatch marker composes with staleness

- **GIVEN** a store recorded for a different workspace whose sources have also drifted from the indexed content
- **WHEN** a query runs against it
- **THEN** the answer carries the workspace-mismatch marker and the staleness flag, each independently

#### Scenario: Unavailable comparison disclosed as unknown

- **GIVEN** a store whose recorded workspace identity cannot be compared with the workspace being queried
- **WHEN** a query runs against it
- **THEN** the answer discloses the workspace relationship as unknown rather than carrying no marker

#### Scenario: Build over a different workspace's store discloses and re-records

- **GIVEN** a recognized store carrying the expected schema version, recorded for a different workspace
- **WHEN** the index is built from the current workspace
- **THEN** the build discloses both workspace identities, succeeds, and the resulting store records the workspace it was built from

## MODIFIED Requirements

### Requirement: Incompatible index stores are replaced or refused, never half-used

> Previously: the schema version alone decided — a build over any file whose recorded version differed was replaced wholesale, and ownership of the file was never established.

The system SHALL record the schema version in every store it creates; a build over a store the system recognizes as its own whose recorded schema version differs from the version the binary writes SHALL replace the store and succeed; and every query against such a recognized, version-mismatched store SHALL refuse with an error naming the store's version, the expected version, and the recovery action — never surfacing a storage-level failure mid-operation.
A file not recognized as the system's own store is outside replacement's reach and is governed by the Store ownership recognition requirement.

Serves: own-store-only

#### Scenario: Build replaces an incompatible store

- **GIVEN** a store the system created, recorded under a different schema version than the binary writes
- **WHEN** the index is built
- **THEN** the build succeeds and the resulting store carries the current schema version

#### Scenario: Query refuses an incompatible store with guidance

- **GIVEN** a store the system created, recorded under a different schema version than the binary expects
- **WHEN** any query or status request runs against it
- **THEN** a typed error names the store's version, the expected version, and the recovery action, and no storage-level error surfaces

#### Scenario: Matching version operates normally

- **GIVEN** a store the system created, recorded under the schema version the binary expects
- **WHEN** any command runs against it
- **THEN** it operates normally with no version error
