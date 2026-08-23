# Proposal: c10r-settings

> **Placeholder.**
> This states intent, stories, and boundaries so the shape is recorded while it is fresh.
> The delta specs, `design.md`, and `tasks.md` are not written yet, and the open questions below are unresolved — none of them should be settled silently when the change is worked.

## Intent

Some of what `c10r` needs to know is decided once for a project and then holds: which analyzer, which store path, what shape the semantic index is built in.
Today every one of those arrives the same way as a per-query preference — as a flag on a single invocation, defaulting to a value compiled into the binary when it is not supplied.

That makes a project-level decision only as durable as the shell line that carried it.
A setting chosen deliberately is re-decided, implicitly and at the compiled-in default, by the next caller who does not repeat it.
When that caller is an agent, the operator is not present to notice.

The concrete case that surfaced this: the chunk parameters shipped by `semantic-chunking` are recorded with the build and compared by the currency check, so they genuinely govern what the index means.
An operator who builds at a non-default chunk size, and whose agent later runs an ordinary `build`, gets the whole index re-derived at the compiled-in defaults — no refusal, no disclosure, because the agent's invocation carried the defaults implicitly and the currency check correctly saw a different regime.
The machinery behaved exactly as specified.
The input surface is what is wrong.

## User Stories

### Story: project-regime-stays-put

As a developer, I want a setting I chose for a project to still be in effect the next time the index is built, whoever or whatever runs the build, so the index never changes regime without someone deciding that it should.

> Ladders to the principle **Calibration over coverage** — an index silently re-derived under a regime nobody chose answers confidently from a state its operator did not authorize.
> Supports north-star outcome 5 (**Calibrated trust**): an answer's recorded provenance is only worth reading if the regime it names is the one the operator picked.

### Story: agent-cannot-change-the-regime

As a developer whose agent runs `build` on my behalf, I want the agent unable to alter the settings that govern what the index means, so delegating the work never costs me the decisions behind it.

> Ladders to **Calibration over coverage** and to **Agent-native, human-rendered** — agents are a first-class caller, which is exactly why the settings a human owns must not be reachable from the agent's surface.

### Story: one-place-to-look

As a developer returning to a project, I want one file that states the settings in effect, so I can read the project's configuration instead of reconstructing it from shell history.

> Ladders indirectly.
> Recorded honestly as a consequence of the first two rather than as independent justification: a durable setting has to live somewhere, and that somewhere is legible for free.

## Scope

**In scope:**

- A project settings file, `.c10r.yaml`, found by walking upward from the working directory to the repository root.
  Serves: project-regime-stays-put, one-place-to-look.
- A global settings file in the XDG configuration directory, consulted when the walk finds none.
  Serves: project-regime-stays-put.
- One environment variable per settable setting.
  Serves: project-regime-stays-put, agent-cannot-change-the-regime.
- Precedence, lowest to highest: compiled-in default, then settings file, then environment variable, then command-line flag.
  Serves: project-regime-stays-put.
- Deciding which settings become project-scoped, and which of those keep a command-line flag at all.
  Serves: agent-cannot-change-the-regime.
- Disclosing the settings in effect, so the precedence stack is readable rather than inferred.
  Serves: one-place-to-look.

**Out of scope:**

- Per-invocation presentation preferences — output format and color decide how one answer is rendered, not what the project is.
- Secrets of any kind.
  Nothing `c10r` needs is a credential, and keeping it that way is what lets the file be plain, committed, and read at a glance.
- A generated or tool-written settings file.
  The file is hand-authored; `c10r` reads it and never writes it.
- Changing what any setting means, or what is recorded with a build.
  A setting that shapes derivation is already recorded in the store's identity; this change moves where the value comes from, not what it does.

## Approach

The layering is the standard one and the project stack already names the crate for it: defaults below a file, below the environment, below flags.

Discovery has two stages.
The project stage walks upward from the working directory looking for `.c10r.yaml`, stopping at the repository root so a walk never escapes the project into an unrelated parent.
The global stage applies when the project walk finds nothing, and reads from the XDG configuration directory.

The membership question — which settings are project-scoped — is the part with real content, and it is not a formality.
The candidates that exist today are the store path, the workspace identity, the analyzer language, the declared interpreter environment, and the chunk parameters.
They are not all the same kind of thing, and the change should admit each on its own argument rather than sweeping in everything that currently has a flag.

For a setting that is admitted, a second question follows: whether it keeps a command-line flag.
Removing one is a command-surface change with a version bump and a consequence for the MCP server's tool definitions, so it is a decision to take deliberately, not a side effect of adding a file.

## Open Questions

1. **Which settings are project-scoped?**
   Membership has to be argued per setting.
   A store path and a chunk size are both "set once," but only one of them changes what an answer means.

2. **Nearest file wins, or merge along the walk?**
   In a monorepo a package-level file over a repository-level one is useful; it is also a second precedence rule to explain and to get wrong.
   Neither reading should be picked silently.

3. **Which YAML implementation?**
   The repository parses no YAML today, and the ecosystem's long-standing crate is no longer maintained upstream — confirm before depending on it.
   Worth asking whether the file should be TOML instead, which the project already has a parser lineage for and which `AGENTS.md` names first.

4. **How are the effective settings disclosed, and does the disclosure name the layer?**
   "Chunk size is 512" is much less useful than "chunk size is 512, from the project file" when the question is why a rebuild triggered.

5. **Does a flag get removed, or only demoted?**
   Leaving a flag in place keeps the escape hatch and keeps the agent's reach; removing it closes both.
   The answer may differ per setting.

6. **What does an unreadable or malformed settings file do?**
   Refusing is the honest default at a trust boundary, but a global file that breaks every invocation in every project is a harsh failure mode.
