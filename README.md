# silent-cartographer

Coding agents (Claude Code and peers) have two compounding failure modes on non-trivial codebases:

**Task myopia.**
The agent knows the files it is currently touching but has no model of what it is outside those files.
Without considerable effort, it cannot innately reason about blast radius, architectural implications, or whether its in-flight changes degrade structural health.

**Search inefficiency.**
The agent's primary tool for locating relevant code is grep (or ripgrep), which returns plausible, unranked matches.
On a large codebase, a single grep call can consume most of a context window before the agent has written a line of code.

Silent cartographer (`c10r`) is a persistent codebase knowledge graph that uses static (AST-based) and semantic ([SCIP](https://github.com/sourcegraph/scip)/LSP-based) analysis for structural and type-aware symbol resolution.
Users and agents can query it to find code by symbol name or concept, trace the blast radius of a change, and understand architectural structure.
In comparison to grepping through the whole codebase, `c10r` increases search precision and reduces token utilization.

## Shell completions

`c10r completions <shell>` emits a completion script to standard output, generated from the same command definition the parser executes, so the completed surface cannot drift from the real one.

zsh:

```sh
c10r completions zsh > ~/.zfunc/_c10r
```

Add `~/.zfunc` to `fpath` before `compinit` runs (e.g. in `~/.zshrc`), then start a new shell.

bash:

```sh
c10r completions bash > ~/.local/share/bash-completion/completions/c10r
```

Or source it directly in `~/.bashrc`:

```sh
source <(c10r completions bash)
```

## Usage patterns

### Keep the index current

`impact`, like every query, grades its answer against the index's last build.
Rebuild after every commit so the index tracks `HEAD` and stays on the exact path rather than drifting into the disclosed-approximate one:

```sh
c10r hooks install
```

This installs a post-commit hook that reruns `c10r build`, resolved through git so it lands wherever this worktree actually keeps its hooks — a linked worktree or a relocated `core.hooksPath` included.
It refuses rather than overwrites when a hook is already present at that path; add the `c10r build` line to your existing hook by hand instead.

### Assessing impact

Make an edit, then ask what it could affect:

```sh
c10r impact
```

The answer lists the seeds — the declarations the diff actually touched — followed by their dependents: everything that could break if the change lands.
Narrow to a subtree, or scope to what's staged, exactly as `git diff` would:

```sh
c10r impact -- src/some/dir
c10r impact --staged
```

Every answer carries an `exactness` grade.
`exact` means the index matches the change's pre-change state, so the reach shown is the real reach.
`approximate` means something has drifted since the build (an unrelated edit, a change made before the index was refreshed), so the dependents shown may be incomplete; treat it as directional, not a ship/no-ship verdict.

### Finding a symbol's tests

Before changing a symbol, ask what test code exercises it:

```sh
c10r trace some_symbol --relation tests
```

The answer is the symbol's reference sites filtered to the ones sitting inside test code, so it includes shared test helpers, not only runnable test cases.
Test code is recognized by language convention — Rust `#[test]`-style attributes, `#[cfg(test)]` modules, and `tests/` directories; Python `test_*.py` / `*_test.py` / `tests.py` / `conftest.py` files and `tests/` directories — and every site carries the rule that classified it.
Because that classification is heuristic, every `tests` answer is structurally labeled `"classification": "convention"` (with a matching notice in the human rendering) rather than presented with the confidence of the resolved relations.
An empty answer means no convention-classified reference site was found — not proof that nothing tests the symbol.

### Recovering an exact answer

An approximate answer carries a runnable recovery recipe: shell commands that rebuild the index at the change's exact base revision, in a throwaway worktree, and re-run the same `impact` query against it — with the base revision, the `--db` path, and the `--workspace` identity already filled in.
Copy the steps and run them as printed:

```sh
c10r --json impact | jq -r '.outcome.results[0].recovery.steps[]'
```

## References

- [Why coding agents fail in large codebases (and what to do about it) | Sourcegraph](https://sourcegraph.com/blog/why-coding-agents-fail-large-codebases)
- [10 Principles for Agent-Native CLIs - by Trevin Chow](https://trevinsays.com/p/10-principles-for-agent-native-clis)

## AI Disclosure

This project uses spec-driven development to allow AI coding assistance to work on well-specified features.
See the `sdd-*` family of [ahgraber/skills: Agent skills](https://github.com/ahgraber/skills).
