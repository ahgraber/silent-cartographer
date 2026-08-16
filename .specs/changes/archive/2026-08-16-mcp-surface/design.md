# Design: mcp-surface

## Context

The CLI is complete and stable, and this change does not touch it.
Everything below describes a consumer of an existing contract.

Three facts about the existing system shape the design more than anything else.

`c10r` resolves its whole world from the process's working directory.
`--db` defaults to the relative path `.c10r/index.db`, the store's recorded-root comparison canonicalizes `"."`, and `impact` runs git in place.
No command takes a workspace root as a global option.

`c10r manifest` already emits a versioned structural index of the command surface — commands, options, valid values, defaults — and its surface version changes whenever that structure changes.
It was built so a caller could detect that its understanding of the surface had gone stale.
This change is the first caller to use it that way.

Every store records the workspace root it was built for, and any answer read from a store describing a different root carries a disclosure marker.
Misrouting is therefore already visible in the answer rather than silent.

Two facts about the MCP ecosystem constrain the rest.
The `2026-07-28` specification revision removed the `initialize` handshake and, with it, the server-initiated back-channel the older elicitation flow used.
Session identifiers never existed on stdio, so their removal is irrelevant here, but the handshake removal is protocol-level rather than transport-level and does reach stdio.
FastMCP 4 — currently a pre-release — is the only version that serves both protocol eras from one process.

## Decisions

### Decision: Host the server as an in-repo Python package on FastMCP

**Chosen:** A uv-managed Python package under `mcp/` in this repository, exposing a stdio MCP server built on FastMCP, invoking the user's installed `c10r` binary as a child process.

**Rationale:** FastMCP is the requested host, and it is the only implementation that serves both protocol eras from a single process today.
Keeping the package in this repository keeps the surface-version pin, the conformance test, and the CLI it mirrors in one commit and one review.
`ruff` linting and formatting are already configured in `.pre-commit-config.yaml`, so a Python subtree is covered by the existing gate from the first commit.

**Alternatives considered:**

- Serve MCP in-process from the existing Rust binary using the official Rust SDK, as a `c10r mcp` subcommand: one artifact, one toolchain, no child process, and tool schemas derivable from the same `clap` tree that already feeds `manifest` and completions.
  Rejected because the user selected FastMCP; recorded because it remains the lower-moving-parts option if the Python subtree becomes a maintenance burden.
- A separate repository: keeps this repository Rust-only, but puts a cross-repository sync obligation on every surface-version bump — exactly the drift this design spends effort preventing.

### Decision: Pin FastMCP to an exact pre-release

**Chosen:** Pin `fastmcp` to one exact pre-release version rather than a range.

**Rationale:** Dual-era service requires FastMCP 4, whose only releases are pre-releases; there is no stable version that satisfies the requirement.
A range across pre-releases invites a breaking change landing silently in a lockfile refresh, which is the failure mode a lockfile exists to prevent.

**Alternatives considered:**

- FastMCP 3 (the stable line): cannot serve modern-protocol clients at all, so it fails the `every-client` story outright.
- A permissive range on the 4 pre-releases: cheaper to maintain, but converts an upstream pre-release break into an unreproducible local failure.
- Waiting for 4.0.0 final: defers the whole change on an upstream schedule this project does not control.

### Decision: Scope the workspace by the child process's working directory

**Chosen:** Every invocation runs `c10r` with its working directory set to the resolved workspace root, and passes no path-bearing flags derived from that root.

**Rationale:** This is the single lever that scopes everything at once.
Store discovery, the store's recorded-root comparison, freshness evaluation, and `impact`'s git seeding all resolve from the working directory, so setting it correctly makes all four correct together.
Translating the root into flags would require the wrapper to know which commands take which path options and to keep that knowledge aligned with the CLI — a second copy of the surface, which is the thing this design is trying not to build.
It also means cross-workspace isolation is structural: the store path derives from the root, so distinct roots are distinct files with no coordination.

**Alternatives considered:**

- Pass `--db` computed from the resolved root: scopes the store correctly and leaves `impact`'s git seeding pointed at the wrong repository, and leaves the recorded-root comparison comparing against the wrong directory — a silent mismatch in exactly the disclosure meant to catch it.

### Decision: Resolve the workspace per call, over a launch-time default

**Chosen:** Precedence is the root named on the invocation, then the root the server was explicitly configured with at launch, then the directory the server process itself resolves to.
A configured root that is not an existing directory refuses at startup rather than falling back.

**Rationale:** The two install shapes need different halves of this.
A per-repository install puts the server entry in the project's own configuration, so the launch-time default is already correct and the agent passes nothing — no path in the model's hands on the common path.
A global install depends on the host launching the server's child process in the session's directory, which is normal for stdio servers but is host-specific behavior this project does not control; the per-call override is what makes that case work regardless.
The refusal on an unresolvable configured root exists because the fallback would otherwise be silent, and a server quietly answering about its own launch directory instead of the project it was configured for is the confidently-wrong answer this product treats as fatal.

**Alternatives considered:**

- Launch-time only: simplest schemas and no path parameter, but bets the global-install case entirely on host behavior, with no recourse when the bet is wrong.
- Per-call only, with the root required on every tool: works everywhere, but puts a required path parameter on every call and a filesystem path in the model's output on every call, for no benefit in the common per-repository case.

### Decision: Pass the answer through unmodified and expose the CLI's bounding controls

**Chosen:** Each tool invokes its command with `--json` and returns the resulting structured answer as the tool's result, unmodified.
Result size is governed by exposing `--limit`, `--cursor`, `--detail`, `--max-lines`, and `--from` as tool parameters.

**Rationale:** The answer envelope is already small — analyzer name and version, a freshness label, a stale flag, an outcome tag, and a set of optional disclosures omitted entirely when they do not apply.
The size that matters lives in the rows, which those five controls already govern.
Reshaping would therefore buy little and cost a second output contract to specify, test, and keep aligned with the CLI's, plus a standing risk that a reshaping step drops one of the calibration labels the product depends on.

**Alternatives considered:**

- Reshape for token budget: a second contract, and the labels most at risk of being trimmed are the ones the north star is most protective of.
- Faithful by default with an opt-in compact mode: still two shapes to specify and test, and the compaction it would offer is already available through the five parameters.

### Decision: Map exit codes to typed errors carrying the diagnostic verbatim

**Chosen:** Each of the CLI's non-success exit codes maps to a distinct tool error carrying the command's standard-error text unaltered.
Exit code 0 always produces a result, including a typed-empty answer.

**Rationale:** The CLI already validates before acting and writes rejections that name the valid alternative or the recovery — an absent index names the rebuild, an unrecognized store names both recoveries.
Reproducing that guidance in the wrapper would duplicate text that changes on the CLI's schedule.
Carrying it through verbatim means the recovery instructions stay correct without the wrapper knowing what they say.
The exit code supplies the category the caller branches on; the text supplies the human-readable specifics.

**Alternatives considered:**

- Wrapper-authored error messages per category: reads more consistently in an MCP context, and goes stale the moment the CLI's guidance changes.

### Decision: Hand-write the tools and hold them to `manifest` with a conformance test

**Chosen:** Tools are ordinary hand-written functions with descriptions composed for tool selection.
A test then runs `c10r manifest` and asserts that every parameter names a real option, every accepted enumerated value lies in the reported valid set, and every default a description states matches the reported default.

**Rationale:** Tool descriptions are read by a model choosing between tools; `--help` text is read by a person who has already chosen.
Those are different jobs, and generating descriptions from the second to serve the first gives up the thing that most determines whether an MCP server is used at all.
The conformance test recovers what generation would have given for free: the descriptions stay hand-written, but every factual claim in them is checked against the binary rather than trusted.

**Alternatives considered:**

- Generate tool schemas from `manifest` at startup: drift becomes structurally impossible, at the cost of descriptions written for the wrong audience and a dynamic tool surface that is harder to read and review.
- Build-time code generation with a check that regeneration produces no diff: keeps the static, reviewable module, but still derives descriptions from help text, and adds a generator to maintain.

### Decision: Refuse to serve on any surface-version difference

**Chosen:** The server compares the installed binary's reported surface version against the version it was built against at startup, and refuses to serve on any difference, naming both versions and the version-alignment remediation.

**Rationale:** The surface version changes when any command's structure changes, so a difference does not prove that a covered tool is affected — refusing is therefore stricter than strictly necessary.
It is chosen anyway because the alternative requires deciding, per tool, whether a structural change touched it, and being wrong in that judgment produces exactly the failure this gate exists to prevent: a tool that offers an option the binary no longer has.
A loud startup refusal with two version numbers and a remediation is cheap to act on; a subtly wrong tool schema is not.

This refusal is deliberately distinct from an index-state failure.
A version difference is fixed by aligning versions — upgrade the server package, or pin the binary.
An absent or incompatible index is fixed by rebuilding, arrives per call rather than at startup, and carries the CLI's own diagnostic.
Conflating them would send a caller to the wrong remedy.

**Alternatives considered:**

- Disable only tools whose schema no longer conforms and serve the rest: more available, but it makes the server's tool list vary with the installed binary in a way the caller cannot predict, and it requires the per-tool judgment described above.
- Warn and serve: the failure it warns about is a wrong answer, which is the one outcome the north star will not trade availability for.

### Decision: Six query tools named for their commands

**Chosen:** `get`, `trace`, `find`, `search`, `similar`, `impact`, plus `build` and `hooks_install`, named without a prefix.
`trace`'s relation stays a parameter rather than becoming seven tools.

**Rationale:** The CLI's partitioning of the question space was designed for this consumer; re-partitioning it for MCP would create a second information architecture with no rationale the first lacks, and a mapping to maintain between them.
`find`, `search`, and `similar` stay separate rather than folding into one tool with a mode parameter because they carry different trust grades — `find` is an exact match whose empty answer is a definite none, while `search` and `similar` are model-derived estimation and are labeled as such — and merging them would present three different guarantees as one tool with a knob.
The protocol does not namespace tool names and hosts prefix them with the server's name when presenting them, so a self-applied prefix stutters rather than disambiguates.

**Alternatives considered:**

- A single dispatch tool taking a command name and an argument map: minimal context cost and zero drift, but gives up per-command schema validation and leaves the model choosing between one opaque tool and grep.
- Three to four broad tools folding commands into mode parameters: fewer schemas, at the cost of blending trust grades.
- Seven relation tools instead of one `trace`: more discoverable per relation, but the relation set is a closed vocabulary that a parameter expresses exactly, and it triples the tool count for one command.

### Decision: Gate lifecycle tools with an acknowledgment parameter, then require confirmed consent

**Chosen:** `build` and `hooks_install` refuse until the invocation sets that tool's acknowledgment parameter, and the refusal states the cost.
The tool then obtains the caller's confirmation before acting.
A client that offers no elicitation capability is refused rather than proceeding, and the refusal names the command that performs the operation.

**Rationale:** The two guards guarantee different things and neither substitutes for the other.
The acknowledgment parameter guarantees deliberateness — the tool cannot be triggered by an agent exploring the surface, and the refusal is where the operation's cost is disclosed — but not consent, since the agent sets the parameter itself.
Elicitation is what gives the server consent it can verify.
Treating its absence as license to proceed would make the strength of the guard depend on a client property the caller never sees, so the same acknowledged invocation would be gated on one host and ungated on another with nothing in the answer to distinguish them.
Refusing instead keeps the guarantee constant: the operation happens behind verified consent or not at all, and which one occurred is always stated.

That refusal is affordable only because the remedy is guaranteed to exist.
The server's whole mechanism is running the CLI, so every operation it can refuse has a command that performs it, and the refusal names that command.
This is the contract floor this project's spec guidance requires, taken at what every client can _observe_ rather than what every client can _answer_ — the weaker of those two would license the silently-ungated write.

Most hosts additionally prompt a human before an un-allowlisted tool call, but that is host behavior the server can neither require nor observe, so it cannot stand in for the confirmation and is not part of the contract.
The cost is real: on a host with no elicitation support, self-service setup ends at a refusal naming a command a human must run.

**Alternatives considered:**

- Elicitation optional, the acknowledgment parameter governing alone where it is absent: keeps self-service setup working on every host, at the price of a repository-mutating write whose only gate is a parameter the agent set for itself, with nothing in the answer saying so.
- The acknowledgment parameter as the only gate: removes both era-specific implementations, and gives up consent the server can verify before writing into `.git`.

### Decision: Run every tool to completion within its invocation

**Chosen:** No background execution.
Every tool, `build` included, runs synchronously; a build that outlasts the client's request timeout is recovered by running the command in a shell, which the refusal path already names.

**Rationale:** Background execution and confirmed consent cannot both hold inside the tool body, because a task's body runs in a worker with no MCP session, and the client's declared capabilities are not carried into it — the submission snapshot carries the access token, headers, request id, session id, and owning tool name, and nothing about capabilities.
So a background lifecycle tool cannot tell whether the client can be asked, and must either ask blindly — which fails outright on a client that cannot answer, with an error raised inside the client's own driver that this server cannot shape or replace with its remedy — or skip the ask, which is the silently-ungated write the decision above rejects.

Confirming ahead of dispatch resolves that and was verified to work, but it buys back only timeout resilience, and it is not free either: the extension providing task execution is built on a distributed queue whose in-process mode still installs its Redis client, cron scheduler, metrics exporter, and serializer, none of which a single stdio server runs.
Paying that, plus a second place where era branching lives, to protect against a timeout whose remedy is already a documented shell command, is not a trade this change makes.

**Alternatives considered:**

- Background execution with the confirmation moved ahead of dispatch: keeps a long build alive past the client's timeout, and costs the dependency weight above plus era-branching in a second layer.
- Background execution with `build` gated by its acknowledgment parameter alone: cheapest way to keep both features, and it is exactly the silently-ungated write the consent decision rejects.

### Decision: Implement both era-specific elicitation paths

**Chosen:** Branch on the negotiated protocol version and use the era's own mechanism: a mid-execution request on the handshake era, and a returned input request that the client answers by re-invoking the tool on `2026-07-28`.

**Rationale:** There is no compatibility shim to choose instead.
The two mechanisms are strictly gated — using the handshake mechanism on a modern connection, or the modern mechanism on a handshake connection, raises an era error rather than degrading — so a server that supports both eras and elicits at all must implement both and select at request time.
Confining that branch to one confirmation helper keeps it out of every tool.

The modern path re-runs the tool from the top on each round, so the tool holds no state between rounds and must be written to re-derive what it needs from its arguments and the carried request state.
For a single stdio server, the default per-process sealing key for that carried state is correct; no key configuration is needed.

**Alternatives considered:**

- Support elicitation on the handshake era only: silently drops verified consent for modern clients, which is the population that grows.
- Support it on the modern era only: drops it for every client shipping today.

### Decision: Validate enumerated values in the schema, and leave cross-parameter validity to the CLI

**Chosen:** Parameters with a closed value set are typed as enumerations, so the schema rejects an out-of-set value before any command runs.
Constraints spanning more than one parameter — a depth or ordering option that is only meaningful with one particular relation — are not re-checked in the wrapper; the command rejects them.

**Rationale:** Typing the closed sets puts the valid values in the tool schema where the model can read them before choosing, which prevents the error rather than reporting it.
Cross-parameter constraints cannot be expressed in a tool schema, and re-implementing them in the wrapper would duplicate validation logic that the CLI already owns and would drift from it.

**Consequence for verification:** a usage error therefore has two producing paths — the schema layer and the command — and each needs its own evidence.
A test of the schema rejection does not exercise the command's rejection, and the reverse.

**Alternatives considered:**

- Accept free-form strings and let the command reject everything: single validation path, but the model no longer sees the valid values in the schema, which is most of what a typed tool surface is for.
- Re-implement cross-parameter constraints in the wrapper: a second copy of rules the CLI owns, guaranteed to drift.

## Architecture

```text
  MCP client (either protocol era, stdio)
        │
        │  tools/list · tools/call
        ▼
  ┌──────────────────────────────────────────────────────────┐
  │  server                                                  │
  │                                                          │
  │  startup                                                 │
  │    ├─ locate the binary        ──▶ absent: refuse         │
  │    ├─ read its surface index   ──▶ differs: refuse        │
  │    └─ resolve default root     ──▶ unresolvable: refuse   │
  │                                                          │
  │  per call                                                │
  │    ├─ schema validation (closed value sets)              │
  │    ├─ resolve root: argument ▸ default ▸ process dir      │
  │    ├─ confirm (lifecycle tools only)                     │
  │    │    ├─ acknowledgment parameter        [else refuse]  │
  │    │    └─ elicitation, era-selected       [else refuse]  │
  │    └─ invoke                                             │
  └──────────────────────────────────────────────────────────┘
        │
        │  argv from parameters · cwd = resolved root · --json
        ▼
  ┌──────────────────────────────────────────────────────────┐
  │  c10r (installed binary)                                 │
  │    resolves .c10r/index.db, the recorded-root            │
  │    comparison, freshness, and git seeding — all from cwd │
  └──────────────────────────────────────────────────────────┘
        │
        │  stdout: structured answer     stderr: diagnostic
        │  exit:   0 ▸ result            1-6 ▸ typed error
        ▼
  answer returned unmodified · diagnostic carried verbatim
```

Workspace isolation needs no coordination between concurrent calls.
Each call's store path derives from its own resolved root, so two roots are two files.
No query tool writes, so concurrent query calls are concurrent readers.

The conformance test closes the loop the startup gate opens: the gate checks that the binary is the version the tools were written against, and the test checks that the tools were written correctly against that version.

## Risks

- **FastMCP 4 is a pre-release and its API may move before final.**
  Pinned exactly so a break is a deliberate upgrade rather than a lockfile surprise; a re-pin task is carried explicitly rather than left as an intention.

- **A `build` runs synchronously on every client and can exceed the client's timeout.**
  The acknowledgment refusal states the cost before the operation starts, the remedy is the same shell command every other refusal names, and the commit hook the `hooks_install` tool installs is what makes a manual rebuild rare rather than routine.

- **A host with no elicitation support cannot use the lifecycle tools at all.**
  Accepted as the price of a constant guarantee: the refusal names the command that does the job, so the caller is redirected rather than blocked.
  It does narrow self-service setup on those hosts to a handoff, which the `self-service-setup` story states rather than assumes.

- **The conformance test needs a built binary, so it cannot run on a checkout that has not built one.**
  It must fail rather than skip when the binary is absent: a skipped conformance check reports as a pass and is exactly the silent drift the design is built to prevent.

- **Testing both eras and both elicitation paths requires a client that can pin its negotiated era.**
  FastMCP's own client can, so the coverage is reachable; without that the era branch would be unverifiable.

- **Per-call process spawn adds latency the CLI does not pay once per session.**
  Accepted: the queries themselves are millisecond-scale against an existing index, and the alternative — a resident process — is a north-star item deferred beyond v1.

- **A host that launches the server outside the intended project makes the launch-time default wrong.**
  The per-call override is the recourse, and any answer read from a store describing a different root already carries the CLI's workspace-relationship disclosure, so the condition is visible rather than silent.
