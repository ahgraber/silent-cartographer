# Proposal: cli-alignment

## Intent

c10r's query engine already answers precise navigation questions, but its command surface is half-built: answers print as a Rust `Debug` dump rather than a legible render, nothing bounds an answer's size, there is no fuzzy locate for a half-remembered name, and the reverse-structure edges the graph already persists (`imports`, `type_hierarchy`) are only reachable bundled inside a transitive `dependents` walk.
There is also no operational surface — no preflight check that the language indexers are installed, no one-command index reset, and no machine-readable self-description an agent can read on arrival.
This change closes those gaps as one coherent pass over the CLI's output-and-interaction contract, so that every command — the ones here and the deferred query commands built after it — shares one bounded, honestly-rendered, scriptable envelope.

## User Stories

### Story: context-bounded-answers

As an agent working within a limited context window, I want every answer bounded in result count and content size, with truncation disclosed and a way to fetch the next page, so that a single query can never blow my context and I always know when I am seeing a partial answer.

> Ladders to the north star's **search-inefficiency** need and outcome #1 (**precise locate** at low token cost) — a query the agent can trust to stay within budget is what lets it stop grepping.

### Story: legible-terminal-output

As a developer reading c10r at a terminal, I want answers rendered as legible text rather than a debug dump — and a retrieved signature, interface, or body shown as source code, the way it reads in the file — so that I can act on an answer, or read the code it returned, without mentally parsing a serialized data structure.

> Ladders to the guiding principle **Agent-native, human-rendered** — the human view is a faithful render of the machine answer, never a second-class path — and outcome #1 (**precise locate**): the retrieved chunk is the code the developer came for, so reading it should feel like reading the file.

### Story: scriptable-cli

As an agent driving c10r programmatically, I want a stable closed set of flags, a defined exit-code taxonomy, and machine output on stdout with diagnostics on stderr, so that I can invoke it deterministically and branch on its outcome without scraping prose.

> Ladders to the guiding principle **Agent-native, human-rendered** (structured, deterministic, bounded output first) and outcome #5 (**calibrated trust**) — a predictable contract is a precondition for leaning on the tool.

### Story: locate-by-fragment

As someone who half-remembers a symbol's name, I want to search for symbols by a name fragment, so that a partial or approximate guess still lands me on the right symbol instead of failing like an exact lookup.

> Ladders to outcome #1 (**precise locate**) and the roadmap pillar #6 (**find by intent**) — this is the first, cheapest rung of find-by-name, ahead of the later typo-tolerant and semantic search.

### Story: trace-reverse-structure

As an agent assessing a change, I want to ask directly who imports a module and who implements a trait, so that I can see the reverse structural dependencies that a call-graph would miss, as first-class one-hop questions rather than by reading a transitive impact walk.

> Ladders to outcome #2 (**blast radius before change**) and outcome #3 (**structural understanding**).

### Story: orient-on-arrival

As an agent dropped into an unfamiliar repo, I want a lean machine-readable index of the CLI surface — the commands and their valid arg and flag values and defaults — so that I can form valid invocations without trial-and-error, following its pointers into `--help` for the per-flag prose and response-shape vocabulary.

> Ladders to outcome #3 (**structural understanding**) and the principle **Agent-native, human-rendered**.

### Story: detect-contract-drift

As an agent built against a prior version of c10r, I want the structural index to carry a surface version, so that I can tell whether my understanding of the contract still holds and adapt rather than silently mis-invoke against a changed surface.

> Ladders to outcome #5 (**calibrated trust**) and the principle **Agent-native, human-rendered** — an agent can only calibrate against a contract whose version it can read.

### Story: preflight-readiness

As someone setting up a repo or a new language, I want to check whether the required language indexers are installed before I build, so that I get a clear "install this" instead of a confusing mid-build failure.

> Ladders to outcome #4 (**honest under edit**) and the principle **Adopt over build** — the tool depends on external indexers, so it must report their absence honestly rather than fail opaquely.

### Story: reset-index

As someone whose index can grow stale or wedged, I want to reset the stored index in one command, so that a bad state is one command away from fixed.

> Ladders to outcome #5 (**calibrated trust**) — recovering a known-good state cheaply is part of trusting the tool at all.

### Story: complete-at-the-shell

As a developer at a terminal, I want shell tab-completion for c10r's commands and flags, so that I can form valid invocations without memorizing the surface.

> Ladders to the guiding principle **Agent-native, human-rendered** — the human surface is first-class, and completion is the terminal-native analog of the machine-readable `manifest`.

## Scope

Capabilities are listed in build-dependency order: the cross-cutting CLI contract (`command-surface`) is defined first; the navigation commands that inherit it (`code-navigation`) follow.

**In scope:**

- **A `command-surface` capability** (new) defining the CLI envelope every command shares (`command-surface`).
  Serves: scriptable-cli, legible-terminal-output, orient-on-arrival, detect-contract-drift, preflight-readiness, reset-index, complete-at-the-shell.
  - A closed flag vocabulary: a fixed set of canonical flags, with the CLI refusing (or never defining) alias spellings, so the surface an agent learns is stable.
  - An exit-code taxonomy that distinguishes success (including a typed-empty answer), usage error, no-index, and indexer/setup failure.
  - Teaching errors: input is validated before side effects, and a rejected invocation's diagnostic names the valid alternatives — the accepted values of an enum flag, the corrected invocation form — so an agent can self-correct from the message alone rather than guess.
  - Non-interactive by default: no command prompts; every input arrives as an argument or flag, and a non-terminal environment is treated as headless.
  - Output discipline: the machine answer on stdout, all diagnostics on stderr; `--json` selects the machine answer verbatim and the default human render is a faithful projection of the same answer.
  - Render discipline for content: a content-bearing detail (signature, interface, body) renders as its source text — multi-line, as it reads in the file — not as an escaped scalar; any color or styling is emitted only to a terminal, never when stdout is redirected or under `--json`, and is controllable by a flag.
  - A `doctor` command reporting each required language indexer's presence and version, and an install hint when absent.
  - A `cache` command that removes the stored index for the workspace, reporting the path affected and failing honestly on an OS error.
  - A `manifest` command emitting the lean, versioned structural index of the CLI surface (Layer 2 of the three-layer introspection model): a surface version, the command list, each command's valid arg/flag values and defaults, plus the current repo/index state.
    It carries structure and version only — the per-flag prose and response-shape vocabulary stay in `--help` (Layer 1), which it points at rather than duplicates.
    Authored last, once the surface it indexes is settled.
  - A completion-script command derived from the same parser definition; README install instructions.
    Serves: complete-at-the-shell.
- **`code-navigation` deltas** — the query commands that consume the contract (`code-navigation`).
  - Output bounding on every result-bearing answer: a per-query result limit, a content-size bound with truncation disclosed, and an opaque continuation token that resumes the next page deterministically.
    Serves: context-bounded-answers.
  - A `find` command: symbol lookup by case-insensitive name fragment, returning the matching symbols bounded by the same limit contract, distinct from `get`'s exact resolution.
    Serves: locate-by-fragment.
  - Two new named `trace` relations — `importers` (the modules that import the subject) and `implementers` (the types that implement the subject trait) — reverse-walking the `imports` and `type_hierarchy` edges the graph already persists, as one-hop relations.
    Serves: trace-reverse-structure.

**Out of scope:**

- **`blast-radius`** (diff-seeded reverse reachability).
  Its net-new work is a git external boundary, not CLI shape; drafted as its own change (`.specs/changes/blast-radius/`), dependent on this contract landing first.
- **`tests` relation / `is_test` classification.**
  Graph enrichment (a heuristic per-language classifier plus a schema bump), not CLI shape; drafted as its own change (`.specs/changes/is-tested/`), dependent on this change's named-relation convention.
- **`callers` / `callees` relations.**
  The graph persists `uses` as reference-grade (any mention, not only a call); offering caller/callee traces over it would conflate a call with any reference — a confidently-wrong answer the north star forbids.
  They wait for a real `calls` edge, a future semantic-precision change.
- **Typo-tolerant and semantic search.**
  `find` here is name-fragment matching only; trigram/edit-distance robustness and vector search belong to the deferred find-by-intent change.
  The reserved `embedding` column and FTS surface stay untouched.
- **Incremental / diff-aware build, and any change to build's indexing behavior.**
  This change touches how the CLI is invoked and how answers are shaped, not how the index is produced.
- **A config file or environment-variable surface, and profiles.**
  No new configuration is introduced; the index path stays the existing flag.
  This consciously declines the agent-native "persistent identity through profiles" principle: c10r is repo-scoped (the index is discovered by path), so per-invocation profiles solve a problem it does not have.
- **Syntax highlighting and theming of rendered source.**
  Content renders as plain, source-faithful text behind a `--color` gate; a highlighter needs a theme, and theme choice is a user preference c10r has no configuration surface for, so highlighting waits for a later config-bearing change and rides additively over the `--color` gate established here.
- **The MCP surface, and the introspection that depends on it.**
  This change is CLI-only: the new tool has no MCP surface yet, and CLI `--help` cannot be guaranteed equivalent to MCP tool descriptions without a shared source, so promising both here would over-claim.
  The MCP server is its own change; it carries its own Layer 1 (MCP tool descriptions), extends `manifest` with the MCP↔CLI mapping, and is the natural home for the Layer 3 long-form `SKILL.md` composition/workflow docs.
  This change therefore delivers Layers 1 (`--help`) and 2 (`manifest`) for the CLI surface only.
- **Async-execution and two-way I/O affordances (`--wait`, a job ledger, `--deliver`, `feedback`).**
  Every c10r operation is synchronous and local, and stdout is the delivery channel; these agent-native principles address long-running remote operations c10r does not have.

## Approach

The change is authored contract-first: `command-surface` fixes the envelope, then `code-navigation` extends the query commands to fill it.

- **Human rendering** replaces the `{:#?}` `Debug` output.
  The JSON answer is the source of truth (the query engine already serializes every answer type via `serde`); the human render is a deterministic projection of that same structure, so the two views can never disagree about membership.
  Rendering is detail-aware: a location result is a one-line `path:span`; a row-bearing answer (`trace`, `find`, `dependents`) is a header line plus one line per result; and a content-bearing detail (`signature`/`interface`/`body`) is rendered as source — the retrieved span printed as it reads in the file.
  Content rendering is plain, source-faithful text, with a `--color=<auto|always|never>` flag gating any styling — `auto` emits it only to a terminal, never to a pipe or under `--json`.
  Syntax highlighting is deliberately not built here: a highlighter needs a theme, and theme choice is a user preference c10r has no configuration surface for, so it waits for a later config-bearing change and rides additively over the `--color` gate this change establishes.
  No new answer data is introduced for rendering; it reads the fields already present.
- **Output bounding** rides the calibrated output contract already in `code-navigation`.
  A result limit caps the row set; a content bound caps per-row tier text with the truncation flagged; a continuation token encodes the query parameters plus a page index so a resumed page is deterministic and a token issued against different parameters is rejected rather than silently mis-paged.
  The token is opaque to callers; its internal composition is a design decision.
- **`find`** matches against the persisted `display_name` column by case-insensitive fragment — no new persisted data, no new extraction.
  It is deliberately the weak first rung: the design records that robust search supersedes it, so shipping it now does not commit the later change to preserving its exact semantics.
- **The two new relations** reverse-walk edges the store already holds (`imports`, `type_hierarchy`), reusing the existing edge tables; they are named relations rather than a `--kind` filter on `dependents`, for discoverability and to keep the folded `trace` surface the project already chose.
- **`doctor` / `cache`** are thin operational commands over surfaces that already exist: `doctor` probes the backends the build already invokes; `cache` removes the index at the known path.
  Both are held to the same exit-code and stdout/stderr discipline as the query commands.
- **`manifest`** is authored last and seeded minimal — a surface version, the command list with valid arg/flag values and defaults, and current repo state — deriving its structure from the contract this change fixes, so it indexes a settled surface and can grow additively.
  It deliberately does not restate the response-shape vocabulary: that is Layer 1 (`--help`), which clap generates from the same doc comments, and which `manifest` points at.
  It does not yet carry an MCP↔CLI mapping — the MCP surface is a later change, and that change extends `manifest` with the mapping once there is an MCP surface to map.

## Open Questions

- **Continuation-token composition.**
  What the opaque token encodes (a hash of query parameters plus a page index, versus an offset, versus a keyset cursor) is a design decision — the contract only requires that resuming is deterministic and a parameter-mismatched token is refused.
  Resolved in `design.md`.
- **Human-render layout.**
  The exact per-command header and row format (field order, how truncation and staleness are shown) is a rendering decision, not a contract one; the contract is only that the render is a faithful, deterministic projection of the JSON answer.
  Resolved in `design.md`.
- **`manifest` payload boundary.**
  Exactly which structural fields the first version emits — and where the Layer 2 / Layer 1 line falls (e.g. valid `--relation` and `--detail` values are input _constraints_ and belong in `manifest`, whereas the edge-kind/confidence vocabulary that appears in _responses_ stays in `--help`) — is a scoping decision for `design.md`; the contract floor is a surface version plus the command list with each command's valid arg/flag values and defaults.
- **Forward relation pairs.**
  Whether `imports` (what a module imports) and an `implements` (what a type implements) forward direction are also wanted, or only the reverse `importers`/`implementers`, is a small surface decision deferred to `design.md` under YAGNI — added only if a story needs them.

## References

- [10 Principles for Agent-Native CLIs - by Trevin Chow](https://trevinsays.com/p/10-principles-for-agent-native-clis)
