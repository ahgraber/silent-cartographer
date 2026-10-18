# Delta for Code Graph

## ADDED Requirements

### Requirement: A structural build indexes what the syntax tree proves

A structural build SHALL index each discovered file from its syntax tree alone, and SHALL run no semantic indexer.
It SHALL persist, for each file, the file itself and each declaration the language's syntax profile names in it, with the declaration's structural identity, its recorded location, its header end, and its tier content, under the same rules as a semantic build.
It SHALL persist the enclosure relation inside each file: a file encloses its outermost declarations, and a declaration encloses the declarations written directly inside it.
It SHALL NOT persist enclosure between two files, such as a Rust `mod name;` declaration and the file it loads.
For Rust, the declarations are modules, structs, enums, unions, traits, impls, functions, constants, statics, and type aliases; for Python, classes and functions.
In Rust and Python, a declaration local to a function body is not one, and fields, variables, and module globals are not declarations of a structural index.
A structural build has no semantic occurrences, so `Unresolved calls are recorded` records every call it identifies.
A structural build SHALL persist no reference occurrence, no `uses`, `imports`, or `type_hierarchy` edge, no test classification, and no join discrepancy.
It SHALL build the semantic corpus, its representations, and the clone-equivalence keys over its declarations as a semantic build does.
It SHALL record the grammar's name and version as the index's analyzer provenance, so that a change of grammar version marks the index stale.
It SHALL record each indexed file whose syntax tree holds a syntax error.
A structural build replaces every row of a prior build, semantic or structural, as `Builds wholly supersede prior derived state` requires, and a semantic build replaces every row of a prior structural build.

Serves: structural-navigation-without-semantic-indexer, calibrated-capability-disclosure

#### Scenario: A file with a syntax error is recorded

- **GIVEN** a Bash workspace whose file `broken.sh` holds a syntax error and whose file `lib.sh` holds none
- **WHEN** a structural index is built
- **THEN** `broken.sh` is recorded as holding a syntax error, `lib.sh` is not, and the declarations the grammar recovers from `broken.sh` are persisted

#### Scenario: A structural build over a semantic store keeps no semantic rows

- **GIVEN** a Python workspace with a semantic index holding reference occurrences, `uses`, `imports`, and `type_hierarchy` edges, test classifications, and join discrepancies
- **WHEN** a structural index is built over it with the structural flag
- **THEN** no reference occurrence, edge other than `contains`, test classification, or join discrepancy remains, and every symbol has a structural identity

#### Scenario: A semantic build over a structural store keeps no structural rows

- **GIVEN** a Python workspace with a structural index
- **WHEN** a semantic index is built over it
- **THEN** no symbol has a structural identity, and no file remains recorded as structural

#### Scenario: A Bash file yields its functions and their nesting

- **GIVEN** a Bash file that defines the function `deploy`, inside which it defines the function `retry`
- **WHEN** a structural index is built
- **THEN** the file, `deploy`, and `retry` are persisted, the file encloses `deploy`, and `deploy` encloses `retry`

#### Scenario: A structural Python build persists classes and methods without references

- **GIVEN** a Python workspace whose module defines a class with a method that another module calls, built structurally
- **WHEN** a structural index is built
- **THEN** the module, the class, and the method are persisted with their enclosure, the call is recorded as an unresolved call, and no reference occurrence or `uses` edge is persisted

#### Scenario: A function-local Python declaration is not persisted

- **GIVEN** a Python function whose body defines a nested function, built structurally
- **WHEN** a structural index is built
- **THEN** the outer function is persisted and the nested function is not

#### Scenario: A Rust file module is not enclosed by the file that declares it

- **GIVEN** a Rust workspace whose `src/lib.rs` declares `mod util;` and whose `src/util.rs` defines `pub fn load()`, built structurally
- **WHEN** a structural index is built
- **THEN** `src/util.rs` encloses `load`, and `src/lib.rs` does not enclose `src/util.rs`

#### Scenario: A Python base class yields no type-hierarchy edge

- **GIVEN** a Python workspace with the class `Admin(User)`, built structurally
- **WHEN** a structural index is built
- **THEN** no `type_hierarchy` edge is persisted

#### Scenario: Structural provenance names the grammar

- **GIVEN** a Bash workspace
- **WHEN** a structural index is built
- **THEN** the index's analyzer provenance is the Bash grammar's name and version

#### Scenario: A new grammar version marks a structural index stale

- **GIVEN** a structural index recorded under one grammar version
- **WHEN** the grammar in effect is a different version
- **THEN** results from that index are reported as stale

### Requirement: Bash declarations and calls

The Bash syntax profile SHALL name as declarations the function definitions written in either form, `name() { … }` and `function name { … }`, at any nesting depth.
It SHALL name as calls the simple commands whose command name is a literal word, with that word as the called name and no receiver, wherever the command appears, including in a pipeline, a list, a subshell, and a command substitution.
A function's name, and so a called name, is the whole word, and may contain `::`.
A command whose command word is the pipeline prefix `time` SHALL be named as a call of its command operand, the first word after the options of `time`, when that operand is a literal word; it SHALL name no call when the operand is formed by an expansion, and SHALL NOT be named as a call of `time`.
A command whose name is formed by an expansion, such as `$tool` or `"$(pick)"`, SHALL NOT be named as a call.
A function definition's signature tier SHALL be its text up to the opening of its body, and its interface tier SHALL add the comment lines directly above it.

Serves: structural-navigation-without-semantic-indexer

#### Scenario: Both definition forms are declarations

- **GIVEN** a Bash file defining `setup() { … }` and `function teardown { … }`
- **WHEN** a structural index is built
- **THEN** `setup` and `teardown` are both persisted as declarations

#### Scenario: Commands in a pipeline and a substitution are calls

- **GIVEN** a Bash function whose body runs `build | upload` and `version=$(read_version)`
- **WHEN** a structural index is built
- **THEN** calls named `build`, `upload`, and `read_version` are recorded, each with the function as its enclosing declaration

#### Scenario: A top-level command attributes to its file

- **GIVEN** a Bash file whose top level runs `main "$@"`
- **WHEN** a structural index is built
- **THEN** a call named `main` is recorded with the file as its enclosing declaration

#### Scenario: A timed command is a call of the command

- **GIVEN** a Bash function whose body runs `time deploy` and `time -p deploy`
- **WHEN** a structural index is built
- **THEN** two calls named `deploy` are recorded, and no call named `time`

#### Scenario: A timed expansion is not a call

- **GIVEN** a Bash function whose body runs `time "$tool" deploy` and `time -p "$tool" deploy`
- **WHEN** a structural index is built
- **THEN** no call is recorded for either command

#### Scenario: A command named by an expansion is not a call

- **GIVEN** a Bash function whose body runs `"$tool" --version`
- **WHEN** a structural index is built
- **THEN** no call is recorded for that command

#### Scenario: A comment above a function is its documentation

- **GIVEN** a Bash function preceded by two comment lines
- **WHEN** its tier content is retrieved
- **THEN** its interface tier holds the two comment lines and its signature, and not its body

### Requirement: Bash source discovery

A Bash build SHALL discover the files with a `.sh` or `.bash` extension, and the files with no extension whose first line is a shebang that runs `sh` or `bash`, directly or through `env`.
It SHALL NOT discover a file whose first line is a shebang that runs another interpreter, whatever its extension.
It SHALL NOT discover a path that is not a regular file.
`Source discovery excludes undecodable files` applies to the files it discovers.
Every consumer of the discovered set, including the status report and `impact`, SHALL use this rule, so that a file's discovery is decided by its content as well as its path.

Serves: structural-navigation-without-semantic-indexer

#### Scenario: Extensions and shebangs select files

- **GIVEN** a workspace holding `lib.sh`, `run.bash`, an extensionless `bin/deploy` starting `#!/usr/bin/env bash`, an extensionless `bin/init` starting `#!/bin/sh`, an extensionless `bin/prompt` starting `#!/bin/zsh`, `release.sh` starting `#!/bin/zsh`, and `notes.sh` starting `#!/usr/bin/python3`
- **WHEN** a Bash build discovers sources
- **THEN** `lib.sh`, `run.bash`, `bin/deploy`, and `bin/init` are discovered, and `bin/prompt`, `release.sh`, and `notes.sh` are not

#### Scenario: A changed shebang changes discovery

- **GIVEN** a structural Bash index holding the extensionless `bin/deploy`
- **WHEN** its shebang is changed to run `zsh` and the index status is reported
- **THEN** the report names `bin/deploy` as removed

### Requirement: A Rust or Python build is structural only on request

A Rust or Python build SHALL produce a structural index only when the caller requests one with the build's structural flag, confirms one at the build's prompt, or builds over a store that holds a structural index of the same workspace root and language.
With the structural flag, the build SHALL produce a structural index without running the semantic indexer.
Without the flag, a build whose store is current for the selected indexer SHALL do no work, as `A build over unchanged inputs does no work` requires.
Otherwise the build SHALL index with the selected semantic indexer, and when that succeeds SHALL produce a semantic index, replacing a structural one.
When the indexer is unavailable or fails, the build SHALL:

- over a store that holds a structural index of the same workspace root and language, produce a structural index without asking;
- otherwise, when the build is not run by a refresh hook and standard input and standard error are attached to a terminal, ask whether to build a structural index instead, stating that a structural index does not answer references or dependents and that its impact answer lists the touched declarations without their dependents, and produce one only on a yes;
- otherwise, fail as `Unavailable backend is a typed refusal, never a partial index` requires, naming the structural flag, and leave any existing store unchanged.

Every build that produces a structural index for a Rust or Python workspace SHALL state in its answer that the index is structural, and why: that the flag requested it, or the indexer failure the build observed.

Serves: consent-before-structural-index, calibrated-capability-disclosure

#### Scenario: The flag builds a structural index

- **GIVEN** a Python workspace with no store
- **WHEN** a headless build runs with the structural flag
- **THEN** a structural index is produced without running the indexer, and the build answer states that it is structural because the flag requested it

#### Scenario: A headless build without the flag fails and names it

- **GIVEN** a Python workspace with no store, on a host without the selected indexer
- **WHEN** a headless build runs without the structural flag
- **THEN** the build fails with the indexer setup error, the error names the structural flag, and no store is created

#### Scenario: A current semantic store does no work

- **GIVEN** a Rust workspace whose semantic index is current
- **WHEN** a build runs without the structural flag
- **THEN** the indexer is not run and the store is unchanged

#### Scenario: A failing indexer leaves a semantic store unchanged

- **GIVEN** a Rust workspace with a semantic index, whose indexer now fails on the workspace
- **WHEN** a headless build runs without the structural flag
- **THEN** the build fails, the error names the structural flag, and the semantic index is unchanged

#### Scenario: A yes at the prompt builds a structural index

- **GIVEN** a Python workspace on a host without the selected indexer
- **WHEN** a build runs at a terminal without the structural flag and the user answers yes
- **THEN** the prompt states that a structural index does not answer references or dependents and that its impact answer lists touched declarations without their dependents, and a structural index is produced whose build answer names the indexer failure

#### Scenario: A no at the prompt fails as a refusal

- **GIVEN** a Python workspace with a semantic index, on a host without the selected indexer
- **WHEN** a build runs at a terminal without the structural flag and the user answers no
- **THEN** the build fails with the indexer setup error and the semantic index is unchanged

#### Scenario: A hook build never asks

- **GIVEN** a Rust workspace with a semantic index, whose indexer now fails, and a refresh hook run with standard input and standard error attached to a terminal
- **WHEN** the hook's build runs
- **THEN** no question is asked, no input is read, and the semantic index is unchanged

#### Scenario: A structural store stays structural without asking

- **GIVEN** a Python workspace with a structural index, on a host without the selected indexer
- **WHEN** a build runs without the structural flag, at a terminal and headless in turn
- **THEN** each build produces a structural index without asking, and its answer states that the indexer is unavailable

#### Scenario: Structural consent does not cross workspaces or languages

- **GIVEN** a store holding a structural Bash index of workspace A, and a Python workspace B on a host without the selected indexer
- **WHEN** a headless build of workspace B targets that store without the structural flag
- **THEN** the build fails, names the structural flag, and the store is unchanged

#### Scenario: A working indexer replaces a structural store

- **GIVEN** a Python workspace with a structural index, on a host where the selected indexer is now available
- **WHEN** a build runs without the structural flag
- **THEN** a semantic index replaces the structural one

## MODIFIED Requirements

### Requirement: Build selects exactly one language backend

A build SHALL select the backend whose language the workspace's project manifest declares; when the workspace declares manifests for more than one supported language and no explicit selection is supplied, the build SHALL refuse with a typed error naming the selection mechanism; an explicit selection SHALL override detection; and the selected backend SHALL be recorded in the index's provenance.
When the workspace declares no Rust or Python project manifest and holds at least one file that Bash source discovery finds, the build SHALL select Bash.
The explicit selection SHALL accept Bash.
A Bash build SHALL always produce a structural index.

> Previously: a workspace with no project manifest was refused, and Bash could not be selected.

Serves: structural-navigation-without-semantic-indexer

#### Scenario: Single-language workspace selects its backend

- **GIVEN** a workspace declaring only a Python project manifest
- **WHEN** a build runs with no explicit language selection
- **THEN** the Python backend is selected and recorded in the index's provenance

#### Scenario: Two manifests without a selection refuse

- **GIVEN** a workspace declaring both a Rust and a Python project manifest
- **WHEN** a build runs with no explicit language selection
- **THEN** the build refuses with a typed error naming the explicit selection mechanism, and no index is persisted from the attempt

#### Scenario: Explicit selection overrides detection

- **GIVEN** a workspace declaring both a Rust and a Python project manifest
- **WHEN** a build runs with an explicit language selection
- **THEN** the build proceeds with the selected backend and records it in the index's provenance

#### Scenario: A workspace of shell scripts selects Bash

- **GIVEN** a workspace with no project manifest that holds `deploy.sh`
- **WHEN** a build runs with no explicit language selection
- **THEN** Bash is selected, a structural index is produced, and the Bash grammar is recorded in the index's provenance

#### Scenario: A manifest takes precedence over shell scripts

- **GIVEN** a workspace declaring a Rust project manifest that also holds `scripts/release.sh`
- **WHEN** a build runs with no explicit language selection
- **THEN** the Rust backend is selected

#### Scenario: An empty workspace is still refused

- **GIVEN** a workspace with no project manifest and no file that Bash source discovery finds
- **WHEN** a build runs with no explicit language selection
- **THEN** the build refuses with a typed error naming the explicit selection mechanism

#### Scenario: Bash is selected explicitly

- **GIVEN** a workspace declaring a Python project manifest that also holds shell scripts
- **WHEN** a build runs with Bash selected explicitly
- **THEN** a structural Bash index is produced from the shell scripts

### Requirement: Unresolved calls are recorded

For every build, the system SHALL record each call in an indexed document whose called name has no semantic occurrence at the called name's location, and SHALL NOT record a call whose called name has a semantic occurrence there, whatever symbol that occurrence names, including a symbol local to the document such as a parameter.
A call's called name is the last name its callee is written with: `name` in Python `name(...)` and `receiver.name(...)`, and in Rust `name(...)`, `receiver.name(...)`, and `path::name(...)`.
In Bash, a call is a command that `Bash declarations and calls` names, and its called name is its command word.
A call whose callee ends in no name, such as a call of a subscript or of a parenthesized expression, SHALL NOT be recorded.
Each recorded call SHALL carry its called name, the location of the called name, the source text of its receiver when the callee has one (the expression before the last `.` or `::`), and its enclosing declaration, attributed under the same rule as a reference occurrence, or its module or file when no narrower declaration encloses it.
Receiver text longer than a documented bound SHALL be recorded as its leading part with a marker that it was shortened.
A call written inside the arguments of a Rust macro invocation is outside this requirement.

> Previously: calls were identified in Rust and Python only, and a call outside every declaration was attributed to its module.

Serves: structural-navigation-without-semantic-indexer

#### Scenario: A Python attribute call the indexer cannot resolve is recorded

- **GIVEN** a Python test function whose parameter has no annotation and whose body calls `client.fetch()`, indexed by each supported Python indexer in turn
- **WHEN** the index is built
- **THEN** the call is recorded with the called name `fetch`, the receiver text `client`, the location of `fetch`, and the test function as its enclosing declaration

#### Scenario: A Python plain call with no occurrence is recorded without a receiver

- **GIVEN** a Python module that calls a function imported from a third-party package, indexed by an indexer that records no occurrence for that call
- **WHEN** the index is built
- **THEN** the call is recorded with its called name and no receiver text

#### Scenario: A Rust call in code the analyzer does not analyze is recorded

- **GIVEN** a Rust function whose body calls `value.render()` under a `#[cfg(...)]` condition that is false for the build host
- **WHEN** the index is built
- **THEN** the call is recorded with the called name `render` and the receiver text `value`

#### Scenario: A Rust path call carries its path as receiver

- **GIVEN** a Rust call `Widget::build()` whose called name has no semantic occurrence
- **WHEN** the index is built
- **THEN** the call is recorded with the called name `build` and the receiver text `Widget`

#### Scenario: A resolved call is not recorded

- **GIVEN** a Python call and a Rust call whose called names each have a semantic occurrence
- **WHEN** the index is built
- **THEN** neither call is recorded as unresolved

#### Scenario: A call resolved to a local binding is not recorded

- **GIVEN** a Python function that calls its parameter `callback()`, indexed by each supported Python indexer in turn, and a Rust function that calls its closure parameter `callback()`, where each indexer records an occurrence of the local parameter at the called name
- **WHEN** the index is built
- **THEN** neither call is recorded as unresolved

#### Scenario: A module-level call attributes to the module

- **GIVEN** an unresolved call at module scope, enclosed by no narrower declaration
- **WHEN** the index is built
- **THEN** the recorded call's enclosing declaration is the module

#### Scenario: A long receiver is shortened with a marker

- **GIVEN** an unresolved call whose receiver is a call spanning many lines, longer than the documented bound
- **WHEN** the index is built
- **THEN** the recorded receiver text is the receiver's leading part, no longer than the bound, and is marked as shortened

#### Scenario: Every call of a structural build is recorded

- **GIVEN** a Python workspace built structurally, whose function calls `helper()` and `self.save()`, where `helper` is defined in the same module
- **WHEN** a structural index is built
- **THEN** both calls are recorded as unresolved calls

### Requirement: Per-symbol test classification

The system SHALL classify, at build time, every persisted in-workspace symbol of a semantic index as test code or not, from statically observable language-convention signals, and SHALL persist the classification with the build, wholly superseded by each subsequent build.
A symbol SHALL be classified test code exactly when a convention rule for the workspace's language accepts it, and a symbol no rule accepts SHALL be classified non-test.
For Rust, three rules: a test-attribute rule accepting a declaration bearing an attribute whose path's terminal segment is `test`; a test-configuration rule accepting a module gated to the test configuration and every symbol it transitively contains; and a test-directory rule accepting every symbol whose document lies under a directory named `tests` — the static reading of the integration-test layout, symmetric with the Python directory rule.
For Python, two rules following the test runners' file-collection conventions: a test-file rule accepting every symbol in a document whose file name is test-prefixed or test-suffixed (`test_*.py`, `*_test.py`), the conventional single-file test module `tests.py`, or the reserved fixture file `conftest.py`; and a test-directory rule accepting every symbol in a document lying under a directory named `tests`.
A declaration-name convention alone — a test-prefixed function in a document no rule accepts — SHALL NOT classify a symbol as test code, because the runner's own collection would not reach it and a confident false positive is worse than a disclosed miss.
Every test classification SHALL carry the convention rule that stamped it as provenance; rule families are per-language and are extended as calibration evidence justifies each one, so a stronger future rule can join the set without redefining the classification.
A consumer MUST treat the rule vocabulary as open, reading an unrecognized rule name as a valid classification rather than an error.
The classification SHALL NOT alter join alignment, reference attribution, or dependency-edge derivation.
A structural index SHALL classify no symbol, because the `tests` relation and the test relation labels of `similar` are unavailable on it.

> Previously: the rule applied to every persisted symbol, because every index was semantic.

Serves: calibrated-capability-disclosure

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

#### Scenario: A structural index classifies no symbol

- **GIVEN** a Python workspace whose file `tests/test_app.py` defines `test_save`, built structurally
- **WHEN** the index is built
- **THEN** `test_save` carries no test classification

### Requirement: Declared subtype edges (type_hierarchy)

In a semantic index, the system SHALL persist a `type_hierarchy` edge from a subtype to the supertype it explicitly declares — a Rust type's declared trait implementation, or a Python class's declared base class — for every such declaration in the indexed sources whose subtype and supertype are both persisted symbols, including declarations whose supertype name carries generic or parameterized forms, and one edge per declared supertype when a declaration names several.
A structural index SHALL persist no `type_hierarchy` edge, because deciding which declaration a base or trait name denotes needs a semantic index.

> Previously: the rule applied to every index, because every index was semantic.

Serves: calibrated-capability-disclosure

#### Scenario: Plain trait implementation produces an edge

- **GIVEN** a type with a declared implementation of a workspace trait
- **WHEN** the index is built
- **THEN** a `type_hierarchy` edge from the type to the trait is persisted

#### Scenario: Generic trait implementation produces an edge

- **GIVEN** a type with a declared implementation of a trait whose name carries generic parameters
- **WHEN** the index is built
- **THEN** a `type_hierarchy` edge from the type to that trait is persisted

#### Scenario: External trait implementation produces an edge

- **GIVEN** a workspace type with a declared implementation of a trait defined outside the workspace
- **WHEN** the index is built
- **THEN** a `type_hierarchy` edge from the type to the external trait's persisted symbol is persisted

#### Scenario: Python base class produces an edge

- **GIVEN** a Python class declaring a base class defined in the workspace
- **WHEN** the index is built
- **THEN** a `type_hierarchy` edge from the subclass to the base class is persisted

#### Scenario: Multiple bases each produce an edge

- **GIVEN** a Python class declaring more than one base class
- **WHEN** the index is built
- **THEN** a `type_hierarchy` edge is persisted from the subclass to each declared base

#### Scenario: A structural index persists no type-hierarchy edge

- **GIVEN** a Python workspace with the classes `User` and `Admin(User)`, built structurally
- **WHEN** the index is built
- **THEN** no `type_hierarchy` edge is persisted

### Requirement: Override edges (overrides)

> Previously: the rule applied to every index, because every index was semantic.
> This block is based on the text that `method-overrides` syncs into the baseline; rebase it on the synced baseline before this change syncs.

In a semantic index, the system SHALL persist an `overrides` edge from a method to each method it overrides, and SHALL persist no other `overrides` edge.
In Python, a method declared directly in a class overrides, for each base class that has a `type_hierarchy` edge from the class, the method of the same name declared in that base; when that base binds the name to no declaration in its body, the same rule applies in turn to each of that base's own bases, and when that base binds the name to anything other than a method, such as a class attribute assigned a value, the rule stops along that base; an annotation with no value binds nothing.
A Python name that begins with two underscores and does not end with two underscores is private to its class, so a method with such a name overrides nothing and is overridden by nothing.
In Rust, a function declared in the body of an impl of a trait overrides the item of that trait with the same name, whether that item is required or has a default body, whatever form the impl's self type takes.
When a class, impl, or trait declares several persisted methods of one name, such as a Python property's getter and setter, the rule applies to each of them.
When a method's definition, or the trait or base name a declaration uses, resolves to more than one persisted symbol, the system SHALL persist no edge from that method or through that name, and SHALL NOT choose one of the symbols.
A method SHALL NOT be said to override a declaration that is not a method, a method of a base or trait that is not a persisted symbol, or a same-named method reached through a route other than its declared bases or implemented trait.
Methods that a macro generates, and impls that a macro generates, are outside this requirement: no edge is persisted from or to them by virtue of the macro.
For methods that each supported indexer reports with one definition, the edges SHALL be the same whichever supported indexer built the index, for the same sources and the same resolved bases.
A structural index SHALL persist no `overrides` edge, because the rule follows `type_hierarchy` edges and resolved trait names, which a structural index does not have.

Serves: calibrated-capability-disclosure

#### Scenario: A Python method overrides its base's method

- **GIVEN** a Python class `B(A)` where `A` and `B` each declare a method `m`, indexed by each supported Python indexer in turn
- **WHEN** the index is built
- **THEN** an `overrides` edge from `B.m` to `A.m` is persisted

#### Scenario: A Python override skips a base that does not declare the method

- **GIVEN** Python classes `A2`, `B2(A2)`, and `C2(B2)`, where `A2` and `C2` declare a method `m` and `B2` does not
- **WHEN** the index is built
- **THEN** an `overrides` edge from `C2.m` to `A2.m` is persisted

#### Scenario: A Python override links to its nearest ancestor only

- **GIVEN** Python classes `A3`, `B3(A3)`, and `C3(B3)`, each declaring a method `m`
- **WHEN** the index is built
- **THEN** `overrides` edges from `C3.m` to `B3.m` and from `B3.m` to `A3.m` are persisted, and none from `C3.m` to `A3.m`

#### Scenario: A Python override of several bases links to each

- **GIVEN** a Python class `E(X, Y)` where `X`, `Y`, and `E` each declare a method `m`, indexed by each supported Python indexer in turn
- **WHEN** the index is built
- **THEN** `overrides` edges from `E.m` to `X.m` and from `E.m` to `Y.m` are persisted

#### Scenario: A Python base in another module

- **GIVEN** a Python class whose declared base is imported from another module of the workspace, and both declare a method `m`
- **WHEN** the index is built
- **THEN** an `overrides` edge from the subclass's `m` to the base's `m` is persisted

#### Scenario: A Python decorated override

- **GIVEN** a Python class `B(A)` that overrides a `@property`, a `@classmethod`, and a `@staticmethod` of `A` with same-named methods carrying the same decorators
- **WHEN** the index is built
- **THEN** an `overrides` edge from each of `B`'s three methods to the same-named method of `A` is persisted

#### Scenario: A Python method shadowing a base attribute is not an override

- **GIVEN** a Python class `B(A)` where `A` assigns a class attribute `x` and `B` declares a method `x`
- **WHEN** the index is built
- **THEN** no `overrides` edge from `B.x` is persisted

#### Scenario: A class attribute in a middle class stops the lookup

- **GIVEN** Python classes `Root`, `Mid(Root)`, and `Leaf(Mid)`, where `Root` and `Leaf` declare a method `m` and `Mid` assigns `m = None`
- **WHEN** the index is built
- **THEN** no `overrides` edge from `Leaf.m` is persisted

#### Scenario: An annotation with no value does not stop the lookup

- **GIVEN** Python classes `Root`, `Mid(Root)`, and `Leaf(Mid)`, where `Root` and `Leaf` declare a method `m` and `Mid` holds only the annotation `m: object`
- **WHEN** the index is built
- **THEN** an `overrides` edge from `Leaf.m` to `Root.m` is persisted

#### Scenario: A private Python name is not an override

- **GIVEN** a Python class `B(A)` where `A` and `B` each declare a method `__m`, and each also declares `__eq__`
- **WHEN** the index is built
- **THEN** no `overrides` edge from `B.__m` is persisted, and an `overrides` edge from `B.__eq__` to `A.__eq__` is persisted

#### Scenario: A Python property's accessors each link

- **GIVEN** a Python class `B(A)` where `A` and `B` each declare a property `size` with a getter and a setter, indexed by an indexer that reports each accessor as its own definition
- **WHEN** the index is built
- **THEN** each of `B`'s `size` definitions has an `overrides` edge to each of `A`'s `size` definitions

#### Scenario: A Python class with no declared base has no override

- **GIVEN** a Python class that declares no base and has a method with the same name as a method of an unrelated class
- **WHEN** the index is built
- **THEN** no `overrides` edge from that method is persisted

#### Scenario: A Rust impl method overrides a required trait item

- **GIVEN** a Rust trait `Req` with a required method `req`, implemented for two types
- **WHEN** the index is built
- **THEN** an `overrides` edge from each impl's `req` to `Req::req` is persisted

#### Scenario: A Rust impl method overrides a default trait item

- **GIVEN** a Rust trait `Dflt` with a method `dm` that has a default body, implemented for one type that declares `dm` and one that does not
- **WHEN** the index is built
- **THEN** an `overrides` edge from the declaring impl's `dm` to `Dflt::dm` is persisted, and no edge is persisted for the other type

#### Scenario: A Rust blanket impl method overrides its trait item

- **GIVEN** a Rust impl `impl<T: Display> Tr for T` that declares the trait's method `tr`
- **WHEN** the index is built
- **THEN** an `overrides` edge from that impl's `tr` to `Tr::tr` is persisted

#### Scenario: A Rust impl for a reference type overrides its trait item

- **GIVEN** Rust impls `impl Tr for &Widget` and `impl Tr for (u8, u8)` that each declare the trait's method `tr`
- **WHEN** the index is built
- **THEN** an `overrides` edge from each impl's `tr` to `Tr::tr` is persisted

#### Scenario: A Rust file in several build units links its shared identities

- **GIVEN** one Rust source file, compiled into a library and a binary, that declares a trait `Tr` and an impl of it declaring `tr`
- **WHEN** the index is built
- **THEN** an `overrides` edge from the impl's shared `tr` to the shared `Tr::tr` is persisted

#### Scenario: A trait name that resolves to several symbols makes no edge

- **GIVEN** a Rust impl whose trait name resolves to more than one persisted symbol
- **WHEN** the index is built
- **THEN** no `overrides` edge from that impl's methods is persisted

#### Scenario: A trait in another document is reached through its use in the impl header

- **GIVEN** a Rust trait declared in one file and implemented, with a method, in another
- **WHEN** the index is built
- **THEN** an `overrides` edge from the impl's method to the trait's method is persisted

#### Scenario: A macro-generated method has no edge

- **GIVEN** a Rust impl of a workspace trait whose body holds only a macro invocation that expands to the trait's method
- **WHEN** the index is built
- **THEN** no `overrides` edge from that impl's method is persisted

#### Scenario: A Rust inherent method is not an override

- **GIVEN** a Rust type with an inherent method `name` and an impl of a trait whose method is also named `name`
- **WHEN** the index is built
- **THEN** an `overrides` edge from the trait impl's `name` to the trait's `name` is persisted, and none from the inherent `name`

#### Scenario: A method of an external base or trait is not a target

- **GIVEN** a Python class whose declared base is defined outside the workspace, and a Rust impl of a trait defined outside the workspace, each declaring a method of the base or trait
- **WHEN** the index is built
- **THEN** no `overrides` edge to a method that is not a persisted symbol is persisted

#### Scenario: A structural index persists no override edge

- **GIVEN** a Python workspace whose class `Admin(User)` and its base `User` both declare `save`, built structurally
- **WHEN** the index is built
- **THEN** no `overrides` edge is persisted

### Requirement: A Python build selects its indexer

A Python build SHALL index with the indexer its `--python-indexer` option names (`ty-scip` or `scip-python`).
Without the option, it SHALL index with the indexer recorded in the existing store, and with scip-python when no store exists; over a structural store, it SHALL index with the indexer that store records as last attempted, and with scip-python when the store records none.
When the selected indexer is not available, the build SHALL refuse with a typed setup error that names that indexer and how to install it, and SHALL NOT index with the other indexer in its place.
An indexer is available when it is present and answers its readiness probe within the probe bound; a refusal for an indexer that is present but does not answer SHALL name it unresponsive.
A semantic build SHALL record the indexer that produced the index as that index's analyzer provenance, and a structural build that follows a failure of the selected indexer SHALL record that indexer as the last attempted.
A stored semantic Python index SHALL be judged current by a build only while the indexer that build selects is the recorded indexer at the recorded version, and SHALL be judged fresh by staleness reporting only while the recorded indexer is at the recorded version.
A build of a non-Python workspace that supplies `--python-indexer` SHALL be refused as a usage error.

> Previously: every Python index was semantic and recorded its indexer as provenance.

Serves: consent-before-structural-index

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

#### Scenario: An upgrade retries the indexer the fallback attempted

- **GIVEN** a Python workspace built with `--python-indexer ty-scip`, whose analysis failed and whose structural index was confirmed at the prompt, on a host where ty-scip now works and scip-python is absent
- **WHEN** a build runs without `--python-indexer`
- **THEN** ty-scip produces a semantic index that replaces the structural one
