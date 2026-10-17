# Proposal: Retained Utility Without Semantic Indexing

## Intent

c10r currently has no useful index when its required semantic indexer cannot run, even though it can still parse source code and identify reliable local structure.
This leaves an agent or developer with grep at exactly the time they need orientation in an unfamiliar or partially configured workspace.
This change will retain useful structural navigation without representing it as resolved, type-aware navigation.

## User Stories

### Story: structural-navigation-without-semantic-setup

As an agent or developer, I want c10r to provide reliable structural navigation when I cannot configure a semantic indexer, so that I can orient in a codebase without falling back immediately to unranked text search.

### Story: calibrated-capability-disclosure

As an agent or developer, I want each answer to state the capability that supports it, so that I do not treat a structural result as a resolved semantic fact.

## Scope

**In scope:**

- A structural indexing mode for a language that has a c10r syntax grammar and declaration profile, even when no usable semantic indexer is available.
- A retained set of exact structural answers, including discovered declarations, source locations, source content, and lexical containment.
- Explicit capability disclosure in CLI and MCP answers, including whether the index has structural or resolved-semantic support.
- Typed unavailability for query relations that require resolved cross-file identity, rather than an empty answer or a guessed result.
- An unchanged resolved-semantic path when its semantic indexer is available.
- Bash and sh as motivating candidates for a future syntax profile, not as a promised part of this change.

**Out of scope:**

- Claiming that structural occurrences are resolved references, imports, dependents, or semantic impact.
- Inferring semantic edges from matching names, command spelling, or other heuristic evidence.
  Listing occurrences that share a name, labeled as unresolved and kept out of every relation, is not an inferred edge; `unresolved-calls` defines that answer shape.
- Supporting every language automatically without a syntax grammar and declaration profile.
- Building a Bash or sh semantic analyzer.
- Changing the semantic-index contract for languages that already have a usable semantic backend.

## Approach

Define structural support as a distinct capability tier, not a degraded form of semantic support.
When c10r has an applicable syntax grammar but cannot use the selected semantic backend, it can build an index from declarations, lexical enclosures, source spans, comments, and source content.
The stored provenance and every answer will disclose that tier.

Structural mode should preserve only facts that the syntax tree proves: declaration identity within its source context, lexical containment, and source-derived content.
Queries that require cross-file name resolution must return a typed capability-unavailable result.
This avoids the dangerous ambiguity between "no relationship exists" and "this index cannot determine the relationship."

The public contract must make the retained command set and unavailable relations explicit for both CLI and MCP consumers.
The initial design should use the existing Rust and Python syntax support to prove the tier, while keeping each future language profile independent.

## Open Questions

- Should `build` enter structural mode automatically only when a semantic indexer is unavailable, or should the user select it explicitly?
- Is a failed semantic analysis after the indexer starts eligible for structural mode, or should c10r preserve the current failure so configuration and source errors remain visible?
- Which existing query commands remain available in structural mode, and should unavailable relations be reported as a query result or as a distinct command error?
- How should a structural declaration identity be rendered so that it cannot be confused with the stable semantic identity used by resolved indexes?
- Should the first implementation add a Bash/sh syntax profile, or first prove retained utility with the Rust and Python grammars already in c10r?
- Does structural mode add a query that lists the calls with a given name and pages through them?
  `unresolved-calls` left that query to this change, because in structural mode every call is unresolved.
