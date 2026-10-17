# Tasks: method-overrides

## Setup

- [ ] Create the worktree `.worktrees/method-overrides` on branch `method-overrides` from `main`, after `unresolved-calls` has landed on `main`.

## Fixtures

- [ ] Add `tests/fixtures/method-overrides/python/` with a `pyproject.toml` and a package holding one class group per Python scenario of `Override edges (overrides)`: a plain override; a three-level chain where every level declares `m`; a chain whose middle class does not declare `m`; a class with two bases that both declare `m`; a base imported from another module; property, classmethod, and staticmethod overrides; a method shadowing a base class attribute; a three-level chain whose middle class assigns `m = None`; a base and subclass that each declare a private `__m` and an `__eq__`; a base and subclass that each declare a property with a getter and a setter; a diamond where `C(B)` declares `m`, `B(P, Q)` does not, and `P` and `Q` do; a class with no base sharing a method name with an unrelated class; a subclass of a standard-library class overriding one of its methods; and callers of the methods, so that reference counts are not zero.
- [ ] Generate the fixture's scip-python index (`index.scip`) and ty-scip index (`ty-scip.scip`), and commit them with `SCIP-PYTHON-VERSION` and `TY-SCIP-VERSION` files in the format of `tests/fixtures/python-conformance/`.
- [ ] Add `tests/fixtures/method-overrides/rust/`, a crate holding: a trait with a required method implemented for two types; a trait with a default method, implemented for one type that declares it and one that does not; a blanket impl `impl<T: Display> Tr for T`; a type with an inherent method and a trait impl method of the same name; impls of a workspace trait for `&Widget` and for `(u8, u8)`; an impl whose body holds only a macro invocation that expands to the trait's method; an impl of `std::fmt::Display`; a source file included by both the library and a binary (through `#[path]`) that declares a trait and an impl of it; a trait declared in one file and implemented in another; and callers of the methods.
- [ ] Generate the crate's rust-analyzer index (`index.scip`) and commit it with a `RUST-ANALYZER-VERSION` file in the format of `tests/fixtures/rust-conformance/`.
- [ ] Add support helpers in `tests/support/mod.rs` that load each new fixture's sources and indexes, following `python_fixture_index_from` and `rust_fixture_index`.

## Syntax (code-graph)

- [ ] Extend `TraitImpl` and `SyntaxTree::trait_impls` in [src/graph/syntax.rs](../../../src/graph/syntax.rs) to report every impl of a trait, with the self type's name span optional, and the name span of each function declared directly in the impl body; keep the `type_hierarchy` derivation skipping an impl with no self-type span.
- [ ] Unit test: an impl body with two functions, an associated constant, and a nested macro invocation reports exactly the two function name spans; a blanket impl, an impl for `&Widget`, and an impl for a tuple each report their function, with no self-type span for the last two.
- [ ] Extend `ClassBases` and `SyntaxTree::class_bases` to report the names bound directly in the class body, each marked as a function definition (including decorated ones) or another binding (assignment, annotated assignment, nested class, import).
- [ ] Unit test: a class body with a plain method, a decorated method, two property accessors of one name, a class attribute, an annotated attribute, a nested class, and a function nested inside a method reports each binding with its kind, and does not report the nested function.

## Override edges (code-graph)

- [ ] Add `EdgeKind::Overrides` (`"overrides"`) and `DependencyKind::Overrides`, ordered after `TypeHierarchy`, in [src/graph/store.rs](../../../src/graph/store.rs).
- [ ] Raise `SCHEMA_VERSION` in [src/graph/schema.rs](../../../src/graph/schema.rs) by one from the base branch's value, add a version note, and update the schema-version tests.
- [ ] Add the override pass to `ingest_with_params` in [src/graph/mod.rs](../../../src/graph/mod.rs), following `design.md` § Derive override edges from resolved bases and declared methods.
- [ ] Test, under each Python index of the fixture: `A Python method overrides its base's method`, `A Python override skips a base that does not declare the method`, `A Python override links to its nearest ancestor only`, `A Python override of several bases links to each`, and `A Python base in another module`.
- [ ] Test: `A Python decorated override`, `A Python method shadowing a base attribute is not an override`, and `A Python class with no declared base has no override`.
- [ ] Test, on the Rust fixture: `A Rust impl method overrides a required trait item`, `A Rust impl method overrides a default trait item`, `A Rust blanket impl method overrides its trait item`, and `A Rust inherent method is not an override`.
- [ ] Test (`A method of an external base or trait is not a target`): neither the standard-library subclass's method nor the `Display` impl's `fmt` has an `overrides` edge.
- [ ] Test: a Python diamond, where `C(B)` declares `m`, `B(P, Q)` does not, and `P` and `Q` both declare `m`, gets one edge from `C.m` to each of `P.m` and `Q.m`.
- [ ] Test (`A class attribute in a middle class stops the lookup`), under each Python index.
- [ ] Test (`A private Python name is not an override`), under each Python index: no edge from `B.__m`, and an edge from `B.__eq__` to `A.__eq__`.
- [ ] Test (`A Python property's accessors each link`): under ty-scip, each of `B`'s two `size` definitions links to each of `A`'s; under scip-python, the one `B.size` links to the one `A.size`.
- [ ] Test (`A Rust impl for a reference type overrides its trait item`): the `&Widget` and tuple impls' `tr` each link to `Tr::tr`.
- [ ] Test (`A Rust file in several build units links its shared identities`): the shared impl method links to the shared trait method.
- [ ] Test (`A trait in another document is reached through its use in the impl header`).
- [ ] Test (`A trait name that resolves to several symbols makes no edge`): a hand-built index with two aligned identities at the impl header's trait-name span yields no edge from that impl's methods, and a hand-built index with an ambiguous method definition location yields no edge from that method.
- [ ] Test (`An annotation with no value does not stop the lookup`), under each Python index, next to the valued `m = None` case.
- [ ] Test: resolving the shared-file fixture with its occurrences in reversed order produces the same `overrides` edges.
- [ ] Test (`A macro-generated method has no edge`).
- [ ] Test: a Python class cycle reachable through `type_hierarchy` edges, built from a hand-made index, ends the pass without a repeated edge.
- [ ] Test: the set of `overrides` edges between methods that each Python index reports with one definition is identical under the two indexes.
- [ ] Test (`Builds wholly supersede prior derived state`): build the Python fixture, remove a subclass's base, rebuild, and assert its former `overrides` edges are gone.

## Dependents and ranking (code-graph, code-navigation)

- [ ] Add `'overrides'` to the edge-kind list of the dependents query in [src/graph/store.rs](../../../src/graph/store.rs).
- [ ] Test (`Dependent via override`): `dependents` of a base method reports its override at distance one with kind `overrides` and the override's caller at distance two.
- [ ] Test (`An override's dependents exclude the method it overrides`): `dependents` of an override reports neither its base method nor the base method's callers.
- [ ] Test: `impact` seeded by a diff that changes a base method lists the override and its caller.
- [ ] Add `'overrides'` to the edge-kind list of the rank projection, raise `RANK_VERSION` in [src/graph/rank.rs](../../../src/graph/rank.rs) from 1 to 2, and update its doc comment's description of the projection.
- [ ] Test (`An overridden method ranks by its overrides' dependents`): the hand-built graph the scenario specifies, where a base method with 1 direct caller, overridden by one method with 6 callers, and a function with 3 direct callers are both direct dependents of one seed, and no other edge leaves the override, the callers, or the function; the ranked order puts the base method first.
- [ ] Test (`An override does not inherit its base's importance`): an override with no callers, whose base has many callers, ranks after a function with one caller.
- [ ] Test: a continuation token issued under `RANK_VERSION` 1 is refused.

## Relations (code-navigation)

- [ ] Add `Implements` to `RelationArg` in [src/cli.rs](../../../src/cli.rs), and to `Relation` and `TraceRelation` in [src/query/mod.rs](../../../src/query/mod.rs), with help text stating the type and method meanings.
- [ ] Make `implementers` read the sources of `type_hierarchy` and `overrides` edges, and `implements` read their targets, in canonical-identity order.
- [ ] Test (`Trace implementers of a method`, `Trace implementers of a Rust trait method`): direct overrides only, for a Python method and a Rust trait method.
- [ ] Test (`Trace implements of a method`, `Trace implements of a type`, `Trace implements of a Rust impl method`).
- [ ] Test (`An empty implements answer is a definite none`, `An empty structural relation is a definite none`): the human rendering of each empty answer contains the definite message.
- [ ] Test (`Trace implementers`): the existing type-subject scenario still returns exactly the subtypes.

## Override disclosure (code-navigation)

- [ ] Add a reference-count query to [src/graph/store.rs](../../../src/graph/store.rs) that counts a symbol's resolved reference occurrences.
- [ ] Add the `override_relations` field to `Answer` in [src/query/output.rs](../../../src/query/output.rs), serialized only when present, with the shape in `design.md` § The override disclosure is a new answer field with counts and commands.
- [ ] In the `References` arm of `QueryEngine::trace`, attach the disclosure when the subject's kind is `method`, on the found and the empty answer, computed by transitive walks over `overrides` edges in both directions with a visited set.
- [ ] In the `trace` command handler in [src/commands.rs](../../../src/commands.rs), fill each non-zero part's `command` from the absolute store path, the workspace identity, and the subject's canonical identity, each quoted with `shell_quote`.
- [ ] Pass the field through unchanged in `apply_pagination` and every other `apply_*_pagination` function in [src/query/page.rs](../../../src/query/page.rs).
- [ ] Test (`An overridden method discloses its overrides`, `Overrides of overrides are counted`).
- [ ] Test (`An overriding method discloses what it overrides`): the second part's counts, and the empty line and the part's line both naming the relation.
- [ ] Test (`The part's command runs against the same store`): build into a non-default store a method whose identity holds a shell-special character, run the first part's command through `sh -c`, and assert it lists the direct overrides from that store.
- [ ] Test (`A Rust impl method discloses the trait item it implements`).
- [ ] Test (`A method with no override relation carries zeros`), including the human rendering with no override line.
- [ ] Test (`A type subject carries no override disclosure`), and an ambiguous or unresolved subject carries none.
- [ ] Test: after the rebuild that removes a subclass's base, the subclass method's `implements` answer is empty and its override disclosure carries zeros.
- [ ] Test (`Every page carries the same override disclosure`): through the CLI, the first and the resumed page carry equal disclosures, and the paging counts are the subject's own.

## Human rendering (code-navigation)

- [ ] In [src/render.rs](../../../src/render.rs), print one line per non-zero part after the unconfirmed-use group, stating that the count includes overrides of overrides and that the command lists direct relations, following `design.md` § The human rendering adds one line per non-zero part.
- [ ] Add a clause per non-zero part to the scoped empty line of a `references` answer.
- [ ] Test (`The rendering separates the transitive count from the direct listing`): the rendered line for `A.m` in a three-level chain states two methods, including overrides of overrides, and describes the command as listing direct overrides.
- [ ] Test: an empty `references` answer whose unresolved-call group, unconfirmed-use group, and both override parts are all non-zero renders every count in its empty line and every group and part after it.

## Surface and MCP

- [ ] Raise `SURFACE_VERSION` in [src/manifest.rs](../../../src/manifest.rs) by one from the base branch's value, and regenerate `tests/fixtures/surface_manifest.snapshot.json`.
- [ ] Add `implements` to the relation enumeration in [mcp/src/c10r_mcp/params.py](../../../mcp/src/c10r_mcp/params.py) and to the tool descriptions in [mcp/src/c10r_mcp/server.py](../../../mcp/src/c10r_mcp/server.py), and raise the server's pinned surface version to match.
- [ ] MCP test: a `trace` call with relation `implements` passes through the command's answer unchanged.

## Documentation

- [ ] Update [docs/commands/trace.md](../../../docs/commands/trace.md): the `implementers` row for method subjects, a new `implements` row, the `overrides` kind in the `dependents` section, the override disclosure in the `references` section, and the cases with no edge (unresolved bases, external bases and traits, structural Protocol satisfaction).
- [ ] Update the README command table's `trace` row to name `implements`.
- [ ] Draft the feature commit's body with the ranking migration note (`RANK_VERSION` 2: ranked orders shift once; continuation tokens from earlier builds are refused), so `just changelog` carries it into the release notes.

## Dogfood

- [ ] Build Flask with scip-python and with ty-scip from the working tree, record in `dogfood.md` the number of `overrides` edges under each, confirm the edges between methods that each indexer reports with one definition are identical, and classify every other difference (property accessors, duplicate definitions).
- [ ] Build ripgrep with rust-analyzer, record the number of `overrides` edges, and record `references` and `dependents` for one trait method with several impls.
- [ ] Record in `dogfood.md` the ranked `dependents` of one Flask symbol whose distance-1 layer includes an overridden method, before and after the change.

## Checks

- [ ] Run the full Rust test suite and the MCP test suite.
- [ ] Run the pre-commit hooks on every changed and new file, and fix any failure.
