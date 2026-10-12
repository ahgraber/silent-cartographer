# Proposal: Outcome Specs and Messages

## Intent

On 2026-10-11, a review of the baseline specs and a review of every user-facing message found two kinds of defect.

First, some spec sentences describe how c10r works inside instead of what a user can observe: the order of internal steps, internal structures such as stored rows or syntax trees, or the route one operation takes through another.
A refactor that keeps every observable behavior would still force those sentences to change.
Some scenarios also check things that no sentence of their requirement promises.

Second, of the 122 warnings, refusals, and errors c10r reports, 59 lack at least one of four things: what failed, why, what it means for the answer, and what to do next.
For example:

- A missing `rust-analyzer` gives no install step.
- A store written by a newer c10r says to rebuild, and a rebuild replaces that newer store.
- A damaged store says to rebuild, but a plain `c10r build` does nothing when the sources are unchanged.
- `stale (environment changed)` is also shown when the environment could not be checked at all.

This change restates the affected requirements as observable outcomes, adds one requirement for what every message must say, and rewrites the 59 messages.
No value in a machine answer and no exit code changes.

## User Stories

### Story: actionable-messages

As a user or agent whose command failed, was refused, or returned a degraded answer, I want the message to say what failed, why, what it means for my answer, and what to do next, so that I can recover without reading c10r's source.

### Existing stories

The restated requirements keep the stories they already had.
Each `Serves:` line repeats the story names from the change that introduced the requirement, for example `honest-heuristic-label` from the archived change `2026-08-01-is-tested`.

## Scope

**In scope:**

- semantic-engine: restate `Same-descriptor definitions extracted as distinct symbols`, `Unavailable backend is a typed refusal, never a partial index`, and `Optional capabilities are queryable` as outcomes.
- code-graph: restate `Per-symbol tier content`, `Staleness reflects underlying change`, `Reference occurrences carry enclosing-declaration attribution`, and `Builds wholly supersede prior derived state`.
  In `Incompatible index stores are replaced or refused, never half-used`, the recovery a refusal names depends on whether the store is older or newer than the binary.
- code-navigation: restate eight requirements that describe internal steps, internal structures, or what a continuation token is tied to internally.
- command-surface: add `Messages say what happened, why, what it means, and what to do`; restate `Shell completion scripts`, `Refresh-hook installation`, and `Refresh timing`.
- mcp-surface: state in `A lifecycle operation proceeds only on confirmed consent` the promise that its scenario already checks.
- eval-telemetry: restate `Cross-Run Comparability` as the fields a trial record carries.
- Rewrite the 59 messages, in the Rust CLI and in the MCP server.

**Out of scope:**

- Requirements that the separate change `canonical-identity` already restated.
- `Mandatory extraction contract` in semantic-engine.
  The separate change `ty-scip-backend` rewrites that requirement, so its wording is settled there.
- The three performance requirements stated as counts of internal work: one parse per document, work per attribution, and traversal work.
  The user chose to keep them.
- Review findings left as they are, each because the current wording is a promise users or implementers rely on:
  - the order in which evaluation arms run, which was fixed before the experiment began;
  - the contract that indexer adapters implement;
  - the workspace name taken from the directory name, which appears in every symbol identity;
  - the `uses` edge kind, which appears in `dependents` output;
  - the scenarios that check the stored chunk parameters, because recording them is itself a code-graph requirement;
  - the dependents scenario that checks answer order, because the requirement promises an unchanged answer and order is part of the answer.
- Any change to values in a machine answer or to exit codes.
  The three questions below are revisited after this change.

## Approach

Each modified requirement is the baseline requirement with only the sentences that its `> Previously:` line describes changed.
Each rewritten message states only what the code knows at the point where it reports, so no message guesses a cause.
Remedies are written as CLI commands and flags.
The MCP server passes the CLI's message through unchanged, as `Command failures surface as typed tool errors` requires, so an agent sees the same remedy.
`messages.md` in this change directory lists every message reviewed, with its current text, which of the four parts it lacks, and proposed text.
Each message has an ID (for example `C12`) that the tasks use.

## Open Questions

To revisit after this change, at the user's request:

- Should `build` refuse to replace a store written by a newer c10r, instead of replacing it?
- Should the machine answer distinguish an environment that changed from one that could not be checked?
  That needs a new `stale_*` value, which changes published output.
- Should the warnings about undecodable file paths print only during a build, instead of on every query?
