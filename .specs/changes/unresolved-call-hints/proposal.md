# Proposal: unresolved-call-hints

> **Placeholder.**
> This proposal records the intent, stories, and scope of the change.
> The delta specs, `design.md`, and `tasks.md` are not written yet.
> Each open question below needs an explicit decision before the change is implemented.
> This change follows `unresolved-calls`, which records the data it reads.

## Intent

`unresolved-calls` records each attribute call that the analyzer could not resolve and lists those calls in `references` answers.
The calls show where the index is blind, but not why.
Most of them have a cause that the developer can remove, and removing it makes the analyzer resolve the calls.

A measurement on 2026-10-03 over four Python repositories found that most unresolved attribute calls go through a function parameter, often an untyped pytest fixture.
An annotation on that parameter, or a return annotation on that fixture, would let the analyzer resolve them.

## User Stories

### Story: build-names-the-fix

As a developer, I want `build` to report the receivers that leave the most calls unresolved, with the annotation that would resolve them where c10r can identify it, so that I can make the index see more of my code.

> Ladders to north-star outcome 5 (**Calibrated trust**): the index states where it is blind and how to reduce that.
> Ladders to north-star outcome 2 (**Blast radius before change**): each resolved call joins the blast radius.

## Scope

**In scope:**

- A `build` report of unresolved attribute calls grouped by receiver, ordered by count.
- A suggested annotation site where the syntax tree identifies it: a function parameter, or a pytest fixture's return.
- Labels that present every suggestion as a suggestion, not as a diagnosis.

**Out of scope:**

- Tracing a receiver through reassignment, aliases, inherited attributes, or fixture overrides.
  Such tracing is not reliable from the syntax tree.
- Editing source files.
- Calls the analyzer attributed to one possible class out of several, which have a resolved target.

## Open Questions

- Whether the report appears in every `build` answer, behind a flag, or in `doctor`.
- How many groups the report names before it summarizes the rest.
- Whether `status` also reports the count of unresolved calls.
- Whether the report, or `status`, names the unresolved share only when it exceeds a threshold, and which threshold.
  Candidates raised on 2026-10-16 were 2%, 5%, and 10% of calls.
  The measured shares were 2.4% to 34.6% of attribute calls in four Python repositories, and 0% to 3.4% of calls in three Rust repositories.
