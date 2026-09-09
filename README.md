# silent-cartographer

Silent Cartographer (`c10r`) exists because coding agents fail on non-trivial codebases in two compounding ways:

**Task myopia**: an agent knows the files it is touching but has no model of what lies outside them, so it cannot reason about blast radius or architectural consequences without spending most of its context window reconstructing structure.
**Search inefficiency**: its main discovery tool is grep, which returns plausible, unranked matches — one broad grep can consume more context than the change being written.

`c10r` indexes a Rust or Python workspace into a single SQLite file, then answers questions about it: where a symbol is defined, what references it, what breaks if it changes, what tests exercise it, what code matches a description.
Answers return in milliseconds and are small enough to hand to a coding agent.
Symbol resolution is type-aware, through [SCIP](https://github.com/sourcegraph/scip) indexers.
Structure comes from tree-sitter.

## Install

Silent Cartographer is available as a command-line interface and/or an MCP server.

### A prebuilt binary

Each [release](https://github.com/ahgraber/silent-cartographer/releases) carries an archive per platform — Linux (x86-64) and macOS (Apple Silicon) — named `c10r-<tag>-<target>.tar.gz` for you to download, extract, and place on your `PATH`.

### With `uv`

`uv tool install` builds the binary from GitHub and puts it on your `PATH`:

```sh
uv tool install git+https://github.com/ahgraber/silent-cartographer@<tag>
```

`<tag>` pins a released version; a branch name or a commit hash works there too, and omitting `@<tag>` builds the default branch.
To upgrade, run the command again with the new tag.
`uv tool uninstall c10r` removes it.

### From the repository

`cargo install` builds and installs straight from GitHub, with no clone of your own:

```sh
cargo install --git https://github.com/ahgraber/silent-cartographer --tag <tag> --locked
```

`--tag` pins a released version; `--branch` or `--rev` take a branch or a commit instead, and omitting all three builds the default branch.
`--locked` builds against the committed `Cargo.lock` rather than re-resolving dependencies.

### From a clone

```sh
cargo build --release          # built at target/release/c10r
cargo install --path . --locked # or install it onto your PATH
```

All three source routes additionally need the Rust toolchain pinned by `rust-toolchain.toml`, which rustup installs automatically.

The first build downloads the ~33 MB embedding model that `search` and `similar` use, and checks it against `models/potion-code-16M-v2.manifest.json`.
The model is compiled into the binary, so `c10r` needs no network or configuration at runtime.

### Language indexers

Whichever route you install by, `c10r build` shells out to an indexer for the language it is indexing:

```sh
# Rust workspaces
rustup component add rust-analyzer
# Python workspaces
npm install -g @sourcegraph/scip-python
```

You only need the one for the languages you index.
`c10r doctor` reports which are present and which are missing.

## Quick start

### Initial setup

```sh
cd your-project
# check the language indexers are installed and responsive
c10r doctor
# index the workspace into .c10r/index.db
c10r build
# rebuild the index after every commit
c10r hooks install
```

### Using `c10r`

```sh
# symbols whose name contains "retry"
c10r find retry
# a symbol's source
c10r get with_backoff --detail body
# every reference site
c10r trace with_backoff --relation references
# what could break if it changes
c10r trace with_backoff --relation dependents
# find code by meaning
c10r search "parse a config file into typed settings"
# similar code, clones marked
c10r similar with_backoff
# what the current working-tree change could affect
c10r impact
```

Add `--json` to any query for the machine-readable answer.

Agents should start with `c10r manifest`, which prints the whole command surface — commands, flags, valid values, defaults — as JSON, versioned so a cached copy can be invalidated.

## Commands

| Command                                       | Question it answers                                                                                             |
| --------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| [`get`](docs/commands/get.md)                 | What is this symbol — its location, signature, interface, or body?                                              |
| [`trace`](docs/commands/trace.md)             | What stands in a relation to it — containers, contents, references, importers, implementers, dependents, tests? |
| [`find`](docs/commands/find.md)               | Which symbols have this fragment in their name?                                                                 |
| [`search`](docs/commands/search.md)           | Which code matches this natural-language description?                                                           |
| [`similar`](docs/commands/similar.md)         | Which code is most similar to this symbol, and is any of it a clone?                                            |
| [`impact`](docs/commands/impact.md)           | What could this git diff break?                                                                                 |
| [`build`](docs/commands/build.md)             | (Re)index the workspace.                                                                                        |
| [`status`](docs/commands/status.md)           | What does the index know, and how well did indexing align?                                                      |
| [`doctor`](docs/commands/doctor.md)           | Are the language indexers installed and responsive?                                                             |
| [`cache`](docs/commands/cache.md)             | Where is the stored index, how much disk does it use, and remove it.                                            |
| [`hooks`](docs/commands/hooks.md)             | Install the post-commit hook that rebuilds the index.                                                           |
| [`manifest`](docs/commands/manifest.md)       | Print the machine-readable command surface and index state.                                                     |
| [`completions`](docs/commands/completions.md) | Print a shell completion script.                                                                                |

The shared flags, the symbol reference forms, and the index store are documented in [common](docs/commands/common.md).

## Exit codes

| Code | Meaning                                                   |
| ---- | --------------------------------------------------------- |
| 0    | Success, including a typed-empty answer                   |
| 1    | Generic operational failure                               |
| 2    | Usage error (malformed invocation)                        |
| 3    | No index at the store path — run `c10r build`             |
| 4    | Store built under a different schema version — rebuild it |
| 5    | A required language indexer is missing or unresponsive    |
| 6    | The file at the store path is not `c10r`'s own store      |

## MCP server

The server lets an agent in an MCP host call `c10r` as named tools instead of composing shell commands.
It lives in `mcp/`, runs over stdio, and runs the `c10r` binary you installed.

It advertises eight tools, one per command: `get`, `trace`, `find`, `search`, `similar`, `impact`, `build`, and `hooks_install`.
Each returns that command's structured answer unmodified, so provenance, freshness, typed absence, and heuristic labels arrive intact.
A command failure arrives as a tool error carrying the exit category and `c10r`'s diagnostic.
`cache` is deliberately absent: no tool on this surface removes an index store.

`c10r` and `c10r-mcp` are versioned independently, and their version numbers drift apart.
Matching numbers mean nothing, and a mismatch is not a problem.
What has to agree is the command-surface version the server was built against, which the server checks at startup and reports as a refusal if it differs — see [startup refusals](#startup-refusals).

### Install

```sh
cargo build --release
cargo install --path . --locked
uv sync --project mcp
```

### Configure a client

The server answers about one workspace per call.
It uses the `root` named by the tool call, then the root the server was launched for, then the directory the server process started in.

Per repository, where the launch directory is the project, no root needs naming:

```json
{
  "mcpServers": {
    "c10r": {
      "command": "uv",
      "args": [
        "run",
        "--project",
        "/path/to/silent-cartographer/mcp",
        "c10r-mcp"
      ]
    }
  }
}
```

Installed once for many projects, name the workspace explicitly:

```json
{
  "mcpServers": {
    "c10r": {
      "command": "c10r-mcp",
      "args": [
        "--workspace",
        "/path/to/project"
      ]
    }
  }
}
```

| Setting            | Effect                                                   |
| ------------------ | -------------------------------------------------------- |
| `--workspace PATH` | The workspace root used when a call names none           |
| `C10R_WORKSPACE`   | The same, when no `--workspace` is given                 |
| `C10R_BINARY`      | An explicit path to `c10r`, overriding the `PATH` lookup |

A configured workspace that is not an existing directory is refused at startup rather than replaced by the process's own directory.

### Startup refusals

The server checks the binary before advertising anything, and refuses on either of two conditions:

| Refusal                                                                                  | Remedy                                                                                       |
| ---------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| The binary's command-surface version does not match the one the server was built against | Upgrade the server package, or install a matching `c10r`. Rebuilding the index does nothing. |
| The binary cannot be found or cannot be run                                              | Install `c10r`, or point `C10R_BINARY` at it                                                 |

Neither is an index problem.
A missing or incompatible index arrives per call as an `absent_index` or `incompatible_store` tool error, and is fixed by a rebuild.

### Changing state via MCP requires consent

`build` and `hooks_install` require explicit user consent, which is enforced via an MCP `acknowledge` parameter.
If the client offers no way to ask, the operation is refused and the refusal names the CLI command that does the job: `c10r build` or `c10r hooks install`.

The MCP tools are blocking and sequential.
A `build` that finds the index already current returns immediately.
A `build` that must analyze a large repository can outlast a client's request timeout; run `c10r build` in a shell and install the commit hook so manual rebuilds stay rare.

## Embedded model credit

Semantic search uses [`potion-code-16M-v2`](https://huggingface.co/minishlab/potion-code-16M-v2), a static code-embedding model by [The Minish Lab](https://minish.ai/), redistributed under the MIT License (see `models/potion-code-16M-v2.LICENSE`).
Inference runs through [`model2vec-rs`](https://github.com/MinishLab/model2vec-rs); vectors are stored with [`sqlite-vec`](https://github.com/asg017/sqlite-vec).

## AI disclosure

This project uses spec-driven development so that AI coding assistance works from [written specifications](.specs/).
For more information, see the `sdd-*` family of [ahgraber/skills](https://github.com/ahgraber/skills).
