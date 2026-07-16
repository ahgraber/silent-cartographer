# Dogfood record: chunk-tiers

Recorded 2026-07-15; amended the same day after an external review pass (see § Post-review amendments).
Suite at record time: 289 passed, 0 failed, 2 ignored (the pre-existing fixture-generator tests requiring live indexer tools); `cargo fmt --check` clean; `cargo clippy --all-targets` zero warnings.

## Pins, tools, and a session note on the targets

Pins: httpx2 `9b7ee8ec` (v2.5.0), Flask `22d9247` (3.1.3), ripgrep `48b0c795`, fd `5a5852e1`, self = working tree on `61965a3` + this change.
Tools: scip-python 0.6.6, rust-analyzer 1.95.0, Python 3.14.4, toolchain via `rust-toolchain.toml`.
The previous sessions' target clones lived under `/private/tmp` and were reaped by the tmp cleaner (only the store files survived); all four targets were re-cloned fresh at the recorded pins into `~/.cache/silent-cartographer/` — a persistent location — with each survivor v10 store kept beside the new build as `index-v10-baseline.db` (Flask: `control-v10.db`, see below).

**Python environment drift and the control-build method.**
The rebuilt venvs (`uv sync` from each repo's committed lock) carry different installed-package fingerprints than the archived builds recorded (the original venvs are gone, and the fingerprint — a hash over sorted dist-info names — is not invertible).
For httpx2 the drift is inert: the v11 build reproduced the archived accounting byte-for-byte anyway.
For Flask the richer venv resolves more optional-dependency references (asgiref, greenlet, python-dotenv appear in its tests), so the raw SCIP index itself grew (+431 occurrences vs the archived record) — an input change, not a behavior change.
The gate was therefore run as a **same-environment control**: Flask built twice from the identical fresh venv, once with a v10 binary compiled from `HEAD` (pre-change) and once with this change's v11 binary.
Self was measured the same way (v10 and v11 binaries over the identical working tree).

## Alignment accounting — the change alters no alignment behavior

| target | comparison | result |
| --- | --- | --- |
| httpx2 | v11 build vs archived baseline | **byte-identical** (aligned 35249, exact 30770, module_name 4354, self_name 5, module_marker 119, import_alias 1, text_mismatch 1102, dup 0, syntax_only 0) |
| ripgrep | v11 build vs archived baseline | **byte-identical** (aligned 50391; every bucket equal) |
| fd | v11 build vs archived baseline | **byte-identical** (aligned 7505; every bucket equal) |
| Flask | v11 vs same-env v10 control | **byte-identical** (aligned 16710, exact 14618, module_name 1802, self_name 188, module_marker 83, import_alias 19, text_mismatch 109, dup 6, syntax_only 2); archived-baseline delta is env drift only |
| self | v11 vs same-tree v10 control | **byte-identical** (aligned 23760 after the post-review fixes landed in-tree; every bucket equal) |

Twin-group disclosure: httpx2 16, Flask 31, ripgrep 12, fd 1 — all equal to the archived records.
Self disclosed 15 groups vs the archived 11: repo growth (this change's diff adds same-named test helpers across test files), not a disclosure change.

## Row-level store comparison (the byte-identical gate is replaced by design)

Per design, the whole-store byte-identical gate does not apply to this change — module rows change by design.
The replacement gate, run on all four external targets:

- **Non-module symbol rows**: byte-identical in both directions (columns shared with v10) on httpx2 (vs the 4-day-old archived baseline), Flask (vs the same-env control), and fd; ripgrep byte-identical except the six rows below.
- **Occurrences and edges**: byte-identical on all four targets.
- **Module identity sets**: unchanged on all four targets.
- **Module-row delta** (the designed change): httpx2 — all 119 in-workspace modules previously persisted empty spans; 111 widened to `(0, len)` whole-document spans and 8 correctly persist `(0, 0)` because their documents (empty `__init__.py`) are zero-length. Flask — all 83 previously empty; 80 widened, 3 zero-length documents. fd — all 36 module rows byte-identical (Rust file modules were already whole-document via the old fallback; 12 inline `mod` blocks keep their declaration spans).
- **ripgrep nuance (6 module rows)**: modules whose SCIP definition occurrence sits on a re-export token (`pub extern crate grep_cli as cli;` in the `grep` facade crate ×5, `extern crate test;` in globset's bench ×1) matched no declaration and previously persisted a 3–5 byte name-token body; they now take the module branch and persist their *defining document* (the small facade `lib.rs`, 656 bytes) as their body. Neither representation is the module's true source document (which lives in another crate); the new one is uniform with the module contract and strictly more useful. Recorded residual: re-export-defined modules body-project their defining document.

## Store growth (two nullable TEXT tier columns)

| target | v10 | v11 | growth |
| --- | --- | --- | --- |
| self | 18.6 MiB | 18.9 MiB | +1.5% |
| httpx2 | 25.7 MiB | 27.5 MiB | +7.2% |
| flask | 11.0 MiB | 12.0 MiB | +9.5% |
| ripgrep | 28.3 MiB | 29.4 MiB | +4.0% |
| fd | 3.8 MiB | 3.9 MiB | +2.8% |

Python targets grow more because module bodies went from empty spans to whole documents and docstring interfaces are long; all growth is bounded as the design predicted.

## Tier sampling (read at source through the CLI)

- Documented Rust type, interface detail: `globset::glob::Glob` returns its `///` run, both attributes (`#[derive(...)]` and the interleaved `#[cfg_attr(...)]`), and the header, cut before the body.
- Rust module interface, inner **block** docs: `globset`'s bench crate root returns its `/*! ... */` run — both inner-doc forms extract.
- Self module interface: `graph::join` returns the file's `//!` run without the body.
- Const: `DEPENDENTS_HORIZON` at signature detail returns the full declaration (`pub const DEPENDENTS_HORIZON: u32 = 20;`) per the no-distinct-body clause.
- Python module interface: httpx2 `_decoders`/`_exceptions`/`_sse` module rows carry their qualified-name signature followed by their real module docstrings; module signatures are qualified names (a duplicated crate root keeps its `#0` disambiguator — honest identity, as designed).
- `trace references --detail signature`: a site inside `DecompressionMatcherBuilder::build` projects **that method's** signature (the attributed enclosing declaration); a module-scope site projects the module's qualified-name signature.
- `trace dependents --detail interface`: detail rows carry each dependent's doc-bearing interface alongside kind and distance; beyond-bound aggregate rows carry no content field.
- An ambiguous `get` reference still returns the typed candidate set (calibrated behavior unchanged).
- The v10 binary against a v11 store refuses with the teaching error naming both versions and the rebuild recovery — observed live.

## Fallback census (which rows carry name-token tiers)

Rows whose persisted body equals their bare name token (the non-module name-span fallback, where tiers mirror the token): self — constant 227, type 115; ripgrep — constant 738, method 706, other 35, type 259.
These are symbols with no matching syntax declaration to expand into — enum-variant/associated constants, derive-synthesized methods anchored at the type name, type parameters — the same rows that carried name-token *bodies* before this change; tiers degrade identically, never error.
The arm carries re-runnable evidence beyond this census: a persistence test builds a struct-field symbol (which aligns but matches no persisted declaration kind) and asserts body, signature, and interface all equal the bare name token.
No aligned declaration kind hit the unknown-kind total-extraction fallback on any target.

## Post-review amendments (2026-07-15)

An external review pass surfaced three defects and several documentation gaps; all were fixed, and every target was rebuilt and re-gated afterward (alignment accounting byte-identical to the figures above on all four external targets; self's control comparison re-run on the amended tree, still byte-identical at aligned 23760).

- **Module projection tie (real bug, found in this store):** `crates/grep/src/lib.rs` holds six module rows with identical whole-document spans (the file module plus five re-export-defined modules), and the span-then-identity ordering picked `grep::cli` over the file module `grep::crate#1` for NULL-attributed sites. `module_of_document` now selects by the widest module *definition occurrence* in the document — the file module's spans the document, a re-export's sits on a name token — with a regression test whose re-export identity deliberately sorts first.
- **Python docstring detection:** a comment above a docstring (including a shebang line above a module docstring) and implicitly concatenated string literals are real docstrings per CPython and were missed; both are now recognized. Fixing the comment case exposed a grammar subtlety — tree-sitter-python attaches a header-adjacent comment *outside* the `body` field — so the Python signature now cuts at the header-ending `:` token itself rather than at the body's start, which the tier-extraction contract wanted all along.
- **Module interface composed:** a documented file module's interface tier is now its qualified-name signature followed by its module documentation, restoring the uniform interface contract (signature together with documentation) instead of a module-only exception; verified live on Flask (`flask.json.tag::__init__` + docstring) and self (`graph::join` + `//!` run).
- **Coverage:** added projection tests for the `containers` arm at body detail, explicit location detail (no content field), and exact per-row attributed-tier equality at signature detail; added the inline-module scenario test pinning that `mod name { … }` declarations keep their declaration spans.
