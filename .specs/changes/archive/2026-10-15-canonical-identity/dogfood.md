# Dogfood: canonical-identity

Run 2026-10-13 with a binary built from this change (`target/release/c10r`, schema 19).

## rust-libbin sample

A copy of `._scratch/research/identity-forms/samples/rust-libbin` (library and binary `tool` each declare `mod util { Config; load() }`; `tests/a.rs` and `tests/b.rs` share `tests/common/mod.rs`), built with rust-analyzer 1.95.0 into `.c10r/dev.db`.

```text
$ c10r get libbin::util::Config
rust-libbin::libbin::util::Config  Config  [type]
  src/lib.rs:3:5

$ c10r trace libbin::util::load --relation references
1 result
  rust-libbin::libbin::util::load  load  at src/bin/tool.rs:14:29 in rust-libbin::libbin+bin.tool::main

$ c10r get util::Config
ambiguous: 2 candidates
  rust-libbin::libbin+bin.tool::util::Config  Config  [type]
  rust-libbin::libbin::util::Config  Config  [type]

$ c10r find fixture
1 result
  rust-libbin::libbin+shared::common::fixture  fixture  at tests/common/mod.rs:1:1
```

The build reported `duplicate_ambiguous=0`.
The binary's call `libbin::util::load()` is a reference of the library's `load`.

## Flask

The dogfood clone at `~/.cache/silent-cartographer/flask`, built with scip-python 0.6.6 into a store outside the clone.

```text
built: aligned=16710 (exact=14618 ... module_name=1802 self_name=188 module_marker=83 import_alias=19 ...) text_mismatch=109 semantic_only=0 duplicate_ambiguous=6 syntax_only=2

$ c10r get flask.app.Flask
flask::flask::flask::app::Flask  Flask  [type]
  src/flask/app.py:81:1

$ c10r get flask.globals.current_app
flask::flask::flask::globals::current_app  current_app  [constant]
  src/flask/globals.py:28:1

$ c10r get flask.globals
ambiguous: 2 candidates
  flask::flask::flask::globals  globals  [module]
  flask::flask::globals  globals  [module] (external)
```

The annotated module global `current_app` carries its name once.

A Python module now resolves by its dotted name.
In the prior form every module's display name was `__init__`, so a dotted module name resolved to no symbol.
In 18 cases an external module symbol that scip-python records under the project's package (for example `flask::flask::globals`) has the same dotted name as an in-workspace module, so the answer is a candidate set.
34 external module symbols have a name that ends an in-workspace module's name (for example `db` and `examples.tutorial.flaskr.db`); these make a shorter dotted name such as `flaskr.db` ambiguous.

A build over a copy of the clone, with the clone's own environment, names the copy's modules by path (`src::flask::globals`), because the environment's editable install of `flask` points at the clone's `src/`.
The status line reports the environment as changed in that case.

## Flask relative imports

The same clone rebuilt with scip-python 0.6.6 into `/tmp/claude-501/ci-dogfood/flask-rel.db`, once with the relative-import step and once without it (`flask-before.db`).
The build line is identical in both runs (`aligned=16710`, `module_name=1802`).

The importers of `flask.globals` rose from 6 to 18.
Without the step only the test modules were listed, because they import it by absolute path.
With the step the package modules that import it with `from .globals import …` are listed too.

```text
$ c10r trace flask::flask::flask::globals --relation importers
18 results
  flask::flask::flask  flask  [module] at src/flask/__init__.py:1:1
  flask::flask::flask::app  app  [module] at src/flask/app.py:1:1
  flask::flask::flask::blueprints  blueprints  [module] at src/flask/blueprints.py:1:1
  flask::flask::flask::cli  cli  [module] at src/flask/cli.py:1:1
  flask::flask::flask::ctx  ctx  [module] at src/flask/ctx.py:1:1
  flask::flask::flask::debughelpers  debughelpers  [module] at src/flask/debughelpers.py:1:1
  flask::flask::flask::helpers  helpers  [module] at src/flask/helpers.py:1:1
  flask::flask::flask::json  json  [module] at src/flask/json/__init__.py:1:1
  flask::flask::flask::logging  logging  [module] at src/flask/logging.py:1:1
  flask::flask::flask::templating  templating  [module] at src/flask/templating.py:1:1
  flask::flask::flask::views  views  [module] at src/flask/views.py:1:1
  flask::flask::flask::wrappers  wrappers  [module] at src/flask/wrappers.py:1:1
  flask::flask::tests::conftest  conftest  [module] at tests/conftest.py:1:1
  flask::flask::tests::test_appctx  test_appctx  [module] at tests/test_appctx.py:1:1
  flask::flask::tests::test_basic  test_basic  [module] at tests/test_basic.py:1:1
  flask::flask::tests::test_reqctx  test_reqctx  [module] at tests/test_reqctx.py:1:1
  flask::flask::tests::test_session_interface  test_session_interface  [module] at tests/test_session_interface.py:1:1
  flask::flask::tests::test_testing  test_testing  [module] at tests/test_testing.py:1:1
```

External module symbols that share their dotted name with an in-workspace module: 18 without the step, 0 with it.
The 18 without the step are `flask.app`, `flask.blueprints`, `flask.cli`, `flask.config`, `flask.ctx`, `flask.debughelpers`, `flask.globals`, `flask.helpers`, `flask.json`, `flask.sansio.app`, `flask.sansio.blueprints`, `flask.sansio.scaffold`, `flask.sessions`, `flask.signals`, `flask.templating`, `flask.testing`, `flask.views`, and `flask.wrappers`.
Resolving each of the 83 in-workspace modules by its dotted name gives the same 18 as ambiguous answers without the step and none with it.
External module symbols whose name ends an in-workspace module's name fell from 34 to 10.
The 10 are absolute imports of example and test apps outside the configured source roots (`flaskr`, `flaskr.db`, `blueprintapp`, `blueprintapp.apps.admin`, `blueprintapp.apps.frontend`, `cliapp.app`, `hello`, `js_example`, `js_example.views`, `task_app`).
External module symbols fell from 122 to 98; the remaining ones are third-party packages and the absolute imports outside the source roots (`flaskr.db`).
