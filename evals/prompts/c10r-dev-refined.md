This repository has a pre-built code index.
You query it by running the `c10r` command-line program through the Bash tool.
There is no tool called `c10r` — it is a shell command, like `grep`.
It answers structural questions precisely, at far lower cost than scanning the repository.

Rule: a c10r result replaces the search you would otherwise run.
When a c10r command answers your question, act on the answer.
Do not run `grep`, `rg`, `find`, Grep, or Glob to confirm something c10r already told you.
Confirming costs a second step and a second result, both re-sent from then on.
If you are about to grep for `def foo` and then read the lines around the match, run one command instead.
Use `c10r get foo --detail body`.

Rule: ask several questions in one step.
Issue c10r commands together, not one per step — they are independent and cheap.
Everything a command prints stays in front of you for the rest of the task.
A result you fetch early gets re-read on every step after it.
Fewer, fuller steps cost less than many small ones.

Rule: narrow the list, then take the whole symbol.
Start with `--limit 5` so a candidate list stays short.
Once you choose a symbol, read all of it with `c10r get <name> --detail body`, not a few lines at a time.

Rule: when a command fails, read its help before you retry.
`c10r --help` lists the commands.
`c10r <command> --help` gives the exact syntax and flags for one of them.
Read a command's help on its first error only.
The text does not change, so a second read tells you nothing.
Apply what you learn to every later call of that command.
`c10r trace` is the easiest to get wrong: the symbol comes first and `--relation` is required.
Write `c10r trace <name> --relation references`, not `c10r trace references <name>`.
A rejected command is a usage mistake, not a reason to stop using c10r.
`absent: nothing resolved` is an answer, not a failure — nothing is indexed under that name.
Look for the right name with `c10r find` before you fall back to grep.

Commands, each run with Bash:

- Find code by what it does, in natural language: `c10r search "<what the code does>" --limit 5`
- Find a symbol by (partial) name: `c10r find <fragment> --limit 5`
- See a symbol's definition: `c10r get <name> --detail body`
- See relations between symbols: `c10r trace <name> --relation <relation> --limit 5`

Every result-bearing command takes `--limit <N>`.
It returns 25 rows when you do not set one.
A truncated list ends with `resume with --cursor <TOKEN>`.
Run the same command with that token when none of the rows you saw fit.

`c10r search` matches on meaning.
Give it a phrase that describes the behavior, not a bare keyword and not a regular expression.
`c10r search "load an ODS spreadsheet" --limit 5` is the right shape.

`c10r find` matches names only.
Give it a name fragment, not a filename and not `class Layout` or `def layout`.
`c10r find Layout` is the right shape.

`c10r get` and `c10r trace` accept a short name (`connect`) or a qualified name (`Client::connect`).
They also accept the full identity that every answer prints.
Prefer the short name; an ambiguous one returns a list of candidates to choose from.
Do not invent a module path — c10r does not accept a form like `package.module.Class::method`.

`c10r trace` takes the symbol first; `--relation` is required and takes one of:

| Relation       | Returns                                     |
| -------------- | ------------------------------------------- |
| `references`   | the sites that use the symbol               |
| `dependents`   | everything that depends on it, transitively |
| `containers`   | what encloses it                            |
| `contains`     | what it encloses                            |
| `importers`    | the modules that import it                  |
| `implementers` | the types that implement it                 |
| `tests`        | the tests that exercise it                  |

Use `references` or `dependents` to decide whether the fix belongs in the symbol or in one of its callers.
