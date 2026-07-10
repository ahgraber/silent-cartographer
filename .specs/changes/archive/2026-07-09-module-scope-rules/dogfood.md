# Dogfood: module-scope-rules

Recorded 2026-07-09.
Tooling: scip-python `0.6.6` (Node v24.14.0), rust-analyzer per pinned toolchain, Python 3.14.4.
Targets and pins: httpx2 `v2.5.0` (`9b7ee8e`), ripgrep `48b0c79`, fd `5a5852e`, self (working tree of this change), Flask `3.1.3` (`22d9247`, probed and indexed cleanly on the first attempt — no substitution needed).
Baselines: httpx2 from `.specs/changes/archive/2026-07-09-python-adapter/dogfood.md`; ripgrep/fd/self from `.specs/changes/archive/2026-07-07-duplicate-identity/dogfood.md`.

## The first rebuild's deviations and the resulting amendments

The first httpx2 rebuild deviated from the stated expectations in four ways; per the design's expected-delta discipline each was investigated before recording, and three became fixes/amendments (all user-approved, spec + design amended in place):

- **D1 (defect):** the `build` output line omitted the `module_name` bucket (buckets summed to 30,777 under a printed 34,577). Fixed.
- **D2 (defect):** the Rust module-span rule accepted 8 Python occurrences — zero-width module markers on *empty* `__init__.py` files, where a zero-width span vacuously equals the whole document. Kind-scoped rules are now language-gated in dispatch; the 8 correctly refuse.
- **D3 (amendment):** the residual module family decomposed into relative-import spans (`._api`, 253), full dotted-path spans (`anyio.abc`, 106), prefix-token spans (`h2` carrying `h2.connection` — a range quirk, 269), and zero-width markers (111).
  The first two carry the same clean evidence as the bare terminal; the rule was amended to the trailing-component-run condition.
  Prefix tokens and markers stay refused.
- **D4 (wrong prediction, correct behavior):** `duplicate_ambiguous` fell to 0, not the predicted ≈8 — the four "class-attribute" twin groups turned out to be *two same-named classes per file* (async/sync test variants), whose references sit cleanly in one class's scope each.
  Hand-verified: `ErrorOnRequestTooLargeStream.count` refs at lines 96/98 attribute to the line-93 definition's twin, refs at 146/148 to the line-143 one.

One audited surprise in the final numbers: `module_name` came in 67 above the amended prediction.
A programmatic audit of all 4,226 accepted spans found the surplus is `{httpx2.__version__}` inside f-string interpolations — real executable attribute accesses resolving to the `__version__` module, where tree-sitter exposes no interior identifier so the raw-span gate (added for dotted/relative shapes) legitimately carries them.
Zero acceptances sit in comments or plain strings.

## Final accounting (all five targets)

| target | aligned | exact | module_name | other rules | text_mismatch | dup_ambiguous | syntax_only | aligned % |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| httpx2 | 34995 | 30769 | 4226 | 0 | 1356 | 0 | 0 | 96.3% |
| Flask | 15951 | 14362 | 1589 | 0 | 437 | 6 | 2 | 97.3% |
| ripgrep | 49956 | 45427 | 0 | 4529 | 1093 | 72 | 111 | 97.5% |
| fd | 7446 | 6702 | 0 | 744 | 88 | 0 | 20 | 98.6% |
| self | 18036 | 16251 | 0 | 1785 | 335 | 0 | 4 | 98.2% |

- **httpx2 vs baseline** (30700/5582/69): `text_mismatch` −4,226, `duplicate_ambiguous` 69 → 0, `exact` +69 (the scope-resolved twin references, all aligning under the default rule).
  `semantic_only` 0 throughout.
- **ripgrep vs baseline** (49951/1093/77): `exact` +5 = `duplicate_ambiguous` −5, everything else byte-identical.
  The 5 are the `MyError` twins (two same-named structs in separate test-function scopes, 4 refs, hand-verified) and one `glue::tests::SHERLOCK` reference; the 72 residual = 34 SHERLOCK + 6 HAYSTACK (document-top-level macro consts — no declaration discriminates; honest) + 32 facade re-export aliases (unchanged, the fast-follow's Rust instance).
- **fd**: byte-identical to baseline.
- **self**: not byte-comparable (the repo grew by two changes since its baseline); invariants hold — 0 ambiguous, 0 semantic_only, `module_name` 0 on Rust.
- **Scope-rule usage**: httpx2 69, Flask 85, ripgrep 5 `declaration_scope` attributions.

## Disclosure gate (scip-check, every target)

Duplicated-group disclosure vs the raw index's multi-definition population: httpx2 16=16, Flask 31=31, ripgrep 12=12, fd 1=1, self 10=10.
Flask's population is the richest yet — 31 groups with sizes up to 6 (test files re-defining same-named view functions per test method; `@typing.overload` parameter twins), 90 twins total, 85 references scope-resolved, 6 honestly ambiguous.

## Spot checks (hand ground truth)

- httpx2: class-attr twin attributions verified against source (above); f-string `module_name` audit over all 4,226 rows.
- Flask: 3 `module_name` attributions read at source (`_pytest` bare terminal, `_pytest.monkeypatch` and `_typeshed.wsgi` full dotted paths — all import statements naming exactly those modules); 2 `declaration_scope` attributions (`locate_app::app_name#2` overload-parameter twins, references inside the implementing body).
- ripgrep: `MyError` twin cluster read at source (above).

## Alias harvest (fast-follow design input)

Alias-shaped families, per target, with the binding mechanism:

1. **Import-as aliases (Flask):** `from jinja2 import Environment as BaseEnvironment` → 6 refs refused as expected `Environment` found `BaseEnvironment`; `from werkzeug.wrappers import Response as BaseResponse` → 5 refs (`Response`@`BaseResponse`).
   Binding evidence is the `import … as` statement in the same document — a per-document alias table from the syntax layer would cover it.
2. **Assignment-bound runtime alias (httpx2):** `pytest.mark` → `MARK_GEN` (248 refs) — the alias is `mark = MarkGenerator()` inside `_pytest`, an assignment in a *third-party* module; per-document syntax evidence cannot see it.
   A different evidence source (the semantic index's own definition of `mark` as a re-binding) would be needed.
3. **Facade re-exports (ripgrep, Rust):** `pub use` path aliases (`grep::matcher` → `grep-matcher` crate root), 32 refs — the Rust instance of family 1.
4. **NOT alias-shaped (recorded to prevent future confusion):**
   - scip-python representative-symbol misattribution through star re-exports (httpx2: 787 refs, e.g. `httpx2.Response` carrying the `Headers` symbol) and through `os.path` (Flask: ~65 refs, `join`@`dirname` etc.) — tool bugs the guard correctly refuses; an alias rule must not absorb them.
   - Flask's `__name__` family (166 refs): `Flask(__name__)` sites where the occurrence resolves to the containing module but the token spells `__name__` — a distinct candidate family (the token is the module's runtime name, not its spelled name); filed separately.
   - Prefix-token spans (httpx2 201, Flask ~33): range quirks, stay refused.

## Verdict

Both rules land calibrated: every acceptance carries rule/locality provenance, every residual refusal decomposes into named, evidenced families, and no target shows fabricated attributions (`semantic_only` = 0 everywhere, audits clean).
Python alignment moved from ~84.5% to 96.3% (httpx2) / 97.3% (Flask) — now in the same band as the Rust targets (97.5–98.6%).
