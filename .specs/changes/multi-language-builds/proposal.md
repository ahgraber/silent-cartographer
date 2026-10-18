# Proposal: multi-language-builds

> **Placeholder.**
> This proposal records the intent, stories, and scope of the change.
> The delta specs, `design.md`, and `tasks.md` are not written yet.
> Each open question below needs an explicit decision before the change is implemented.
> This change follows `retained-utility`, which records a language and a tier for each file.

## Intent

A build indexes exactly one language.
A workspace with both a Rust and a Python project manifest is refused unless `--language` selects one.
Shell scripts inside a Rust or Python workspace, such as `scripts/*.sh`, are not indexed at all, although `retained-utility` adds a structural Bash profile.

This change recognizes every supported language in a workspace and indexes each one into the same store, each at the tier its indexer allows.

A survey on 2026-10-17 found about 32 code sites and about 20 baseline requirements, in six spec files, that assume one language per store.
The store records one analyzer name, and that name is the only record of the store's language.

## User Stories

### Story: every-supported-language-is-indexed

As an agent or developer in a workspace that mixes languages, I want one build to index every supported language in it, so that I can navigate the scripts and the code that they run without a second store or text search.

> Ladders to north-star outcome 3 (**Structural understanding**) and outcome 1 (**Precise locate**).

## Scope

**In scope:**

- Detection of every supported language in the workspace, in place of the refusal when two project manifests exist.
- One store holding several languages, each with its own provenance and tier.

**Out of scope:**

- Edges between languages, such as a Python function that runs a shell script.

## Open Questions

- When one language's indexer fails and the others succeed, does the build fail as a whole, ask per language as `retained-utility` asks for one, or index the others?
  The baseline requirement that an unavailable backend never produces a partial index forbids the last option today.
- When one of several analyzers changes version, is the whole index stale, or only the answers about that language?
  Staleness reasons and refresh commands would then name the language.
- A Rust package and a Python project with the same name, each with a module `bar`, both produce the identity `ws::foo::bar`.
  Does the identity gain a language, or does the disambiguator separate them?
- Does `doctor` require only the indexers of the languages the workspace uses?
- What do `--language` and `--python-indexer` mean when a build selects several languages?
- After the primary work is complete, does dogfood use of the structural tier show a need for a query that lists calls by name, with paging?
  Such a query answers "where does this workspace run `kubectl`?" for a name with no declaration in the index, which the unresolved-call group of `references` cannot answer.
