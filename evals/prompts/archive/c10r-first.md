This repository has a pre-built code index.
You query it by running the `c10r` command-line program through the Bash tool.
There is no tool called `c10r` — it is a shell command, like `grep`.
It answers structural questions precisely at far lower cost than scanning the repository.

Rule: before you run `grep`, `rg`, `find`, or the Grep or Glob tools, run at least one `c10r` command for the thing you are looking for.
If what comes back does not cover it, search however you like — the c10r command just comes first.

Commands, each run with Bash:

- Find code by what it does, in natural language: `c10r search "<what the code does>"`
- Find a symbol by (partial) name: `c10r find <fragment>`
- See a symbol's definition: `c10r get <name>` (add `--detail signature|interface|body` for more)
- Who references or depends on a symbol: `c10r trace references <name>`, `c10r trace dependents <name>`
- What contains a symbol, or what it contains: `c10r trace containers|contains <name>`
- Which modules import it, which types implement it, which tests exercise it: `c10r trace importers|implementers|tests <name>`

`c10r search` matches on meaning, so give it a phrase describing the behaviour — `c10r search "load an ODS spreadsheet"` — not a bare keyword and not a regular expression.
A literal or pattern match is what grep is for, after the c10r query.
For exact syntax, run `c10r --help` or `c10r <command> --help`.

Workflow discipline:

1. Start from the issue's key terms: `c10r search` for behavior, `c10r find` for names.
2. Confirm each candidate with `c10r get` and read the code it points to before including it in your answer.
3. Use `c10r trace references` / `dependents` to decide whether the fix lives in the symbol or in a caller.
4. Keep your answer to the minimal file set the fix requires; then write the JSON answer exactly as the task instructs and stop.
