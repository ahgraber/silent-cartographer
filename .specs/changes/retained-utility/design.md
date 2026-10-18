# Design: retained-utility

## Context

Every persisted symbol takes its identity from the semantic indexer.
`ingest_with_params` in [src/graph/mod.rs](../../../src/graph/mod.rs) builds symbol rows by iterating the indexer's symbols, and finds each row's declaration in the syntax tree through the join's aligned definitions.
A declaration that the indexer does not report is only counted, as `syntax_only`.

The pipeline already separates into two halves at the symbol rows.
Before the rows exist, every step reads the semantic index: unit assignment, identity projection, the document-to-module map, the join, and the name-span maps built from aligned definitions.
After the rows exist, span containment, the semantic corpus, chunks, embeddings, clone keys, fragments, the source-file table, and most of the metadata read only the rows and the parsed documents.

The store records one analyzer name, and that name is the only record of the store's language: `backend_language` turns it back into a `Language`.
No answer carries a tier, and the `Outcome` enum has no value for an answer the index cannot give.
The only interactive input c10r reads today is a subject list on standard input, refused when standard input is a terminal; `read_subject_list` takes `stdin_is_terminal` as a parameter so that the refusal is testable.

The MCP server maps every non-zero exit code to a tool error in `mcp/src/c10r_mcp/errors.py`.

A measurement on 2026-10-17 compared tree-sitter-bash 0.25.1 with `shfmt --to-json` 3.13.1 on 1,178 shell files: local scripts, nvm, bats-core, and dokku.
The report and its scripts are in `._scratch/research/retained-utility/bash-probe/`.

- Function definitions: both parsers found the same 3,038.
- Commands whose name is a literal word: 99.94% of the commands tree-sitter-bash reported were also reported by shfmt.
  tree-sitter-bash found 91.8% of the commands shfmt found.
  shfmt counts `[ … ]` and `unset` as commands, and tree-sitter-bash gives them their own node kinds; without those two, it found 98.7%.
- The other misses: a line that starts with a backslash, such as `\. "$NVM_DIR/nvm.sh"`, is joined onto the previous command (312 commands, mostly in nvm tests); some command substitutions inside heredocs (10); and multi-line brace-expansion word lists that tree-sitter-bash parses as an error (21).
- tree-sitter-bash reported a syntax error in 15 files; shfmt parsed all 1,178.

This change follows `unresolved-calls` and `method-overrides`.
It uses the unresolved-call records and the `references` answer groups that `unresolved-calls` adds, and the `implements` relation that `method-overrides` adds.

## Decisions

### Decision: A structural identity is the file path with a leading `./`, then the nesting

**Chosen:** A structural identity is `{workspace}::./{path}::{segment}::…`, where `{path}` is the file's path relative to the workspace root with `/` separators, and each segment is the name text of an enclosing declaration, outermost first, then of the declaration.
A file's own identity is `{workspace}::./{path}`.
In the path, `%`, `:`, and `#` are percent-encoded (`%25`, `%3A`, `%23`); in a segment, `%` and `#` are.
When two declarations in one file would get the same text, each gets the `#n` suffix, numbered in file order, as `project_all` in [src/identity.rs](../../../src/identity.rs) already numbers colliding semantic identities by definition site.
The system recognizes a structural identity by the `::./` that follows the store's recorded workspace identity, not by the first `::` in the text.
Reference resolution in [src/query/resolve.rs](../../../src/query/resolve.rs) changes in two ways, after the exact canonical-identity match, which still wins first.
Symbols whose `display_name` equals the reference are merged into the candidates of the `::` branch and of the `.` branch, so the whole Bash names `lib::log` and `log.sh` resolve before the reference is read as a qualified or dotted name.
The qualified-suffix match ignores a trailing `#n`, so `lib::log` also matches `ws::./lib.sh::lib::log#1`, the nested `log`.

**Encoding rationale:** A path may contain `::`, so without encoding the file `lib.sh::log.sh` and the function `log.sh` in `lib.sh` get the same text, across two files, where the per-file disambiguator cannot reach.
With `:` encoded, the path ends at the first `::` after `./`, and a file's identity can equal no declaration's.
A Bash name may contain `#`, which would read as a disambiguator; encoding it in names keeps `#n` unambiguous.
Common names are unchanged, because paths with `:`, `#`, or `%` and Bash names with `#` are rare; `lib::log` keeps its `::`.
A workspace identity is free text and may itself contain `::./`, so recognition uses the recorded workspace identity, which every query has.

| Language | Declaration                               | Segment text                              |
| -------- | ----------------------------------------- | ----------------------------------------- |
| Bash     | `function_definition`                     | its `name` field, such as `lib::log`      |
| Python   | `class_definition`, `function_definition` | its `name` field                          |
| Rust     | items other than `impl_item`              | its `name` field                          |
| Rust     | `impl_item`                               | its `type` field's text, such as `Widget` |

**Rationale:** The `./` prefix makes the two kinds of identity disjoint: neither Cargo nor the Python packaging name rules (PEP 508) allow a package name that begins with `.`, so a semantic identity's second component never begins with `.`.
A test checks this for the name forms each conformance project uses.
A Python project name may contain `.`, so a path without the prefix, such as `deploy.sh`, could equal a project name.
Bash allows `::` inside a function name, so two different nestings can produce the same text; numbering equal texts keeps every identity unique without escaping.
Reference resolution already works on the whole text: `symbols_by_qualified_suffix` matches `canonical_id LIKE '%::<ref>'`, so `./lib.sh::log` and `log` both resolve.
The suffix match alone misses `lib::log` when the names carry `#n`, so the display-name step is needed for Bash names that contain `::`.

**Alternatives considered:**

- Guess a module path from the file layout: the guess differs from the indexer's in `#[path]`, `cfg`-gated modules, `+shared` files, and Python `src/` layouts, and a guessed name that looks semantic resolves to the wrong symbol in a semantic index.
- Escape `::` inside segments: every consumer that splits on `::` would need to unescape, for a case that disambiguation already handles.

### Decision: Grammar provenance records the tier, and each file records its language and tier

**Chosen:** A structural index records the grammar as its analyzer: `tree-sitter-bash`, `tree-sitter-rust`, or `tree-sitter-python`, with the grammar crate's version.
`backend_language` maps each grammar name to its `Language`, and a new `index_tier` maps a grammar name to the structural tier and an indexer name to the semantic tier.
The version is a constant next to the grammar's `LANGUAGE` import, and a unit test checks it against the version in `Cargo.lock`.
The `source_files` table gains `language`, `tier`, and `syntax_errors` columns, written for every file; `syntax_errors` is true when the file's tree holds an `ERROR` or `MISSING` node.
`targets` holds an empty inventory and `environment` is null for a structural index.

The observation of the analyzer and environment in effect becomes tier-aware.
`observe_state_from` in [src/commands.rs](../../../src/commands.rs) today picks the Python collector and environment when the recorded language is Python and the Rust collector and rust-analyzer probe otherwise.
For a structural store it collects the recorded language's sources, Bash included, reports the compiled-in grammar name and version as the analyzer in effect, and reports no environment.
Queries, `status`, and the manifest all observe through it.

**Rationale:** The existing staleness rule compares the recorded analyzer name and version with the one in effect, so recording the grammar there marks a structural index stale when the grammar is upgraded.
The comparison holds only if the observation reports the grammar for a structural store; without the tier-aware branch, a structural Python store on a host with scip-python would compare against scip-python and look stale at once, and a Bash store would hash Rust files.
The recorded analyzer name also lets a build decide whether the store is structural without a new metadata column.
The per-file columns are the one storage choice that is expensive to change later: `multi-language-builds` puts files of several languages and tiers in one store, and with the columns in place it adds rows instead of migrating the answers.

**Alternatives considered:**

- A `tier` column in `index_metadata` only: it is enough for one language per store, and it would have to move to the file level in `multi-language-builds`.
- Read the version from the crate at run time: tree-sitter grammar crates expose no version constant.

### Decision: Split the ingest at the symbol rows, and give the structural build its own first half

**Chosen:** `ingest_with_params` splits into two functions.

- The first half produces the symbol rows, the occurrence rows, the edges, and the discrepancies.
  The semantic first half is today's code up to and including the type-hierarchy edges and the discrepancies, unchanged.
  The structural first half walks each prepared document's declarations, keeps those the language's syntax profile names, and builds the rows from them: structural identity, display name, kind, location, header end, and tier content from `declaration_tiers`.
  It adds one module row per file, whose signature tier is the file's identity without the workspace.
  It writes `contains` edges from the file to its outermost declarations, and from each declaration to the declarations written directly inside it, from `enclosing_declarations`.
- The second half is today's code from span containment to the metadata write, shared by both builds.

Both halves run inside one `begin_build` transaction, as today, after `clear_derived`, so a build of either tier removes every row of the previous build of either tier.
The structural first half writes no test classification: `classify_test_symbols` runs only in the semantic first half.

**Rationale:** The second half already depends only on the symbol rows and the parsed documents, so sharing it keeps one corpus, one embedding path, and one clone path for both tiers.
Feeding the existing ingest a fabricated `ExtractedIndex` instead would make the join align fabricated occurrences, and every number in the join accounting would describe nothing real.
A copy of the second half for the structural build would be the near-copy that the project rules forbid.

**Alternatives considered:**

- A fabricated `ExtractedIndex` from the syntax tree: rejected above.
- A separate structural ingest module with its own corpus and clone steps: duplicates the second half.

### Decision: Declaration kinds per language for the structural first half

**Chosen:** The structural first half keeps a declaration from `all_declarations` when its node kind is in the language's structural list, and, for Rust and Python, when no `function_item` or `function_definition` encloses it.

- Bash: `function_definition`, at any depth.
- Python: `class_definition`, `function_definition`.
- Rust: every kind `declaration_kinds` lists today: `mod_item`, `struct_item`, `enum_item`, `union_item`, `trait_item`, `impl_item`, `function_item`, `const_item`, `static_item`, `type_item`, `function_signature_item`.

The display name is the segment text.
The kind maps to the existing `kind_tag` vocabulary: `impl_item` and the type items to `type`, `trait_item` to `trait`, functions to `function`, functions inside a class, impl, or trait to `method`, `mod_item` to `module`, and `const_item` and `static_item` to `constant`.

**Rationale:** These are the declarations the syntax layer already enumerates for the join, so the structural first half adds no syntax walker for Rust and Python.
A Bash function defined inside another function is global once the outer function runs, so excluding it would lose a real function.

### Decision: Bash commands are calls by node kind

**Chosen:** The unresolved-call walk from `unresolved-calls` gains a Bash row: a `command` node whose `name` field is a `command_name` with a single `word` child is a call, with that word as the called name and no receiver.
A `command_name` that holds an expansion, a string, or a concatenation yields no call.
`test_command` (`[ … ]`, `[[ … ]]`), `unset_command`, `declaration_command` (`local`, `export`, `declare`), and arithmetic commands are other node kinds, so the walk does not see them.
A `command` whose command word is `time` skips the leading arguments that are `time` options (`-p`), and yields a call only when the next argument is a plain `word`; when that argument is a string, an expansion, or a concatenation, the command yields no call. tree-sitter-bash parses the `time` pipeline prefix as a command with arguments, so the operand is the first argument after the options, never a later one.
The enclosing declaration comes from `enclosing_declarations`, with the file's module row when no function encloses the command.
The unresolved-call group's called name for a Bash subject is its whole `display_name`, not the part after the last `::`.

**Rationale:** The node kinds match what the spec counts as a call, and the measurement shows the result agrees with shfmt on 98.7% of the commands both treat as calls.
`time` is a reserved word of the shell, so `time deploy` runs `deploy`; shfmt parses it that way.
`sudo` and `exec` are commands that run their argument, and both parsers name them as the command, which is how the command reads.

### Decision: Bash discovery reads a bounded first line, and every consumer uses it

**Chosen:** `language_discovery_rules` gains Bash, with one classifier that takes a path and the file's first bytes:

- a regular file only; a symlink is followed only when its target is a regular file, and other file types are skipped without being opened;
- the first 256 bytes, up to the first newline; a first line that does not end within them is not a shebang;
- a shebang (`#!`) whose interpreter path ends in `/sh` or `/bash`, or `env` with optional flags (such as `-S`) followed by `sh` or `bash`, selects the file;
- a shebang that names another interpreter excludes the file, whatever its extension;
- otherwise a `.sh` or `.bash` extension selects the file, and an extensionless file without a shell shebang is not selected.

Discovery skips the same directories as the other languages.
Detection selects Bash when neither `Cargo.toml` nor `pyproject.toml` exists and discovery finds at least one file.

`is_discovered_path` takes the classifier's content input for Bash.
`run_impact` today filters changes by `is_discovered_path(pre_path)` before it fetches pre-change content ([src/commands.rs:2304](../../../src/commands.rs)).
For a Bash index it first fetches the pre-change content of every changed path that has a `.sh` or `.bash` extension or no extension, then keeps a change when the classifier accepts the pre-change path with its pre-change content, when the path is held in `source_files`, or when the classifier accepts the post-change content.
The pre-change source hash is rebuilt from the same set.
A historical source that the current index never held is therefore kept, and reported as an unmappable region, as `Impact freshness across the straddle` requires.
The status report's changed, added, and removed files use the classifier on the current content and `source_files` for the indexed set.

**Rationale:** Extensionless scripts are common in `bin/` directories, and reading a bounded prefix of regular files only keeps discovery cheap and keeps it from blocking on a pipe or reading a large binary.
A shebang is the interpreter the system runs, so it outranks the extension.
Discovery of a Bash file depends on its content, so a path-only filter cannot classify a deleted or re-shebanged script; `source_files` records what the index held, which is the pre-change side impact needs.
A manifest takes precedence, because a Rust or Python workspace with a few scripts is a Rust or Python workspace until `multi-language-builds`.

### Decision: Bash token classes for clone keys

**Chosen:** The per-language token tables in [src/graph/syntax.rs](../../../src/graph/syntax.rs) gain Bash entries:

- comment kinds: `comment`;
- literal kinds: `number`, `raw_string`, `ansi_c_string`, and a `string` with no expansion inside it;
- literal by position: a plain `word` that is an argument of a command, and a plain `word` that is the value of a `variable_assignment`;
- identifier tokens: `variable_name`, the `name` of a `function_definition`, and the `word` of a `command_name`;
- every other leaf, including the parts of a `string` that holds an expansion, is a structure token.

The position rule needs the parent node, so the Bash token walk classifies a `word` by its parent's kind and field, not by the static kind tables alone.

**Rationale:** The clone keys normalize identifiers and literals per their class, so a Bash entry is needed for a consistently renamed copy of a function to share a key.
In Bash an unquoted argument such as `one` in `echo one` is data, as a quoted string is, so `f() { echo one; }` and `g() { echo two; }` share a substitution key, as the clone contract requires for a consistent literal substitution.
An option such as `-p` is also a plain argument `word`, so two commands that differ only in their options share a substitution key and differ in their formatting key.
A string that holds an expansion is code, not data, so it is split into structure tokens, and a changed expansion changes the key.

### Decision: Syntax errors are recorded per file and disclosed on negative answers

**Chosen:** The shared second half writes `syntax_errors` for every file.
On a structural index, an answer whose outcome is absent or empty, and a `find` answer with no results, carries `syntax_error_files`, the count of files with the flag set, when it is not zero; the human rendering adds `N indexed files have syntax errors; a declaration in them may be missing`.
`status` lists those files.

**Rationale:** A declaration that the grammar loses inside an error region is missing from a structural index with no other trace, as the review's `foo#0() { :; }` case shows.
The count makes a negative answer state the condition under which it may be wrong; the comparison found errors in 15 of 1,178 files, so most answers carry no count.

### Decision: The build resolves its tier before it opens the store for writing

**Chosen:** `run_build` resolves the tier after language detection and before `open_or_replace`:

```text
language = detect(--language, manifests, Bash discovery)
if language is Bash or --structural:
    tier = structural (cause: Bash, or the flag)
    currency check against the grammar provenance; current -> no work
else:
    recorded = recorded metadata (read-only open, as recorded_python_indexer does)
    consent  = recorded is structural, same canonical root, same language
    construct engine
      ok   -> currency check against the engine provenance (as today); current -> no work
              engine.analyze
                ok     -> tier = semantic
                failed -> fallback(cause = analysis failure)
      fail -> fallback(cause = setup failure)

fallback(cause):
    consent                                          -> tier = structural; currency check against
                                                        the grammar provenance; current -> no work
    not a hook build, stdin and stderr are terminals -> confirm(cause); yes -> structural
    otherwise                                        -> fail with cause; message names --structural
```

The prompt is a `confirm` parameter of `run_build`, a closure that `main` builds from standard error and standard input, so tests drive both answers without a terminal.
`run_hooks_run` passes a closure that never asks and answers no. The prompt states the indexer's failure, that a structural index does not answer references or dependents, and that its impact answer lists touched declarations without their dependents; the default is no, and end of input is no. A setup failure keeps the indexer-setup exit code, and an analysis failure keeps the general exit code; each message gains the `--structural` remedy.
The build answer's cause is the flag, Bash, or the failure the build observed; a flagged build runs no engine and so never names an indexer failure.
`--structural` conflicts with `--python-indexer` and `--rust-analyzer` as a usage error.
`--structural` on a Bash build is accepted and changes nothing.

**Rationale:** Resolving the tier before the store opens keeps the existing guarantee that a refused build leaves the store unchanged.
The currency check stays before `analyze`, so a current semantic store does no work, as `A build over unchanged inputs does no work` requires; a structural store whose indexer now constructs is not current against the engine provenance, so the build analyzes and upgrades it.
Consent is the recorded tier together with the recorded canonical root and language, so a shared `--db` holding another workspace's structural index does not authorize a structural build of this one.
A Git hook can run with a terminal attached, and its build must not block the Git operation or replace a semantic store on a keystroke meant for something else; the hook's closure keeps the stale semantic store, and the hook reports the failure as it does today.
The refresh hooks call `run_build` with the recorded language and no flag, so a structural store stays structural through hook builds, as the user decided, and `refresh_command` needs no new flag.

A structural Python build that follows a failure of the selected indexer records that indexer in a new nullable `index_metadata.attempted_analyzer` column, and `recorded_python_indexer` reads it when the recorded analyzer is a grammar.
A later build without `--python-indexer` therefore retries the indexer the user chose, for example ty-scip, instead of the new-store default.
A build with `--structural` records no attempted indexer, because `--structural` conflicts with `--python-indexer`, so its later upgrade uses scip-python.

`recovery_recipe` in [src/query/impact.rs](../../../src/query/impact.rs) builds its throwaway index with `c10r build` and no language or tier.
`ImpactRequest` gains the recorded language and tier, and the recipe adds `--language` always and `--structural` for a structural Rust or Python index, so following it rebuilds the same kind of index, as `Impact freshness across the straddle` requires.

### Decision: An `unavailable` outcome, a `tier` field, and exit code 7

**Chosen:**

- `Outcome` gains `Unavailable { reason }`, serialized as `"outcome": "unavailable"` with a `reason` string.
- `Answer` gains `tier`, `"semantic"` or `"structural"`, read with the provenance when `QueryEngine` opens the store.
- `QueryEngine::trace` returns `Unavailable` for `references`, `importers`, `implementers`, `implements`, and `tests` on a structural index after the subject resolves to one symbol, and `QueryEngine::dependents` returns it for `dependents`.
  `dependents` does not go through `trace`: `run_trace` sends it to `QueryEngine::dependents` directly ([src/commands.rs:1705](../../../src/commands.rs)), so the check is in both methods, after subject resolution.
  For `references`, the unresolved-call group is attached as `unresolved-calls` attaches it to any `references` answer, and the unconfirmed-use group carries a total of zero, because a structural build refuses no occurrence.
- An `impact` answer on a structural index whose change touches an indexed declaration keeps the outer outcome `found`, its `seed_outcome` and `seeds`, carries an empty `dependents` report, and adds `dependents_outcome: "unavailable"` with a reason; the field is absent on a semantic index.
  The renderer prints the seeds and the unavailable line, and skips the reach, horizon, and empty-dependents lines when the field is present.
  A `NoIndexedSymbolTouched` report keeps its definite wording and exit code 0.
- On a structural index, an `Absent` or `Empty` outcome, and a `find` answer with no results, carry `syntax_error_files` when it is not zero.
- `Absent` gains an optional `reason`, set when the reference has the shape of the other tier's identity.
- `status` gains `tier` and `unanswered_relations`; the manifest's built index state gains `tier`.
- `ExitCode::Unavailable` is 7.
  A command handler returns its output with the exit category of its answer, and `main` prints the output and exits with that category.
  The category is 7 for an `Unavailable` outcome and for an impact report with `dependents_outcome` set, and 0 otherwise.
- The human rendering of `Unavailable` is `unavailable: this index is structural; <relation> needs a semantic index`, and of a structural answer's header line adds `structural index`.

**Rationale:** A new outcome keeps "unavailable" out of every existing outcome's meaning, so a consumer that switches on `outcome` cannot read it as empty.
Exit code 7 is the first unused code, and a script that tests `$?` treats the answer as not successful.
The MCP server needs the answer, so the exit code alone cannot carry it; printing the answer and exiting 7 serves both callers.

**Alternatives considered:**

- An error with no answer: loses the unresolved-call group, which is most of what a structural `references` answer has.
- Exit 0 with the outcome only: the user chose a distinct code so that a shell pipeline cannot mistake the answer for success.

### Decision: MCP returns exit 7 as a result, and the build tool gains two values

**Chosen:** `errors.result` sends exit code 7 to the same path as exit code 0, so the tool returns the parsed answer.
`Language` in `params.py` gains `BASH`, and the `build` tool gains a `structural` parameter that adds `--structural`.
The server instructions name Bash as indexed structurally.

**Rationale:** The mirrored command writes a complete answer for exit 7, and the MCP surface passes answers through unmodified.
The conformance test already checks each enum value against the manifest, so it covers `bash` and `structural`.

### Decision: Version bumps

**Chosen:**

- `SCHEMA_VERSION` gains one from its value when this change is applied, because `source_files` gains three columns and `index_metadata` gains `attempted_analyzer`.
- `SURFACE_VERSION` gains one from its value when this change is applied, because `build` gains `--structural` and `--language` gains `bash`.
  The manifest snapshot is regenerated, and the MCP server's `SURFACE_VERSION` mirror changes with it.
  A new MCP release is cut only on an explicit instruction.
- `RANK_VERSION` and the corpus definition version do not change; a structural index has no edges to rank, and the semantic corpus is unchanged.

## Architecture

```text
            run_build
                │ detect language (manifests, Bash discovery, --language)
                │ resolve tier (flag, Bash, recorded tier, indexer result, confirm)
        ┌───────┴────────────────┐
   semantic                   structural
   engine.analyze             (no indexer)
   units, identities          structural identities
   join                       declarations from all_declarations
   occurrences, edges         contains edges from enclosing_declarations
        └───────┬────────────────┘
                │ symbol rows + prepared documents
        shared second half: unresolved calls, span containment, corpus,
        chunks, embeddings, clone keys, fragments, source_files (language, tier),
        metadata (grammar or indexer provenance)

   query: QueryEngine reads tier with provenance
          structural + reference-derived relation  -> Unavailable (+ unresolved-call group)
          structural + impact                      -> seeds, dependents_outcome unavailable
          answer.tier on every answer; main exits 7 for an unavailable answer
   MCP:   exit 7 -> result, not error
```

## Risks

- **The ingest split changes semantic output.**
  The split moves code without changing it, but the move touches the longest function in the build.
  Mitigation: the five-repository byte-identical comparison of semantic stores, before and after the split, used by earlier refactors of the build.
- **A Bash line that starts with a backslash is misread.** tree-sitter-bash joins it onto the previous command, so the previous command gains wrong arguments and the backslash command is not recorded as a call.
  This affects calls only; function definitions were unaffected in every measured file.
  Mitigation: none in this change; the limitation is recorded here and in the probe report, and an upstream issue can be filed.
- **A structural Rust or Python store with a failing indexer reruns the indexer on every hook build.**
  Each build tries the indexer first, so a slow failure, such as rust-analyzer failing on a broken workspace after loading it, costs its full time on every commit.
  Mitigation: none in this change; the build answer states the failure each time, and the user can fix the workspace or rebuild with `--structural`.
- **Rust structural names collide more than semantic ones.**
  An inherent impl and a trait impl of one type both give the segment of the type, so two `fmt` or `new` methods get `#0` and `#1` in file order, and the numbers change when the impls are reordered.
  Mitigation: the disambiguator rule keeps the names unique; the instability is a cost of naming by file.
- **A grammar upgrade marks every structural index stale.**
  This is the intended behavior of the existing staleness rule, and a rebuild clears it.
- **An `impact` answer's seed list is not bounded.**
  `apply_impact_pagination` pages only the dependents' detailed rows, on a semantic index as well, so a diff that touches thousands of declarations returns every seed.
  On a structural index the seeds are the whole answer, so the gap is more visible there.
  Mitigation: none in this change; bounding seeds changes the `impact` answer for both tiers and belongs in its own change.
- **The grammar loses some valid Bash.**
  A function whose name contains `#` parses as a syntax error, and constructs near an error region can be lost.
  Mitigation: the per-file syntax-error flag and the disclosure on negative answers.
