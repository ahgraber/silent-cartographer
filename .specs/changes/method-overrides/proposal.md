# Proposal: method-overrides

> **Placeholder.**
> This proposal records the intent, stories, and scope of the change.
> The delta specs, `design.md`, and `tasks.md` are not written yet.
> Each open question below needs an explicit decision before the change is implemented.
> This change follows `unresolved-calls`.

## Intent

c10r records a type-hierarchy edge between a class and each base class it names, and between a Rust type and each trait it implements.
It records no edge between a method and the method it overrides.
A change to a base method therefore reaches only the base method's direct callers in `dependents` and `impact`, and not the overriding methods or their callers.

scip-python 0.6.6 records an implementation relationship from an overriding method to the base method when the class names its base explicitly: a plain base class, a `typing.Protocol` named as a base, or an `abc.ABC` base.
This was verified with a fixture on 2026-10-03. c10r reads no SCIP relationships today. scip-python records no relationship for a class that satisfies a Protocol only by having its methods.

rust-analyzer emitted no SCIP relationships when this was last checked, on 2026-07-05.
In Rust, a function inside `impl Trait for Type` must be an item of `Trait`, so the syntax tree identifies the trait method it implements without a guess.

## User Stories

### Story: base-change-reaches-overrides

As an agent or developer changing a base method, I want `dependents` and `impact` to include the methods that override it and their callers, so that a rename or signature change reaches every implementation.

> Ladders to north-star outcome 2 (**Blast radius before change**).

### Story: overrides-are-listed

As an agent or developer reading a class hierarchy, I want to list the methods that override a method and the method a method overrides, so that I can find every implementation of a contract.

> Ladders to north-star outcome 3 (**Structural understanding**).

### Story: overridden-method-is-not-reported-dead

As an agent auditing a method for removal, I want the `references` answer to state that the method is overridden and how many resolved callers the overrides have, so that I do not remove a contract that is used through its implementations.

> Ladders to north-star outcome 5 (**Calibrated trust**).

## Scope

**In scope:**

- A method-to-method override edge, from an overriding method to the method it overrides.
- Python: the edge comes from the semantic index's implementation relationships.
- Rust: the edge comes from the syntax tree, for each function inside an `impl Trait for Type` block, to the trait item of the same name.
- `dependents` and `impact` follow the edge from a method to its overrides.
- A `trace` relation that lists a method's overrides and the method it overrides.
- The `references` answer for an overridden method carries one disclosure: the number of overriding methods and the number of their resolved callers, with the command that lists them.

**Out of scope:**

- Listing the callers of overriding methods inside the `references` answer.
  `dependents` lists them; the disclosure names the command.
- Structural Protocol satisfaction, where a class has a Protocol's methods but does not name the Protocol as a base.
  Deciding it needs a type checker.
- Override edges inferred from matching method names in Python.

## Open Questions

- Whether rust-analyzer now emits implementation relationships, which would let both backends use one source.
- Whether the capability is mandatory for both backends or an optional, queryable capability, under the contract-floor rule.
- The name of the `trace` relation and of the disclosure fields.
- How the edge treats a Python method that overrides methods of several bases through multiple inheritance.
