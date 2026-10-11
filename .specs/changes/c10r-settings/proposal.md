# Proposal: c10r-settings

> **Placeholder.**
> This proposal records the intent, stories, and scope of the change.
> The delta specs, `design.md`, and `tasks.md` are not written yet.
> Each open question below needs an explicit decision before the change is implemented.

## Intent

Some `c10r` settings are chosen once for a project and should stay fixed: the analyzer, the store path, and the chunk parameters of the semantic index.
Today each of these settings is a command-line flag.
When a caller omits the flag, `c10r` uses the default compiled into the binary.

As a result, a project setting lasts only until the next invocation that omits its flag.
That invocation uses the compiled-in default without warning.
When an agent runs the command, no person sees the change.

The chunk parameters from `semantic-chunking` show the problem.
Each build records the chunk parameters, and the currency check compares them against the current values.
Suppose an operator builds the index with a non-default chunk size, and an agent later runs `build` without the chunk flags.
The agent's build uses the default chunk size, the currency check reports a mismatch, and `c10r` rebuilds the whole index at the defaults.
`c10r` does not refuse the build and does not report the change.
Each component behaves as specified.
The defect is that a command-line flag is the only place a setting can come from.

## User Stories

### Story: project-regime-stays-put

As a developer, I want a setting I chose for a project to stay in effect for every later build, whoever runs it, so the index changes only when someone decides to change it.

> Ladders to the principle **Calibration over coverage**: an index rebuilt with settings nobody chose still answers queries, and nothing in the answer shows that the settings changed.
> Supports north-star outcome 5 (**Calibrated trust**): the provenance recorded with an answer is useful only if it names the settings the operator chose.

### Story: agent-cannot-change-the-regime

As a developer whose agent runs `build` for me, I want the agent unable to change the settings that determine how the index is derived, so I keep control of those settings when I delegate the work.

> Ladders to **Calibration over coverage** and to **Agent-native, human-rendered**.
> Agents call `c10r` as often as people do, so a setting the developer owns must not be changeable through the agent's commands.

### Story: one-place-to-look

As a developer returning to a project, I want one file that lists the settings in effect, so I can read the project's configuration instead of reconstructing it from shell history.

> Ladders indirectly.
> This story follows from the first two: a persistent setting must be stored somewhere, and a file in the project can be read.

## Scope

**In scope:**

- A project settings file, `.c10r.yaml`, found by searching upward from the working directory to the repository root.
  Serves: project-regime-stays-put, one-place-to-look.
- A global settings file in the XDG configuration directory, used when the project search finds no file.
  Serves: project-regime-stays-put.
- One environment variable for each setting that can be set.
  Serves: project-regime-stays-put, agent-cannot-change-the-regime.
- Precedence, lowest to highest: compiled-in default, settings file, environment variable, command-line flag.
  Serves: project-regime-stays-put.
- A decision on which settings become project-scoped, and which of those keep a command-line flag.
  Serves: agent-cannot-change-the-regime.
- A way to show the settings in effect, so a user can read the result of the precedence rules.
  Serves: one-place-to-look.

**Out of scope:**

- Per-invocation presentation preferences, such as output format and color.
  These control how one answer is displayed, not how the index is built.
- Secrets of any kind.
  No `c10r` setting is a credential, so the file can be committed and read without risk.
- A settings file that `c10r` generates or edits.
  Users write the file by hand, and `c10r` only reads it.
- Changes to what a setting does or to what a build records.
  A setting that affects how the index is derived is already recorded in the store's identity.
  This change adds new places for the value to come from.

## Approach

Settings are layered in the order given in Scope.
The project's technology rules name `figment` for this kind of layered configuration.

Settings files are found in two steps.
First, `c10r` searches upward from the working directory for `.c10r.yaml`.
The search stops at the repository root, so it never reads a file from an unrelated parent directory.
If the search finds no file, `c10r` reads the global file from the XDG configuration directory.

The main design work is deciding which settings are project-scoped.
Five settings are candidates today: the store path, the workspace identity, the analyzer language, the declared interpreter environment, and the chunk parameters.
The proposed `ty-scip-backend` change adds a sixth: the Python indexer.
Unless `--python-indexer` names one, a build uses the indexer recorded in the existing store, and scip-python when no store exists.
Some candidates change what an answer means, such as the chunk parameters.
Others only locate data, such as the store path.
Each setting needs its own argument for inclusion.
A flag that exists today is not by itself a reason to include its setting.

For each included setting, the change must also decide whether to keep its command-line flag.
Removing a flag changes the command surface.
It requires a version bump and changes the tool definitions of the MCP server.

## Open Questions

1. **Which settings are project-scoped?**
   Each setting needs its own argument.
   A store path and a chunk size are both set once, but only the chunk size changes what an answer means.

2. **Does the nearest file win, or are files merged along the search path?**
   In a monorepo, a package-level file that overrides a repository-level file is useful.
   It also adds a second precedence rule that users must learn and that the implementation can get wrong.

3. **Which YAML crate?**
   The repository does not parse YAML today.
   `serde_yaml`, the most widely used crate, is no longer maintained.
   The file could use TOML instead.
   The project's technology rules list the `toml` crate for serialization, although `c10r` does not depend on it yet.

4. **How are the settings in effect shown, and does the output name the layer each value comes from?**
   When a user asks why a rebuild happened, "chunk size is 512, from the project file" answers the question and "chunk size is 512" does not.

5. **Is a flag removed, or kept at a lower priority?**
   A kept flag gives the user a way to override the file, and it also lets an agent change the setting.
   Removing the flag takes away both.
   The answer can differ per setting.

6. **What happens when a settings file cannot be read or is malformed?**
   Refusing to run is the safe default for invalid input.
   But a malformed global file would then stop every invocation in every project.
