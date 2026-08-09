# Notes: ranked-impact — dogfood gates

## Store provenance for the gates

The flask, httpx2, and ripgrep clone indexes predate the store-ownership marker (schema v12, unstamped), and scip-python is not installed on this host, so the Python stores cannot be rebuilt here.
For the gates, each v12 store was copied to `.c10r/index-v13-bench.db` and the copy migrated by hand: `ALTER TABLE index_metadata ADD COLUMN workspace_root TEXT` (the only v12→v13 DDL delta), then the v13 `user_version` and ownership stamps.
The originals are untouched; the graph data (symbols, edges, occurrences) is byte-identical to what a rebuild of the same sources produced at v12. fd's store was already v13; it was copied unmodified for symmetry.
All four bench stores read `freshness: fresh` under the release binary of this change.

## Pre-registered quality check

Protocol: subjects were chosen and their distance-1 layers enumerated using **unranked** queries only; the predictions below were committed before the first ranked query ran.
Pass criterion per subject: the predicted symbol is the first detailed row of the ranked answer (distance-1 layer, most-important-first).

| Clone   | Subject                                   | Predicted top-of-layer dependent                     | Reasoning                                                                                                                                                                                                |
| ------- | ----------------------------------------- | ---------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| httpx2  | `httpx2::httpx2::httpx2._client::Client`  | `httpx2::httpx2::httpx2._api::request`               | Every top-level convenience verb (`get`, `post`, …) delegates to `_api.request`, so it is the most depended-upon member of the layer; the test functions sharing the layer have no dependents at all.    |
| flask   | `flask::flask::flask.app::Flask`          | `flask::flask::examples.tutorial.flaskr::create_app` | The tutorial factory is imported and called by the flaskr test suite's fixtures and factory tests; the sibling layer members (session-interface methods, CLI locators) each have one or two callers.     |
| ripgrep | `ripgrep::ripgrep::flags::hiargs::HiArgs` | `ripgrep::ripgrep::crate#1`                          | The crate root module is what the rest of the binary imports through, so import edges concentrate on it; the sibling function members (`run`, `search`, `files`) are each called from one or two places. |
| fd      | `fd::fd-find::config::Config`             | `fd::fd-find::crate#0`                               | Same reasoning as ripgrep: the crate root accumulates the crate's import edges; the sibling printers and walkers have few callers each.                                                                  |

Results are recorded below after the ranked runs; a failed prediction feeds the edge-kind-weighting follow-up decision rather than any silent retuning.

## Quality check results

Recorded 2026-08-09, release build, first ranked runs after pre-registration.

| Clone   | Predicted top                          | Actual ranked top of the distance-1 layer                                                                                                           | Result |
| ------- | -------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| httpx2  | `httpx2._api::request`                 | `httpx2._main::main`, then `_main::__init__`; the prediction landed third                                                                           | FAIL   |
| flask   | `examples.tutorial.flaskr::create_app` | `flask::__init__` (the package root module), then `templating::_render`, `testing::FlaskClient`, `templating::_stream`; the prediction landed fifth | FAIL   |
| ripgrep | `ripgrep::crate#1`                     | `ripgrep::crate#1`                                                                                                                                  | PASS   |
| fd      | `fd-find::crate#0`                     | `fd-find::crate#0`                                                                                                                                  | PASS   |

2/4 predictions pass.
Both failures have the same shape: module-grade symbols (a package `__init__`, a CLI module's members) accumulate import-driven rank and top the layer ahead of the predicted function-grade symbols — the "import noise dominates layers" risk `design.md` pre-registered.
The flask result is arguably a better answer than the prediction (`flask/__init__` re-exports `Flask`, so it is the most load-bearing dependent by any reading); the httpx2 result is more debatable (`_main::main` over `_api::request`).
Per the task's protocol this feeds the edge-kind-weighting follow-up decision (down-weighting `imports` relative to `uses`); no retuning was performed in this change.

## Benchmark: ranked vs unranked

Recorded 2026-08-09, release build, best of 7 runs per invocation (whole-process wall clock, which includes source-hash freshness scanning in both arms).
Traces are hub-symbol subjects at `--depth 2 --limit 0`; impact is a 3-symbol working-tree diff (probe lines in ripgrep's `main`, `run`, and `search`) at `--depth 2 --limit 0`, file restored after.

| Query                          | Ranked   | Unranked | Delta    |
| ------------------------------ | -------- | -------- | -------- |
| httpx2 trace `_client::Client` | 94.1 ms  | 71.2 ms  | +22.9 ms |
| flask trace `flask.app::Flask` | 81.7 ms  | 70.3 ms  | +11.4 ms |
| ripgrep trace `hiargs::HiArgs` | 104.9 ms | 75.3 ms  | +29.6 ms |
| fd trace `config::Config`      | 62.3 ms  | 58.4 ms  | +3.9 ms  |
| ripgrep impact (3-symbol diff) | 367.8 ms | 336.5 ms | +31.3 ms |

**Gate: PASS everywhere** — the largest ranked-vs-unranked delta is 31.3 ms, well inside the 100 ms budget; no persisted-rank fallback is needed.
