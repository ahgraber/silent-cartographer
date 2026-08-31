# Dogfood evidence: undecodable-sources

Reproducer: [sphinx-doc/sphinx](https://github.com/sphinx-doc/sphinx), commit `e44a40e` (default branch, cloned 2026-08-28).

`tests/roots/test-pycode/cp_1251_coded.py` is a genuine CP-1251-encoded fixture Sphinx ships to test its own encoding handling — a two-line module declaring a single variable, `X`.
Its bytes are not valid UTF-8 — confirmed independently with Python's own decoder:

```text
'utf-8' codec can't decode byte 0xd5 in position 47: invalid continuation byte
```

Before this change, source discovery aborted the entire build on this file with no other files touched.
After this change, the build completes, names the excluded file, the excluded file's own symbol (`X`) is not queryable, and the rest of the workspace — 775 discovered Python files — is queryable.

## Build

```text
$ c10r --db <db> build sphinx --language python
exit: 0
```

Standard error carries the exclusion diagnostic for the one undecodable file, alongside `scip-python`'s own unrelated diagnostics (a `pyproject.toml` parse warning and a package-lookup notice on some runs, both pre-existing and out of this change's scope):

```text
warning: tests/roots/test-pycode/cp_1251_coded.py is not valid UTF-8; excluded from the build
```

Standard output carries the build's answer:

```text
built: aligned=146593 (exact=134368 crate_root=0 operator_desugar=0 module_span=0 self_keyword=0 module_name=11271 self_name=124 module_marker=773 import_alias=57 range_literal=0 use_list_self=0 super_keyword=0) text_mismatch=713 semantic_only=3 duplicate_ambiguous=450 syntax_only=6
```

The resulting store holds 28,421 persisted symbols (27,113 `in_workspace`, 1,308 `external`; `select class, count(*) from symbols group by class`).

## The excluded file's own symbol is absent, not a hollow `Found`

```text
$ c10r --db <db> --workspace sphinx find "cp_1251_coded" --json
{"outcome": {"outcome": "absent"}}

$ c10r --db <db> --workspace sphinx get "tests.roots.test-pycode.cp_1251_coded.X" --json
{"outcome": {"outcome": "absent"}}
```

Both report `absent`, not a `Found` result with empty content — closing the gap an earlier version of this evidence and its paired test left open (the join persisted a row for a symbol found only in an excluded document; `get`/`find` returned that row as `Found` with every content field empty).

## Queryable afterward

```text
$ c10r --db <db> --workspace sphinx find "make_id" --json
```

returns real symbols from `tests/test_util/test_util_nodes.py` (`test_make_id`, `test_make_id_already_registered`, `test_make_id_sequential`), confirming the rest of the workspace answers normally with the excluded file absent from the index.

## Build and currency check agree

```text
$ c10r --db <db> status --workspace sphinx
```

carries the identical diagnostic (`tests/roots/test-pycode/cp_1251_coded.py is not valid UTF-8; excluded from the build`), confirming the currency check's `collect_python_sources` call excludes the same file the build did.

## Full suite (no-regression gate)

741 passed, 1 failed, 2 ignored (`cargo test`, 2026-08-31).

The one failure, `python_live_tool_matches_committed_fixture_shape` (`tests/semantic_engine.rs`), is excluded from this change's pass/fail claim: neither that test file nor `src/semantic/python_adapter.rs` appears in this diff (`git diff --stat HEAD` against both is empty), and the failure reproduces independent of any change here — `python3 -m venv` fails because `ensurepip`'s pip-bootstrap step stages a `cacert.pem` temp file, and this sandbox denies writes to `**/*.pem` paths.
Reproduced directly outside the test harness with the same `PermissionError: [Errno 1] Operation not permitted` on a `*cacert.pem` path.
The 2 ignored tests are the pre-existing fixture-regenerator tests, unrelated to this change.
