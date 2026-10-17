# Design: method-overrides

## Context

A measurement on 2026-10-16 (`._scratch/research/override-relationships/out.md`) showed that the indexers do not supply override links that c10r could read for every store. scip-python links each override to one target only, ty-scip links no methods, and rust-analyzer records no relationships.
The build already resolves what an override rule needs: each Python class's declared bases, under both Python indexers, and each Rust impl's trait.
A check on the same fixtures confirmed that c10r's `type_hierarchy` edges for multiple bases, multi-level chains, and cross-module bases are identical under scip-python and ty-scip.

A probe on 2026-10-17 (`._scratch/research/override-relationships/mro_probe.py`, run with CPython) confirmed three facts about Python dispatch that the rule depends on:

- For `class Combined(Left, Right)` declaring `m`, a call typed as `Right` on a `Combined` instance runs `Combined.m`.
  For `C(B)` with `B(P, Q)`, where only `P`, `Q`, and `C` declare `m`, a call typed as `Q` on a `C` instance runs `C.m`.
  So a method overrides the same-named method of every base it reaches, not only the one the method resolution order picks for `super()`.
- A method `__m` in `B(A)` does not override `A.__m`: `A.call()`, which calls `self.__m()`, still runs `A.__m` on a `B` instance.
- `Mid(Root)` assigning `m = None` hides `Root.m`: `Mid().m` is `None`.

Constraints from the baseline and from the code:

- `type_hierarchy` edges are derived in `ingest_with_params` from `SyntaxTree::trait_impls` and `SyntaxTree::class_bases`, with each endpoint resolved through the aligned occurrence at its name span ([src/graph/mod.rs](../../../src/graph/mod.rs)).
- `SyntaxTree::trait_impls` reports an impl only when both its trait and its self type have a name token ([src/graph/syntax.rs](../../../src/graph/syntax.rs)), so `impl Tr for &Widget`, a slice, or a tuple is skipped today.
- A Rust `type_hierarchy` edge needs both the type and the trait to resolve.
  For a blanket impl `impl<T: Display> Tr for T`, the type `T` resolves to no persisted symbol, so no such edge exists.
- `occ_by_location`, the map the `type_hierarchy` derivation uses, holds aligned references and definitions and keeps the first identity when several sit at one location, while `definitions_at` holds definitions only and records such a location as ambiguous.
  The symbol-identity baseline gives a file compiled into several build units one `+shared` identity per declaration, so a shared file does not by itself put several identities at one location.
- The symbol-identity baseline gives a Python property's getter and setter one identity under scip-python and two under ty-scip.
- `contains` edges link a class or trait to its methods, and a Rust type to the methods of its impl blocks.
  A trait-impl method has its own identity, for example `rsfix::rsfix::impl::Both::Named::name`, distinct from the inherent `rsfix::rsfix::impl::Both::name`.
- The dependents traversal and the rank projection each name the edge kinds they read in SQL ([src/graph/store.rs](../../../src/graph/store.rs)).
- `Answer` carries no subject, and `to_human` receives only the answer, so a command naming the subject must be built before rendering, as the `refresh` command is built in [src/commands.rs](../../../src/commands.rs) with `shell_quote`.
- `unresolved-calls` lands first, by `.specs/changes/ORDER.md`.
  It adds two groups beside the outcome of a `references` answer, makes the empty line of reference-derived relations a scoped line, and modifies `Relationship trace`.
  This change composes with all three and rebases its `Relationship trace` block on the synced text.

## Decisions

### Decision: Derive override edges from resolved bases and declared methods

**Chosen:** A pass in `ingest_with_params` runs after the `type_hierarchy` edges and the `contains` edges are derived.

- It builds, from the `contains` edges of the build, a map from each class or trait to its methods, keyed by display name, holding every method of that name: a name can map to several identities, such as a property's getter and setter under ty-scip.
- Python: `SyntaxTree::class_bases` also reports, for each class, the names bound directly in its body and, for each, whether the binding is a function definition (including decorated ones) or something else (an assignment, an annotated assignment with a value, a nested class, an import).
  An annotation with no value, such as `m: object`, is not reported, because it creates no class attribute: the 2026-10-17 probe shows `AnnotatedMid().m()` still runs `Root.m`.
  For each class and each declared method name that is not private (it does not begin with two underscores, or it also ends with two underscores), the pass visits each declared base that has a `type_hierarchy` edge from the class.
  If the base binds the name to methods, edges go to each of them.
  If the base binds the name to something else, the walk along that base stops.
  If the base does not bind the name, the pass visits that base's own bases, with a visited set that stops cycles.
  The class-to-bases map is built from all `type_hierarchy` edges of the build, so bases in other modules are reached.
- Rust: `SyntaxTree::trait_impls` reports every impl of a trait, with the self type's name span made optional, and the name span of each function declared directly in the impl body.
  The existing `type_hierarchy` derivation keeps requiring the self type's span.
  For each impl, the trait resolves through its name span, each declared function resolves through its definition name span, and an edge goes to each of the trait's methods of the same name.
- A declared method resolves through `definitions_at` at its definition name span; a location that map records as ambiguous yields no edge from that method.
  The trait in a Rust impl header resolves through the aligned reference occurrences at the header's trait-name span, collected into a map that keeps every identity at a location; one identity is used, and several yield no edge from that impl's methods.
  Python bases come from the existing `type_hierarchy` edges.
- Edges are inserted through the existing unique edge index, so a method reached twice through a diamond gets one edge.

**Rationale:** Python resolves an attribute by name along an instance's class and its bases, so a method overrides every same-named method of a base it reaches, as the 2026-10-17 probe confirmed for multiple bases and diamonds.
Private names are renamed per class by Python, and a non-method binding hides the methods behind it, so both stop the match.
Rust requires every function in a trait impl to be an item of that trait.
Using only bases and traits the index already resolved keeps the rule from matching same-named methods of unrelated classes.
The rule reads no indexer-specific data, so the contract floor holds for all three indexers for every method an indexer reports with one definition.

**Alternatives considered:**

- Read SCIP `is_implementation` relationships: only scip-python records them, for one target per override, so ty-scip and Rust stores would have no edges.
- Follow Python's method resolution order to a single target: for `C(B)`, `B(P, Q)` with `m` only in `P` and `Q`, a call typed as `Q` on a `C` instance runs `C.m`, so a single target would drop a real override.
- Resolve endpoints with `occ_by_location`: it keeps the first identity when several sit at one location, so the edge would depend on occurrence order.
- Pair identities by build unit: the identity baseline gives a file compiled into several units one `+shared` identity, so there are no per-unit identities to pair.

### Decision: Dependents and the rank projection both follow `overrides`

**Chosen:** `DependencyKind` gains `Overrides`, ordered after `TypeHierarchy` in the tie-break order.
The dependents query's edge-kind list gains `'overrides'`.
The rank projection's edge-kind list gains `'overrides'` too.
In the projection, importance flows from a dependent to what it depends on, so an `overrides` edge from `B.m` to `A.m` passes `B.m`'s importance to `A.m` and none back.
`RANK_VERSION` changes from 1 to 2, so a continuation token issued under the old scores is refused instead of resuming against reordered rows, and the changelog entry for the release carries a migration note.

**Rationale:** The ranked ordering exists to put first the rows a reader should check first.
A base method whose overrides carry many callers is such a row, even when it has few direct callers.
On 2026-10-16 the user, and a separate model asked without seeing this design, both expected the base of a large hierarchy to rank above a plain function with more direct callers, and an override to rank by its own callers only.
PageRank divides a symbol's importance among everything it depends on, so an override that depends on many other symbols passes only a small share to its base; the ranking scenario therefore fixes its graph exactly, and the requirement states the direction of flow, not a guarantee about caller counts.
The migration cost is a one-time shift in the order of ranked answers, including answers with no override nearby, because the scores are global.
The answers' membership, distances, and edge kinds do not change, and the `unranked` ordering is unaffected.

**Alternatives considered:**

- Leave the projection unchanged: a base method would rank by its direct callers alone, so `Leaf.run` with 25 callers would rank above `Base.run` with 1 direct caller and 12 overrides, which both reviewers called the wrong order.
- Weight `overrides` edges differently from other edges: no evidence supports a particular weight, and uniform edge weights are part of the model `RANK_VERSION` 1 recorded.

### Decision: `implementers` and `implements` read both hierarchy edge kinds

**Chosen:** `implementers` returns the sources of `type_hierarchy` and `overrides` edges that lead to the subject, and `implements` returns their targets, in canonical-identity order.
A type subject has only `type_hierarchy` edges and a method subject only `overrides` edges, so the union never mixes kinds for one subject.
`RelationArg`, `Relation`, and `TraceRelation` gain `Implements`.

**Rationale:** One query per relation keeps the kind of the subject out of the dispatch.

### Decision: The override disclosure is a new answer field with counts and commands

**Chosen:** `Answer` gains a field `override_relations`, serialized only when present, on every `references` answer whose subject is a symbol of kind `method`:

```json
"override_relations": {
  "overridden_by": {
    "methods": 2,
    "references": 14,
    "command": "c10r trace --db='/abs/.c10r/dev.db' --workspace='flask' 'flask::flask::app::Base::m' --relation implementers"
  },
  "overrides": { "methods": 0, "references": 0 }
}
```

`overridden_by` counts the methods reached from the subject by following `overrides` edges backward, transitively.
`overrides` counts the methods reached by following them forward, transitively.
`references` is the sum of the resolved reference occurrences of the counted methods, read with a new count query.
Both walks use a visited set.
`command` is present exactly when `methods` is not zero.
`QueryEngine::trace` fills the counts; the `trace` command handler in [src/commands.rs](../../../src/commands.rs) fills `command` from the store path, the workspace identity, and the subject's canonical identity, each quoted with `shell_quote`, the way it builds `refresh`.
The shared pagination step passes the field through unchanged on every page.

**Rationale:** Counts keep the disclosure a fixed size whatever the number of subclasses, and the `trace` relations list the methods.
The command must name the same store, because a bare command opens the default store, and must be quoted, because canonical identities can contain characters a shell treats specially.
A field beside the outcome leaves the results and the paging counts unchanged, as the `unresolved-calls` groups do.

### Decision: The human rendering adds one line per non-zero part

**Chosen:** After the unconfirmed-use group, the renderer prints each non-zero part:

```text
overridden by 2 methods, counting overrides of overrides, with 14 resolved references; list the direct overrides with: <command>
overrides 1 method, counting what it overrides in turn, with 40 resolved references; list the direct ones with: <command>
```

The scoped empty line that `unresolved-calls` defines for `references` gains a clause per non-zero part, for example `; it overrides 1 method with 40 resolved references (see below)`.

**Rationale:** The transitive count and the direct listing differ in a chain of three or more, so the line says which is which.

### Decision: Version bumps

**Chosen:** `SCHEMA_VERSION` rises by one from the base branch's value, because a store from an earlier build has no `overrides` edges and would answer `dependents` without them.
`SURFACE_VERSION` rises by one, because the relation value `implements` changes the command surface.
The surface snapshot fixture is regenerated.
The MCP server adds `implements` to its relation enumeration and its tool descriptions, and its pinned surface version follows `SURFACE_VERSION`.
Publishing the MCP server waits for an explicit release instruction.

## Architecture

```text
build
  ingest_with_params
    contains edges ──► methods by class or trait (display name → every id)
    type_hierarchy edges ──► bases by class
    definitions_at ──► a method's identity (ambiguous location: no edge)
    aligned references by location ──► every identity at a trait-name token (several: no edge)
    override pass
      Rust: trait_impls() (self type optional) with body function spans → same-named trait methods
      Python: class_bases() with body bindings → walk bases; stop at a valued non-method binding; skip private names
    insert_edge(Overrides, method, overridden)

trace
  implementers  ← edge_sources(TypeHierarchy ∪ Overrides)
  implements    ← edge_targets(TypeHierarchy ∪ Overrides)
  dependents    ← traversal over uses, imports, type_hierarchy, overrides
                  ranked order ← PageRank over the same four kinds (RANK_VERSION 2)
  references    ← results as before
                  + Answer.override_relations (transitive walks + reference counts)
  command handler fills override_relations.*.command (store, workspace, subject, quoted)
render
  override lines after the unconfirmed-use group; a clause per part in the scoped empty line
```

## Risks

- **Display-name matching depends on identity rendering**: if a method's display name ever carried decoration, such as a Rust generic suffix, two same-named methods would not match.
  Mitigation: the scenarios test plain, decorated, generic-impl, blanket-impl, and reference-self-type methods in both languages.
- **External bases**: an override of `threading.Thread.run` or `Display::fmt` gets no edge, because the base method is not a persisted symbol.
  The proposal scopes this out; `docs/commands/trace.md` states it.
- **Macro-generated methods**: a trait method that a macro supplies inside an impl, or an impl a macro generates, gets no edge, because the syntax tree does not contain the generated function.
  The requirement scopes this out explicitly; `docs/commands/trace.md` states it.
- **Existing first-wins resolution**: the `type_hierarchy` derivation still resolves endpoints with `occ_by_location`, so where several aligned identities sit at one base-name token, a Python class gets an edge to the first one only, and its override edges follow that edge.
  This change does not alter that derivation; the gap is recorded here for a later fix.
- **Large hierarchies**: the transitive walks for the disclosure run per `references` query.
  The walk is bounded by the number of related methods, and each reference count is one indexed query.
