# Change notes: semantic-chunking

## Dogfood rebuild at the defaults (2026-08-20)

All five persistent dogfood clones rebuilt from scratch with the working-tree binary.
Wall times include analysis (rust-analyzer or scip-python), which dominates.
Peak RSS is the whole build process — analyzer, ingest, and the compiled-in model included — measured by the user in a separate un-sandboxed `/usr/bin/time -l` run of the same forced rebuilds (2026-08-21; the sandbox denies the resource read).
The embedding step's own memory collapse was measured before the change landed: Faker peak heap 22,395 MiB → 122 MiB with padding disabled, and the whole-process 2.3 GiB peak below confirms the 22.4 GiB spike is gone end to end.

| workspace | wall   | peak RSS | passages | chunks (overlap 0) | chunks (overlap 64) | split passages | max chunks/passage |
| --------- | ------ | -------- | -------- | ------------------ | ------------------- | -------------- | ------------------ |
| ripgrep   | 11.4 s | 1.06 GiB | 3,354    | 3,670              | 3,694               | 130            | 52                 |
| fd        | 5.4 s  | 0.88 GiB | 426      | 464                | 469                 | 13             | 19                 |
| httpx2    | 9.5 s  | 0.80 GiB | 2,240    | 2,389              | 2,402               | 106            | 6                  |
| flask     | 6.0 s  | 0.58 GiB | 1,630    | 1,726              | 1,732               | 54             | 6                  |
| Faker     | 96.4 s | 2.33 GiB | 5,794    | 8,695              | 9,089               | 256            | 684                |
| total     |        |          | 13,444   | 16,944             | 17,386              | 559            |                    |

Split rate under the shipped rule: 559 of 13,444 passages (4.16%), against the proposal's projected 567; total vectors 16,944 (1.26x) against the projected 17,104 (1.27x) — the projections held, and the proposal's figures were corrected to the measured values.
Faker's shape matched the design's predictions almost exactly (8,695 chunks vs 8,697 projected; largest symbol 684 chunks vs 681).

## The 14 recorded dogfood queries (2026-08-20/21)

Re-run against the rebuilt stores at overlap 0, at the shipped default (overlap 64), and at overlap 128, each tallied against the outcomes recorded in `archive/2026-08-14-semantic-investigation/notes.md`.

**Tally at all three settings: identical to the recorded outcomes — 10 HIT (the 6-query regression floor included), 2 PARTIAL, 2 MISS.**
No query's outcome class moved in either direction.

Movements within classes are rank jitter only, attributable to the padding fix — every stored vector changed bit-wise because batch contamination is gone — not to splitting: only two recorded targets are passages that split (`Flask.handle_exception`, `WalkParallel::visit`), and both held (rank 1 and top-3 respectively, at every setting).
Between settings, 16 of 84 result lines differ from 0 to 64 and 14 from 64 to 128, all rank-4/5 movement or head rotation within the target neighborhood.
Exemplars of the jitter: the gitignore query's head rotated between `WalkBuilder` and `IgnoreOptions`; the context-lines query put `ContextModeLimited::get` above `ContextMode::set_before`/`set_after`.
The two recorded misses (httpx2 timeout, fd exec) remain misses with the same failure shape: the paraphrase shares no domain vocabulary with the target.

## Boundary-sensitivity probe (2026-08-20)

Built as designed: for every leaf passage that splits at the default chunk size, up to three queries were generated from content spanning a chunk boundary (the first, middle, and last boundary), and the owning symbol's rank recorded in the dense leg alone — the lexical leg indexes each whole render and would find verbatim boundary text trivially, washing out the signal overlap exists to repair.
Query texts derive from the overlap-0 split, so all three settings answer the same query set.
Container passages (synthesized interface-tier content) were excluded from query generation; the probe covers 520 split leaves and 949 queries across the five workspaces.

| overlap | rank 1      | rank 2–5 | rank 6–20 | beyond 20 | absent |
| ------- | ----------- | -------- | --------- | --------- | ------ |
| 0       | 669 (70.5%) | 199      | 65        | 16        | 0      |
| 64      | 734 (77.3%) | 166      | 37        | 12        | 0      |
| 128     | 755 (79.6%) | 150      | 33        | 11        | 0      |

Per-workspace rank-1 at 0 → 64 → 128: ripgrep 107 → 116 → 124 of 153; fd 17 → 19 → 19 of 23; httpx2 79 → 87 → 86 of 134; flask 54 → 55 → 56 of 70; Faker 412 → 457 → 470 of 569.

**Verdict: the lift is material, and the default moved to 64.**
The pre-registered rule said a null result keeps no overlap and a material lift changes the default; the result was not null — +6.8pp rank-1 at 64, consistent in direction across all five workspaces, at +2.6% vectors (far below the 1.34x projection, because packing absorbs carried units). 128 adds +2.3pp rank-1 for +6.7% total vectors: half the marginal gain at more than double the carried cost, so 64 is the knee.
The user ratified the move; `DEFAULT_CHUNK_OVERLAP` is 64, the README and surface snapshot record it, and the 14-query regression pass above was re-run at the new default with no movement.

The probe instrument and its per-run outputs are kept in `._scratch/semantic-chunking-probes/`.

## Review remediation (2026-08-21)

A parallel AI review raised nine findings; each was probed before acceptance.

- **Windowed runs carried no token overlap (CONFIRMED, fixed).**
  Probed: 30 windows at overlap 32, zero overlapped boundaries — the suffix search took the largest cut, and the empty suffix always fit the budget.
  Fixed to the smallest fitting cut, with the carried suffix capped at half the window so an overlap near the chunk size cannot degenerate into one-char creep; pinned by a windowed-overlap regression test.
  The unit-aligned overlap path — the mechanism the probe experiment measured — was unaffected: the corrected matrix re-run reproduced every rank row of the decision run, moving only fd's chunk counts (+5 at 64, +13 at 128).
- **Degenerate chunk sizes could exceed the bound (PLAUSIBLE, floor raised).**
  Not reproduced — sizes 2 through 8 held the bound against multi-byte content — but the fallback's escape hatch made it possible in principle.
  The minimum accepted size is now 8, where the bound holds by arithmetic, pinned by a minimum-size Unicode test.
- **Identity-head trimming contradicted the HeaderBudget decision (CONFIRMED, decision amended).**
  The behavior was deliberate and undocumented at the design level; the user ratified amending the decision — the hard size bound outranks header wholeness, sacrifice runs from the signature's parameter run inward — with the identity-exceeds-budget test added.
- **The KNN pool is bounded in chunks before symbol aggregation (CONFIRMED, spec amended).**
  The 4,096-row pool predates this change and every answer already carries the candidates-not-completeness framing; the user ratified scoping the delta's no-chunk-advantage guarantee to the candidates a signal considers, with bounded pools disclosed by that framing.
  The design's crowding risk and revisit trigger stand.
- **`similar` cost is linear in subject chunks (CONFIRMED, disclosed).**
  Measured 12.1 s on Faker's 782-chunk generated data table against a ~1 s baseline; the user ratified disclose-and-revisit — a subject-side cap would reintroduce the content-late-in-a-long-symbol failure — recorded as a design risk with the batching fix shape.
- **Answers omitted the chunk parameters from provenance (CONFIRMED, fields added).**
  Resolved by the surface split the user ratified: the machine answer carries the recorded identity whole (the chunk parameters are the operator-variable part of the regime, so without them two stores at different parameters carry identical provenance), while the human rendering keeps its condensed form; the labeling delta states the split and the machine-answer test asserts the fields.
- **Build memory holds all pending chunks before persistence (accepted, recorded).**
  Tens of megabytes at the defaults, bounded further by the size floor; recorded as a design risk with the bounded-batch fix shape.
- **The vocabulary sweep never reached the baseline specs (CONFIRMED, fixed).**
  The permanent `archive` directory under `.specs/changes/` read as an active change; it is now excluded.
- **The 14-query pass lacked the 128 arm (CONFIRMED, run).**
  All three settings now tallied, identically.

The experiment stores were also found rebuilt under the new default by the peak-memory run, which had silently voided the overlap-0 stores; the full matrix was re-run from explicitly parameterized rebuilds, and the probe now derives its query set at pinned overlap-0 parameters rather than `Default`, so a future default change cannot drift the instrument.

## Upstream report

The `model2vec-rs` padding defect is written up for filing in `upstream-model2vec-rs.md` (pooling counts pad ids; the loader honours a serialized `BatchLongest` configuration), with the reproduction and the measured effect.
