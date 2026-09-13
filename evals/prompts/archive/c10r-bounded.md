This repository has a pre-built code index.
You query it by running the `c10r` command-line program through the Bash tool.
There is no tool called `c10r` — it is a shell command, like `grep`.
It answers structural questions precisely at far lower cost than scanning the repository.

Rule: a c10r result replaces the search you would otherwise have run.
When a c10r command answers your question, act on the answer.
Do not run `grep`, `rg`, `find`, Grep, or Glob to confirm something c10r already told you.
Search that way only when a c10r command returned nothing you can use.

Rule: ask for a few rows, then page for more.
Every result-bearing command takes `--limit <N>` and returns 25 rows when you do not set it.
Start with `--limit 5`.
Everything a command prints stays in front of you for the rest of the task, so a wide result you skim once is re-read on every later step.

A truncated result ends with its position and a continuation token:

```text
page 1: 5 of 63 results (truncated)
  resume with --cursor abc123
```

That means you saw 5 of 63, strongest matches first for `search`.
If none of them fits, run the same command again with `--cursor abc123` to get the next rows.
Page while the rows are still getting closer to what you want, and stop when they are not.

Commands, each run with Bash:

- Find code by what it does, in natural language: `c10r search "<what the code does>" --limit 5`
- Find a symbol by (partial) name: `c10r find <fragment> --limit 5`
- See a symbol's definition: `c10r get <name>` (add `--detail signature|interface|body` for how much source to show)
- Who references or depends on a symbol: `c10r trace references <name> --limit 5`, `c10r trace dependents <name> --limit 5`
- What contains a symbol, or what it contains: `c10r trace containers|contains <name> --limit 5`
- Which modules import it, which types implement it, which tests exercise it: `c10r trace importers|implementers|tests <name> --limit 5`

`c10r search` matches on meaning, so give it a phrase describing the behaviour — `c10r search "load an ODS spreadsheet" --limit 5` — not a bare keyword and not a regular expression.
For exact syntax, run `c10r --help` or `c10r <command> --help`.

Workflow:

1. Start from the issue's key terms: `c10r search` for behaviour, `c10r find` for names.
2. Read a candidate's code once, with `c10r get <name> --detail body` or by reading the file.
   One pass is enough.
3. Use `c10r trace references` or `c10r trace dependents` when you need to decide whether the fix belongs in a symbol or in one of its callers.
4. Name a file only when you can say what change it needs.
   A file you merely found is not an answer.
5. Write the JSON answer exactly as the task instructs, then stop.
