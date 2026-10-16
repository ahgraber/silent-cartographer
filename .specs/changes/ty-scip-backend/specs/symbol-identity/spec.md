# Delta for Symbol Identity

## MODIFIED Requirements

### Requirement: Identity does not depend on the indexer

> Previously: the scenarios held only scip-python and rust-analyzer to their reference lists.

When two conforming indexers for a language both report a declaration with the same qualified name, its canonical identity SHALL be the same whichever indexer built the index.
This does not cover a declaration that only one indexer reports, or a name for which the indexers report different numbers of definitions; `Identity uniqueness within a workspace` governs those identities.

Serves: choose-python-indexer

#### Scenario: scip-python yields the Python reference identities

- **GIVEN** the shared Python conformance project and its reference list of canonical identities, written by hand from this spec
- **WHEN** the project is indexed through scip-python and canonical identities are assigned
- **THEN** every module, class, function, method, and module global the index reports has the identity the reference list gives it

#### Scenario: ty-scip yields the Python reference identities

- **GIVEN** the shared Python conformance project and its reference list of canonical identities
- **WHEN** the project is indexed through ty-scip and canonical identities are assigned
- **THEN** every module, class, function, method, and module global that the index reports with one definition has the identity the reference list gives it

#### Scenario: A name defined twice has one identity per definition under ty-scip

- **GIVEN** the class `Widget` in the module `pkg.shapes` of the shared Python conformance project, whose property `size` is defined twice, once as the getter and once as the setter
- **WHEN** the project is indexed through scip-python, and again through ty-scip
- **THEN** the scip-python index gives `size` the one identity `<workspace>::python-conformance::pkg::shapes::Widget::size`, and the ty-scip index gives each of the two definitions its own distinct identity

#### Scenario: rust-analyzer yields the Rust reference identities

- **GIVEN** the shared Rust conformance project and its reference list of canonical identities, written by hand from this spec
- **WHEN** the project is indexed through rust-analyzer and canonical identities are assigned
- **THEN** every module, struct, function, and method the index reports, in the library and in the binary, has the identity the reference list gives it
