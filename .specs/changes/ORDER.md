# Change order

This file records the recommended order in which the active changes are fleshed out and implemented, and why.
Remove an entry when its change is archived.

`force-fresh-store`, `method-overrides`, and `retained-utility` are fully proposed.
The other seven active changes are placeholders: each has a proposal and no delta specs, `design.md`, or `tasks.md`.
The order puts dependencies first, then severity.
Severity ranks a wrong answer above a loud failure, and a loud failure above a missing feature.

## Sequence

01. `force-fresh-store`
02. `method-overrides`
03. `retained-utility`
04. `multi-language-builds`
05. `unresolved-call-hints`
06. `unresolved-call-filter` (conditional; see its section)
07. `scip-range-encodings`
08. `unreadable-sources`
09. `c10r-settings`
10. `ty-scip-default` (blocked; see Blocked)

### 1. `force-fresh-store`

It makes `build --force` build a new store beside the old one and swap it in only on success, and makes every forced build require confirmation.
It fixes no wrong answer, so by severity it would rank low.
It is first by the user's choice on 2026-10-21: it is small and independent, and it builds the confirmation hook and the general wording of "Non-interactive operation" that `retained-utility` then reuses for its own prompt.

### 2. `method-overrides`

Its disclosure goes into the `references` answer, beside the unresolved-call and unconfirmed-use groups.
Its delta modifies `Relationship trace`, whose baseline text changed on 2026-10-20 after the delta was written, so the delta is rebased on the baseline text before it syncs.

### 3. `retained-utility`

It uses the unresolved-call group of the `references` answer for an index with no semantic analyzer, where every call is unresolved.
Its deltas modify four requirements whose baseline text changed on 2026-10-20: "Relationship trace", "Unresolved calls are recorded", "References answers disclose unresolved calls that share the subject's name", and "Empty answers of reference-derived relations claim no proof".
Each is rebased on the baseline text before the change syncs.
It is the largest of the changes.
It goes after the smaller changes that fix its interfaces.
Its structural-build prompt reuses the build's confirmation hook that `force-fresh-store` adds, and its delta of "Non-interactive operation" is rebased on that change's general wording.

### 4. `multi-language-builds`

It indexes every supported language of a workspace into one store, such as Bash scripts inside a Rust workspace.
It depends on `retained-utility`, which adds the Bash profile and records a language and tier for each file.
It follows that change directly, because most shell scripts live inside workspaces of another language.

### 5. `unresolved-call-hints`

It is small and depends on no active change; the unresolved calls it reads are recorded by every build.
It could move up to first, because it is cheap.
It stays fourth because it has the lowest severity: without it no answer is wrong, the index only resolves fewer calls.

### 6. `unresolved-call-filter`

It narrows the unresolved-call group of a `references` answer by receiver.
It is built only if dogfood use shows that the group is too noisy, or too large for an MCP tool result, for common names.
A query that lists calls by name, which `multi-language-builds` asks about, may make it unnecessary.

### 7. `scip-range-encodings`

It makes c10r read the newer SCIP position encoding, `typed_range`, as well as the deprecated `range` that it reads today.
No indexer c10r uses emits only the newer encoding yet, so no answer is wrong today.
If one does, the build drops almost every occurrence without an error, and every answer is empty or partial without notice.
That is a wrong answer, so it ranks above `unreadable-sources`, which fails loudly.
It ranks below the earlier changes because their wrong answers happen today.
It should land before `ty-scip-default` and before any change that adopts a SCIP indexer for a new language.
ty-scip is at 0.1.0 and changing quickly, so it is the indexer most likely to switch encodings; this is a guess, not a measurement.

### 8. `unreadable-sources`

It depends on no other change, and no other change depends on it.
It fails loudly instead of giving a wrong answer, so it ranks below the changes that fix wrong answers.
Its size depends on its first open question.

- If an unreadable directory or entry keeps failing the invocation and the error names the path, the change extends the baseline requirement "Source discovery excludes undecodable files".
  It is a few lines and two tests, and it can move first.
- If an unreadable directory or entry is excluded and disclosed instead, the change reverses that requirement and adds a value to the `change` field of the `status` and `build` answers.
  Decide this before agents depend on the `changed`, `added`, and `removed` values those answers carry now.

A persistent list of excluded paths, which this change leaves out of scope, belongs to `c10r-settings`.

### 9. `c10r-settings`

It depends on no other change, so its place depends on how urgent it is.
The chunk parameters are the case that motivates it: a build that omits the chunk flags rebuilds the whole index at the defaults without a report.

The Python indexer is a weaker case.
Since `ty-scip-backend`, a build without `--python-indexer` uses the indexer recorded in the existing store, so the indexer does not reset on later builds.
The indexer reverts to the default only for a new store.
That gap matters when `ty-scip-default` changes the default, so `c10r-settings` comes before `ty-scip-default`.

### 10. `ty-scip-default`

It decides whether ty-scip becomes the indexer for a new Python store.
It cannot be decided until the conditions in Blocked are met.
It follows `c10r-settings`, so that a project can fix its indexer before any store exists.

## Shared specs

Most of these changes modify `code-graph`.
The change that syncs second rebases its delta on the synced text.
These pairs are expected to modify the same requirement:

- The `references` answer and `Relationship trace`: `method-overrides` and `retained-utility`.
- "A Python build selects its indexer": `c10r-settings` and `ty-scip-default`.
- "Non-interactive operation" in `command-surface`: `force-fresh-store` and `retained-utility`.
- "Build selects exactly one language backend" and "Unavailable backend is a typed refusal, never a partial index": `retained-utility` and `multi-language-builds`.
- The status report's changed-file classification in `code-navigation`: `unreadable-sources`, if it chooses exclusion.

## Independent

- `scip-range-encodings`
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
