# Proposal: unresolved-call-filter

> **Placeholder.**
> This proposal records the intent, stories, and scope of the change.
> The delta specs, `design.md`, and `tasks.md` are not written yet.
> Each open question below needs an explicit decision before the change is implemented.
> This change follows `unresolved-calls`, whose unresolved-call group it filters.

## Intent

`unresolved-calls` adds to every `references` answer a group of the unresolved calls that share the subject's name.
The group lists at most `--limit` calls, has no continuation token, and is sorted by file path.
For a common name such as `get`, the calls shown first are often unrelated to the subject, and the rest are reachable only with `--limit 0`.

Counts of `.get(` calls in the dogfood repositories on 2026-10-17 were 26 in Faker, 123 in ai-vfs, 261 in httpx, 395 in Flask, and 808 in sympy, resolved and unresolved together.
At about 250 bytes per JSON row, several hundred rows can exceed the tool-result size an MCP client accepts, so an agent cannot read the whole group through MCP.

This change is built only if dogfood use of `unresolved-calls` shows that the noise or the size is a real problem.

## User Stories

### Story: narrow-the-unresolved-calls

As an agent auditing a method with a common name, I want to list only the unresolved calls whose receiver could be the method's owner, so that I can check them without reading every call that shares the name.

> Ladders to north-star outcome 5 (**Calibrated trust**) and to the agent-native principle: structured, bounded output.

## Scope

**In scope:**

- A way to restrict the unresolved-call group of a `references` answer by receiver text.

**Out of scope:**

- Ranking or ordering unresolved calls by likelihood.
- Inferring a receiver's type.

## Open Questions

- Does the filter take a literal receiver text, a pattern, or the subject's class name with its common spellings (`cache`, `self.cache`)?
- Does the filter also apply to the unconfirmed-use group?
- Is the filter a flag on `trace`, which changes the command surface and the MCP tool schema, or a separate query?
- Does `retained-utility`'s listing of calls by name make this change unnecessary?
