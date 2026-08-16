# Proposal: mcp-surface

## Intent

The north star commits v1 to "an on-demand CLI plus MCP," and only the CLI exists.
An agent in an MCP host reaches c10r today only by composing shell invocations, which puts the burden of learning the flag vocabulary on the agent and leaves c10r competing with grep on grep's own terms.

This change adds the MCP interface as a wrapper over the existing CLI: a Python server, built on FastMCP, that runs the installed `c10r` binary as a subprocess and returns its structured answer unmodified.
Nothing about the CLI changes.

The wrapper is deliberately not a redesign of the question surface.
The CLI's command split, detail axis, relation vocabulary, and bounding model were designed for this consumer already; re-partitioning them for MCP would create a second information architecture with no rationale the first one lacks, plus a mapping to keep aligned.
The tools mirror the commands.

Three things make this more than plumbing.
The wrapper must not lie about the surface it exposes — a tool that offers a flag the installed binary dropped is exactly the confidently-wrong answer the north star says kills the product, so `c10r manifest` becomes a conformance oracle at test time and a compatibility gate at startup.
The wrapper must scope every answer to the right project, because an MCP server has no working directory in the sense the CLI does, and a misrouted answer is confidently wrong in the same way.
And the wrapper must serve MCP clients on both sides of the `2026-07-28` protocol revision, which removed the `initialize` handshake and, with it, the mechanism the older elicitation flow depended on.

## User Stories

### Story: tools-not-shell

As a coding agent in a host that exposes MCP tools, I want c10r's queries as named tools with typed parameters, so that I reach for them directly instead of learning and composing a shell command line.
Ladders to the north-star success measure ("an agent trusts c10r enough to stop grepping") and the "Agent-native, human-rendered" principle: an agent that must construct a shell invocation to ask a question will keep reaching for the tool it already knows.

### Story: faithful-answers

As an agent consuming a c10r answer through MCP, I want the answer to be exactly the structured answer the CLI produces, so that provenance, freshness, typed absence, and heuristic-grade labels reach me intact rather than being summarized away.
Ladders to north-star outcome #5 (Calibrated trust) and outcome #4 (Honest under edit): the calibration labels are the product, and a wrapper that reshapes the payload can drop them.

### Story: surface-truth

As an agent choosing and invoking a tool, I want the tool's parameters and stated defaults to match the `c10r` binary actually installed, so that I never invoke an option that no longer exists or act on a default that has changed.
Ladders to north-star outcome #5 (Calibrated trust) and the "Calibration over coverage" principle: a tool schema is a claim about the world, and a stale claim is a confident lie.

### Story: project-scoped-answers

As a developer running several agent sessions across different projects at the same time, I want every answer scoped to the project its session is about, so that concurrent sessions never read each other's index.
Ladders to north-star outcome #5 (Calibrated trust): an answer that is correct about the wrong workspace is indistinguishable from a correct answer until it causes damage.

### Story: self-service-setup

As an agent that opens a repository with no index, I want to build the index and install the freshness hook through tools that gate every such operation behind a confirmation I cannot give myself, so that I can start answering questions without handing the task back to the human wherever the host can carry that confirmation, and never start an expensive or repository-mutating operation by accident.
Where the host cannot carry it, I want to be told so and given the command that does the job, rather than the operation proceeding on my say-so alone.
Ladders to the north-star success measure: an agent that meets an unindexed repository and gives up returns to grep and does not come back.

### Story: every-client

As a user whose MCP host may sit on either side of the `2026-07-28` protocol revision, I want one server that serves both, so that c10r works in the host I have without my tracking protocol revisions or running two servers.
Ladders to the current-horizon commitment that v1 ships "an on-demand CLI plus MCP": an MCP surface that only some hosts can speak does not deliver that.

## Scope

**In scope**, ordered by build dependency:

- mcp-surface: a uv-managed Python package in this repository, distributed as a runnable MCP server, depending on FastMCP pinned to an exact pre-release.
- mcp-surface: workspace resolution — a launch-time default (an explicit argument or environment variable, falling back to the server process's own working directory) with a per-call override, refusing to serve a directory that does not resolve.
- mcp-surface: invocation and answer fidelity — every tool runs the installed `c10r` binary with its working directory set to the resolved workspace root and `--json`, returns the resulting structured answer unmodified, and maps c10r's exit-code taxonomy onto typed tool errors that carry c10r's own diagnostic text.
- mcp-surface: query tools mirroring the read-only commands one to one — `get`, `trace`, `find`, `search`, `similar`, `impact` — with the CLI's own bounding controls exposed as parameters.
- mcp-surface: gated lifecycle tools — `build` and `hooks install` — that refuse to act without an explicit per-call acknowledgment, then seek interactive confirmation through the negotiated protocol era's own mechanism, and refuse — naming the command that performs the operation — when the client cannot be asked at all.
- mcp-surface: protocol-era support — one server serving both the handshake era and `2026-07-28`, with era-appropriate confirmation on each.
- mcp-surface: surface conformance — a startup gate that refuses to serve when the installed binary's surface version differs from the one the server was written against, naming both versions and the remediation, and a test that cross-checks every tool's parameters, accepted values, and stated defaults against `c10r manifest`.
- mcp-surface: server instructions telling a client when to prefer c10r over textual search, and how to recover from an absent or incompatible index.

**Out of scope:**

- Any change to c10r's CLI contract, exit taxonomy, output shape, or Rust source.
  This change consumes the existing surface; if it finds a gap, that is a separate change.
- Exposing `cache` (removes the store), `status`, `doctor`, `manifest`, or `completions` as tools.
  `cache` is destructive; the other four answer questions an MCP client either does not have (the tool schemas already describe the surface) or already receives inline (every answer carries its own provenance and freshness).
- A composite setup tool that fuses building and hook installation.
  It would be a capability the CLI does not have, it would need defined partial-success semantics because hook installation refuses rather than overwrites an existing hook, and it would place a `.git` write and an index write behind a single acknowledgment.
  Adding it later is additive; removing it later is breaking.
- Any transport other than stdio, and therefore authorization, multi-tenancy, and horizontally scaled deployment.
- MCP resources and prompts; this change exposes tools only.
- Reshaped, compacted, or re-labeled output.
  The CLI's own bounding controls are exposed as tool parameters, which is where result size is already governed.
- Sampling and roots, both deprecated by the `2026-07-28` revision.
- Background execution of `build` through the tasks extension.
  Every tool runs to completion within its invocation; a build that outlasts the client's timeout is recovered by running the command in a shell, which the refusal names.
- Publication to a package index, and bundling the `c10r` binary with the server.
  The server invokes a `c10r` the user installed.

## Approach

Mechanism sketch, to be formalized in `design.md`.

**Working directory is the whole of workspace scoping.** c10r resolves everything relative to the process's working directory: `--db` defaults to the relative path `.c10r/index.db`, the store's recorded-root comparison canonicalizes `"."`, and `impact` runs git there.
So the wrapper translates no paths into flags — it sets the subprocess's working directory to the resolved workspace root, and store discovery, freshness, workspace-mismatch disclosure, and diff seeding all follow from that one decision.
Isolation between concurrent sessions is then structural rather than engineered: the store path derives from the workspace root, so distinct projects are distinct files, and with no write path on the query tools, concurrent readers are all that exist.

**Faithful passthrough costs almost nothing.**
Every c10r answer wraps its results in a small fixed envelope — analyzer name and version, freshness label, stale flag, outcome tag — and omits every optional disclosure that does not apply.
The result size lives in the rows, which the CLI's `--limit`, `--detail`, `--max-lines`, and `--from` already govern.
Exposing those as tool parameters gives the caller the same control a second output shape would, without a second contract to specify, test, and keep aligned.

**`manifest` is the anti-drift mechanism, used two ways.**
The tools are hand-written so their descriptions can be composed for a model choosing between tools rather than for a human reading `--help`.
A test then holds those hand-written descriptions to account: it runs `c10r manifest` and asserts that every tool parameter maps to a real option, every accepted value is in the manifest's valid set, and every default a description states matches the manifest's default.
At runtime, the server compares the binary's surface version against the version it was written against and refuses to serve on any difference.
That refusal is a version-alignment failure and its remediation says so — upgrade the server package or pin the binary — which is a different failure from an absent or incompatible index, whose remediation is a rebuild and which arrives per call carrying c10r's own diagnostic.

**The protocol revision reaches stdio, and splits the confirmation path.**
Session identifiers never existed on stdio, so their removal changes nothing here.
The removal of the `initialize` handshake is protocol-level rather than transport-level, so it does reach stdio: a modern client sends no handshake and carries its protocol version and capabilities on every request.
The consequence that costs work is elicitation.
On the handshake era a tool pauses mid-execution and asks; on `2026-07-28` that back-channel is gone, so a tool asks by returning a description of what it needs and is re-invoked with the answer attached.
The two are strictly gated — using the wrong one raises an era error — so a server that supports both implements both and branches on the negotiated protocol version.

**A lifecycle tool is guarded twice, and the two guards guarantee different things.**
An acknowledgment parameter guarantees deliberateness: the tool refuses until the caller sets it, and the refusal is where the cost is disclosed, so an agent cannot stumble into a rebuild while exploring.
It does not guarantee consent — the agent sets the parameter itself.
Elicitation is what gives the server consent it can verify, and it is a client capability rather than a guarantee: a client with no elicitation handler cannot be asked anything.
Rather than treat that as license to proceed on the agent's own say-so, a client that cannot be asked is refused, and the refusal names the command that performs the operation.
The server wraps that command, so the remedy is always present by construction and the caller is never left without a route.
The contract floor is therefore what every client can observe rather than what every client can answer: the operation happens only behind consent the server verified, or not at all, and which of the two occurred is always stated.

**Tools are named for the commands they mirror.**
The protocol does not namespace tool names, and hosts prefix them with the server's name when presenting them, so a self-applied `c10r_` prefix would stutter rather than disambiguate.

## Open Questions

- **Python tooling floor for a Rust repository.** `ruff` linting and formatting are already configured in `.pre-commit-config.yaml`, so a Python subtree lints from day one.
  No type checker is configured, and this change introduces the repository's first Python package.
  Whether to add one, and which, is unresolved.

- **Re-pinning when FastMCP 4 reaches final.**
  The exact pre-release pin is deliberate, because a range invites a pre-release-to-pre-release break landing silently in a lockfile refresh.
  It also means an explicit follow-up is required rather than optional.

- **Whether a long `build` needs a route that survives a client timeout.**
  Background execution is out of scope, so a build that outlasts the client's request timeout is recovered by running the command in a shell.
  Whether that is sufficient in practice is unmeasured.
