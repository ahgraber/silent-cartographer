# Instructions

Deliver exactly what was requested.
Avoid speculative extras, but include the minimum tests, documentation, and safeguards needed to keep behavior correct and prevent regressions.

## Directive Priority

If directives conflict, prioritize:

1. Correctness and safety at external boundaries
2. Explicit user instructions
3. Minimal scope and simplicity

## Clean-Room Independence (the wall)

This is a from-scratch reimplementation of a prior tool ("the source") at `/Users/mithras/_code/_worktrees/silent-cartographer/old-c10r-worktree/`.
The rewrite must stay uncontaminated by _how the source is built_ so the new design is reasoned out independently.
This section overrides ordinary "read the existing code" reflexes.

### Rules binding the assistant

1. **No contact with the source.**
   Never read, open, grep, or index anything under the source path; no tool (search, MCP, graph, editor) may point at it. 100% of contact routes through a subagent.
2. **Fresh subagent every crossing.**
   Each consultation spawns a new, stateless, one-shot dirty-room subagent.
   No subagent carries memory across questions — nothing accumulates a picture that could cross the wall in aggregate.
3. **Batch, don't probe.**
   Questions to the subagent are problem-space, few, and high-altitude, asked up front — never an iterative narrowing stream, never yes/no mechanism questions.
4. **The user is the default channel.**
   The subagent is a last-resort gap-filler ("why did the old one bother with X?"), not the primary way to learn what to build.
5. **Verification is quarantined.**
   The trust / correctness / completeness / validation dimension is relitigated from scratch and never requested across the wall.
6. **User-sovereign injection — accepted without pushback.**
   Anything the _user_ chooses to share is taken as-is, even verbatim source snippets.
   The wall constrains what the assistant _reaches for_, never what the user _hands over_.
   Do not lecture about contamination when the user injects; use exactly what was given and do not spider outward from it into the source.
7. **The wall is a discipline, not a proof.**
   Do not build machinery to _prove_ the clean room is uncontaminated — that reproduces the very correctness rabbit-hole this rewrite exists to escape.
   Reasonable discipline, then build.

### Dirty-room subagent contract (launch prompt)

Compliance is judged on whether an answer **constrains the new design**, not on whether it avoided banned words.
If a sentence would let a reader reconstruct the source's structure, sequencing, schema, naming, or validation by inference, it violates this contract regardless of phrasing.

**Precedence:** where any permission and any prohibition apply to the same content, the prohibition wins.
The second-order quarantine overrides every permission.

**MAY return** (in its own conceptual framing, about a hypothetical tool that does not exist yet):

- **Purpose** — stated only as the user's unmet need or end-state; never an
  operation the tool performs internally.
- **Domain vocabulary** — only terms that exist independently of the source;
  defined by what the term means to a human, never by its parts, fields, or
  composition; never a source-coined name.
- **First-order interaction stories** — what a human or AI agent is trying to accomplish at the surface, each story **standalone**: no sequencing, ordering, workflow, or one-goal-feeds-another.
  May say the user seeks a deliverable; never describe its structure, fields, format, ordering, or grouping.

**MUST NOT return:**

- The existence of any named entity, persisted object, or internal concept — verbatim, paraphrased, renamed, or generalized.
  The ban is on the concept, not the spelling.
- Any mechanism: architecture, control flow, phases, pipelines, algorithms, data
  structures, output schema, response shape.
- Any command/tool decomposition: how capability splits across surfaces,
  subcommands, flags.
- Any number, threshold, default, or limit.
- Any second-order content: trust, correctness, completeness, verification,
  validation — including any user goal whose success criterion is confidence in
  the output.
- Any absence / non-goal claim ("the source does not do X").
- Any concrete test case or scenario; any recommendation to replicate.

**Refusal:** if a question needs a forbidden line, reply only with a _category_ — "That requires [mechanism / schema / decomposition / second-order / out-of-scope] detail, which is out of altitude" — then offer the nearest admissible _category_ of question.
Never name the specific thing being withheld.

**Output:** (1) _Answer_ — purpose / vocabulary / standalone stories only. (2) _Withheld_ — category labels only from the fixed set {mechanism, schema, decomposition, second-order, out-of-scope}.
No instance nouns, no counts.

**Accepted inheritance:** that the tool has a CLI and an MCP interface is a deliberate carry-over chosen by the user; naming those surfaces is allowed, but how capability splits across them is not.

## 1. Think Before Coding

Objective: surface ambiguity and tradeoffs before writing any code.

- State assumptions explicitly.
- If uncertainty would materially change the implementation, ask.
  Otherwise, state your assumption and proceed.
- If multiple interpretations exist, present them — don't pick silently.
- If a simpler approach exists, say so.
  Push back when warranted.

## 2. Simplicity First

Objective: write the minimum change that meets the request.

- No features, abstractions, or configurability beyond what was asked.
- No "flexibility" or "configurability" that wasn't requested.
- Prefer the golden path for internal logic; let tests define edge-case expectations.
- Add explicit validation and error handling at external boundaries (I/O, network, persistence, auth, parsing, external APIs).
- If you write 200 lines and it could be 50, rewrite it.
- Extend an existing function only while it stays readable at a glance.
  When a new requirement adds another branch or nesting level to an already-branchy function, split along the new axis instead of growing the function.
- Apply YAGNI ruthlessly.

## 3. Surgical Changes

Objective: every changed line traces directly to the request.

When editing existing code:

- Touch only what the request requires.
  Don't "improve" adjacent code, comments, or formatting.
- Match existing style, even if you'd do it differently.
  Don't refactor existing code unless it is part of the request.
- If the request doesn't fit the existing design, say so before writing code.
  Reshaping is in scope when the alternative is a near-copy of an existing code path; propose the reshape and its blast radius, then implement it.
- Never resolve a mismatch between request and design by duplicating a function, class, or module and editing the copy.
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked — mention it instead.
- Propose unrequested improvements in one line at the end; don't fold them into the change.
- Change one thing at a time and report it before starting the next.

## 4. Goal-Driven Execution

Objective: define success criteria, then loop until verified.

Transform tasks into verifiable goals:

- "Add validation" → write tests for invalid inputs, then make them pass.
- "Fix the bug" → write a test that reproduces it, then make it pass.
- "Refactor X" → ensure tests pass before and after.

Testing guardrails:

- Never modify a failing test to make it pass.
  Fix the code under test.
- If a test is genuinely wrong, explain why and await user approval before changing it.
- Write implementations that solve the general problem, not code that special-cases specific test inputs.
- Cover each distinct behavior once.
  More tests of the same shape is not more coverage.

For multi-step tasks, state a brief plan defining the step task and associated verification checks.

## 5. Definition of Done

The required checks are the full test suite and every hook in `.pre-commit-config.yaml`.
Slow, or looking unrelated to the change, is not a reason to skip one.

- The requested behavior works as specified.

- The test suite passes, not just tests for this change; previously working behavior is part of the acceptance criteria.

- Behavior changes are covered by tests, or testing gaps are explicitly stated.

- Public contract changes are documented.

- The hooks pass on everything changed since `HEAD`, staged or not, including new files.
  Pass the paths NUL-delimited so names with spaces survive:

  ```sh
  { git diff -z --name-only --diff-filter=d HEAD; git ls-files -z --others --exclude-standard; } | xargs -0 {{ hook_runner }} run --files
  ```

  Report failing hook output verbatim and fix the cause — a failure is a defect, not an unavailable check.

- A check is unavailable only when the command itself fails to run — missing binary, permission error, no network.
  Then name the check, quote the error, and give the user the exact command to run.

- Never call a change "confirmed", "verified", or "working" unless you ran the command in this session and read its output.
  Do not describe expected output as if you had seen it.

- Re-read a file immediately before reporting on it.
  Never report from a snapshot taken earlier in the session — the user edits files between turns.

## Defaults

- Review available skills for relevance before writing code.
- If intermediate, user-aligned work is needed before final output, surface in .\_scratch/.
- If worktrees are warranted use a local .worktrees/ directory.
- Use descriptive, consistent naming conventions.
- Write docstrings or comments for public contracts and non-obvious behavior.
- Comments and docstrings describe what exists now (or the rationale for the current design), never what the code used to be.
  No "previously…", "no longer…", "changed from…", or "renamed from…" — that history belongs in commit messages and changelogs.
  When editing, delete stale historical asides you encounter rather than preserving them.
- Use type annotations where the language supports them.
- Use structured logging where the project uses logging.
- Run lint/format/test through project tooling when available; do not hand-format code.
- Do not hand-wrap markdown prose; write each sentence (or a natural paragraph) on a single line and let the formatter hook reflow it (the repo uses a one-sentence-per-line style).
- Write tests for public behavior and regressions, not implementation details.

## Terminology and Tone

- Prefer the tone of a professional technical writer.
- Use the vocabulary already in the project.
  Do not invent jargon, and name a thing after its effect rather than its mechanism.
- State findings plainly.
  No flattery, no hedging.
- Label an unresolved question `OPEN QUESTION` and queue it; do not present it as a conclusion.

## Technology & Data Handling Requirements

This is a multi-language repo (Python and Rust).
Cross-cutting rules apply to both; language-specific tooling is grouped under its own subsection.

- Default runtime targets CPU-only execution; GPU use requires an explicit cost and ops justification.
- Storage of raw inputs and derived artifacts must permit replay; blob/object storage locations are recorded alongside metadata.
- Secret management: Secrets/keys MUST NOT be committed.
  Store them in a gitignored `.env` file (or a secret manager) and access them via environment variables at runtime.

### Python

- Python code runs in the uv-managed environment; dependencies are pinned via uv/lockfiles (pyproject.toml + uv.lock) and honored by Nix devshells — no ad-hoc global installs.
- Process identification: Every Python process MUST set a descriptive process title using `setproctitle` so hosts running multiple Python processes can distinguish them.
- Give executable Python scripts a `uv` shebang, not a system interpreter: `#!/usr/bin/env -S uv run --script`, paired with a PEP 723 `# /// script` block declaring `requires-python` and `dependencies`.
  This makes the script self-contained and reproducible; `#!/usr/bin/env python3` picks up whatever interpreter and site-packages happen to be on `PATH`.

### Rust

- The toolchain is pinned via `rust-toolchain.toml` (channel plus the `rustfmt`, `clippy`, and `rust-src` components); no ad-hoc global toolchains.
  rustup requires that exact filename — it does not accept a dotfile form — so this is the one config file here that is not a dotfile by choice.
- Dependencies are managed by Cargo and pinned via the committed `Cargo.lock`; no unpinned global `cargo install` into the build.
- Formatting and lints are config-driven via the dotfiles `.rustfmt.toml` (edition-2024 style, 119-column width) and `.clippy.toml`.
  Run `cargo fmt` and `cargo clippy` through project tooling; do not hand-format.
- Process identification: long-running Rust processes SHOULD set a descriptive process title (e.g. the `proctitle` crate) for parity with the Python rule above.
- Running `c10r`: the copy on `PATH` (installed with `cargo install --path . --locked` into `~/.cargo/bin`) is a released build and never reflects the working tree.
  Exercise working-tree changes with `cargo run --release -- <args>`, which rebuilds if stale; do not put `target/` on `PATH`.
  Run `which -a c10r` before concluding anything from a bare `c10r` invocation — unrelated binaries of that name may shadow the installed one.
- Give the working-tree build its own store, e.g. `cargo run --release -- --db .c10r/dev.db …`.
  The `--db` default `.c10r/index.db` is shared with the installed build, and a store written under a different schema version is refused on open.
- Config and trust boundaries map the Python stack onto Rust crates:
  - (de)serialization (pydantic models) → `serde` with `toml` / `serde_json`;
  - layered configuration, pydantic-settings style (defaults < file < env < flags) → `figment` (or `config`);
  - the CLI surface → `clap` (derive);
  - cross-field invariants the type system can't encode → `garde`.
    Prefer parse-don't-validate with newtypes over runtime validators wherever practical.

## Workflow & Quality Gates

- Use Spec-Driven Development and Test-Driven Development.
- Any change to data schemas, embedding parameters, or retrieval scoring requires a migration/test plan and version bump of the affected artifact.
- Code review checks for reproducibility (pinned deps, seeded operations), privacy adherence, and observability hooks (structured logs + metrics).

### Spec authoring

- **Contract floor (weakest common denominator).**
  When a spec defines a contract (protocol, interface, adapter) satisfied by more than one implementation or backend family, define the MANDATORY contract at the **intersection** of what every declared implementation guarantees — never the union, never the strongest one.
  Expose capabilities only some implementations provide as **optional, queryable capabilities** (feature detection, e.g. `native_text_search() -> None`), never as mandatory clauses only some backends satisfy.
  Name exemplar implementations in scenarios; keep exemplar-specific behavior out of contract prose.
  This is the Liskov Substitution Principle for backends: any declared implementation MUST be substitutable without callers observing a behavioral change.
- **Value-chain laddering.**
  Every change's user stories (in `proposal.md`) MUST ladder to the product north star (`.specs/NORTH-STAR.md`); every delta-spec requirement that advances a story carries a `Serves: <story>` backlink.
  A requirement that ladders to no story is scope to question, not implement.
- **Document hierarchy (single source of truth).**
  North star = product intent; baseline `specs/` = contracts; per-change `design.md` = decisions and rationale (there is no separate ADR store).
  On any conflict, north star + specs win.

## Governance Practices

- Semantic Versioning is REQUIRED (MAJOR.MINOR.PATCH).
- Conventional Commits are REQUIRED for commit messages and/or PR titles.
- Keep a Changelog is REQUIRED; it follows <https://keepachangelog.com> format.
  `just changelog <package> <bump>` drafts the next section from the commit history with `git-cliff`, that draft is edited by hand, and `just release <package> <bump>` then commits, tags, and pushes it; GitHub Actions tests the tag and publishes.
  The `c10r` crate tags `v*`, the `c10r-mcp` package in `mcp/` tags `c10r-mcp-v*`, and each changelog is scoped to its own series.
  See `docs/releasing.md`.
- Never cut a release, push a tag, or bump a version without an explicit instruction to release.
- After the first MINOR release, all changes affecting data/schema/contracts MUST include a migration plan and a deprecation schedule.

## Testing

TBD

## Commit & Review Guidelines

- Run `prek run --all-files` before committing.
  Report failing hook output verbatim and fix the cause.
  Never pass `--no-verify` or work around a hook silently.
- **Hard gate before committing**: before running `git agent-commit`, present the user with (1) the proposed commit message and (2) a concise diff summary covering which files changed and what each change does.
  Wait for explicit user approval; do not proceed if the user requests changes.
- **Every commit message draft, without exception, must be produced by invoking the `commit-message` skill first.**
  A prior invocation earlier in the same session does not satisfy this requirement — re-invoke for each request.
  Drafting inline, from memory, or from habit is not acceptable.
- Commit format: `type(scope): summary` (e.g., `feat(zsh): …`, `fix(vscode): …`).
  Scope should reflect directories or logical surfaces.
- Separate unrelated changes (docs vs configs vs lockfile updates) into distinct commits.
- Use `git agent-commit` (not `git commit`) to create signed commits; this alias uses the dedicated agent signing key at `~/.ssh/id_ed25519_agent_signing`.
- Commit only when the user asks, and draft the message at that point, not in advance.
- Never pass `--no-verify` or work around a hook silently.

## Sandbox Limitations

- The sandbox may not be able to run installs, network calls, or other privileged commands (permission errors) — attempt the command first rather than assuming failure.
- **Delegate to the user** only if a command actually fails on a permission or network error.
  Describe the exact command to run.
