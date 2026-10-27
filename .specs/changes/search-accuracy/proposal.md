# Proposal: search-accuracy

> **Placeholder.**
> This proposal records the intent, stories, and scope of the change.
> The delta specs, `design.md`, and `tasks.md` are not written yet.
> Each open question below needs an explicit decision before the change is implemented.

## Intent

`search` ranks symbols for a natural-language query.
No measurement shows how often its top rows contain the code that a task needs.
Ranking changes are accepted on dogfood use and on queries that the author of the change wrote.
Queries written by the author of a change test only the cases that the author already checked.

The localization eval measures whether an agent with c10r names the files that a historical fix changed.
It cannot attribute a result to `search` ranking, because the agent chooses which queries to run and which rows to read.
Each measurement also costs a full agent sweep.

This change measures `search` directly.
The query is taken from the issue text of an eval instance, the expected answer is the gold answer of that instance, and no agent runs.

Prior art: the entire-graph project scores its search the same way, with SWE-bench issue text as queries and the fixed files as gold ([entireio/entire-graph#247](https://github.com/entireio/entire-graph/issues/247)).
In that issue, a fixture eval written from the motivating examples passed through two defects that the issue-text corpus found.

## User Stories

### Story: measured-search-ranking

As a c10r maintainer, I want to measure how highly `search` ranks the files that a historical fix changed, given the issue text as the query, so that I accept or reject a ranking change on measured evidence.

> Ladders to north-star outcome 6 (**Find by intent**): the measurement scores search by what code does, from text written by people who did not write the code.
> Ladders to north-star outcome 1 (**Precise locate**): the measurement shows whether search puts the needed code in the first rows.

### Story: ranking-regression-gate

As a c10r maintainer, I want a check that fails when a change lowers search accuracy below a recorded baseline, so that a ranking regression does not ship unnoticed.

> Ladders to north-star outcome 6 (**Find by intent**).

## Scope

**In scope:**

- A command that runs `search` once per eval instance against an index of the instance's repository at its base commit, with a query taken from the instance's issue text.
- File-level scores for each instance and in aggregate: the share of gold files in the first k rows at several cutoffs, and the reciprocal rank of the first gold file.
  A symbol row counts for the file that contains it.
  A file's rank is the rank of its first row.
- Reuse of the eval's gold-answer derivation and of its dev and frozen subsets.
  Tuning uses dev instances only.
  The baseline and every reported score come from frozen instances.
- A recorded baseline for each c10r version, and a check that compares a run against it and lists the instances whose rank rose or fell.

**Out of scope:**

- Agent runs, and any change to the localization eval's arms, criteria, or registered claims.
- Correctness of `trace` and `impact` relations against an independent oracle such as a compiler.
- Symbol-level gold answers.
  The localization eval does not score symbols either.
- Changes to `search` ranking.

## Approach

- The suite lives in `evals/` and reuses its instance manifest, its gold answers, and its subset split.
- A `search` answer may come from a bounded set of candidates, and the baseline spec does not fix how the requested row count affects that set.
  The suite requests one row count for every instance and scores all cutoffs from that one answer, so that a cutoff never changes which candidates are considered.
- The gate needs the same ranking from the same c10r build, index, and query.
  Whether embedding inference on CPU gives identical scores across runs needs a check before the gate compares exact ranks.

## Open Questions

- Which part of the issue text is the query: the title, the first sentences, or the full problem statement.
  The agent receives the full statement.
  The suite can score more than one form.
- Which cutoffs to report, and which row count to request.
- Which languages to cover.
  The eval instances come from the SWE-bench-Live Python split.
  Rust needs another instance source, such as SWE-bench Multilingual.
- Whether the gate requires exact agreement with the baseline or allows a tolerance, and whether it runs in CI or by hand.
  An index for every frozen instance is expensive to build.
- Whether the suite reuses the indexes built into the treatment images or builds its own.
