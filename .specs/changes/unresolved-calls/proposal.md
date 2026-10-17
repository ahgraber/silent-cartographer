# Proposal: unresolved-calls

## Intent

`trace --relation references` reports `empty: the relation holds no instances (a definite empty set)` when the semantic index records no reference to the subject.
The semantic index records only the calls the analyzer resolved.
A call whose target the analyzer could not determine has no recorded target, so the answer claims an absence that the index cannot establish.

In a dogfood session on 2026-10-02, `references` on `MetadataStore.touch_blob` in the ai-vfs repository reported a definite empty set, and text search found 35 calls.
The calls went through untyped pytest fixture parameters, a receiver typed `Any`, and an attribute assigned from a function with no return annotation.
An agent that audits for dead code with `references` alone would remove a live method.

Three measurements show where calls go unresolved.
In each, a call is unresolved when the semantic index records no occurrence at the call's name.

- On 2026-10-03, scip-python 0.6.6 indexed four Python repositories.
  Every unresolved call was an attribute call (`x.name(...)`).
  The share of attribute calls without an occurrence was 34.6% in Flask and in ai-vfs, 26.4% in Faker, and 2.4% in httpx2.
  Most of them were in test code, and most went through a function parameter.
- On 2026-10-10, ty-scip and scip-python indexed Flask.
  The ty-scip indexer also records no occurrence at the name of a call it cannot resolve: of the 1,004 attribute calls scip-python left unresolved, ty-scip left 1,000 unresolved.
  Unlike scip-python, ty-scip also left 88 of 976 plain calls (`name(...)`) unresolved, 74 of them calls into third-party code, because ty-scip records no third-party references.
- On 2026-10-16, rust-analyzer 1.95.0 indexed ripgrep, fd, and c10r on macOS.
  Outside macro bodies, 1.1% of ripgrep's calls, 3.4% of fd's, and none of c10r's were unresolved.
  The unresolved calls that were inspected were all in code gated by `#[cfg(windows)]`, which rust-analyzer does not analyze on macOS.
  A function called only from such code has no resolved reference on that host.

## User Stories

### Story: references-name-what-they-cannot-see

As an agent or developer auditing a method, I want the `references` answer to list the calls that use the method's name but that the analyzer could not resolve, and the uses that c10r could not confirm, so that I do not treat an absence of resolved references as proof that nothing calls the method.

> Ladders to north-star outcome 5 (**Calibrated trust**): an answer separates what the index established from what it could not determine.
> Ladders to north-star outcome 2 (**Blast radius before change**): a removal decision needs the calls the analyzer missed.

### Story: empty-answers-claim-no-proof

As an agent auditing a symbol for removal, I want an empty `references`, `dependents`, `importers`, or `impact` answer to state what c10r cannot see, so that I do not delete a function that a framework or the language calls without naming it.

> Ladders to north-star outcome 5 (**Calibrated trust**) and to the principle **Calibration over coverage**.
> On 2026-10-17, `references` of the Flask view function `blog.index`, registered with `@bp.route("/")`, and of `flask.config.Config.__init__`, which runs whenever an app is created, each reported `a definite empty set`.
> No call in Flask names either one, so no list of calls by name can find their callers.

## Scope

**In scope:**

- semantic-engine: each backend also reports the positions of occurrences of symbols local to a document, such as parameters, without giving them identities.
  The build needs them to tell a call through a parameter, which the analyzer resolved, from an unresolved call.
- code-graph: the build records each call whose name has no semantic occurrence, local occurrences included, with its name, location, receiver text, and enclosing declaration or module.
  This covers every call form that has a name: attribute and plain calls in Python, and method, path, and plain calls in Rust.
  Receiver text longer than a fixed bound is shortened, and the shortening is marked.
- code-graph: the build records, for each reference occurrence the join refused, the persisted symbols it may denote: the duplicates it could not choose between, or the one symbol whose position did not match the source text.
- code-navigation: the `references` answer carries the unresolved calls as a separate, labeled group of calls that share the subject's name and have no resolved target.
  Each call carries its receiver text, so a caller can tell `client.get` from `cache.get` without a new grouping.
- code-navigation: the `references` answer carries, as a second labeled group, the refused uses that may denote the subject, each with the reason it was refused: the other definitions it could mean, or the source text found at its position.
- code-navigation: each group lists at most the result limit's number of entries and carries its total, as an ambiguous-reference answer does with its candidates.
  A group is not a paged result set and has no continuation token; a larger `--limit`, or `--limit 0`, lists more.
  Every page of a paged answer carries the same groups, and their entries are not counted in the paging disclosure.
- code-navigation: no empty answer of `references`, `dependents`, or `importers`, and no `impact` answer whose seeds have no resolved dependent, claims to be a definite empty set.
  An `impact` answer whose change touches no indexed symbol keeps its definite wording, because the syntax tree establishes it.
  Its human rendering states that no resolved instance was recorded, gives the counts of the two groups where the answer has them, and states that calls made by frameworks, implicit calls such as constructors and operators, and lookups by string are not seen.
  The documented meaning of the JSON `empty` outcome for these relations, and the MCP tool descriptions, say the same.
  The structural relations (`containers`, `contains`, `implementers`) keep the definite wording.
- The answer shape is designed so that `retained-utility` can use it for an index built without a semantic analyzer, where every call is unresolved.

**Out of scope:**

- Counting unresolved calls or refused uses as references, or adding them to `dependents`, `impact`, or `tests`.
- Grouping or filtering the unresolved calls by receiver or by file.
  A later change, `unresolved-call-filter`, adds a filter if dogfood use shows that a flat list is too noisy, or too large for an MCP tool result, for common names.
- A flag that omits the group, and a flag or query that lists only unresolved calls with paging.
  Each new flag changes the command surface and the MCP tool schemas.
  A listing of calls by name, with no resolved subject, belongs to `retained-utility`, where every call is unresolved.
- Calls inside Rust macro bodies.
  The syntax tree does not parse a macro body into calls, so no call inside one can be detected.
- Calls through a value that does not carry the function's name, such as `callback()` where `callback` is a parameter bound to the function.
  Such a call names a local symbol, so no list based on names can find it.
- Calls that the analyzer attributed to one possible class out of several.
  Such a call has a resolved target, so this change cannot detect it.
- Suggestions for fixing unresolved calls, and counts of unresolved calls in `build`, `status`, or `doctor`.
  `unresolved-call-hints` owns those.

## Approach

The build already holds each document's syntax tree and its semantic occurrences.
One pass over the calls of each tree compares each call's name with the occurrences at the same range and records the calls with none.
The detection rule depends only on the absence of an occurrence, so it applies unchanged to scip-python, ty-scip, and rust-analyzer.

A query for the references of a symbol reads the recorded calls whose name equals the last component of the symbol's name, and the refused uses whose recorded symbols include the subject.

The store schema version changes, because the build records new data.
The command-surface version does not change, because no command or flag changes.
