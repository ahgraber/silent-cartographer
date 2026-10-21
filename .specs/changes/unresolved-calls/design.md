# Design: unresolved-calls

## Context

A `references` answer lists only the occurrences the semantic index recorded.
A call the indexer could not resolve has no occurrence, so it is missing from the answer without notice.
Three measurements, summarized in the proposal, show that scip-python, ty-scip, and rust-analyzer all record no occurrence at the name of a call they cannot resolve.
The detection rule therefore needs no indexer-specific logic: a call is unresolved when no semantic occurrence covers its called name.

Constraints from the baseline and from earlier decisions:

- The build parses each document once ("Deriving the graph costs one pass over each document").
  The detection pass walks the syntax tree that the build already holds.
- A build replaces all derived state ("Builds wholly supersede prior derived state").
  The new rows are derived state.
- The MCP server passes answers through unmodified and declares no output schema, so a new answer field needs no server change.
- The command-surface version tracks commands and flags only.
  This change adds neither.

## Decisions

### Decision: Carry local occurrence ranges across the extraction boundary

**Chosen:** `translate_index` in [src/semantic/scip.rs](../../../src/semantic/scip.rs) does not persist an occurrence whose symbol has no global descriptor: a local symbol, or a symbol string that does not parse.
It keeps the range of each such occurrence in a per-document list in `ExtractedIndex`, with no symbol identity and no role, and both adapters return that list.
An occurrence with an empty symbol string names nothing, so its range is not kept.
The join and every persisting step ignore the list; only the unresolved-call pass reads it.
The conformance suite checks each listed range as it checks occurrence ranges: its document declares a position encoding, and the range is well formed.
A fixture test checks, for each shipped adapter, that the range of a call through a local binding is in the list: the Python `helper` token and the Rust `callback` token.

**Rationale:** A call through a local binding is resolved by every indexer to a local symbol: in Rust a parameter or closure binding such as `callback()`, and in Python a variable assigned inside a function such as `helper()`.
Both Python indexers give a function parameter its own global symbol instead, so a Python `callback()` through a parameter is covered by that symbol's occurrence.
Without these ranges the pass would see no occurrence and record the call as unresolved, contrary to the requirement.
The 2026-10-16 Rust measurement counted 257 such calls in c10r alone.
The list also keeps the ranges of symbol strings that do not parse, because the requirement counts an occurrence at a called name as resolving the call, whatever symbol it names.
The suite does not require a backend to report any local range, because a project with no local bindings has none; the Rust conformance fixture is one such project.
The field keeps the name `local_occurrence_ranges`, because renaming it would touch every place an index is built, for a naming gain only.

**Alternatives considered:**

- Detect local bindings from the syntax tree: scoping rules for comprehensions, closures, `global`, and pattern bindings would make a second, weaker name resolver.

### Decision: Detect unresolved calls by exact span equality against every semantic occurrence

**Chosen:** For each document, the pass collects the byte span of every semantic occurrence the index holds for that document, whether the join aligned or refused it, and of every local occurrence range.
A call is unresolved when the byte span of its called name equals none of those spans.
Occurrence ranges are converted to byte spans with the same normalization the join uses (`range_to_span` with the document's position encoding).
An occurrence whose range cannot be normalized covers no call.
An occurrence with an empty symbol string covers no call: the SCIP format allows an occurrence with no symbol, such as one that carries only highlighting information, and it says nothing about what the name refers to.

**Rationale:** The spec says a call is recorded when its called name has no semantic occurrence, whatever symbol the occurrence names.
An occurrence the join refused, for example because two definitions share its descriptor, is still evidence that the indexer resolved the call.
Counting it as resolved keeps the group to calls the indexer did not resolve.
Exact equality is the rule the three measurements used.
On Flask it agreed with ty-scip for 1,000 of 1,004 attribute calls.

**Alternatives considered:**

- Compare against aligned occurrences only: a call the indexer resolved but the join refused would appear as unresolved, which is false.
- Accept an occurrence that overlaps the name: no measured indexer emits a partial range at a call's name, and overlap admits occurrences of the receiver in edge cases such as `a.a()`.

### Decision: Calls in a source the analyzer did not index are recorded

**Chosen:** The pass walks every source the build parsed, whether or not the semantic index has a document for it.
A source the analyzer left out of its index has no occurrences, so every call in it is recorded.
`docs/commands/trace.md` states this.

**Rationale:** The analyzer resolved none of those calls, so each is a call with no resolved target, like a call in code under a `#[cfg(...)]` condition that is false for the build host.
On 2026-10-19, rust-analyzer left ripgrep's `crates/globset/src/serde_impl.rs` out of its index, and the build recorded its 28 calls.
A function called only from such a source would otherwise look unused.
The baseline uses "indexed source" for a source the build discovered, which matches this reading of "indexed document".
Decided by the user on 2026-10-19.

**Alternatives considered:**

- Skip sources absent from the semantic index: the lists get shorter, but a function called only from such a source looks unused again.

### Decision: Identify calls and called names from the syntax tree

**Chosen:** A new `SyntaxTree` method walks the tree and yields, for each call, the called name's byte span, the name text, and the receiver's byte span when there is one.
The callee is the `function` field of a Python `call` node or a Rust `call_expression` node.

| Language | Callee node         | Called name                                          | Receiver           |
| -------- | ------------------- | ---------------------------------------------------- | ------------------ |
| Python   | `identifier`        | the identifier                                       | none               |
| Python   | `attribute`         | the `attribute` field                                | the `object` field |
| Rust     | `identifier`        | the identifier                                       | none               |
| Rust     | `field_expression`  | the `field` field                                    | the `value` field  |
| Rust     | `scoped_identifier` | the `name` field                                     | the `path` field   |
| Rust     | `generic_function`  | resolved from its `function` field by the rows above | as for that field  |

Any other callee yields no call.
Python decorators are `call` nodes too, so `@app.route("/")` is covered.
A Rust macro invocation's arguments are a token tree, not parsed expressions, so they contain no `call_expression` node and the walk finds no call there.

**Rationale:** Per-language syntax knowledge lives in `src/graph/syntax.rs`, dispatched on `Language`, next to similar walkers such as `class_bases` and `alias_bindings`.
The walk reuses the tree the build already parsed.

**Alternatives considered:**

- Detect calls inside Rust macro arguments with a token-level heuristic: the Rust measurement used one and found it misfires, for example on `cfg(windows)` attributes inside `vec![]`.
  A false call in the group would be a confident wrong row.

### Decision: Store receiver text up to a bound

**Chosen:** The row stores the receiver's source text as written, including line breaks, up to 200 characters.
A longer receiver is stored as its first 200 characters, cut at a character boundary, with a flag that marks it as shortened.
The JSON answer carries the stored text and the flag.
The human rendering collapses each run of whitespace to one space, passes the result through the existing `sanitize` function, and appends `…` to a shortened receiver.
`docs/commands/trace.md` documents the bound.

**Rationale:** A receiver is usually a short name, but it can be a call whose arguments span thousands of lines, such as `factory([...]).fetch()`.
Without a bound, one such call would put that whole argument list into a `references` answer at any `--limit`, past the content bound every other part of an answer respects.
200 characters keeps every receiver form the measurements sampled, such as `self.stream` or `app.config`, whole.

**Alternatives considered:**

- No bound: rejected for the reason above.
- Store the receiver's span and let a caller read the text: the caller would need a second query for the most useful part of the row.

### Decision: A new store table, cleared with the rest of the derived state

**Chosen:** A table `unresolved_calls` with the document path, the called name, the called name's byte span and line and column, the receiver text (nullable) and its shortened flag, and the enclosing identity.
The enclosing identity is the declaration that `enclosing_symbol` returns, or, when it returns none, the module symbol of the document, read from the build's document-to-module map; it is null only for a document with no persisted module symbol.
An index on the called name serves the query.
`clear_derived` deletes its rows before it deletes symbols.
The pass runs inside `ingest_with_params`, in the build's transaction, after the join, because enclosing attribution needs the maps of persisted definitions that function builds.
Enclosing attribution calls `enclosing_declarations` and `enclosing_symbol`, the same functions that attribute reference occurrences.
Line and column come from the same `PositionIndex` the other recorded locations use.
`SCHEMA_VERSION` changes from 19 to 20, so a store written by an earlier build is refused or replaced, never read without the table.

**Rationale:** The rows derive only from sources and the semantic index, which the build's currency check already covers, so "A build over unchanged inputs does no work" needs no change.
Reusing the attribution and position functions keeps unresolved calls consistent with reference occurrences.

### Decision: Refused reference occurrences record the symbols they may denote

**Chosen:** A table `discrepancy_symbols` links a row of `join_discrepancies` to each persisted symbol the refused occurrence may denote, and `clear_derived` deletes its rows before the discrepancies and the symbols.
Rows are written only for refused occurrences whose role is a reference, not a definition.
For a duplicate-ambiguous occurrence, the linked symbols are the members of the duplicated-descriptor group of the occurrence's descriptor, which the build already records for "Duplicated descriptors are disclosed".
For any other refused occurrence, the linked symbol is the occurrence's own symbol when that symbol is persisted, and there is no row otherwise.
An index on the symbol column serves the query.

**Rationale:** The join already holds each refused occurrence's symbol and role in `UnalignedOccurrence`, and the build already knows each duplicate group, so the rows cost one insert per link.
A separate link table keeps `join_discrepancies` unchanged for its existing readers.

### Decision: Duplicate groups are keyed by the full descriptor

**Chosen:** A duplicate group is the set of definitions that share one full descriptor, segment kinds included.
The join records that descriptor on each duplicate-ambiguous refusal, and the links of the previous decision go to the persisted definitions of exactly that descriptor.
The build also stores, for each duplicated symbol, a kind-aware serialization of its descriptor in `symbols.duplicate_descriptor`, and the duplicated-descriptor disclosure of `status` groups by it.
The printed `descriptor_base` is unchanged, so two groups whose descriptors differ only in a segment kind print the same base and list their definitions separately.
Each listed definition carries its symbol kind, in the JSON and the human rendering, so such groups can be told apart.
When the descriptors differ only in the kind of an inner segment, the definitions' symbol kinds can be equal, and the groups still print alike; showing every segment's kind would need a readable descriptor notation, which this change does not add.

**Rationale:** The unit-free key that grouped duplicates before is built from segment names only.
Two descriptors such as a term `f` and a method `f`, each defined twice, shared one key, so a refused use of one was disclosed on the other's definitions, and `status` reported one group of four instead of two groups of two.
An external review found the first case on 2026-10-20; the user decided on 2026-10-20 to fix both in this change.
The new column is part of schema version 20, which this change introduces.

### Decision: The unconfirmed-use group is a second answer field

**Chosen:** `Answer` gains a field `unconfirmed_uses`, present exactly when `unresolved_calls` is present:

```json
"unconfirmed_uses": {
  "total": 3,
  "truncated": false,
  "uses": [
    {
      "location": { "document_path": "src/flask/cli.py", "start_line": 353, "start_column": 58, "…": "…" },
      "reason": "duplicate_ambiguous",
      "candidates": ["…::locate_app::raise_if_not_found#0", "…#1", "…#2"]
    },
    {
      "location": { "document_path": "src/flask/json/tag.py", "start_line": 71, "start_column": 20, "…": "…" },
      "reason": "text_mismatch",
      "found_text": "serializer"
    }
  ]
}
```

`reason` is the discrepancy's outcome kind.
`candidates` appears for `duplicate_ambiguous`, with `candidates_total`; it lists at most 10 identities, in canonical-identity order, and `candidates_shortened` is true when it lists fewer than the total.
`found_text` appears for every other kind, cut at 200 characters like a receiver, with `found_text_shortened`; it is omitted when no source text was found at the position.
`location` is omitted when the discrepancy recorded no span, for example an occurrence whose range lies beyond its document; `document_path` is always present.
Uses are ordered by document path, then by start offset, with uses that have no location last within their document, ordered by refusal kind and expected name.
The human rendering prints `at <path> (position unknown)` for a use with no location.
The shared pagination step caps `uses` at the effective limit exactly as it caps `unresolved_calls.calls`.

**Rationale:** The same shape as the unresolved-call group lets a caller read both groups the same way.
The reason tells the reader what to check: which definition a call means, or whether the indexer placed a use at the wrong position.
The build stores found text already cut, with a flag that records the cut, so the discrepancy listing and this group show the same text.
One bound for all quoted source text replaces the earlier 120-byte bound on stored found text; on 2026-10-19 none of 2,506 found texts in the Flask, ai-vfs, and ripgrep dogfood stores was longer than 28 characters, so no listed text changes.

### Decision: The group is a new answer field beside the outcome

**Chosen:** `Answer` gains a field `unresolved_calls`, serialized only when present.
It is present on every `references` answer whose subject resolves to one symbol, and absent on every other answer.
Its shape:

```json
"unresolved_calls": {
  "name": "test_client",
  "total": 6,
  "truncated": true,
  "calls": [
    {
      "receiver": "app",
      "location": { "document_path": "tests/conftest.py", "span_start": 0, "span_end": 0, "start_line": 68, "start_column": 16, "end_line": 68, "end_column": 27 },
      "enclosing": "flask::flask::tests::conftest::client"
    }
  ]
}
```

`receiver` and `receiver_shortened` are omitted for a call with no receiver, and `enclosing` is omitted only when the document has no persisted module symbol.
`location` has the same shape as a reference row's location.
The subject's name for matching is the last component of the subject's display name, after the final `::` or `.`.
Calls are ordered by document path, then by start offset, as reference occurrences are.

**Rationale:** A field beside the outcome leaves `outcome.results` and `page` unchanged, so existing callers read the same results and counts.
The `truncated` flag states the shortfall directly, so a caller need not compare the list length with the total.

**Alternatives considered:**

- A new `TraceItem` variant inside `results`: the calls would then be paged and counted as results, which the spec forbids.
- Omit the field when the total is zero: a missing field cannot be told apart from an answer by a binary that does not report the group.

### Decision: Cap the group in the shared pagination step

**Chosen:** `apply_pagination` caps the group's calls at the effective limit, as it caps an ambiguous answer's candidates, and sets `truncated` when it drops calls.
The cap applies on found and empty outcomes alike, because an empty answer still carries the groups.
The cap ignores the page index, so every page carries the same group.
The group carries no continuation token; the spec states that it is not a paged result set, so the baseline rule that a truncated result set carries a token does not apply to it.
An unbounded request (`--limit 0`) leaves the group whole.
The continuation token's parameter hash does not change, because the group does not affect which results a page holds.
Every other `apply_*_pagination` function that rebuilds `Answer` field by field passes the group through unchanged.

**Rationale:** The shared pagination step already owns the effective limit, and the ambiguous-candidate cap is the precedent the user chose.

### Decision: Empty answers of reference-derived relations name what c10r cannot see

**Chosen:** The renderer distinguishes the relation an empty answer came from.
For `references`, `dependents`, and `importers`, the `Outcome::Empty` arm prints a scoped line instead of the definite-empty message.
For `references` the line carries both group totals:

```text
empty: no resolved reference recorded; 6 calls named test_client have no resolved target and 0 uses are unconfirmed (calls by frameworks, implicit calls such as constructors and operators, and lookups by string are not seen)
```

For `dependents` and `importers` the line reads `empty: no resolved dependent recorded (…)` and `empty: no resolved importer recorded (…)`, with the same parenthesis.
`impact` always answers with a found report, even with no dependents ([src/query/impact.rs](../../../src/query/impact.rs)), so its rendering is separate from `Outcome::Empty`.
In `render_found`, a `Seeded` report with no dependent rows prints `no resolved dependent recorded (…)` with the same parenthesis after the seed list.
A `NoIndexedSymbolTouched` report keeps its definite line, because the diff touching no indexed declaration is a fact the syntax tree establishes; `NoneResolvable` keeps its existing unresolved line.
The `containers`, `contains`, and `implementers` relations keep the definite-empty message.
`to_human` receives only the answer, and `Answer` carries no relation today, so `Answer` gains an internal field naming the empty-answer scope (resolved references, dependents, or importers), marked `#[serde(skip)]` so the JSON answer does not change.
`QueryEngine::trace` sets it for `references` and `importers`.
`dependents` does not pass through `QueryEngine::trace`: `run_trace` dispatches it to `QueryEngine::dependents`, so that method sets the scope on its answer, its empty return included, and `apply_dependents_pagination` keeps it.
The doc comment of `Outcome::Empty`, the `trace` command's help text, and the MCP `trace` and `impact` tool descriptions state that an empty answer of these relations means that no resolved instance was recorded.

After the results, or after the empty line, the renderer prints each non-empty group:

```text
unresolved: 3 of 6 calls named test_client have no resolved target (each may or may not call this symbol)
  app.test_client  at tests/conftest.py:68:16 in flask::flask::tests::conftest::client
  list all 6 with --limit 0
unconfirmed: 3 uses may refer to this symbol but are not counted above
  at src/flask/cli.py:353:58  could mean any of 3 definitions of raise_if_not_found (#0, #1, #2)
  at src/flask/json/tag.py:71:20  the indexer placed a use of this symbol here, but the source text is `serializer`
```

A group with a total of zero prints nothing beyond its count in the empty line.
The `list all` line appears only when a group is truncated.

**Rationale:** On 2026-10-17, `references` of Flask's route-registered `blog.index` and of `Config.__init__` each claimed a definite empty set, although Flask calls both.
No list of calls by name can find such callers, so no empty answer of a relation built from references can prove absence.
The machine answer keeps the `empty` outcome, so agents that branch on it are unaffected; the change is in what the answer and the tool descriptions say that outcome means.
Structural relations come from declarations the syntax tree proves, so their empty answers stay definite.

**Alternatives considered:**

- Keep the definite message when both group totals are zero: it would still be false for the Flask cases above.

### Decision: An empty tests answer follows the same rule

**Chosen:** "Empty answers of reference-derived relations claim no proof" covers `tests`.
An empty `tests` answer keeps its convention marker and its line `empty: no convention-classified test reference found (not proof that nothing tests the subject; …)`, and the parenthesis gains the same list of unseen cases as the other relations.
It carries neither group, because unresolved calls and refused uses stay out of `tests`.

**Rationale:** `tests` is built from the same resolved references, so it has the same blind spots.
On ai-vfs, the 33 calls to `MetadataStore.touch_blob` go through untyped pytest fixtures and none resolves, so its `tests` answer is empty for the reason the other relations' empty lines name.
One rule for every reference-derived relation also removes a conflict: "Relationship trace" sent every reference-derived empty answer to this requirement, which named only three of the four relations.

**Alternatives considered:**

- Let the baseline requirement "Heuristic-grade answers are structurally labeled" govern empty `tests` answers alone: the conflict goes away, but the empty line says "not proof" without saying what is not seen.

## Architecture

```text
build
  ingest_with_params
    PreparedCorpus (one parse per document)
    join ──────────────► aligned + refused occurrences
    unresolved-call pass
      SyntaxTree::calls()           calls with name span and receiver span
      occurrence spans per document (aligned and refused)
      keep calls whose name span matches no occurrence span
      enclosing_declarations + enclosing_symbol, else the document's module
      PositionIndex line and column; receiver text cut at 200 characters
    insert_unresolved_calls ───► table unresolved_calls
    refused reference occurrences ──► discrepancy_symbols (duplicates, or the named symbol)

trace --relation references
  QueryEngine::trace
    references_of(subject)               ──► results (unchanged)
    unresolved_calls_named(last name)    ──► Answer.unresolved_calls
    unconfirmed_uses_of(subject)         ──► Answer.unconfirmed_uses
    empty-answer scope (not serialized)  ──► references | dependents | importers
  apply_pagination
    results paged as today; both groups capped at limit on every page
  render
    Empty arm prints the scoped line for the three relations; groups printed after results
```

## Risks

- **A common name floods the group**: for `get` or `close`, most listed calls are unrelated to the subject.
  Mitigation: each call carries its receiver text, and the total is on every page.
  Through MCP, `--limit 0` for a name with several hundred unresolved calls can exceed a client's tool-result size limit, so such a list cannot be read whole there.
  The placeholder change `unresolved-call-filter` adds a receiver filter if dogfood use shows either problem.
- **Implicit and framework calls stay invisible**: a route-registered view function, `__init__` reached through a constructor call, and `__eq__` reached through `==` have callers that no list of calls by name can find.
  Mitigation: no empty answer of a reference-derived relation claims a definite empty set, and its message names these cases.
- **Span mismatch between an indexer and the syntax tree produces false unresolved calls**: a position encoding error would make resolved calls appear unresolved.
  Mitigation: the pass uses the join's normalization, and a dogfood check compares the count on Flask under scip-python with the 1,004 attribute calls the 2026-10-03 measurement found.
- **Rust results depend on the build host**: code under a `#[cfg(...)]` condition that is false on the host has no occurrences, so its calls appear in the group, and the group differs between hosts.
  This is the intended disclosure, and `docs/commands/trace.md` states it.
- **ty-scip leaves calls through a comprehension variable unresolved**: in `[g() for g in handlers]`, ty-scip records the variable's definition but no occurrence at the call, while scip-python records a local occurrence there.
  Under ty-scip such a call is listed in the group of any symbol named `g`.
- **ty-scip adds third-party calls to the group**: ty-scip records no third-party references, so a workspace symbol that shares a name with a third-party function lists calls to that function.
  Each such call is labeled as unresolved, never counted as a reference.

## Spec Changes

- semantic-engine, scenario `Local occurrence ranges extracted`: the Python case calls a local variable as `helper()` instead of a parameter as `callback()`.
  Source: the requirement's clause on symbols local to a document.
  A 2026-10-19 probe showed that scip-python 0.6.6 and ty-scip 0.1.0 give a function parameter a global symbol, such as `run().(callback)`, and give a variable assigned inside a function a `local` symbol.
  The scenario's Python premise was false under both indexers; the revised case is one that both indexers report as local.
  Approved by the user on 2026-10-19.
- code-navigation, requirement `Empty answers of reference-derived relations claim no proof`: covers `tests`, with its rendering stating that no convention-classified test reference was found; new scenario `An empty tests answer states what it cannot see`.
  Source: user decision on 2026-10-19, after verification found that the requirement named three of the four reference-derived relations.
- code-graph, requirement `Duplicated descriptors are disclosed` (MODIFIED): a group holds only definitions whose descriptors match in every segment's name and kind, and each listed definition carries its symbol kind; new scenario `Descriptors that differ only in kind form separate groups`.
  Source: user decision on 2026-10-20, after verification found that the grouping change had no spec record and that the groups could not be told apart.
