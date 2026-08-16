# silent-cartographer

Silent Cartographer (`c10r`) is a persistent code knowledge graph with a command-line interface and an MCP server, built for coding agents and the humans who work with them.
It indexes a Rust or Python workspace once, stores the result in a single SQLite file, and then answers precise questions — where a symbol is defined, what references it, what breaks if it changes, what tests exercise it, what code matches a natural-language description — in milliseconds and a handful of tokens.

It exists because coding agents fail on non-trivial codebases in two compounding ways.
**Task myopia**: an agent knows the files it is touching but has no model of what lies outside them, so it cannot reason about blast radius or architectural consequences without spending most of its context window reconstructing structure.
**Search inefficiency**: its main discovery tool is grep, which returns plausible, unranked matches — one broad grep can consume more context than the change being written.

`c10r` replaces both with indexed answers.
Resolution is semantic (type-aware, via [SCIP](https://github.com/sourcegraph/scip) indexers), and structure is syntactic (tree-sitter).

## Installation

Requirements:

- The Rust toolchain pinned by `rust-toolchain.toml` (rustup picks it up automatically).
- `rust-analyzer` on `PATH`, for indexing Rust workspaces.
- `scip-python` (`npm install -g @sourcegraph/scip-python`), for indexing Python workspaces.

```sh
cargo build --release
# the binary lands at target/release/c10r
```

The first build downloads the embedding model behind `search`/`similar` (~33 MB) from its pinned upstream revision and verifies it against the checksum manifest at `models/potion-code-16M-v2.manifest.json`; later builds reuse the verified files.
The model is compiled into the binary, so the built `c10r` needs no network, model cache, or configuration at runtime.

`c10r doctor` reports whether the language indexers are present and responsive.

## Quick start

Index a workspace, then ask questions:

```sh
cd your-project
c10r build                        # index the workspace into .c10r/index.db
c10r hooks install                # keep the index fresh after every commit

c10r find retry                   # symbols whose name contains "retry"
c10r get with_backoff --detail body        # a symbol's source, by name
c10r trace with_backoff --relation references   # every reference site
c10r trace with_backoff --relation dependents   # what could break if it changes
c10r search "parse a config file into typed settings"   # find code by meaning
c10r similar with_backoff         # ranked similar code, clones marked
c10r impact                       # what the current working-tree change could affect
```

Every query takes `--json` for the structured machine answer; the default human rendering is a faithful projection of the same answer.
Agents should start with `c10r manifest`, which emits the whole command surface — commands, flags, valid values, defaults — as JSON, versioned so a cached understanding of the surface can be invalidated.

## Commands

| Command       | Question it answers                                                                                             |
| ------------- | --------------------------------------------------------------------------------------------------------------- |
| `get`         | What is this symbol — its location, signature, interface, or full body?                                         |
| `trace`       | What stands in a relation to it — containers, contents, references, importers, implementers, dependents, tests? |
| `find`        | Which symbols have this fragment in their name?                                                                 |
| `search`      | Which code matches this natural-language description?                                                           |
| `similar`     | Which code is most similar to this symbol, and is any of it a clone?                                            |
| `impact`      | What could this git diff break?                                                                                 |
| `build`       | (Re)index the workspace.                                                                                        |
| `status`      | What does the index know, and how well did indexing align?                                                      |
| `doctor`      | Are the language indexers installed and responsive?                                                             |
| `cache`       | Remove the stored index.                                                                                        |
| `hooks`       | Install the post-commit hook that keeps the index fresh.                                                        |
| `manifest`    | Emit the machine-readable command surface and index state.                                                      |
| `completions` | Emit a shell completion script.                                                                                 |

Symbols are addressed at three tiers: a bare short name (`connect`), a qualified name (`Client::connect`), or the full canonical identity every answer carries.
An ambiguous reference returns a typed candidate set to narrow from — never an arbitrary pick.
`get` and `similar` also accept a source position (`--at path:byte_offset`), which resolves to the enclosing symbol.

Content-bearing answers share one detail axis — `location`, `signature`, `interface` (signature plus the symbol's own documentation), `body` — and one bounding model: `--limit` caps result sets (default 25, `0` unbounded), truncated sets carry an opaque `--cursor` continuation token, and `--max-lines`/`--from` window content.
Every answer carries the analyzer's provenance and a freshness grade (an answer from an out-of-date index says so), an empty result is a typed "definitely none" distinct from a failure, and answers derived from conventions or models — test classification, semantic search, importance ranking — are structurally labeled, so a consumer can always tell resolved fact from heuristic.

## Usage patterns

### Keep the index current

Every query grades its answer against the index's last build.
Rebuild after every commit so the index tracks `HEAD`:

```sh
c10r hooks install
```

This installs a post-commit hook that reruns `c10r build`, resolved through git so it lands wherever this worktree actually keeps its hooks — a linked worktree or a relocated `core.hooksPath` included.
It refuses rather than overwrites when a hook is already present at that path; add the `c10r build` line to your existing hook by hand instead.

### Assessing impact

Make an edit, then ask what it could affect:

```sh
c10r impact                 # working-tree change against HEAD
c10r impact --staged        # only what's staged
c10r impact v1.2..HEAD      # a revision range
c10r impact -- src/some/dir # narrowed to a subtree
```

The answer lists the seeds — the declarations the diff actually touched — followed by their dependents: everything that could break if the change lands, to a depth bound, with the deeper reach aggregated rather than dropped.
Every answer carries an `exactness` grade.
`exact` means the index matches the change's pre-change state, so the reach shown is the real reach.
`approximate` means something has drifted since the build; the answer then carries a runnable recovery recipe — shell commands that rebuild the index at the change's exact base revision in a throwaway worktree and re-run the same query — with every value already filled in:

```sh
c10r --json impact | jq -r '.outcome.results[0].recovery.steps[]'
```

### Reading order: ranked by default

Dependents answers (`trace --relation dependents`, `impact`) order rows by distance first, and within each distance layer by how load-bearing each dependent is to the codebase (global PageRank over the dependency edges), so the first bounded page shows the nearest, most important dependents rather than an arbitrary slice.
Ranking never changes which symbols the answer contains, and no numeric score is published; every answer discloses the ordering in effect, and `--order unranked` requests a model-free ordering by stable structural keys.

### Searching by meaning

When you don't know a symbol's name, describe what the code does:

```sh
c10r search "retry a request with exponential backoff"
```

The answer is the indexed symbols nearest the query by estimated relevance, most relevant first.
A symbol's name (split into its words, so `withBackoff` is findable as "with backoff"), its documentation, and its source all count as evidence, and sharing exact words with the query is not required — the ranking fuses a semantic-embedding signal with a lexical signal, so paraphrases and exact identifiers both land.
Rows carry the signature tier by default; `--detail` changes what each row shows without changing which symbols are returned or their order.

Because the ranking is model-derived estimation, every `search` answer is structurally labeled `"classification": "estimation"` and carries the semantic index's identity as provenance.
The rows are the nearest candidates the index holds — never the complete set of relevant code, and an empty answer is never proof that no relevant code exists.
No relevance score is published: the order is the answer.

### Finding similar code

Before writing a helper, ask whether something like it already exists — or assess duplication around an existing symbol:

```sh
c10r similar some_symbol
c10r similar --at src/lib.rs:1042
```

The answer ranks every other indexed symbol by estimated content similarity, subject excluded, using the same fused ranking `search` uses.
Two deterministic clone markers can ride on rows, above the estimated ranking:

- `exact_clone` — the row's token sequence is identical to the subject's; the sources differ only in whitespace and comments.
- `variant_clone` — identical token structure with only identifiers and literal values consistently substituted (a renamed copy, or a copy with changed constants) — never a claim of behavioral equivalence.

Marked rows rank first (exact before variant), then the unmarked rows by estimated similarity.
The markers derive from persisted equivalence keys, so they are stated as fact; the surrounding ranking carries the same `estimation` label `search` does.

### Finding a symbol's tests

Before changing a symbol, ask what test code exercises it:

```sh
c10r trace some_symbol --relation tests
```

The answer is the symbol's reference sites filtered to the ones sitting inside test code, so it includes shared test helpers, not only runnable test cases.
Test code is recognized by language convention — Rust `#[test]`-style attributes, `#[cfg(test)]` modules, and `tests/` directories; Python `test_*.py` / `*_test.py` / `tests.py` / `conftest.py` files and `tests/` directories — and every site carries the rule that classified it.
Because that classification is heuristic, every `tests` answer is structurally labeled `"classification": "convention"` rather than presented with the confidence of the resolved relations.
An empty answer means no convention-classified reference site was found — not proof that nothing tests the symbol.

### The index store

The index lives at `--db` (default `.c10r/index.db`), and `c10r` only ever writes to, replaces, or removes a store it created itself.
Every store is stamped with an ownership marker at creation, and every command checks that marker before it touches the file, so a mistyped `--db`, a symlinked `.c10r` directory, or a path collision cannot destroy or pollute unrelated data.
A file that is not `c10r`'s own is refused and left untouched; the refusal names both recoveries — rebuild if it is a stale `c10r` index, or correct `--db` if it belongs to something else — and never tells you to delete it.
Queries never create a store, and a store built under an older schema version is `c10r`'s own to replace: `build` rebuilds it in place, while a query refuses it and names the rebuild.

Each store also records the workspace root it was built from.
An answer read from a store that describes a different workspace carries a `workspace_relation` marker naming that root, separately from the staleness flag: stale means the graph is behind, mismatched means it is about somewhere else.

## Exit codes

`c10r` signals outcomes through a closed exit-code taxonomy, one distinct code per category, so a caller can branch without scraping diagnostics:

| Code | Meaning                                                                 |
| ---- | ----------------------------------------------------------------------- |
| 0    | Success, including a typed-empty answer                                 |
| 1    | Generic operational failure                                             |
| 2    | Usage error (malformed invocation)                                      |
| 3    | No index at the store path — run `c10r build`                           |
| 4    | Store built under a different schema version — rebuild it               |
| 5    | A required language indexer is missing or unresponsive                  |
| 6    | The file at the store path could not be confirmed as `c10r`'s own store |

## Shell completions

`c10r completions <shell>` emits a completion script generated from the same command definition the parser executes, so the completed surface cannot drift from the real one.

zsh:

```sh
c10r completions zsh > ~/.zfunc/_c10r
```

Add `~/.zfunc` to `fpath` before `compinit` runs (e.g. in `~/.zshrc`), then start a new shell.

bash:

```sh
c10r completions bash > ~/.local/share/bash-completion/completions/c10r
# or source it directly in ~/.bashrc:
source <(c10r completions bash)
```

## MCP server

An agent in an MCP host can reach `c10r` as named tools instead of composing shell invocations.
The server lives in `mcp/`, runs over stdio, and runs the `c10r` binary you installed — it bundles nothing and changes nothing about the CLI.

It advertises eight tools, one per command it covers: `get`, `trace`, `find`, `search`, `similar`, `impact`, `build`, and `hooks_install`.
Each returns the command's own structured answer unmodified, so provenance, freshness, typed absence, and heuristic-grade labels arrive intact.
A command failure arrives as a tool error carrying the exit category and `c10r`'s own diagnostic, so a caller recovers from the error alone.
`cache` is deliberately absent — nothing on this surface removes an index store.

### Install

```sh
cargo build --release          # the server runs the c10r you install
cargo install --path . --locked
uv sync --project mcp
```

### Configure a client

The server answers about one workspace per call.
Precedence is the `root` a tool call names, then the root the server was launched for, then the directory the server process is started in.

Per repository, where the launch directory is already the project, no root ever needs naming:

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

Installed once for every project, pin the workspace explicitly rather than relying on where the host starts the process:

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

A configured workspace that is not an existing directory is refused at startup rather than silently replaced by the process's own directory.
An answer that is correct about the wrong project is indistinguishable from a correct one until it causes damage.

### Two startup refusals, two different remedies

The server checks the binary before advertising anything, and refuses on either of two conditions.
They are kept apart because their remedies are unrelated:

| Refusal                                                                          | Remedy                                                                                                           |
| -------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| The binary's command-surface version is not the one the server was built against | Align the versions — upgrade the server package, or install a matching `c10r`. Rebuilding an index does nothing. |
| The binary cannot be found or cannot be run                                      | Install `c10r`, or point `C10R_BINARY` at it                                                                     |

Neither is an index-state failure.
An absent or incompatible index arrives per call, as an `absent_index` or `incompatible_store` tool error carrying `c10r`'s own diagnostic, and is fixed by a rebuild.

### Changing state requires consent

`build` and `hooks_install` are gated twice.
Each refuses until the call sets its `acknowledge` parameter, and the refusal states what the operation would do — so an agent cannot stumble into a rebuild while exploring the surface.
Acknowledgment proves deliberateness but not consent, since the agent sets it itself, so each tool then asks the user to confirm, through whichever mechanism the negotiated protocol era provides.

If the client offers no elicitation capability, the operation is refused rather than performed, and the refusal names the command that does the job — `c10r build` or `c10r hooks install`.
The guarantee is constant that way: the operation happens behind confirmed consent or not at all, and the answer always says which.

Every tool runs to completion within its call.
A `build` on a large repository can outlast a client's request timeout; run `c10r build` in a shell when it does, and install the commit hook so a manual rebuild stays rare.

### Tests

The Rust suite and the server's suite are separate commands:

```sh
cargo test
uv run --directory mcp pytest
```

`--directory` rather than `--project`: pytest reads its configuration from the directory it starts in, and the server's settings live in `mcp/pyproject.toml`.
Started from the repository root, pytest finds no configuration and every asynchronous test fails.

The server's tests run against a real `c10r` binary and a real index built by it; they fail rather than skip when no binary is present, since a skipped conformance check reports as a pass.
Build one with `cargo build --release` first.

## Embedded model credit

Semantic search is powered by [`potion-code-16M-v2`](https://huggingface.co/minishlab/potion-code-16M-v2), a static code-embedding model by [The Minish Lab](https://minish.ai/), used and redistributed under the MIT License (see `models/potion-code-16M-v2.LICENSE`).
Inference runs through the Minish Lab's official [`model2vec-rs`](https://github.com/MinishLab/model2vec-rs) crate, and vector storage rides [`sqlite-vec`](https://github.com/asg017/sqlite-vec).
The model files are fetched at build time from a pinned upstream revision and verified against the committed checksum manifest; they are compiled into the binary, so nothing is downloaded at runtime.

## References

- [Why coding agents fail in large codebases (and what to do about it) | Sourcegraph](https://sourcegraph.com/blog/why-coding-agents-fail-large-codebases)
- [10 Principles for Agent-Native CLIs - by Trevin Chow](https://trevinsays.com/p/10-principles-for-agent-native-clis)

## AI Disclosure

This project uses spec-driven development to allow AI coding assistance to work on well-specified features.
See the `sdd-*` family of [ahgraber/skills: Agent skills](https://github.com/ahgraber/skills).
