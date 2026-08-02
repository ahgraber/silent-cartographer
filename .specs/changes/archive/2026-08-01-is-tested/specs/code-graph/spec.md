# Delta for Code Graph

## ADDED Requirements

### Requirement: Per-symbol test classification

The system SHALL classify, at build time, every persisted in-workspace symbol as test code or not, from statically observable language-convention signals, and SHALL persist the classification with the build, wholly superseded by each subsequent build.
A symbol SHALL be classified test code exactly when a convention rule for the workspace's language accepts it, and a symbol no rule accepts SHALL be classified non-test.
For Rust, three rules: a test-attribute rule accepting a declaration bearing an attribute whose path's terminal segment is `test`; a test-configuration rule accepting a module gated to the test configuration and every symbol it transitively contains; and a test-directory rule accepting every symbol whose document lies under a directory named `tests` — the static reading of the integration-test layout, symmetric with the Python directory rule.
For Python, two rules following the test runners' file-collection conventions: a test-file rule accepting every symbol in a document whose file name is test-prefixed or test-suffixed (`test_*.py`, `*_test.py`), the conventional single-file test module `tests.py`, or the reserved fixture file `conftest.py`; and a test-directory rule accepting every symbol in a document lying under a directory named `tests`.
A declaration-name convention alone — a test-prefixed function in a document no rule accepts — SHALL NOT classify a symbol as test code, because the runner's own collection would not reach it and a confident false positive is worse than a disclosed miss.
Every test classification SHALL carry the convention rule that stamped it as provenance; rule families are per-language and are extended as calibration evidence justifies each one, so a stronger future rule can join the set without redefining the classification.
A consumer MUST treat the rule vocabulary as open, reading an unrecognized rule name as a valid classification rather than an error.
The classification SHALL NOT alter join alignment, reference attribution, or dependency-edge derivation.

Serves: tests-for-symbol, locate-a-symbols-tests, honest-heuristic-label

#### Scenario: Test-attribute function classified

- **GIVEN** a Rust function bearing the plain test attribute
- **WHEN** the index is built
- **THEN** the function is classified test code with the test-attribute rule as provenance

#### Scenario: Composed test attribute classified

- **GIVEN** a Rust function bearing an async runtime's test attribute whose path ends in the test segment
- **WHEN** the index is built
- **THEN** the function is classified test code under the test-attribute rule

#### Scenario: Test-configured module classifies transitively

- **GIVEN** a Rust module gated to the test configuration, containing a helper function that bears no test attribute
- **WHEN** the index is built
- **THEN** the helper is classified test code under the test-configuration rule

#### Scenario: Test-configured module classifies across documents

- **GIVEN** a Rust module declaration gated to the test configuration, whose module body lives in its own document
- **WHEN** the index is built
- **THEN** the symbols in that document are classified test code under the test-configuration rule

#### Scenario: Integration-test directory classifies its helpers

- **GIVEN** a Rust helper function without a test attribute, in a document under the package's integration-test directory
- **WHEN** the index is built
- **THEN** the helper is classified test code under the test-directory rule

#### Scenario: Production symbol is non-test

- **GIVEN** a Rust function in a production document, bearing no test attribute and enclosed by no test-configured module
- **WHEN** the index is built
- **THEN** the function is classified non-test

#### Scenario: Python test file classifies all its symbols

- **GIVEN** a Python document whose file name matches the test-file convention, containing a helper function whose own name carries no test prefix
- **WHEN** the index is built
- **THEN** the helper is classified test code under the test-file rule

#### Scenario: Python tests directory classifies shared fixtures

- **GIVEN** a Python fixture module under a tests directory whose file name does not itself match the test-file convention
- **WHEN** the index is built
- **THEN** its symbols are classified test code under the test-directory rule

#### Scenario: Test-prefixed name outside test territory stays non-test

- **GIVEN** a Python function whose name carries a test prefix, in a production document no rule accepts
- **WHEN** the index is built
- **THEN** the function is classified non-test

#### Scenario: Near-miss file name stays non-test

- **GIVEN** a Python document whose file name merely begins with the word test (such as `testimony.py`) and matches no test-file name form
- **WHEN** the index is built
- **THEN** its symbols are classified non-test

#### Scenario: Every classification carries its rule

- **GIVEN** a build producing test classifications under more than one convention rule
- **WHEN** the classifications are retrieved
- **THEN** each carries the convention rule that stamped it as provenance

#### Scenario: Classification does not alter the graph

- **GIVEN** a test-classified function whose body references a production symbol
- **WHEN** the index is built
- **THEN** the reference is attributed and its dependency edge derived exactly as from a non-test declaration
