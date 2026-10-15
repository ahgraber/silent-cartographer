# Proposal: canonical-identity

## Intent

A symbol's canonical identity is the string every answer returns, the store keys on, and agents save for later queries.
Today the identity copies the indexer's descriptor, so the same Python declaration gets a different identity from each Python indexer (`flask.app` as one segment under scip-python, `flask` then `app` under ty-scip), a module carries an `__init__` marker that is not part of its name, and an annotated module global carries its name twice.
A Rust package's library and binary targets land under one package name, so declarations with the same path in two targets become numbered twins whose numbers shift when a sibling file is added, and a call from the binary into the library is attributed to the binary's own declaration.
This change defines the canonical form from the language's own qualified name and the unit that owns the declaration, so that an identity reads as the language spells it, does not depend on which indexer produced it, survives a move of the declaration's file, and distinguishes the units of one package.

## User Stories

### Story: saved-identity-stays-valid

As an agent, I want a symbol's identity to name the same declaration after a re-index, after the declaration's file moves, after a sibling target is added, and whichever conforming indexer built the store, so that identities I saved from one answer keep resolving in later queries.

> Ladders to north-star outcome 1 (**Precise locate**) and outcome 5 (**Calibrated trust**): a saved identity resolves to the declaration it named, or to nothing, never silently to another declaration.

### Story: identity-reads-as-qualified-name

As a human or agent, I want an identity's segments to follow the language's own qualified-name nesting and to carry no tool-specific marker, so that I can read an identity, type one from the code in front of me, and recognize which unit of a package it belongs to.

> Ladders to north-star outcome 3 (**Structural understanding**) and outcome 1 (**Precise locate**): the identity is the qualified name a reader already knows.

### Story: blast-radius-crosses-crate-targets

As an agent changing a library declaration in a Rust package that also has a binary, I want `references` and `dependents` of the library's declaration to include the binary's calls into the library, and the binary's own same-named declaration not to claim them, so that the blast radius across the package's targets is right.

> Ladders to north-star outcome 2 (**Blast radius before change**): a call across Cargo targets reaches the declaration it names.

### Story: relative-imports-resolve

As an agent changing a module, I want code that imports it with a relative import to count as importing it, so that its importers and dependents are complete.

> Ladders to north-star outcome 2 (**Blast radius before change**).

## Scope

**In scope**, in build-dependency order:

- symbol-identity: the canonical form.
  Segments follow the language's declared nesting, one segment per nesting level, with no marker segment for a module or package.
  Every identity names its package and unit, in the grammar `workspace::package[+marker]::segments`: the package's default unit is unmarked, another unit is marked `+kind.name`, and a declaration compiled into more than one unit of its package has one identity marked `+shared`.
  The identity does not depend on the indexer that produced the index, and does not depend on the file or directory that holds the declaration.
  Same-descriptor declarations in different units are distinguished by their unit, with no numbered disambiguator.
  A relative import resolves to the same symbol as its absolute import path.
- code-graph: attribution and currency.
  A reference whose spelling names the package's library (a path beginning with the package name, such as `libbin::util::load()`) is attributed to the library's declaration.
  A change to the build description that assigns units (for example a renamed Cargo target) is a changed input, so a build over it rebuilds.
  The identity form is part of the store's schema version, so the existing requirement `Incompatible index stores are replaced or refused, never half-used` governs stores built under the prior form.
- code-navigation: reference resolution.
  A package-qualified name supplied as a reference resolves to the library's declaration only; a path declared in two units of one package returns a typed candidate set.
- Rust and Python mappings to the canonical form, with the scip-python mapping (per-component module path, no `__init__` marker, doubled module-global names collapsed) applied in the Python adapter.

**Out of scope:**

- ty-scip.
  The change `ty-scip-backend` depends on this change and is paused until it lands.
- Any behavior when an index does not supply a declaration's complete qualified name.
  No supported indexer omits name levels; the rule for future adapters is a design note.
- Mappings for Go, Java, TypeScript, and C#.
  Guidance for future adapters goes in `design.md`, not in requirements.
- Identity for re-exports or public access paths.
  The identity names the declaration, not the names through which it is imported.
- One identity per unit for a source file compiled into several units.
  Such a file has one identity, marked `+shared`.
- Configuration-specific graphs (Cargo features, build tags, target frameworks).
  Build configurations are not units, and the index describes the one configuration the indexer analyzed.
- Translation of identities saved under the prior form.
  A store built under the prior form is replaced on build and refused on query, and a saved identity in the prior form resolves to no symbol.

## Approach

Mechanism thoughts parked here for `design.md`.

- Python mapping, scip-python 0.6.6: the dotted namespace segment (`` `pkg.shapes` ``) splits on `.` into one module segment per component; the `__init__` meta terminal is dropped, so the two module symbols scip-python writes (`` `pkg.shapes`/__init__: `` and `` `pkg.shapes`/ ``) share one identity and their occurrences merge; two consecutive term segments with equal names directly after the module run (`current_app.current_app.`) collapse to one.
  Module classification keys off "a descriptor made only of module segments" and runs in the Python adapter only, because Rust module descriptors are also module-only.
  The join's module-name rule reads a module's dotted name as its module segments joined with `.`.
- Rust mapping, rust-analyzer: one namespace descriptor per module is already the canonical segment form; the crate root is not a segment; the package field is the Cargo package.
  Units come from `cargo metadata --no-deps` targets (kind, name, root file), which the adapter already runs for library roots; each document is assigned to a target root by following `mod` declarations, which the join already derives.
  The derivation moves before identity projection.
  A document reached from more than one root is `+shared`.
  The library kinds (`lib`, `rlib`, `dylib`, `cdylib`, `staticlib`, `proc-macro`) are the default unit; `bin`, `test`, `example`, `bench`, and `custom-build` are marked by kind and target name.
- Unit in the projection: the projection gains a unit alongside the package; the projection writes `workspace::package[+marker]::segments`.
  Two definitions of one backend symbol in different units then have distinct identities and are not twins; twin groups are keyed by the backend's symbol rather than by the projected identity, so the locality rules still attribute references among them.
- Attribution across units: the package-name rule in the join fires today only when the referencing token spells the package name; it extends to a token that is the last component of a path whose first component spells the package name, which the syntax tree supplies.
  References through a `use` import stay with the containing document's unit; that is a known residual.
- Unit unknown: when `cargo metadata` is unavailable or times out, or a document is reached from no target root, the declaration gets no unit marker and the build answer discloses it.
  A malformed `Cargo.toml` or a missing `cargo` makes rust-analyzer fail before the target lookup matters (tested 2026-10-11 on `samples/rust-libbin`), so the path is reached only by a `cargo metadata` timeout or a non-Cargo Rust project built with `--language rust`.
- Currency: the build description that assigns units (`Cargo.toml` and the target list) joins the inputs the currency check compares.
- Conformance evidence: each conformance project carries a committed list of its canonical identities; the Python list is checked against scip-python's index and the Rust list against rust-analyzer's.
  A Rust conformance project does not exist yet and is added by this change.
- Migration: the schema version is bumped, so a store built under the prior identity form is replaced by `build` and refused by every query under the existing code-graph requirement.
  Fixtures and snapshots holding Python identities, binary and test identities, or twin suffixes regenerate.

## Resolved Questions

- 2026-10-11 Segment rule: segments follow the language's semantic nesting, one segment per nesting level, with Python module paths split per component, no `__init__` marker, and scip-python's doubled module-global names collapsed to the plain form.
  Decided with the user.
- 2026-10-11 Unit identity: every identity names its package and unit in the grammar `workspace::package[+unit]::segments`; the default unit is unmarked, another unit is `+kind.name`, a declaration compiled into several units is `+shared`.
  Build configurations are not units.
  Decided with the user.
- 2026-10-11 Cross-target attribution: a reference whose path begins with the package name resolves to the library's declaration; fixed inside this change.
  Decided with the user.
- 2026-10-11 Migration: the identity form is part of the store's schema version; no new migration requirement.
  Decided with the user.
- 2026-10-11 Incomplete names: no requirement; a design note for future adapters.
  Decided with the user.
- 2026-10-11 Placement: a separate change that `ty-scip-backend` depends on; this change does not depend on `ty-scip-backend`.
  Decided with the user.
- 2026-10-11 Unit undeterminable: when the target description is unavailable, the build succeeds, every Rust declaration gets no unit marker, and the build answer discloses that Cargo targets could not be determined; a document reached from no target root gets no unit marker with the same disclosure.
  Decided with the user.
- 2026-10-13 Relative imports: a relative import resolves to the same symbol as its absolute import path, in every language; this is part of the canonical identity and is fixed inside this change. scip-python 0.6.6 records each relative import as a separate external module named by the text after the dots; ty-scip and rust-analyzer already resolve relative imports.
  Decided with the user.
- 2026-10-12 Outcome statements: the delta requirements state outcomes a user can observe; the promises of per-attribution rule provenance and of location-anchored disambiguators are removed, the reference identity lists move from the requirement to design, and the conformance and shared-source scenarios no longer check reference attribution, which code-graph covers.
  Decided with the user.
- 2026-10-11 Indexer-independence evidence: each supported language's conformance project has a reference list of canonical identities, and each conforming backend's index is checked against that list; no constructed imitation index.
  Decided with the user.
- 2026-10-14 Vocabulary: the part of a package a declaration belongs to is a unit; "owner" is not used for it, because c10r already uses owner for the store file; "target" is used only for Cargo's own targets.
  Decided with the user.
