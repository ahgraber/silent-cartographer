# Changelog

All notable changes to `c10r` are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-09-08

### Added

- Add Rust structural spine (identity, join, graph, CLI)
- Harden calibration across identity, join, and store
- Add dependents query, contract dependency edges
- Split duplicate symbols and attribute refs by locality
- Add Python semantic backend via scip-python
- Align Python module refs and scope-resolve same-document twins
- Attribute alias, self-name, marker, and dotted module refs
- Attribute Rust range, tuple-field, and path-keyword refs
- Persist chunk tiers with get/trace detail
- Align command surface with bounding and new commands
- Add diff-seeded impact command and commit hook
- Add convention-classified tests relation
- Refuse to write to or delete unrecognized databases
- Order dependents by structural importance
- Find code by meaning and by similarity
- Skip a build whose index already describes the workspace
- Embed each passage as bounded chunks
