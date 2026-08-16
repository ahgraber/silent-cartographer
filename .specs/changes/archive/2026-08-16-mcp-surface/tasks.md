# Tasks: mcp-surface

## Package foundation

- [x] Create a uv-managed Python project at `mcp/` with `pyproject.toml`, declaring the console entry point that starts the stdio server.
- [x] Pin `fastmcp` to one exact pre-release version and commit the resulting lockfile.
- [x] Confirm the existing `ruff-check` and `ruff-format` hooks cover the new `mcp/` subtree, and add the `mcp/` test command to the project's test invocation.
- [x] Add a test fixture that creates a temporary workspace, builds an index in it, and yields its root.
- [x] Add a test helper that locates the built `c10r` binary and fails — never skips — when none is present.
- [x] Write a test asserting the fixture produces a workspace the located binary answers a query against, so later tests rest on a proven harness.
- [x] Write a test asserting the console entry point, launched as a child process, serves its tools over stdio on both protocol eras.

## Binary invocation and answer fidelity

- [x] Implement binary location from an explicit environment override, falling back to `PATH`.
- [x] Implement the invocation helper that runs the binary with a given argument list, a given working directory, and `--json`, capturing standard output, standard error, and the exit status.
- [x] Implement result construction that parses the captured standard output and returns it as the tool result with no field added, removed, renamed, or reordered.
- [x] Write a test asserting a result-bearing answer returned through the invocation helper equals the answer the same command produces directly.
- [x] Write a test asserting a typed-empty answer is returned as the command's typed-empty answer, distinguishable from both a results-bearing answer and an error.
- [x] Write a test asserting an ambiguous symbol reference returns the command's typed candidate set rather than a single symbol or an error.
- [x] Write a test asserting provenance, freshness, and classification labels survive to the tool result for a heuristic-grade answer read from an out-of-date index.
- [x] Write a test asserting a body-detail answer whose content embeds a terminal-control escape sequence is byte-identical to the command's structured answer.

## Exit-code mapping

- [x] Implement the mapping from each non-success exit code to a distinct typed tool error carrying the captured standard-error text unaltered.
- [x] Implement the success path so exit code 0 always produces a result, a typed-empty answer included.
- [x] Write a test asserting an absent index produces the absent-index error category carrying the diagnostic that names the rebuild.
- [x] Write a test asserting a store recorded under a different schema version produces the incompatible-store category, distinct from absent-index.
- [x] Write a test asserting an unrecognized file at the index path produces the unrecognized-store category, distinct from both absent-index and incompatible-store.
- [x] Write a test asserting a missing language indexer produces the indexer-or-setup category, distinct from every index-state category.
- [x] Write a test asserting a query that stands in no instance of its relation returns a result rather than raising an error.

## Workspace resolution

- [x] Implement launch-time default resolution: an explicit configuration argument, then the environment variable, then the directory the server process resolves to.
- [x] Implement the startup refusal when a configured root is not an existing directory, naming the root that failed to resolve.
- [x] Implement per-call precedence so a root named on the invocation overrides the launch-time default.
- [x] Wire the resolved root into the invocation helper as the child process's working directory.
- [x] Write a test asserting an invocation naming a root derives its answer from that root's store rather than the launch-time default's.
- [x] Write a test asserting an invocation naming no root, against an explicitly configured server, derives its answer from the configured root's store.
- [x] Write a test asserting an invocation naming no root, against an unconfigured server, derives its answer from the store belonging to the server process's own directory.
- [x] Write a test asserting a server configured with a non-existent root refuses at startup, naming that root, rather than falling back.
- [x] Write a test asserting two concurrent invocations resolving to distinct workspace roots each return only symbols from their own workspace.
- [x] Write a test asserting an answer read from a store recording a different workspace root carries the command's workspace-relationship disclosure.

## Startup surface gate

- [x] Record, in the package, the command-surface version the tools are written against.
- [x] Implement the startup step that reads the installed binary's surface index and compares its reported surface version against the recorded one.
- [x] Implement the refusal on any difference, naming both versions and the version-alignment remediation, and distinguishing it from every index-state failure.
- [x] Implement the distinct startup failure when the binary cannot be found or cannot be run, naming the binary.
- [x] Write a test asserting a server whose recorded version equals the binary's reported version starts and serves its tools.
- [x] Write a test asserting a server whose recorded version differs refuses to serve and names both versions and the remediation.
- [x] Write a test asserting an absent or unrunnable binary refuses to serve with a failure distinct from a surface-version difference.

## Query tools

- [x] Implement the `get` tool, exposing the symbol reference, source-position, detail, and content-windowing parameters.
- [x] Implement the `trace` tool, exposing the subject, relation, depth, ordering, detail, and bounding parameters.
- [x] Implement the `find` tool, exposing the name fragment and bounding parameters.
- [x] Implement the `search` tool, exposing the natural-language query, detail, and bounding parameters.
- [x] Implement the `similar` tool, exposing the subject, source-position, detail, and bounding parameters.
- [x] Implement the `impact` tool, exposing the revision, staged, depth, ordering, and path-narrowing parameters.
- [x] Type every parameter with a closed value set as an enumeration so the tool schema carries its valid values.
- [x] Write a test asserting the advertised tool list names exactly the six query tools and the two lifecycle tools.
- [x] Write a test asserting no advertised tool corresponds to the command that removes the stored index.
- [x] Write a test asserting a bounded invocation returns no more results than its cap and discloses that the result set was truncated.
- [x] Write a test asserting an out-of-set value for an enumerated parameter is rejected by the schema, naming the accepted values, before any command runs.
- [x] Write a test asserting a combination of parameters that no single option set permits — a depth option supplied with a relation that does not take one — is rejected as a usage error by the command, naming the valid form.

## Schema conformance

- [x] Write a conformance test that reads the installed binary's surface index and asserts every advertised tool parameter corresponds to an option the index reports for that tool's mirrored command.
- [x] Extend the conformance test to assert every enumerated value a parameter accepts lies within the index's reported valid set for the corresponding option.
- [x] Extend the conformance test to assert every default stated in a tool description equals the index's reported default for that option.
- [x] Make the conformance test fail, rather than skip, when no binary is available to read a surface index from.

## Lifecycle tools and the acknowledgment gate

- [x] Implement the `build` tool with an acknowledgment parameter, refusing until it is set with a diagnostic that states the operation's cost and how to proceed.
- [x] Implement the `hooks_install` tool with an acknowledgment parameter, refusing until it is set with a diagnostic that states what the operation writes and how to proceed.
- [x] Write a test asserting an unacknowledged `build` creates no store and refuses with a diagnostic stating the cost.
- [x] Write a test asserting an acknowledged and confirmed `build` builds the index and returns the command's answer.
- [x] Write a test asserting an unacknowledged `hooks_install` writes no file and refuses with a diagnostic stating what it would write.
- [x] Write a test asserting an acknowledged and confirmed `hooks_install` installs the hook and returns the command's answer.

## Dual-era service

- [x] Configure the server to accept both handshake-era and `2026-07-28` connections from one running instance.
- [x] Write a test asserting a client negotiating the handshake era lists and invokes a query tool successfully.
- [x] Write a test asserting a client negotiating the `2026-07-28` revision lists and invokes a query tool successfully.
- [x] Write a test asserting one query invoked by a client of each era against the same server returns equal results.

## Interactive confirmation

- [x] Implement a confirmation helper that selects its mechanism from the connection's negotiated protocol version and reports back whether the caller confirmed, declined, or offered no elicitation capability.
- [x] Implement the handshake-era mechanism within that helper, issuing the request mid-execution.
- [x] Implement the `2026-07-28` mechanism within that helper, returning the input request and reading the answer on re-invocation, carrying what it needs across rounds rather than holding state.
- [x] Wire both lifecycle tools to the confirmation helper, after their acknowledgment check and before any side effect.
- [x] Implement the refusal for a client offering no elicitation capability, stating that the client cannot be asked and naming the command that performs the operation.
- [x] Write a test asserting a handshake-era client that confirms causes the operation to be performed and the command's answer returned.
- [x] Write a test asserting a `2026-07-28` client that confirms causes the operation to be performed and the command's answer returned.
- [x] Write a test asserting a handshake-era client that declines leaves every store, file, and repository unchanged and reports the operation as not performed.
- [x] Write a test asserting a `2026-07-28` client that declines leaves every store, file, and repository unchanged and reports the operation as not performed.
- [x] Write a test asserting a client offering no elicitation capability leaves every store, file, and repository unchanged and is refused with a diagnostic naming the command that performs the operation.
- [x] Write a test asserting an unacknowledged invocation is refused without any confirmation request reaching a client that offers the capability.

## Synchronous completion

- [x] Write a test asserting a client that negotiated no extension beyond the base protocol receives the command's answer from a confirmed `build` rather than a missing-capability error.

## Invocation integrity

- [x] Resolve the located binary to an absolute path, so the binary the startup check verified is the one every invocation runs whatever workspace it resolves to.
- [x] Stop and reap the command when an invocation is cancelled, so an abandoned build does not go on rewriting the store.
- [x] Write a test asserting a decoy executable at the same relative path inside a workspace is never run in place of the verified binary.
- [x] Write a test asserting a cancelled build leaves no store behind after the caller has stopped waiting.

## Coverage of every query tool

- [x] Write a test invoking each query tool against the fixture workspace and comparing its answer to the same command run directly, so each tool's own argument assembly is exercised.
- [x] Extend that test over `impact`'s distinct shapes — the path separator, the staged flag, depth, and ordering — and assert the staged flag changes what the answer reports seeding from.
- [x] Write a test asserting the answer's fields arrive in the command's own order, which equality alone cannot detect.
- [x] Write a test asserting the workspace environment variable supplies the launch-time default, and that an explicit root outranks it.

## Instructions and documentation

- [x] Write the server instructions telling a client when to prefer c10r over textual search and how to recover from an absent or incompatible index.
- [x] Write a test asserting the instructions reach a client of either protocol era and name every tool, the coverage boundary, and each index-state recovery.
- [x] Document installation and client configuration for both install shapes — per-repository and global — including the workspace configuration argument and environment variable.
- [x] Document the two startup refusals and their distinct remediations: version alignment for a surface-version difference, and a rebuild for an index-state failure.
- [ ] Re-pin `fastmcp` to the stable release once FastMCP 4.0.0 is final, and re-run the full suite against it.
