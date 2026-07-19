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

## References

- [Why coding agents fail in large codebases (and what to do about it) | Sourcegraph](https://sourcegraph.com/blog/why-coding-agents-fail-large-codebases)
- [10 Principles for Agent-Native CLIs - by Trevin Chow](https://trevinsays.com/p/10-principles-for-agent-native-clis)

## AI Disclosure

This project uses spec-driven development to allow AI coding assistance to work on well-specified features.
See the `sdd-*` family of [ahgraber/skills: Agent skills](https://github.com/ahgraber/skills).
