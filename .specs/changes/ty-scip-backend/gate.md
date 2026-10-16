# Comparison Gate: ty-scip against scip-python

The identity-dependent results (identity overlap, edge retention, answer identity, and the wrong-target review) were measured on 2026-10-15, after this change was rebased onto `canonical-identity`.
Determinism, speed, sympy, and sphinx do not depend on the identity form, and were measured on 2026-10-11 with the same ty-scip version.

## Indexer versions

- ty-scip: `ty-scip 0.1.0`, installed with `uv tool install ty-scip==0.1.0` on 2026-10-10.

## Definitions

These definitions were fixed on 2026-10-11, before any gate run.

- **Binary.**
  Every build uses the branch binary, run with an explicit `--python-indexer` and a separate `--db` per indexer.
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

The identity-dependent measures were run on 2026-10-15 from commit `46360fd`, with raw outputs and scripts in `._scratch/research/scip-python/gate/rerun-2026-10-15/` (`metrics-<repo>.md`, `build-<repo>-<indexer>.out`, `edges_compare.py`, `reuse_verdicts.py`).
The 2026-10-11 outputs are in `._scratch/research/scip-python/gate/`.
The clones were at the commits listed above, and `git status --porcelain --ignored` was the same before and after the builds.

| Measure                         | Flask       | httpx2      | Faker       | Threshold        |
| ------------------------------- | ----------- | ----------- | ----------- | ---------------- |
| Identity overlap                | 95.6%       | 95.9%       | 98.1%       | reported         |
| Edges, scip-python / ty-scip    | 2379 / 2509 | 8205 / 8156 | 8473 / 8329 | reported         |
| Edge retention                  | 94.1%       | 91.2%       | 94.9%       | at least 95%     |
| Lost edges                      | 140         | 723         | 428         | each has a cause |
| Gained edges                    | 270         | 674         | 284         | reviewed         |
| Answer identity                 | 84.1%       | 86.5%       | 91.6%       | reported         |
| Join acceptance, ty-scip        | 98.0%       | 95.8%       | 92.8%       | at least 97%     |
| Join acceptance, scip-python    | 99.3%       | 97.0%       | 93.1%       | reference        |
| Two ty-scip runs byte-identical | yes         | yes         | yes         | yes              |

Lost-edge causes (target absent from ty-scip / source absent / no occurrence in ty-scip): Flask 51 / 40 / 49, httpx2 382 / 8 / 333, Faker 47 / 2 / 379.
No lost edge sat at a ty-scip join discrepancy, and no lost edge fell in "other".

Most identities that only one store holds come from names with several definitions. ty-scip gives a separate definition to each assignment of a name, so a name that scip-python reports once has several identities in the ty-scip store: Flask 61 such names, httpx2 211, Faker 143.
An edge to such a name in the scip-python store is lost, and its counterpart in the ty-scip store points at one of the several identities, or at none when the join cannot choose (duplicate-ambiguous: Flask 303, httpx2 1366, Faker 1295 under ty-scip, against 6, 0, and 671 under scip-python).

- Flask speed: 0.71 s / 120 MB, 0.72 s / 120 MB, 0.72 s / 120 MB (2026-10-11).
  Passes.
- sympy: ty-scip indexed in 45.5 s with 2459 MB peak and refused no file (2026-10-11).
  `c10r build` completed in 213 s, and `find` answered.
  Passes.
- sphinx: the clone already has a `ty.toml` with `[src]`, so `exclude-scripts = true` went into that table (2026-10-11).
  `c10r build` completed in 28 s, and `find` answered.
  Passes.

Wrong-target review: all 1,504 packet entries (Flask 359, httpx2 767, Faker 378: each repository's 100-edge sample plus every gained edge) have a verdict against `review-rubric.md`.
A verdict judges whether the tokens at an entry's occurrence sites denote the declaration at its target's definition site, and identity renaming does not change that judgment.
So an entry with the same kind, target definition site, and occurrence sites as a 2026-10-11 entry keeps that entry's verdict (Flask 304, httpx2 697, Faker 295; `reused-<repo>.tsv`).
Haiku reviewers judged the other 208 entries (`verdicts-<repo>-new.tsv`, and `verdicts-httpx2-a.tsv` and `verdicts-httpx2-b.tsv`).

| Repository | Correct | Wrong | Unclear |
| ---------- | ------- | ----- | ------- |
| Flask      | 359     | 0     | 0       |
| httpx2     | 766     | 1     | 0       |
| Faker      | 378     | 0     | 0       |

The one wrong target is a reused verdict, confirmed on 2026-10-11 by reading the source: `exc_info.value.request` reads the property `HTTPError.request` (`_exceptions.py:108`), and ty-scip points it at the subclass assignment `self.request = request` (`_exceptions.py:278`), which only calls the inherited setter. ty-scip treats an assignment through an inherited property as a new attribute of the subclass.

Eleven new Faker entries have a target with no definition site in the store; the reviewer judged them against the `Faker` class declarations in `faker/proxy.pyi`.
Five new verdicts were rechecked by hand from the source (Flask 9 and 18, httpx2 30 and 64, Faker 2); all five agree.

Outcome: edge retention fails on all three repositories (Flask and Faker by less than one point), ty-scip join acceptance fails on httpx2 and Faker, and the wrong-target threshold fails on httpx2 with one wrong target.
Under the gate's rules, ty-scip stays opt-in, and `ty-scip-default` cannot proceed on these results.
