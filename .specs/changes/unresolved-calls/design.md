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

**Chosen:** `translate_index` in [src/semantic/scip.rs](../../../src/semantic/scip.rs) drops every occurrence of a local symbol today.
It keeps them as a per-document list of source ranges in `ExtractedIndex`, with no symbol identity and no role, and both adapters return that list.
The join and every persisting step ignore the list; only the unresolved-call pass reads it.
The conformance suite checks, for each adapter, that a call through a parameter has its range in the list.

**Rationale:** A call through a parameter or a local closure, such as `callback()`, is resolved by every indexer to a local symbol.
Without these ranges the pass would see no occurrence and record the call as unresolved, contrary to the requirement.
The 2026-10-16 Rust measurement counted 257 such calls in c10r alone.

**Alternatives considered:**

- Detect local bindings from the syntax tree: scoping rules for comprehensions, closures, `global`, and pattern bindings would make a second, weaker name resolver.

### Decision: Detect unresolved calls by exact span equality against every semantic occurrence

**Chosen:** For each document, the pass collects the byte span of every semantic occurrence the index holds for that document, whether the join aligned or refused it, and of every local occurrence range.
A call is unresolved when the byte span of its called name equals none of those spans.
Occurrence ranges are converted to byte spans with the same normalization the join uses (`range_to_span` with the document's position encoding).
An occurrence whose range cannot be normalized covers no call.

**Rationale:** The spec says a call is recorded when its called name has no semantic occurrence, whatever symbol the occurrence names.
An occurrence the join refused, for example because two definitions share its descriptor, is still evidence that the indexer resolved the call.
Counting it as resolved keeps the group to calls the indexer did not resolve.
Exact equality is the rule the three measurements used.
On Flask it agreed with ty-scip for 1,000 of 1,004 attribute calls.

**Alternatives considered:**

- Compare against aligned occurrences only: a call the indexer resolved but the join refused would appear as unresolved, which is false.
- Accept an occurrence that overlaps the name: no measured indexer emits a partial range at a call's name, and overlap admits occurrences of the receiver in edge cases such as `a.a()`.

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
- **ty-scip adds third-party calls to the group**: ty-scip records no third-party references, so a workspace symbol that shares a name with a third-party function lists calls to that function.
  Each such call is labeled as unresolved, never counted as a reference.
