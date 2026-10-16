# Delta for Code Graph

## ADDED Requirements

### Requirement: A Python build selects its indexer

A Python build SHALL index with the indexer its `--python-indexer` option names (`ty-scip` or `scip-python`).
Without the option, it SHALL index with the indexer recorded in the existing store, and with scip-python when no store exists.
When the selected indexer is not available, the build SHALL refuse with a typed setup error that names that indexer and how to install it, and SHALL NOT index with the other indexer in its place.
An indexer is available when it is present and answers its readiness probe within the probe bound; a refusal for an indexer that is present but does not answer SHALL name it unresponsive.
The build SHALL record the indexer that produced the index as that index's analyzer provenance.
A stored Python index SHALL be judged current by a build only while the indexer that build selects is the recorded indexer at the recorded version, and SHALL be judged fresh by staleness reporting only while the recorded indexer is at the recorded version.
A build of a non-Python workspace that supplies `--python-indexer` SHALL be refused as a usage error.

Serves: choose-python-indexer, index-large-python

#### Scenario: An explicit selection names the indexer

- **GIVEN** a Python workspace with no store, on a host where both indexers are available
- **WHEN** a build runs with `--python-indexer ty-scip`
- **THEN** the index is produced by ty-scip and its provenance names ty-scip

#### Scenario: A new store defaults to scip-python

- **GIVEN** a Python workspace with no store, on a host where both indexers are available
- **WHEN** a build runs without `--python-indexer`
- **THEN** the index is produced by scip-python and its provenance names scip-python

#### Scenario: The default indexer absent refuses instead of using ty-scip

- **GIVEN** a Python workspace with no store, on a host where ty-scip is available and scip-python is absent
- **WHEN** a build runs without `--python-indexer`
- **THEN** the build refuses with a typed setup error naming scip-python and how to install it, and no index is persisted from the attempt

#### Scenario: A store keeps the indexer it was built with

- **GIVEN** a Python workspace whose store was built by ty-scip, with changed sources, on a host where both indexers are available
- **WHEN** a build runs without `--python-indexer`
- **THEN** the build rebuilds the store with ty-scip, and its provenance names ty-scip

#### Scenario: An explicit selection switches the store's indexer

- **GIVEN** a Python workspace whose store was built by ty-scip, with unchanged sources, on a host where both indexers are available
- **WHEN** a build runs with `--python-indexer scip-python`, and then a build runs without `--python-indexer`
- **THEN** the first build analyzes the workspace and rebuilds the store with scip-python provenance, and the second build reports the index already current

#### Scenario: The recorded indexer absent refuses instead of using the other

- **GIVEN** a Python workspace whose store was built by ty-scip, on a host where ty-scip is absent and scip-python is available
- **WHEN** a build runs without `--python-indexer`
- **THEN** the build refuses with a typed setup error naming ty-scip and how to install it, and the existing store is unchanged

#### Scenario: An unresponsive selected indexer refuses instead of falling back

- **GIVEN** a Python workspace with no store, on a host where scip-python is present but does not answer its readiness probe, and ty-scip is available
- **WHEN** a build runs without `--python-indexer`
- **THEN** the build refuses with a typed setup error naming scip-python as unresponsive, and no index is persisted from the attempt

#### Scenario: An unchanged index stays current and fresh

- **GIVEN** a Python workspace whose store was built by either indexer, with unchanged sources, environment, and indexer version
- **WHEN** a build runs again without `--python-indexer`, and staleness is reported
- **THEN** the build reports the index already current, and staleness reports it fresh

#### Scenario: A changed version of the recorded indexer makes the index stale

- **GIVEN** a Python workspace whose store was built by ty-scip, with unchanged sources, on a host where the installed ty-scip version differs from the recorded version
- **WHEN** staleness is reported
- **THEN** the index is reported stale

#### Scenario: The selection on a Rust build is refused

- **GIVEN** a Rust workspace
- **WHEN** a build runs with `--python-indexer ty-scip`
- **THEN** the build is refused as a usage error and no index is persisted from the attempt
