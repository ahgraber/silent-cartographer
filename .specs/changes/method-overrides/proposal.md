# Proposal: method-overrides

## Intent

c10r records a type-hierarchy edge between a class and each base class it names, and between a Rust type and each trait it implements.
It records no edge between a method and the method it overrides.
A change to a base method therefore reaches only the base method's direct callers in `dependents` and `impact`, and not the overriding methods or their callers.

The `references` answer has the same gap in both directions.
A base method whose callers all go through subclasses can look unused.
An overriding method usually has no direct callers at all, because callers name the base method and Python or Rust dispatches to the override.
An agent that audits for dead code with `references` would remove either one.

A measurement on 2026-10-16 checked what the indexers record about overrides:

- scip-python 0.6.6 links an overriding method to one target: the nearest ancestor that defines the method, and only through the first base listed when a class has several.
  It records no link for some overrides, such as `__eq__` and `dict.__getitem__`.
- ty-scip 0.1.0 records no link between methods.
  It links classes to their workspace base classes.
- rust-analyzer 1.95.0 records no relationships of any kind, on a test crate and on ripgrep, fd, and c10r.

c10r already resolves each declared base class under both Python indexers, and each implemented trait under rust-analyzer, for its `type_hierarchy` edges.
Python finds a method by name along a class's bases, and a Rust function inside `impl Trait for Type` must be an item of `Trait`.
So c10r can derive override edges itself, from the bases it already resolves and the methods each class or impl declares, the same way under every indexer.

## User Stories

### Story: base-change-reaches-overrides

As an agent or developer changing a base method, I want `dependents` and `impact` to include the methods that override it and their callers, so that a rename or signature change reaches every implementation.

> Ladders to north-star outcome 2 (**Blast radius before change**).

### Story: overrides-are-listed

As an agent or developer reading a class hierarchy, I want to list the methods that override a method and the method a method overrides, so that I can find every implementation of a contract.

> Ladders to north-star outcome 3 (**Structural understanding**).

### Story: overridden-method-is-not-reported-dead

As an agent auditing a method for removal, I want the `references` answer to state when the method is overridden or overrides another method, and how many resolved references the related methods have, so that I do not remove a method that is used through dispatch.

> Ladders to north-star outcome 5 (**Calibrated trust**).

## Scope

**In scope:**

- code-graph: an `overrides` edge from an overriding method to each method it overrides.
  - Python: a method of a class overrides the same-named method of each base class the class declares; when a base does not bind the name, the search continues into that base's own bases, and when a base binds the name to something other than a method, the search along that base stops.
    A private name (`__m`) overrides nothing, because Python renames it per class.
  - Rust: a function inside an impl of a trait overrides the item of that trait with the same name, whatever form the impl's self type takes.
  - When a method's definition, or the trait name in an impl, resolves to more than one persisted symbol, no edge is made, rather than one chosen arbitrarily.
  - Both rules use only bases and traits that the build already resolves for `type_hierarchy` edges, so they hold under scip-python, ty-scip, and rust-analyzer alike.
- code-graph: `dependents`, and therefore `impact`, follow the edge from a method to its overrides, labeled with the kind `overrides`.
- code-navigation: the `ranked` ordering counts `overrides` edges in structural importance, so a base method ranks by the reach of its overrides, and an override ranks by its own callers only.
  The ranking version changes, and the release notes carry a migration note.
- code-navigation: the `implementers` relation accepts a method subject and returns the methods that override it.
- code-navigation: a new `implements` relation returns what the subject implements: for a method, the methods it overrides; for a type, the supertypes it declares.
- code-navigation: the `references` answer for a method that is overridden, or that overrides another method, carries a disclosure.
  It names the related methods, directly or through other overrides, and their number of resolved references, with the command that lists them.

**Out of scope:**

- Listing the callers of related methods inside the `references` answer.
  `dependents` lists them, and the disclosure names the command.
- Following the edge from an override to the method it overrides in `dependents`.
  That direction would add every sibling override of a base method to every override's answer.
  The `references` disclosure covers that direction for dead-code audits.
- Structural Protocol satisfaction, where a class has a Protocol's methods but does not name the Protocol as a base.
  Deciding it needs a type checker.
- Bases that c10r does not resolve, such as a base built at run time (`class C(make_base())`).
- Methods of external bases and traits that the index does not record as symbols, such as `threading.Thread.run` or `std::fmt::Display::fmt`.
- Methods and impls that a Rust macro generates, because the syntax tree does not contain them.
- Override links that indexers record as SCIP relationships.
  c10r does not read them, because two of the three indexers record none.

## Approach

The build already holds the declarations of each document and the persisted `type_hierarchy` edges.
After those edges exist, one pass matches each class's or impl's methods by name against the methods of its resolved bases or trait, and persists an `overrides` edge for each match.
Python ancestors are reached through `type_hierarchy` edges, so the nearest defining ancestor is found by walking those edges.

The new `implements` relation value changes the command surface, so `SURFACE_VERSION` changes, and the MCP server's relation list and tool descriptions change with it.
The store schema version changes, because the build records a new edge kind.

## Open Questions

None.
