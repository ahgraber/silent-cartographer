# Dogfood: python-adapter on pydantic/httpx2

Recorded 2026-07-08.
Target: `https://github.com/pydantic/httpx2` pinned at tag `v2.5.0` (commit `9b7ee8ec8c06e8f35a9e7fb437c6fb2f552108bf`).
Tooling: scip-python `0.6.6` (Node v24.14.0), Python 3.14.4.
Environment: `uv sync` venv at the workspace root (`.venv`, 112 dist-info distributions at build time).
Repo shape: a uv workspace root (`package = false`, no `[project]` table) with two member packages, `src/httpx2` and `src/httpcore2` — the Python analog of a multi-crate Rust workspace, and the shape that motivated the unconditional `--project-name`/`--project-version` invocation (see design/P2 below).

## Build accounting

One `c10r build` (manifest auto-detected Python; env resolved from `.venv`):

| bucket | count |
| --- | --- |
| aligned (all rule `exact`) | 30700 |
| text_mismatch | 5582 |
| semantic_only | 0 |
| duplicate_ambiguous | 69 |
| syntax_only | 0 |

Alignment rate 30700 / 36351 ≈ 84.5% under the launch posture (default name-token rule only; no Python kind-scoped rules).
A rebuild reproduces the identical accounting line, confirming whole-build supersession on a Python store.

Store totals: 6751 symbols, 30700 occurrences, 22218 edges, 119 documents.

## Duplicated-group disclosure vs raw ground truth (gate)

`status --duplicates` disclosed **16 groups**; scip-check over the build's raw SCIP index found **16 non-local symbols with >1 definition occurrence** — exact match, memberships identical.
The population is all same-document twins, two families:

- 12 property getter/setter `self` parameters (`@property` + `@x.setter` share one descriptor, e.g. `BaseClient#auth().(self)`).
- 4 test-file class attributes defined at class level and reassigned (`ErrorOnRequestTooLargeStream#count`, `Transport#events`, in sync/async test pairs).

All 69 group references refused as `duplicate_ambiguous` — correct at launch posture: every twin pair is same-document, so `defining_document` cannot discriminate, `module_chain` is inert for Python, and Python supplies no `library_roots`.
This is the Python instance of the §11 "same-document twins need scope-grained locality" follow-up; no new machinery is warranted until that change.

## Hand ground truth

**Definition locations (5/5 byte-exact via `get --detail location`):**
`httpx2._client.Client`, `httpx2._models.Response`, `httpx2._urls.URL`, `httpcore2._sync.connection_pool.ConnectionPool` (classes; spans start at `class X`), `httpx2._models.Response.json` (method; span starts at `def json`).
Each span's bytes were compared against the source file directly.

**Complete reference sets (2 small symbols vs manual read):**

- `httpx2._exceptions.CookieConflict`: trace returned exactly the 2 identifier references (`_models.py:24` import, `_models.py:1138` raise site); the two `__all__` string mentions and one docstring mention are correctly not aligned references.
- `httpx2._decoders.LineDecoder`: exactly the 5 identifier references (`_models.py` import + 2 instantiations, `tests/test_benchmark.py` import + instantiation).

**Declared bases vs `type_hierarchy` (2 modules, complete):**

- `httpx2/_exceptions.py`: 29 class declarations, 29 edges, each matching its declared base.
- `httpcore2/_exceptions.py`: 15 class declarations, 15 edges, each matching.
- External bases resolve to persisted stdlib symbols (`builtins.Exception`, `builtins.UserWarning`, `builtins.RuntimeError`) — the Python analog of Rust external-trait edges.

**Direct dependents of a widely-used symbol (`httpx2._urls.URL`):**
`trace --relation dependents --depth 1` returned 159 dependents (152 `uses` + 7 `imports`); the occurrence-derived cross-check (distinct enclosing declarations of aligned references + distinct documents of module-scope references) gives 152 + 7 = 159.

## Refusal families (filed in brainstorm §11 as rule-family candidates)

The 5582 `text_mismatch` rows decompose exactly:

1. **Module reference named by its own name, descriptor terminal `__init__` — 4428 (79%).**
   scip-python names every module symbol by an `__init__` terminal (`` `httpx2._models`/__init__: ``), so every `import httpx2` / `typing.…` token refuses under the default rule (expected `__init__`, found the module's name).
   This is the Python analog of Rust's crate-root rule and the top rule-family candidate: accept a module reference when the source token equals the terminal segment of the module's dotted namespace name.
2. **Star-re-export misattribution by scip-python — 787 (14%).**
   `httpx2/__init__.py` is all `from ._x import *`; scip-python resolves attribute access through the package namespace to a single representative symbol per source module (every `httpx2.Response`/`Request`/`Cookies` token carries the `Headers` symbol; `httpx2.QueryParams` carries `URL`; `httpx2.AsyncClient` carries `Client`).
   These are tool-level wrong resolutions and the text-equality guard refused every one — a calibration win, not a rule candidate.
   Do not add a rule here; if scip-python fixes its emission the family disappears.
3. **External re-export alias — 248 (4%).**
   All `pytest.mark` sites: the runtime name `mark` is an alias of `_pytest`'s `MARK_GEN`; expected `MARK_GEN`, found `mark`.
   Same shape as ripgrep's facade re-exports (`grep::matcher`): an alias-aware rule family would need re-export evidence; deferred with the Rust instance.
4. **Zero-width module definition markers — 119 (2%).**
   One per document: scip-python's module definition occurrence is a zero-width range at the document origin, which cannot name-token-align.
   Refused and counted by design; the document→module mapping for `imports` edges is taken from the extracted index (identity bookkeeping), so nothing is lost.

Notably absent: the feared decorator/property/`self`/`cls` families did not materialize as refusals — `self`/`cls` occurrences are parameter symbols that align at their own tokens, and decorated definitions align at their `def`/`class` name tokens.

## Environment staleness end-to-end

- `uv pip install tabulate` (verified absent before) into `.venv` → `status` reports `stale_environment`.
- Rebuild → `fresh`, identical accounting.
- Caveat discovered while testing: the first attempt used `six`, which was already a transitive dev dependency — a no-op install correctly stayed `fresh`.

## Defects found and fixed during this change's apply (review loop)

- **P1**: the explicit-environment branch was unreachable — no `--environment` CLI flag existed while the refusal guidance told users to pass one; fixed by wiring `--environment` through `run_build`.
- **P2**: `scip-python index` invoked bare is unsound — probed on 0.6.6: fatal TypeError on non-git dirs without a `[project]` table; package name degenerates to literal `.` on git repos without one (the httpx2 shape).
  Fixed: the adapter always passes `--project-name <workspace identity> --project-version 0` (identity reads package name only, so the constant version is safe).
