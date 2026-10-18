# Tasks: retained-utility

## Setup

- [ ] Create the worktree `.worktrees/retained-utility` on branch `retained-utility` from `main`, after `unresolved-calls` and `method-overrides` have landed on `main`.
- [ ] Rebase the MODIFIED blocks that copy unsynced text on the synced baseline: `Unresolved calls are recorded` (code-graph), and `Relationship trace`, `References answers disclose unresolved calls that share the subject's name`, and `Empty answers of reference-derived relations claim no proof` (code-navigation); confirm the `implements` relation and the unresolved-call groups exist on `main`.
- [ ] Record the five-repository semantic baseline: build httpx2, Flask, Faker, ripgrep, and fd with the `main` binary into separate stores under `~/.cache/silent-cartographer/`, for the byte-identical comparison in Gates.

## Fixtures

- [ ] Add `tests/fixtures/retained-utility/bash/` holding: `lib.sh` defining `log` and `die`; `scripts/deploy.sh` defining `deploy` with `retry` nested inside it, calling `build | upload`, `version=$(read_version)`, `"$tool" --version`, `time deploy`, and `time -p deploy`, and ending with `main "$@"`; a file defining `log` twice; a file defining the top-level `lib::log` and later `lib` with a nested `log`, and files running `lib::log` and `log`; `function teardown { … }` and `setup() { … }` forms; a function preceded by two comment lines; three files that each run `deploy`; `broken.sh` holding `foo#0() { :; }`; extensionless `bin/deploy` (`#!/usr/bin/env bash`), `bin/init` (`#!/bin/sh`), and `bin/prompt` (`#!/bin/zsh`); `release.sh` starting `#!/bin/zsh`; `notes.sh` starting `#!/usr/bin/python3`; a function defining `log.sh`; and `run.bash`.
- [ ] Add `tests/fixtures/retained-utility/python/`, a project with `src/app/models.py` defining `User` with `save`, `Admin(User)`, a function with a nested function, a function calling `helper()` and `self.save()` where `helper` is defined in the same module, two near-identical functions where one calls the other, a second module that calls `User.save`, and `tests/test_app.py` defining `test_save`.
- [ ] Add `tests/fixtures/retained-utility/rust/`, a crate whose `src/lib.rs` declares `mod util;` and `mod a { pub struct S; }`, and whose `src/util.rs` defines `pub fn load()`.
- [ ] Add support helpers in `tests/support/mod.rs` that build each fixture structurally into a temporary store; a helper that runs `run_build` with a scripted `confirm` answer, a chosen terminal state, and a hook or command caller; and a stub engine that counts its `analyze` calls and can be told to fail setup or analysis.
  Files whose names a repository cannot hold portably, such as `lib.sh::log.sh`, are created by the test in a temporary copy of the fixture.

## Structural identity (symbol-identity)

- [ ] Add the structural projection to [src/identity.rs](../../../src/identity.rs): `{workspace}::./{path}::{segments}`, with `%`, `:`, and `#` percent-encoded in the path and `%` and `#` in segments, `#n` numbering of equal texts in file order, and a function that tells a structural identity from a semantic one given the workspace identity.
- [ ] Unit test (`A Bash function is named by its file`, `A Python method nests in its class within its file`, `A Rust item in an inline module nests in the module within its file`): the projected identities of the three declarations.
- [ ] Unit test (`A redefined Bash function is disambiguated in file order`): two `log` definitions get `#0` and `#1` in file order, and the same identities when the files are projected in reversed order.
- [ ] Unit test (`A Bash name containing the separator is disambiguated`): `lib::log` and the nested `log` get distinct identities, numbered in file order.
- [ ] Unit test (`Same name in two files of a structural index carries no disambiguator`).
- [ ] Unit test (`A path containing the separator cannot collide across files`): the file `lib.sh::log.sh` and the function `log.sh` in `lib.sh` get distinct identities, and the file's identity holds `%3A`.
- [ ] Unit test (`A name containing `#` cannot be mistaken for a disambiguator`): `foo#0`, `foo`, and `foo` get three distinct identities, and the first holds `%23`.
- [ ] Unit test (`A structural identity is distinguishable from a semantic one`): every identity of the Rust and Python conformance projects is recognized as semantic and every structural identity of the fixtures as structural, under the fixtures' workspace identity and under a workspace identity that contains `::./`; a semantic identity whose package name contains `.` is recognized as semantic.
- [ ] Test (`A moved Bash function changes its structural identity`): build, move `deploy` to `scripts/release.sh`, rebuild, and assert the new identity and that the old identity resolves to no symbol.
- [ ] Run the existing identity tests unchanged, as evidence for the scoped semantic requirements (`Deterministic canonical identity`, `Segments follow the language's declared nesting`, `Identity names the declaration's package and unit`, `Identity does not depend on source location`).

## Bash syntax profile (code-graph)

- [ ] Add `tree-sitter-bash = "0.25"` to `Cargo.toml`, update `Cargo.lock`, and add `Language::Bash` with its grammar in `SyntaxTree::parse` in [src/graph/syntax.rs](../../../src/graph/syntax.rs).
- [ ] Add a grammar-version constant for each grammar next to its import, and a unit test that compares each constant with the version in `Cargo.lock`.
- [ ] Add Bash to `declaration_kinds` (`function_definition`) and to `declaration_name_span`, and add the Bash signature and interface tiers to `declaration_tiers`: the text up to the body's opening, and the comment lines directly above.
- [ ] Unit test (`Both definition forms are declarations`, `A comment above a function is its documentation`).
- [ ] Add the Bash row to the unresolved-call walk: a `command` whose `command_name` holds one `word`, and for a `time` command the first argument word that does not begin with `-`.
- [ ] Unit test (`Commands in a pipeline and a substitution are calls`, `A command named by an expansion is not a call`): calls `build`, `upload`, `read_version` are yielded with their spans; `"$tool"` yields none; `[ … ]`, `[[ … ]]`, `unset`, `local`, `export`, and `(( ))` yield none.
- [ ] Unit test (`A timed command is a call of the command`, `A timed expansion is not a call`): `time deploy` and `time -p deploy` each yield a call named `deploy` and none named `time`; `time "$tool" deploy` and `time -p "$tool" deploy` yield no call.
- [ ] Add the Bash entries to the clone token tables (comment, literal, and identifier kinds), and classify a Bash `word` by its parent in the token walk, following `design.md` § Bash token classes for clone keys.
- [ ] Test: two Bash functions that differ only in formatting and comments share a formatting key; a consistently renamed copy shares a substitution key; `f() { echo one; }` and `g() { echo two; }`, and `x=one` and `y=two`, share a substitution key; an inconsistently renamed copy, a changed literal, and a changed expansion inside a string each change the keys they should; checked through `clones` and the clone markers of `similar`, with a nested function in one of the copies.
- [ ] Add the Bash classifier to `language_discovery_rules` and the source collectors in [src/commands.rs](../../../src/commands.rs), following `design.md` § Bash discovery reads a bounded first line.
- [ ] Test (`Extensions and shebangs select files`), and test that `is_discovered_path` agrees with the collector for Bash, as `is_discovered_path_agrees_with_collect_sources` does for the other languages.
- [ ] Test: an extensionless file whose first 256 bytes hold no newline is not selected and is not read past that prefix; a named pipe and a symlink to a directory are skipped without being opened; `#!/usr/bin/env -S bash -e` selects a file.
- [ ] Test (`A changed shebang changes discovery`).
- [ ] Test: an undecodable `.sh` file is excluded with a diagnostic, under `Source discovery excludes undecodable files`.

## Store (code-graph)

- [ ] Add `language`, `tier`, and `syntax_errors` columns to `source_files` in [src/graph/schema.rs](../../../src/graph/schema.rs), raise `SCHEMA_VERSION` by one from the base branch's value with a version note, and update the schema-version tests.
- [ ] Map the grammar analyzer names to their languages in `backend_language`, and add `index_tier`.
- [ ] Unit test: `backend_language` and `index_tier` for every indexer and grammar name, and the refusal of an unknown name.
- [ ] Test: a semantic build writes `semantic` and the language for every `source_files` row, and a structural build writes `structural`.
- [ ] Make `observe_state_from` tier-aware, following `design.md` § Grammar provenance records the tier.
- [ ] Test (`A fresh structural index is fresh with a semantic indexer installed`): structural Rust, Python, and Bash stores over unchanged sources are fresh with the semantic tools present and absent; a Bash store's content hash covers its Bash files.
- [ ] Test (`A new grammar version marks a structural index stale`): a store whose recorded grammar version differs from the constant is reported `stale_version`, and a rebuild makes it fresh.

## Structural ingest (code-graph)

- [ ] Split `ingest_with_params` in [src/graph/mod.rs](../../../src/graph/mod.rs) into the semantic first half and the shared second half, with no change in behavior; run the full test suite.
- [ ] Add the structural first half, following `design.md` § Split the ingest at the symbol rows and § Declaration kinds per language, and write `syntax_errors` for every file in the shared second half.
- [ ] Test (`A Bash file yields its functions and their nesting`).
- [ ] Test (`A structural Python build persists classes and methods without references`): the module, class, and method rows and their `contains` edges exist; no occurrence row and no `uses`, `imports`, or `type_hierarchy` edge exists; no join discrepancy is stored.
- [ ] Test (`A structural index classifies no symbol`): `test_save` carries no test classification, and the existing test-classification tests pass unchanged.
- [ ] Test (`A function-local Python declaration is not persisted`).
- [ ] Test (`A Rust file module is not enclosed by the file that declares it`).
- [ ] Test (`A Python base class yields no type-hierarchy edge`).
- [ ] Test (`Every call of a structural build is recorded`), and `A top-level command attributes to its file` on the Bash fixture.
- [ ] Test: a structural build stores tier content, line and column, and header end for every declaration; the module row's signature tier is its identity without the workspace.
- [ ] Test: a structural build stores the semantic corpus, chunks, vectors, and clone keys for its declarations.
- [ ] Test (`Structural provenance names the grammar`).
- [ ] Test (`A file with a syntax error is recorded`).
- [ ] Test (`A structural build over a semantic store keeps no semantic rows`): build the Python fixture semantically, rebuild with `--structural`, and assert every remaining row comes from the structural build.
- [ ] Test (`A semantic build over a structural store keeps no structural rows`).
- [ ] Test: a structural build that fails after clearing begins leaves the prior semantic store unchanged.
- [ ] Test: a structural build over unchanged sources with the same grammar does no work, under `A build over unchanged inputs does no work`.
- [ ] Test (`A structural index persists no type-hierarchy edge`, `A structural index persists no override edge`), and run the existing `type_hierarchy` and `overrides` tests unchanged.

## Build tier and refusals (code-graph, semantic-engine)

- [ ] Extend `detect_language` with the Bash rule, add `bash` to `LanguageArg` in [src/cli.rs](../../../src/cli.rs), and update the hard-coded `--language` texts in the detection errors.
- [ ] Test (`A workspace of shell scripts selects Bash`, `A manifest takes precedence over shell scripts`, `An empty workspace is still refused`, `Bash is selected explicitly`), and run the existing selection tests unchanged.
- [ ] Add `--structural` to `BuildArgs`, conflicting with `--python-indexer` and `--rust-analyzer`.
- [ ] Test: `--structural` with `--python-indexer` or `--rust-analyzer` is a usage error, and `--structural` on a Bash workspace builds the same index as without it.
- [ ] Add tier resolution and the `confirm` parameter to `run_build`, following `design.md` § The build resolves its tier before it opens the store for writing; build the prompt in [src/main.rs](../../../src/main.rs) from standard error and standard input, and pass the never-asking closure from `run_hooks_run`.
- [ ] Test (`The flag builds a structural index`): the stub engine's `analyze` is never called, and the build answer states that the flag requested the structural index and names no indexer failure.
- [ ] Test (`A headless build without the flag fails and names it`): the error is the indexer setup error, names `--structural`, and no store exists.
- [ ] Test (`A current semantic store does no work`): with a current semantic store, the stub engine's `analyze` is not called.
- [ ] Test (`A failing indexer leaves a semantic store unchanged`): with an engine whose analysis fails, the error names `--structural` and the store's bytes are unchanged.
- [ ] Test (`A yes at the prompt builds a structural index`): the prompt names `references` and `dependents` and says the impact answer lists touched declarations without their dependents, and the build answer names the indexer failure.
- [ ] Test (`A no at the prompt fails as a refusal`), and test that end of input at the prompt is a no.
- [ ] Test (`A hook build never asks`): a hook-caller build with a terminal state attached, a semantic store, and a failing engine reads no input, leaves the store unchanged, and reports the failure.
- [ ] Test (`A structural store stays structural without asking`): at a terminal and headless, `confirm` is not called and a structural index is produced.
- [ ] Test (`Structural consent does not cross workspaces or languages`): a store holding workspace A's structural Bash index does not authorize a structural Python build of workspace B; a store holding a structural Python index does not authorize a structural Rust build of the same root.
- [ ] Test (`A working indexer replaces a structural store`).
- [ ] Test: a hook-run build over a structural store produces a structural index, and over a Bash store a Bash index.
- [ ] Add the nullable `index_metadata.attempted_analyzer` column, write it in a structural build that follows an indexer failure, and read it in `recorded_python_indexer` when the recorded analyzer is a grammar.
- [ ] Test (`An upgrade retries the indexer the fallback attempted`), and test that a store built with `--structural` upgrades with scip-python; run the existing `A Python build selects its indexer` tests unchanged.
- [ ] Add the `--structural` remedy, and the statement of what a structural index does not answer, to the setup errors and to the analysis-failure message.
- [ ] Test (`Missing indexer tool refuses with guidance`, `Unresolvable environment refuses with guidance`): each message names `--structural`, states that a structural index does not answer references or dependents, and states that its impact answer lists touched declarations without their dependents.
- [ ] Test (each scenario of `A workspace file the indexer rejects is a typed refusal`): the message names the rejected file, its remedy, and `--structural`.

## Answers (code-navigation)

- [ ] Add `tier` to `Answer` and `Unavailable { reason }` to `Outcome` in [src/query/output.rs](../../../src/query/output.rs), with the tier read when `QueryEngine` opens the store.
- [ ] Test (`Each answer carries its tier`), for each of `get`, `trace`, `find`, `search`, `similar`, `clones`, and `impact` on a semantic and a structural index.
- [ ] Test (`A structural answer says so to a human`), and run the existing `Calibrated output contract` tests unchanged.
- [ ] Return `Unavailable` from `QueryEngine::trace` for `references`, `importers`, `implementers`, `implements`, and `tests`, and from `QueryEngine::dependents` for `dependents`, on a structural index after subject resolution; attach the unresolved-call group to `references`.
- [ ] Test (`Dependents of a Bash function are unavailable`) through the CLI `trace --relation dependents` path, and the same for `importers`, `tests`, and `implements`.
- [ ] Test (`Implementers on a structural Python index are unavailable`).
- [ ] Test (`A trace over a structural index answers or is unavailable`).
- [ ] Test (`References of a Bash function list the calls that name it`): the outcome is `unavailable`, the group lists the three calls, and the unconfirmed-use group's total is zero.
- [ ] Match the unresolved-call group's called name to a Bash subject's whole name.
- [ ] Test (`A Bash name containing the separator matches as a whole`).
- [ ] Test (`An ambiguous subject on a structural index is still a candidate set`), for `dependents` and `references`.
- [ ] Render `Unavailable` and the structural header in [src/render.rs](../../../src/render.rs).
- [ ] Test (`The human rendering does not claim an empty relation`).
- [ ] Merge exact display-name matches into the `::` and `.` branches of [src/query/resolve.rs](../../../src/query/resolve.rs), and make the qualified-suffix match ignore a trailing `#n`.
- [ ] Test (`A Bash name containing the separator resolves by its whole name`): `get lib::log` returns both candidates; `get log.sh` returns the function `log.sh`; the canonical identity of each still resolves uniquely; run the existing resolution tests unchanged.
- [ ] Test (`A Bash function is retrieved with its body`, `A file's functions are its contents`, `A name fragment finds structural declarations`).
- [ ] Test (`Similar rows carry no relation labels`).
- [ ] Add `dependents_outcome` to the impact report in [src/query/impact.rs](../../../src/query/impact.rs), and make the renderer skip the reach, horizon, and empty-dependents lines when it is set.
- [ ] Test (`A changed Bash function is a seed with unavailable dependents`, `The human rendering claims no reach`, `A comment-only change is still a definite none`).
- [ ] Classify impact's Bash changes by `source_files` and post-change content, following `design.md` § Bash discovery reads a bounded first line.
- [ ] Test (`A deleted extensionless script is a seed`), and test a renamed script, a script whose shebang changed in the diff, and a revision-range diff.
- [ ] Test: an approximate revision-range `impact` whose range deletes, or re-shebangs to `zsh`, an extensionless Bash script that the current index does not hold reports the script's pre-change regions as unmappable, not a definite none.
- [ ] Test (`A structural index derives seeds without reach`).
- [ ] Add the structural exception to the override disclosure of `references`.
- [ ] Test (`A structural references answer carries no override disclosure`), and run the existing override-disclosure tests unchanged.
- [ ] Carry the recorded language and tier in `ImpactRequest`, and add `--language` and `--structural` to `recovery_recipe`.
- [ ] Test (`The recovery procedure rebuilds the same tier and language`).
- [ ] Add `syntax_error_files` to negative structural answers and their rendering.
- [ ] Test (`A lost declaration is not a confident absence`, `A clean index adds no disclosure`), for `find`, `get`, and an empty `contains`.
- [ ] Add the optional `reason` to `Absent`, and set it in reference resolution when the reference has the other tier's shape under the index's workspace identity.
- [ ] Test (`A structural identity against a semantic index`, `A semantic identity against a structural index`).
- [ ] Add `tier`, `unanswered_relations`, and the files with syntax errors to the status report.
- [ ] Test (`A structural index reports its tier and missing relations`, `A structural index names its files with syntax errors`, `A semantic index reports its tier`), in JSON and in the human rendering.
- [ ] Test (`A structural index answers unavailable, not empty`), and run the `Empty answers of reference-derived relations claim no proof` tests unchanged.

## Exit code and manifest (command-surface)

- [ ] Add `ExitCode::Unavailable` (7) in [src/exit.rs](../../../src/exit.rs), and make command handlers return their exit category with their output, so that `main` prints the output and exits with that category.
- [ ] Test (`An unavailable answer is distinct and still written`): `trace --relation dependents --json` on a structural index exits 7 and standard output parses as the `unavailable` answer; a typed-empty answer still exits 0.
- [ ] Test (`A structural impact answer with seeds exits with the unavailable code`, `A structural impact answer that touches nothing is success`).
- [ ] Test (`A headless build with an unavailable indexer does not ask`): the build runs with standard input not a terminal and completes without reading it.
- [ ] Add `tier` to the manifest's built index state.
- [ ] Test (`Index state names a structural tier`).
- [ ] Raise `SURFACE_VERSION` in [src/manifest.rs](../../../src/manifest.rs) by one from the base branch's value, regenerate `tests/fixtures/surface_manifest.snapshot.json`, and run `tests/surface_manifest.rs`.

## MCP (mcp-surface)

- [ ] Route exit code 7 to the answer path in `mcp/src/c10r_mcp/errors.py`.
- [ ] Test (`An unavailable answer is a result`): a `trace` tool call over `references` on a structural index returns the answer, with its group, and `is_error` is false; an `impact` tool call with a seed returns the answer with its seeds the same way.
- [ ] Add `BASH` to `Language` in `mcp/src/c10r_mcp/params.py`, and the `structural` parameter to the `build` tool in `mcp/src/c10r_mcp/server.py`.
- [ ] Update the server instructions to name Bash as indexed structurally, and the language list asserted in `mcp/tests/test_tools.py`.
- [ ] Raise the MCP `SURFACE_VERSION` mirror in `mcp/src/c10r_mcp/surface.py` to the binary's new value, and run the MCP test suite including `test_conformance.py`.

## Documentation

- [ ] Document in `README.md`: the structural tier, Bash support, `--structural` and the prompt, the relations a structural index does not answer and what its `impact` answer gives, the `unavailable` outcome, the syntax-error disclosure, and exit code 7.
- [ ] Document the known tree-sitter-bash limitations from `design.md` § Risks in the README's Bash section.

## Gates

- [ ] Byte-identical gate: build the five repositories with the branch binary and compare each semantic store with the baseline from Setup, ignoring only the schema version and the new `source_files` columns.
- [ ] Dogfood: build nvm, bats-core, and dokku from `~/.cache/silent-cartographer/bash-corpus/` as Bash; record build time, declaration and call counts, the files with syntax errors, and the answers of `find`, `get`, `trace --relation references`, and `trace --relation dependents` for one function in each, in `._scratch/research/retained-utility/dogfood.md`.
- [ ] Dogfood: build c10r itself and Flask with `--structural`; record build time and declaration counts against their semantic builds, and the answers of `trace --relation contains` and `trace --relation dependents` for one type in each.
- [ ] Run the full Rust and MCP test suites and every hook in `.pre-commit-config.yaml` on the changed files.
