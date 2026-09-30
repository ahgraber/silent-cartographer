# Delta for Code Graph

## ADDED Requirements

### Requirement: A Python build selects its indexer

A Python build SHALL index with the indexer its `--python-indexer` option names (`ty-scip` or `scip-python`).
Without the option, it SHALL index with ty-scip when ty-scip is available, otherwise with scip-python when scip-python is available, and SHALL refuse with a typed setup error naming how to install each indexer when neither is available.
An indexer is available when it is present and answers its readiness probe within the probe bound; an indexer that is present but does not answer SHALL cause a refusal naming it, and SHALL NOT cause the build to use the other indexer in its place.
The build SHALL record the indexer that produced the index as that index's analyzer provenance.
A stored Python index SHALL be judged current by a build, and fresh by staleness reporting, only while the indexer that the build — or, for staleness reporting, a build without the option — would select now is the recorded indexer at the recorded version.
A build of a non-Python workspace that supplies `--python-indexer` SHALL be refused as a usage error.

Serves: choose-python-indexer, index-large-python

#### Scenario: An explicit selection names the indexer

- **GIVEN** a Python workspace on a host where both indexers are available
- **WHEN** a build runs with `--python-indexer scip-python`
- **THEN** the index is produced by scip-python and its provenance names scip-python

#### Scenario: Both available, ty-scip is detected

- **GIVEN** a Python workspace on a host where both indexers are available
- **WHEN** a build runs without `--python-indexer`
- **THEN** the index is produced by ty-scip and its provenance names ty-scip

#### Scenario: Only scip-python available

- **GIVEN** a Python workspace on a host where scip-python is available and ty-scip is absent
- **WHEN** a build runs without `--python-indexer`
- **THEN** the index is produced by scip-python and its provenance names scip-python

#### Scenario: Only ty-scip available

- **GIVEN** a Python workspace on a host where ty-scip is available and scip-python is absent
- **WHEN** a build runs without `--python-indexer`
- **THEN** the index is produced by ty-scip and its provenance names ty-scip

#### Scenario: Neither available

- **GIVEN** a Python workspace on a host where neither indexer is present
- **WHEN** a build runs without `--python-indexer`
- **THEN** the build refuses with a typed setup error naming how to install each indexer, and no index is persisted from the attempt

#### Scenario: An unresponsive preferred indexer refuses instead of falling back

- **GIVEN** a Python workspace on a host where ty-scip is present but does not answer its readiness probe, and scip-python is available
- **WHEN** a build runs without `--python-indexer`
- **THEN** the build refuses with a typed setup error naming ty-scip as unresponsive, and no index is persisted from the attempt

#### Scenario: A newly installed indexer makes the store stale and rebuilds it

- **GIVEN** a Python workspace whose store was built by scip-python, with unchanged sources, on a host where ty-scip has since become available
- **WHEN** staleness is reported, and then a build runs without `--python-indexer`
- **THEN** staleness reports the index stale, and the build analyzes the workspace and rebuilds the store with ty-scip provenance

#### Scenario: An unchanged index stays current and fresh

- **GIVEN** a Python workspace whose store was built without `--python-indexer`, with unchanged sources, environment, installed indexers, and indexer versions
- **WHEN** a build runs again without `--python-indexer`, and staleness is reported
- **THEN** the build reports the index already current, and staleness reports it fresh

#### Scenario: An index built against detection is reported stale

- **GIVEN** a Python workspace whose store was built with `--python-indexer scip-python` on a host where ty-scip is available
- **WHEN** staleness is reported
- **THEN** the index is reported stale, because a build without the option would select ty-scip

#### Scenario: The selection on a Rust build is refused

- **GIVEN** a Rust workspace
- **WHEN** a build runs with `--python-indexer ty-scip`
- **THEN** the build is refused as a usage error and no index is persisted from the attempt
