# Change order

This file records the recommended order in which the active changes are fleshed out and implemented, and why.
Remove an entry when its change is archived.

`unresolved-calls`, `method-overrides`, and `retained-utility` are fully proposed.
The other six active changes are placeholders: each has a proposal and no delta specs, `design.md`, or `tasks.md`.
The order puts dependencies first, then severity.
Severity ranks a wrong answer above a loud failure, and a loud failure above a missing feature.

## Sequence

1. `unresolved-calls`
2. `method-overrides`
3. `retained-utility`
4. `multi-language-builds`
5. `unresolved-call-hints`
6. `unresolved-call-filter` (conditional; see its section)
7. `unreadable-sources`
8. `c10r-settings`
9. `ty-scip-default` (blocked; see Blocked)

### 1. `unresolved-calls`

It fixes the most dangerous output: a `references` answer that reports a definite empty set for a method with live callers, which leads an agent to remove the method as dead code.
Four other changes depend on it.
`method-overrides` and `retained-utility` use its `references` answer shape, and `unresolved-call-hints` and `unresolved-call-filter` read the calls it records.

### 2. `method-overrides`

Its disclosure goes into the `references` answer that `unresolved-calls` reshapes.
It also modifies `Relationship trace`, which `unresolved-calls` modifies first, so its delta is rebased on the synced text before it syncs.

### 3. `retained-utility`

It uses the `unresolved-calls` answer shape for an index with no semantic analyzer, where every call is unresolved.
It is the largest of the changes.
It goes after the smaller changes that fix its interfaces.

### 4. `multi-language-builds`

It indexes every supported language of a workspace into one store, such as Bash scripts inside a Rust workspace.
It depends on `retained-utility`, which adds the Bash profile and records a language and tier for each file.
It follows that change directly, because most shell scripts live inside workspaces of another language.

### 5. `unresolved-call-hints`

It is small and depends only on `unresolved-calls`.
It could move up to second, because it is cheap once `unresolved-calls` exists.
It stays fifth because it has the lowest severity: without it no answer is wrong, the index only resolves fewer calls.

### 6. `unresolved-call-filter`

It narrows the unresolved-call group of a `references` answer by receiver.
It is built only if dogfood use of `unresolved-calls` shows that the group is too noisy, or too large for an MCP tool result, for common names.
A query that lists calls by name, which `multi-language-builds` asks about, may make it unnecessary.

### 7. `unreadable-sources`

It depends on no other change, and no other change depends on it.
It fails loudly instead of giving a wrong answer, so it ranks below `unresolved-calls`.
Its size depends on its first open question.

- If an unreadable directory or entry keeps failing the invocation and the error names the path, the change extends the baseline requirement "Source discovery excludes undecodable files".
  It is a few lines and two tests, and it can move first.
- If an unreadable directory or entry is excluded and disclosed instead, the change reverses that requirement and adds a value to the `change` field of the `status` and `build` answers.
  Decide this before agents depend on the `changed`, `added`, and `removed` values those answers carry now.

A persistent list of excluded paths, which this change leaves out of scope, belongs to `c10r-settings`.

### 8. `c10r-settings`

It depends on no other change, so its place depends on how urgent it is.
The chunk parameters are the case that motivates it: a build that omits the chunk flags rebuilds the whole index at the defaults without a report.

The Python indexer is a weaker case.
Since `ty-scip-backend`, a build without `--python-indexer` uses the indexer recorded in the existing store, so the indexer does not reset on later builds.
The indexer reverts to the default only for a new store.
That gap matters when `ty-scip-default` changes the default, so `c10r-settings` comes before `ty-scip-default`.

### 9. `ty-scip-default`

It decides whether ty-scip becomes the indexer for a new Python store.
It cannot be decided until the conditions in Blocked are met.
It follows `c10r-settings`, so that a project can fix its indexer before any store exists.

## Shared specs

Most of these changes modify `code-graph`.
The change that syncs second rebases its delta on the synced text.
These pairs are expected to modify the same requirement:

- The `references` answer, its empty outcome, and `Relationship trace`: `unresolved-calls`, `method-overrides`, and `retained-utility`.
- "A Python build selects its indexer": `c10r-settings` and `ty-scip-default`.
- "Build selects exactly one language backend" and "Unavailable backend is a typed refusal, never a partial index": `retained-utility` and `multi-language-builds`.
- The status report's changed-file classification in `code-navigation`: `unreadable-sources`, if it chooses exclusion.

## Independent

- `unreadable-sources`
- `c10r-settings`

## Blocked

`ty-scip-default` waits for two conditions outside this repository:

- ty-scip emits one definition per name ([ty-scip#19](https://github.com/brendan-m-murphy/ty-scip/issues/19)).
- A comparison gate run passes after that fix.
  The 2026-10-16 run failed edge retention on Flask, httpx2, and Faker, and join acceptance on httpx2 and Faker.

## Satisfied prerequisites

`ty-scip-backend` is archived (2026-10-16).
`ty-scip-default` can use its comparison gate and its `--python-indexer` option.
