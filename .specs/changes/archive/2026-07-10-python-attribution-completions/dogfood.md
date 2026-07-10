# Dogfood: python-attribution-completions

Five standing targets rebuilt with the schema-v9 binary (this change's working tree; suite 235 green, clippy/fmt clean).
Baselines: httpx2/Flask/ripgrep/fd from `.specs/changes/archive/2026-07-09-module-scope-rules/dogfood.md`; self is not byte-comparable (the repo carries this change's own diff).

Pins and tools: httpx2 `9b7ee8e` (v2.5.0), Flask `22d9247` (3.1.3), ripgrep `48b0c795`, fd `5a5852e1`, self = working tree on `e12d932` + this change; scip-python 0.6.6; Rust toolchain and rust-analyzer pinned via `rust-toolchain.toml`.
All Python builds through the adapter's own invocation (`--project-name <workspace> --project-version 0`); every store written fresh under schema v9 (replace-on-build verified by the v8→v9 open).

## Final accounting (all five targets)

| target | aligned | exact | module_name | self_name | module_marker | import_alias | other rules | text_mismatch | dup_ambiguous | syntax_only | aligned % |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| httpx2 | 35249 | 30770 | 4354 | 5 | 119 | 1 | 0 | 1102 | 0 | 0 | 97.0% |
| Flask | 16320 | 14414 | 1617 | 188 | 83 | 18 | 0 | 68 | 6 | 2 | 99.5% |
| ripgrep | 49995 | 45427 | 0 | 0 | 0 | 39 | 4529 | 1054 | 72 | 111 | 97.8% |
| fd | 7446 | 6702 | 0 | 0 | 0 | 0 | 744 | 88 | 0 | 20 | 98.8% |
| self | 19581 | 17628 | 0 | 0 | 0 | 2 | 1951 | 345 | 0 | 4 | 98.3% |

Aligned % = aligned / (aligned + text_mismatch + duplicate_ambiguous), matching prior records.
Conservation holds on every target: each aligned delta equals its refusal-bucket delta exactly (httpx2 +254/−254, Flask +369/−369, ripgrep +39/−39, fd ±0).

## Per-family results

### httpx2 (vs 34995 / 1356 / 0)

- `module_marker` **119 — exactly the predicted count**; the store's discrepancy table holds zero remaining zero-width rows.
- `module_name` +128: the qualifying subset of the prefix-token family, recovered through the enclosing dotted construct.
  The 74 leftovers (`expected_name = __init__`) were investigated one by one at the raw index: each is a prefix token whose occurrence carries a **sibling module's symbol** — e.g. the `h2` token inside `h2.connection.H2Connection(...)` (line 49) and inside three `h2.events.…` annotations (line 64) all carry `` `h2.config`/__init__: ``.
  The construct spells a different module than the symbol claims, so the rule refuses — wrong SCIP data staying refused, the guard's exact purpose.
  This is a second scip-python misattribution shape, recorded in the issue draft below.
- `self_name` 5, `import_alias` 1, plus 1 binding-site narrowing acceptance under `exact`.
- `MARK_GEN` residual **248 — verified evidence-unreachable**: the raw index carries `pytest/__init__:` and `` `_pytest.mark.structures`/MARK_GEN. `` but **no `pytest`/`mark` symbol and no relationship binding them**; the `mark = MARK_GEN` assignment lives in unindexed third-party source.
  A rule would need the semantic index to emit the binding (a `mark` symbol under the `pytest` module carrying a relationship to `MARK_GEN`); none exists, so the family is documented residual — follow-up scope only if scip-python ever emits such evidence.
- Star-re-export misattributions (Headers 458, Client 78, HTTPError 74, URL 48, …) unchanged and refused, as scoped.

### Flask (vs 15951 / 437 / 6)

- `self_name` **188 — exactly the predicted count**; `module_marker` 83 — exactly as predicted.
- `import_alias` 18 vs the harvest's ≈11: benign undercount — the harvest counted only the `Environment` family; the rule generalizes to every alias family (`Environment`×5, `Response`, `Request`, `redirect`, `abort`, `Blueprint`), all 18 in `src/flask/`, each verified to have a same-document `from … import … as …` binding.
- `exact` +52: the binding-site occurrences themselves (spans covering the whole `target as alias` text), aligned through target-token narrowing.
- `module_name` +28: the qualifying prefix-token subset (~33 predicted).
- Residual 68 ≈ the `os.path`-style representative-symbol misattributions (~65) — refused, as scoped.
  `duplicate_ambiguous` 6 and `syntax_only` 2 unchanged.

### ripgrep (vs 49956 / 1093 / 72)

- `import_alias` +39, all released from `text_mismatch`; every other bucket byte-identical.
- `duplicate_ambiguous` 72 **unchanged — prediction miss, investigated**: the duplicate-identity harvest's "32 facade `pub use` re-export aliases" turn out not to be document-local alias bindings at all.
  The 32 rows sit in `crates/searcher/src/searcher/glue.rs` with `expected_name = SHERLOCK` and no source span/text (macro-expansion fallout), and that document declares no `use … as` binding — there is nothing for the identity gate to verify, so nothing converts.
  The gate behaved as designed; the old family label was a mischaracterization.

### fd (vs 7446 / 88 / 0)

Byte-identical to baseline, including `import_alias` 0 — exactly as predicted.

### self

Not byte-comparable (the repo grew by module-scope-rules' sync and this change).
Invariants hold: `semantic_only` 0, `duplicate_ambiguous` 0, Python-only buckets 0.
`import_alias` 2: the `SymbolClass as Class` family in `tests/semantic_engine.rs`, both verified at source.
The 27 `scip_types` refusals (expected `scip`) stay refused **on identity grounds**: rust-analyzer attributes those alias tokens to the crate `scip`, while the document's binding (`use scip::types as scip_types;`) targets the module `scip::types` — the "alias bound to a different symbol" clause firing on real data.
Calibrated caution against a coarse tool attribution, not a missed recovery; recorded residual.
The 2026-07-04 "~35 use-as refusals" figure decomposes into this refused family plus the recovered `Class` family plus rows since changed by repo growth.

## Disclosure gates (scip-check, fresh raw indexes)

| target | raw multi-def symbols | disclosed twin groups |
| --- | --- | --- |
| httpx2 | 16 | 16 |
| Flask | 31 | 31 |
| ripgrep | 12 | 12 |
| fd | 1 | 1 |
| self | 10 | 10 |

All five match; no mismatch, no stop.

## Spot checks (read at source)

- `self_name`: `bp = Blueprint("tasks", __name__, …)` in `examples/celery/src/task_app/views.py` → `examples.celery.src.task_app.views` (its own module); `app = Flask(__name__)` in `…/task_app/__init__.py` → `examples.celery.src.task_app`. Both correct.
- `import_alias`: `BaseEnvironment` in `class Environment(BaseEnvironment)` → jinja2 `Environment` via `from jinja2 import Environment as BaseEnvironment`; `RequestBase` in `class Request(RequestBase)` → werkzeug `Request`; `_wz_abort(code, …)` → werkzeug `abort`. All three bindings sit in the same document; symbol identity verified at each binding site.
- Dotted completion: the `h2.config` usage lines `CONFIG = h2.config.H2Configuration(…)` in both `_async/http2.py` and `_sync/http2.py` — raw occurrence spans cover only the 2-byte `h2` prefix token; the persisted spans are the 9-byte `h2.config` construct that spells the module.

## Session notes

- A first query of the httpx2 residual hit `.c10r/index.sqlite` — a stale store file from a pre-rename session sitting next to the live `.c10r/index.db` — and briefly looked like a supersession defect (old counts, marker rows present).
  The live store was verified clean (1102 rows, zero marker rows).
  Stale `index.sqlite` files in scratchpad clones are leftovers, not build outputs.
- `import a.b as c`-form binding sites did not surface in any target's recoveries this round (Flask/httpx2 alias bindings are all `from … import … as`); the form is covered by fixture tests only.

## Upstream issue draft (for the user to file against sourcegraph/scip-python — not posted anywhere)

> **Title:** Star re-exports: all references through the package resolve to a single representative symbol from the source module
>
> **Version:** scip-python 0.6.6 (`scip-python index --project-name starrepro --project-version 0 .`)
>
> **Minimal repro:**
>
> ```text
> pkg/__init__.py:   from ._mod import *
> pkg/_mod.py:       class Headers: ...
>                    class Response: ...
> consumer.py:       import pkg
>                    r = pkg.Response()
>                    h = pkg.Headers()
> pyproject.toml:    [project] name/version present
> ```
>
> **Observed:** in `consumer.py`, the reference occurrences at BOTH `pkg.Response` (range `[2, 8, 16]`) and `pkg.Headers` (range `[3, 8, 15]`) carry the same symbol `` scip-python python starrepro 0 `pkg._mod`/Response# ``; `` `pkg._mod`/Headers# `` has zero occurrences in `consumer.py`.
> One symbol from the star-exported module is chosen as a representative for every name imported through the star.
>
> **Expected:** each reference resolves to its own class's symbol (`pkg.Headers` → `` `pkg._mod`/Headers# ``).
>
> **Scale:** on httpx2 v2.5.0 (an all-star `httpx2/__init__.py`), 787 reference occurrences across the test suite carry a representative symbol that does not match the referenced name (`Headers` claimed by tokens spelling `Response`, `Client`, `HTTPError`, `URL`, …).
>
> **Possibly related second shape:** in dotted module expressions, the leading prefix token sometimes carries a *sibling* module's symbol: with `import h2.config`, `import h2.connection`, and `import h2.events` in scope, the `h2` token inside `h2.connection.H2Connection(...)` and inside `h2.events.…` annotations is emitted with the symbol `` `h2.config`/__init__: `` (74 instances on httpx2's vendored httpcore2).
