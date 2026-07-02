# North Star — silent-cartographer (`c10r`)

> Product intent and enduring principles. Baseline `specs/` define contracts; per-change
> `design.md` records decisions. On any conflict, this north star and the specs win.
> Every change's user stories (in `proposal.md`) MUST ladder to a north-star outcome below;
> every delta-spec requirement that advances one carries a `Serves: <story>` backlink.

## The need

Coding agents (Claude Code and peers) and the humans beside them fail on non-trivial codebases in two compounding ways:

- **Task myopia.**
  The agent models only the files it is touching.
  It cannot innately reason about blast radius, architectural implications, or whether an in-flight change degrades structural health.
- **Search inefficiency.**
  Grep/ripgrep returns plausible, unranked matches.
  On a large codebase a single search can consume most of a context window before a line of code is written.

## What c10r is

A **persistent codebase knowledge graph** that answers precise, type-aware navigation cheaply.
It unifies a **syntax tree** (structure, always fresh) with a **semantic index** (cross-file identity, resolution, and types) into one graph keyed on stable symbol identities.
Users and agents query it — over a CLI and an MCP interface — to find code by symbol, trace the blast radius of a change, and understand structure, at higher precision and far lower token cost than grep.

The single measure of success: **an agent trusts c10r enough to stop grepping.**

## Who it serves

Humans and agents, co-equally, reconciled at the contract layer.
The machine-readable answer is the source of truth; the human-readable view is a render of it.
Agents are the higher-volume consumer; humans reach for c10r for critical "super-LSP" questions.
Neither is a second-class path.

## North-star outcomes (what changes ladder to)

1. **Precise locate.**
   As an agent or developer, I find a symbol's definition and references exactly — no grep, no unranked guesses — so effort goes to the task, not the search.
2. **Blast radius before change.**
   Before altering a symbol, I see its dependents — callers, callees, type usages — so I do not silently break distant code.
3. **Structural understanding.**
   I can ask what a region of code is, what encloses what, and how pieces relate, to orient in an unfamiliar codebase.
4. **Honest under edit.**
   Mid-edit, with the code not compiling, I still get fresh structural answers and clearly-labeled stale-but-precise semantic answers — I am never misled by a confident wrong answer.
5. **Calibrated trust.**
   I can choose precision vs. freshness per query, and every answer carries its provenance and staleness, so I calibrate how far to lean on it.
6. **Find by intent** _(roadmap pillar)._
   I can find code by half-remembered name and, later, by concept — to navigate by what code _does_, not only what it is named.

## Guiding principles

- **Calibration over coverage.**
  The product dies the moment it lies with confidence.
  An honest "I don't know right now" keeps a consumer on the tool; one confidently-wrong blast radius sends them back to grep permanently.
  Under-claim freshness; never over-claim.
- **One identity, two oracles.**
  A single stable symbol identity, with authority assigned by competence: the syntax tree owns structure and enclosure; the semantic index owns cross-file identity, resolution, and types.
  Disagreement is represented, never silently resolved.
- **Precision is the moat; recall is additive.**
  The differentiated value is the exact, type-aware graph.
  Intent/semantic search is built on top of it, not in place of it.
- **Safety by structure, not by caveat.**
  The trustworthy answer is the default response shape; riskier precision is opt-in.
  Tiers never blend; absence is typed, never implied.
- **Adopt over build.**
  Reuse mature parsers and indexers; the unification, the graph, and the product surface are what c10r builds.
- **Agent-native, human-rendered.**
  Structured, deterministic, bounded output first; a readable view layered on top.

## Scope boundaries (non-goals)

- Not a linter, formatter, security scanner, or test runner.
- Not a code generator or refactoring executor — c10r _informs_ changes; it does not mutate code.
- Not a replacement for live language servers — it _consumes_ them; it does not replace the editor loop.
- Not a cross-repository or global code search engine — the focus is one codebase.
- A language is supported only when its syntax and semantic layers unify into precise, type-aware results — breadth never comes at the cost of silent imprecision.

## Current horizon

- **v1 — the structural spine.**
  Trustworthy exact navigation and blast radius for **Rust** and **Python**: a unified syntax+semantic graph, honest staleness, agent-native output with a human render, delivered as an on-demand CLI plus MCP.
- **Beyond v1, as their own changes** — find-by-intent (semantic/embedding search), a live semantic precision overlay, a resident daemon for warm low-latency queries, and additional
  languages (Go, C#, JS/TS).
