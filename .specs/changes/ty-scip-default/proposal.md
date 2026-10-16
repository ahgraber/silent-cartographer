# Proposal: ty-scip-default

> **Placeholder.**
> This proposal records the intent, stories, and scope of the change.
> The delta specs, `design.md`, and `tasks.md` are not written yet.
> Each open question below needs an explicit decision before the change is implemented.
> This change follows `ty-scip-backend`, whose comparison gate supplies the evidence for it.

## Intent

`ty-scip-backend` adds ty-scip as an opt-in Python indexer.
A build without `--python-indexer` uses the indexer recorded in the existing store, and scip-python when no store exists.
That change also runs a comparison gate against scip-python on Flask, httpx2, Faker, sympy, and sphinx, with thresholds fixed before the run.

This change decides, from the gate results, whether ty-scip becomes the default indexer for a new Python store.
The scip-python indexer runs out of memory on some large repositories, such as sympy.
While scip-python is the default, a user meets that failure before learning that ty-scip exists.

## User Stories

### Story: large-python-works-by-default

As an agent working in a large Python repository, I want a build without options to use an indexer that can index the repository, so that I get precise locate and blast radius without knowing that an alternative indexer exists.

> Ladders to north-star outcomes 1 (**Precise locate**) and 2 (**Blast radius before change**).

## Scope

**In scope:**

- The indexer a build uses when it has no `--python-indexer` option and no existing store.
- Which Python indexers the readiness report treats as required.
- The README and command documentation for the default.

**Out of scope:**

- Removing scip-python.
- Exposing the indexer selection through the MCP `build` tool.

## Open Questions

- Does a passed gate decide the default by itself, or does the decision also weigh ty-scip's maturity: one maintainer, and a symbol scheme its author says may change?
- Do existing scip-python stores keep scip-python?
  Under the `ty-scip-backend` rule they do, until an explicit selection replaces it.
- When ty-scip is the default and absent, does a build without a store refuse, or use scip-python?
  `ty-scip-backend` never uses the other indexer in place of an unavailable one.
- Does scip-python become an optional indexer in the readiness report?
- Does `c10r-settings` land first, so that a project can fix its indexer before any store exists?
- Does the default wait for ty-scip to emit one definition per name ([ty-scip#19](https://github.com/brendan-m-murphy/ty-scip/issues/19)) and for a gate run that passes after it?
  The 2026-10-16 gate failed edge retention on Flask, httpx2, and Faker, and join acceptance on httpx2 and Faker.
  References that the join refuses because their target has several definitions rose from 6, 0, and 671 under scip-python to 303, 1366, and 1295 under ty-scip.
- If ty-scip skips a file with no importable module name instead of refusing the index ([ty-scip#18](https://github.com/brendan-m-murphy/ty-scip/issues/18)), how does a build keep the guarantee that it never persists a partial index: by keeping the strict behavior, or by naming the skipped files in the build answer?
- How does the change handle the third-party references that ty-scip does not record?
  A probe on Flask on 2026-10-10 found 497 calls to third-party targets with no occurrence under ty-scip.
