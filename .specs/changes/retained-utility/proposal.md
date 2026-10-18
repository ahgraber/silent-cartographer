# Proposal: retained-utility

## Intent

c10r builds an index only when a semantic indexer runs: rust-analyzer for Rust, and scip-python or ty-scip for Python.
Every stored symbol takes its identity from the indexer, and a declaration that the indexer does not report is counted but not stored.
So c10r has nothing to offer for a language that has no semantic indexer, such as Bash.
It also has nothing to offer for a Rust or Python workspace whose indexer is not installed or fails, such as an agent's sandbox without Node, or a workspace whose Python environment c10r cannot resolve.
In each case the agent or developer falls back to unranked text search.

The syntax tree alone still proves useful facts: which declarations a file holds, what encloses each one, its source text, and which calls name it.
This change adds a structural tier: an index built from the syntax tree alone, whose every answer states its tier.
A relation that needs a name resolved across files is reported as unavailable, never as empty.
The first language that has only the structural tier is Bash.

## User Stories

### Story: structural-navigation-without-semantic-indexer

As an agent or developer in a codebase whose language has no semantic indexer, such as Bash, or whose Rust or Python indexer cannot run, I want c10r to index declarations, their nesting, their source text, and the calls that name them, so that I can orient and find code without falling back to unranked text search.

> Ladders to north-star outcome 3 (**Structural understanding**) and outcome 1 (**Precise locate**) for definitions.

### Story: calibrated-capability-disclosure

As an agent or developer, I want each answer to state whether a structural or a semantic index supports it, and a relation the index cannot answer to be reported as unavailable, so that I do not treat a structural result as a resolved semantic fact or an unanswerable relation as an empty one.

> Ladders to north-star outcome 5 (**Calibrated trust**) and to the principle **Safety by structure, not by caveat**: tiers never blend, and absence is typed.

### Story: consent-before-structural-index

As a developer whose Rust or Python indexer cannot run, I want c10r to ask before it builds a structural index, and an agent to opt in with a flag, so that a semantic index is never replaced by a structural one without a request, and the request names what the structural index cannot answer.

> Ladders to north-star outcome 4 (**Honest under edit**): a failing indexer during an edit leaves the stale semantic index in place.

## Scope

**In scope:**

- symbol-identity: a structural identity names a declaration by its workspace, the path of its file relative to the workspace root, and the declarations that enclose it in that file, for example `ws::./scripts/deploy.sh::deploy`.
  A structural identity cannot be equal to a semantic identity.
  A query against a semantic index that receives a structural identity, or the reverse, reports that it denotes no symbol and names the kind of identity it received.
- code-graph: a structural build indexes the declarations, lexical containment, source text, and calls that the syntax tree of each file proves.
  Every call is recorded as an unresolved call, in the form `unresolved-calls` defines.
  The structural build records no references, imports, dependents edges, type-hierarchy edges, or test classifications.
- code-graph: the store records the language and the tier of each file.
  Every file in one store has the same language and tier; `multi-language-builds` relaxes that.
- code-graph: a Bash language profile, parsed with tree-sitter-bash.
  Its declarations are functions, in both the `name() { … }` and the `function name { … }` forms.
  Its calls are command invocations.
  Its files are those with a `.sh` or `.bash` extension, and extensionless files whose first line is a `sh` or `bash` shebang.
- code-graph: build selection.
  A workspace with no Rust or Python project manifest and at least one Bash file builds as Bash.
  `--language bash` selects Bash explicitly.
  A Bash build is always structural.
- code-graph: a Rust or Python build is structural only on request.
  When its indexer is missing or fails and a terminal is attached, the build asks whether to build a structural index instead, and states what that index cannot answer.
  A headless build requests the structural tier with a flag.
  Without a confirmation or the flag, the build fails as it does today and the failure names the flag.
  A store built structurally on request stays structural on later builds without asking again, until a build with a working indexer replaces it.
- semantic-engine: the requirement that an unavailable backend never produces a partial index keeps its refusal, and the refusal names the way to request a structural index.
- code-navigation: every answer and the `status` report state the index's tier.
- code-navigation: a new answer outcome, `unavailable`, states that the index's tier cannot answer the requested relation, and why.
  On a structural index, `references`, `importers`, `implementers`, `implements`, `tests`, and `dependents` answer `unavailable`.
  The `references` answer still carries the unresolved-call group for the subject's name.
  An `impact` answer lists the declarations the change touches and reports their dependents as unavailable.
  `get`, `find`, `containers`, `contains`, `search`, `similar`, and `clones` answer as they do on a semantic index.
- command-surface: an `unavailable` answer exits with a code of its own.
  `Non-interactive operation` gains one exception: the structural-index confirmation, asked only when a terminal is attached.
  `manifest` reports the index's tier.
- mcp-surface: an `unavailable` answer reaches the caller as a tool result, not as an error, so its unresolved-call group survives.
  The `build` tool's parameters follow the installed surface, so it accepts the structural flag and Bash.

**Out of scope:**

- Indexing more than one language in one store, such as Bash scripts inside a Rust or Python workspace.
  `multi-language-builds` owns that.
- Inferring a reference, import, dependents edge, or base class from matching names.
- A query that lists calls by name with paging.
  The unbounded `references` answer lists every call in the unresolved-call group.
  `multi-language-builds` records the question for dogfood use after the primary work.
- Which indexers `doctor` requires.
  `doctor` checks the host, not a workspace, so it has no tier to report; `multi-language-builds` asks which indexers it requires.
- Bash aliases, variables, and `source` resolution.
- zsh, fish, and other shells whose syntax tree-sitter-bash does not parse.
- Any change to a semantic build when its indexer runs.

## Approach

A structural build parses each file with the language's grammar and stores the declarations, containment edges, source text tiers, and calls the syntax tree yields.
It runs no indexer and no join.
Identities come from the file path and the nesting in the file, so they need no resolution.

Provenance names the grammar and its version in place of an analyzer, so the existing staleness rules mark a structural index stale when the grammar changes.

The design includes a probe that parses a set of real shell scripts with tree-sitter-bash and compares its functions and calls with the output of `shfmt --to-json`, before the Bash profile is fixed.

The store schema version changes, because the store records a language and tier per file.
The command-surface version changes, because `build` gains a flag and `--language` gains a value.

## Open Questions

None.
