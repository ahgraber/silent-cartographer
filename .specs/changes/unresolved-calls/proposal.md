# Proposal: unresolved-calls

> **Placeholder.**
> This proposal records the intent, stories, and scope of the change.
> The delta specs, `design.md`, and `tasks.md` are not written yet.
> Each open question below needs an explicit decision before the change is implemented.

## Intent

`trace --relation references` reports `empty: the relation holds no instances (a definite empty set)` when the semantic index records no reference to the subject.
The semantic index records only the calls the analyzer resolved.
A call through a receiver the analyzer cannot type has no recorded target, so the answer claims an absence that the index cannot establish.

In a dogfood session on 2026-10-02, `references` on `MetadataStore.touch_blob` in the ai-vfs repository reported a definite empty set, and text search found 35 calls.
The calls went through untyped pytest fixture parameters, a receiver typed `Any`, and an attribute assigned from a function with no return annotation.
An agent that audits for dead code with `references` alone would remove a live method.

A measurement on 2026-10-03 indexed four Python repositories with scip-python 0.6.6 and compared every call site with the recorded occurrences. scip-python records no occurrence at the method-name token of a call it cannot resolve.
Every unresolved call was an attribute call (`x.name(...)`); no plain call (`name(...)`) lacked an occurrence.
The share of attribute calls without an occurrence was 34.6% in Flask and in ai-vfs, 26.4% in Faker, and 2.4% in httpx2.
Most of them were in test code, and most went through a function parameter.

## User Stories

### Story: references-name-what-they-cannot-see

As an agent or developer auditing a method, I want the `references` answer to list the calls that use the method's name but that the analyzer could not resolve, so that I do not treat an absence of resolved references as proof that nothing calls the method.

> Ladders to north-star outcome 5 (**Calibrated trust**): an answer separates what the index established from what it could not determine.
> Ladders to north-star outcome 2 (**Blast radius before change**): a removal decision needs the calls the analyzer missed.

## Scope

**In scope:**

- The build records each attribute call whose name token has no semantic occurrence, with its name, location, and receiver text.
- The `references` answer carries those calls as a separate, labeled group of calls that share the subject's name and have no resolved target.
- The empty outcome of `references` no longer claims a definite empty set when that group is not empty.
- The answer shape is designed so that `retained-utility` can use it for an index built without a semantic analyzer, where every call is unresolved.

**Out of scope:**

- Counting unresolved calls as references, or adding them to `dependents` or `impact`.
- Plain calls (`name(...)`): the measurement found none without an occurrence.
- Calls through a value that does not carry the method's name, such as `callback()` where `callback` is a parameter bound to the function.
  Such a call names a local symbol, so no list based on names can find it.
- Calls that the analyzer attributed to one possible class out of several.
  Such a call has a resolved target, so this change cannot detect it.
- Suggestions for fixing unresolved calls during `build`.
  Such suggestions use the same recorded data and can follow as a later change.

## Approach

The build already holds each document's syntax tree and its semantic occurrences.
One pass over the attribute calls of each tree compares each name token with the occurrences at the same range and records the calls with none.
A query for the references of a symbol reads the recorded calls whose name equals the symbol's name.

## Open Questions

- How the list is narrowed for a common name such as `write` or `close`, where many unrelated calls share the name.
  Candidates are paging alone, or grouping by receiver form or by file.
- How the empty outcome reads when no reference resolved and the unresolved group is not empty.
- Whether Rust needs the same pass. rust-analyzer resolves most calls; calls inside macros are the expected remainder.
  This was not measured.
- Whether the recorded data also feeds `status` or `doctor`, for example as a count of unresolved calls per file.
