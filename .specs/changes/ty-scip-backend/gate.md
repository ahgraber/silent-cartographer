# Comparison Gate: ty-scip against scip-python

The results below were measured on 2026-10-11 with ty-scip descriptors converted to scip-python's form.
`canonical-identity` replaces that form, so every identity-dependent result here (identity overlap, edge retention, answer identity, the wrong-target review) must be re-run after this change is rebased onto it.
Determinism, speed, sympy, and sphinx do not depend on the identity form.

## Indexer versions

- ty-scip: `ty-scip 0.1.0`, installed with `uv tool install ty-scip==0.1.0` on 2026-10-10.

## Definitions

These definitions were fixed on 2026-10-11, before any gate run.

- **Binary.**
  Every build uses the branch binary, run with an explicit `--python-indexer` and a separate `--db` per indexer.
  The ty-scip stores are built with the descriptor conversion that gives both indexers one identity form (added 2026-10-11, after the first run found the two Flask stores shared no in-workspace identity).
- **Identity overlap.**
  The share of scip-python's in-workspace identities that the ty-scip store also holds.
  Reported, with no threshold.
- **In-workspace cross-file edge.**
  A row of the store's `edges` table whose source and target are both in-workspace symbols, and whose source and target are defined in different files.
  An edge is identified by its kind, source identity, and target identity, never by its row id.
- **Edge retention.**
  The share of scip-python's in-workspace cross-file edges that the ty-scip store also holds.
- **Lost edge.**
  An in-workspace cross-file edge that only the scip-python store holds.
  Each lost edge is assigned one cause.
- **Gained edge.**
  An in-workspace cross-file edge that only the ty-scip store holds.
  Gained edges are the residuals reviewed for wrong targets, because each one changes a `dependents` or `impact` answer.
- **Join acceptance.**
  From the build answer: aligned / (aligned + text_mismatch + duplicate_ambiguous + semantic_only).
- **Wrong target.**
  An edge whose target is not the symbol that the source code at the edge's occurrence refers to.
- **Answer identity.**
  For each in-workspace symbol that has a direct dependent in either store, whether the two stores give the same set of direct dependents.
  Reported as a share, with no threshold.
- **Determinism.**
  Two ty-scip runs with the adapter's invocation over the same tree produce byte-identical SCIP files.
- **Flask speed.**
  The ty-scip indexer run alone, wall time and peak resident memory.
- **sympy.**
  The 60 s and 4 GB bounds apply to the ty-scip indexer run alone, with the documented exclude list.
  The `c10r build` must then complete and answer a query, with no time bound.
- **sphinx.**
  The `c10r build` with ty-scip completes and answers a query, with `exclude-scripts = true` as the only configuration change.

## Thresholds

For each of Flask, httpx2, and Faker:

- edge retention is at least 95%, and every lost edge has a cause;
- zero wrong targets in a reviewed sample of 100 accepted ty-scip edges, and in every reviewed gained edge;
- join acceptance is at least 97%;
- the two ty-scip runs are byte-identical.

Further targets:

- Flask indexes in under 1 s and 250 MB;
- sympy indexes in under 60 s and 4 GB with the documented exclude list, and the build completes and answers;
- sphinx builds and answers with `exclude-scripts = true` as its only configuration change.

## Repositories

| Repository | Clone                                      | Commit                         |
| ---------- | ------------------------------------------ | ------------------------------ |
| Flask      | `~/.cache/silent-cartographer/flask`       | `22d92470`                     |
| httpx2     | `~/.cache/silent-cartographer/httpx2`      | `9b7ee8ec`                     |
| Faker      | `~/.cache/silent-cartographer/faker-probe` | `4eced9f`                      |
| sympy      | `~/.cache/silent-cartographer/sympy`       | `16fa855` (tag `sympy-1.14.0`) |
| sphinx     | `~/.cache/silent-cartographer/sphinx`      | `e44a40e`                      |

sympy's documented exclude list: `.ci`, `doc/ext/convert-svg-to-pdf.py`, `sympy/parsing/autolev/test-examples`.

Faker has no `pyproject.toml`, so both Faker builds pass `--language python`.

## Results

Measured 2026-10-11.
Raw outputs and scripts are in `._scratch/research/scip-python/gate/` (`results.md`, `metrics-<repo>.md`, `edges_compare.py`).

| Measure                         | Flask       | httpx2      | Faker       | Threshold        |
| ------------------------------- | ----------- | ----------- | ----------- | ---------------- |
| Identity overlap                | 95.3%       | 95.8%       | 98.0%       | reported         |
| Edges, scip-python / ty-scip    | 2268 / 2509 | 7913 / 8156 | 8354 / 8329 | reported         |
| Edge retention                  | 88.0%       | 91.5%       | 94.9%       | at least 95%     |
| Lost edges                      | 273         | 676         | 429         | each has a cause |
| Gained edges                    | 514         | 919         | 404         | reviewed         |
| Answer identity                 | 84.0%       | 85.2%       | 90.9%       | reported         |
| Join acceptance, ty-scip        | 98.0%       | 95.8%       | 92.8%       | at least 97%     |
| Join acceptance, scip-python    | 99.3%       | 97.0%       | 93.1%       | reference        |
| Two ty-scip runs byte-identical | yes         | yes         | yes         | yes              |

Lost-edge causes (target absent from ty-scip / source absent / no occurrence in ty-scip): Flask 202 / 35 / 36, httpx2 383 / 8 / 285, Faker 49 / 2 / 378.
No lost edge sat at a ty-scip join discrepancy, and no lost edge fell in "other".

- Flask speed: 0.71 s / 120 MB, 0.72 s / 120 MB, 0.72 s / 120 MB.
  Passes.
- sympy: ty-scip indexed in 45.5 s with 2459 MB peak and refused no file.
  `c10r build` completed in 213 s, and `find` answered.
  Passes.
- sphinx: the clone already has a `ty.toml` with `[src]`, so `exclude-scripts = true` went into that table.
  `c10r build` completed in 28 s, and `find` answered.
  Passes.
- The sympy and sphinx stores were built before the descriptor conversion.
  The conversion changes identities, not whether a build completes.

Wrong-target review: all 2,088 packet entries (Flask 588, httpx2 1,003, Faker 497: each repository's 100-edge sample plus every gained edge) were judged by Haiku reviewers against `review-rubric.md`, with verdicts in `verdicts-<repo>-<range>.tsv`.

| Repository | Correct | Wrong | Unclear |
| ---------- | ------- | ----- | ------- |
| Flask      | 588     | 0     | 0       |
| httpx2     | 1,002   | 1     | 0       |
| Faker      | 497     | 0     | 0       |

The one wrong target is httpx2 entry 531, confirmed by reading the source: `exc_info.value.request` reads the property `HTTPError.request` (`_exceptions.py:108`), and ty-scip points it at the subclass assignment `self.request = request` (`_exceptions.py:278`), which only calls the inherited setter. ty-scip treats an assignment through an inherited property as a new attribute of the subclass.

No reviewer returned UNCLEAR, which the rubric expected for receivers with no visible type.
Six verdicts were rechecked by hand from the source, chosen at random among attribute accesses on a receiver that is neither `self` nor a module (Flask 31 and 312, httpx2 41, Faker 73, 356, and 407); all six agree.

Outcome: edge retention fails on all three repositories, ty-scip join acceptance fails on httpx2 and Faker, and the wrong-target threshold fails on httpx2 with one wrong target.
