# Proposal: scip-range-encodings

> **Placeholder.**
> This proposal records the intent, stories, and scope of the change.
> The delta specs, `design.md`, and `tasks.md` are not written yet.
> Each open question below needs an explicit decision before the change is implemented.

## Intent

A SCIP occurrence can carry its source position in two encodings.
The older one is the `range` field, a list of three or four integers.
The newer one is the `typed_range` choice, which holds either a `single_line_range` or a `multi_line_range`.
The schema in the `scip` crate that c10r pins, 0.10.0, marks `range` as deprecated.
It says that new producers should set `typed_range` and should not set `range`, and that consumers should prefer `typed_range` when it is present and fall back to `range` otherwise.

c10r reads only `range`.
The translation in `src/semantic/scip.rs` skips an occurrence whose `range` is empty, without an error.
scip-python 0.6.6, ty-scip 0.1.0, and rust-analyzer 1.95.0 set `range` today, so every committed fixture and every dogfood build uses it.

An indexer release that writes only `typed_range` would therefore produce an index with almost no occurrences, and the build would not fail.
Every answer from that index would be empty or partial without notice.
The same holds for any SCIP indexer that c10r adopts for a new language: the north star names Go, C#, and JS/TS beyond v1, and adopting their existing indexers is the stated approach ("Adopt over build").
An indexer written against the current schema may emit only the typed encoding.

The aim is that c10r reads every position encoding the SCIP schema defines, so that a conforming SCIP indexer, current or future, for an existing language or a new one, produces the same index from either encoding.

## User Stories

### Story: any-conforming-indexer-is-read-in-full

As a user who upgrades an indexer or builds a language whose SCIP indexer writes the newer position encoding, I want c10r to read every occurrence that indexer records, so that my answers are as complete as with today's indexers.

> Ladders to north-star outcome 5 (**Calibrated trust**): an index that silently lost its occurrences gives confident, wrong answers.
> Ladders to the principle **Adopt over build**: c10r consumes mature indexers, so it must read the schema they are written against.

### Story: an-unreadable-position-is-not-silent

As an agent or developer, I want a build to report occurrences whose position it cannot read, so that a schema mismatch between c10r and an indexer is visible instead of producing an index that looks complete.

> Ladders to north-star outcome 5 (**Calibrated trust**) and to the principle **Safety by structure, not by caveat**: absence is typed, never implied.

## Scope

**In scope:**

- semantic-engine: read an occurrence's position from `typed_range` when it is present, and from `range` otherwise, as the schema directs consumers.
- semantic-engine: the backend conformance suite covers both encodings, so that an adapter whose indexer emits either one passes or fails on the same terms.
- The treatment of an occurrence with no readable position under either encoding.

**Out of scope:**

- The other deprecated-and-typed field pair, `enclosing_range` and `typed_enclosing_range`, unless a decision below brings it in; c10r reads neither today.
- Adopting a new language or indexer.
  That belongs to the change that adds the language; this change only removes one reason such a change would fail.
- Upgrading the `scip` crate, unless the schema version that defines both encodings requires it.

## Open Questions

- When an occurrence sets both encodings and they disagree, does c10r follow the schema's precedence silently, or report the disagreement?
- Is an occurrence with no readable position a refused occurrence in the existing join-discrepancy disclosure, a separate count in the build summary, or a build failure?
- How is the typed encoding tested without an indexer that emits it: a hand-built fixture that converts a committed index's `range` values to `typed_range`, or a committed index from an indexer that already emits the typed form?
- Is `enclosing_range` worth reading in the same change, given that c10r derives enclosing declarations from the syntax tree?
