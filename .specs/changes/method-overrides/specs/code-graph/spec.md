# Delta for Code Graph

## ADDED Requirements

### Requirement: Override edges (overrides)

The system SHALL persist an `overrides` edge from a method to each method it overrides, and SHALL persist no other `overrides` edge.
In Python, a method declared directly in a class overrides, for each base class that has a `type_hierarchy` edge from the class, the method of the same name declared in that base; when that base binds the name to no declaration in its body, the same rule applies in turn to each of that base's own bases, and when that base binds the name to anything other than a method, such as a class attribute assigned a value, the rule stops along that base; an annotation with no value binds nothing.
A Python name that begins with two underscores and does not end with two underscores is private to its class, so a method with such a name overrides nothing and is overridden by nothing.
In Rust, a function declared in the body of an impl of a trait overrides the item of that trait with the same name, whether that item is required or has a default body, whatever form the impl's self type takes.
When a class, impl, or trait declares several persisted methods of one name, such as a Python property's getter and setter, the rule applies to each of them.
When a method's definition, or the trait or base name a declaration uses, resolves to more than one persisted symbol, the system SHALL persist no edge from that method or through that name, and SHALL NOT choose one of the symbols.
A method SHALL NOT be said to override a declaration that is not a method, a method of a base or trait that is not a persisted symbol, or a same-named method reached through a route other than its declared bases or implemented trait.
Methods that a macro generates, and impls that a macro generates, are outside this requirement: no edge is persisted from or to them by virtue of the macro.
For methods that each supported indexer reports with one definition, the edges SHALL be the same whichever supported indexer built the index, for the same sources and the same resolved bases.

Serves: base-change-reaches-overrides, overrides-are-listed

#### Scenario: A Python method overrides its base's method

- **GIVEN** a Python class `B(A)` where `A` and `B` each declare a method `m`, indexed by each supported Python indexer in turn
- **WHEN** the index is built
- **THEN** an `overrides` edge from `B.m` to `A.m` is persisted

#### Scenario: A Python override skips a base that does not declare the method

- **GIVEN** Python classes `A2`, `B2(A2)`, and `C2(B2)`, where `A2` and `C2` declare a method `m` and `B2` does not
- **WHEN** the index is built
- **THEN** an `overrides` edge from `C2.m` to `A2.m` is persisted

#### Scenario: A Python override links to its nearest ancestor only

- **GIVEN** Python classes `A3`, `B3(A3)`, and `C3(B3)`, each declaring a method `m`
- **WHEN** the index is built
- **THEN** `overrides` edges from `C3.m` to `B3.m` and from `B3.m` to `A3.m` are persisted, and none from `C3.m` to `A3.m`

#### Scenario: A Python override of several bases links to each

- **GIVEN** a Python class `E(X, Y)` where `X`, `Y`, and `E` each declare a method `m`, indexed by each supported Python indexer in turn
- **WHEN** the index is built
- **THEN** `overrides` edges from `E.m` to `X.m` and from `E.m` to `Y.m` are persisted

#### Scenario: A Python base in another module

- **GIVEN** a Python class whose declared base is imported from another module of the workspace, and both declare a method `m`
- **WHEN** the index is built
- **THEN** an `overrides` edge from the subclass's `m` to the base's `m` is persisted

#### Scenario: A Python decorated override

- **GIVEN** a Python class `B(A)` that overrides a `@property`, a `@classmethod`, and a `@staticmethod` of `A` with same-named methods carrying the same decorators
- **WHEN** the index is built
- **THEN** an `overrides` edge from each of `B`'s three methods to the same-named method of `A` is persisted

#### Scenario: A Python method shadowing a base attribute is not an override

- **GIVEN** a Python class `B(A)` where `A` assigns a class attribute `x` and `B` declares a method `x`
- **WHEN** the index is built
- **THEN** no `overrides` edge from `B.x` is persisted

#### Scenario: A class attribute in a middle class stops the lookup

- **GIVEN** Python classes `Root`, `Mid(Root)`, and `Leaf(Mid)`, where `Root` and `Leaf` declare a method `m` and `Mid` assigns `m = None`
- **WHEN** the index is built
- **THEN** no `overrides` edge from `Leaf.m` is persisted

#### Scenario: An annotation with no value does not stop the lookup

- **GIVEN** Python classes `Root`, `Mid(Root)`, and `Leaf(Mid)`, where `Root` and `Leaf` declare a method `m` and `Mid` holds only the annotation `m: object`
- **WHEN** the index is built
- **THEN** an `overrides` edge from `Leaf.m` to `Root.m` is persisted

#### Scenario: A private Python name is not an override

- **GIVEN** a Python class `B(A)` where `A` and `B` each declare a method `__m`, and each also declares `__eq__`
- **WHEN** the index is built
- **THEN** no `overrides` edge from `B.__m` is persisted, and an `overrides` edge from `B.__eq__` to `A.__eq__` is persisted

#### Scenario: A Python property's accessors each link

- **GIVEN** a Python class `B(A)` where `A` and `B` each declare a property `size` with a getter and a setter, indexed by an indexer that reports each accessor as its own definition
- **WHEN** the index is built
- **THEN** each of `B`'s `size` definitions has an `overrides` edge to each of `A`'s `size` definitions

#### Scenario: A Python class with no declared base has no override

- **GIVEN** a Python class that declares no base and has a method with the same name as a method of an unrelated class
- **WHEN** the index is built
- **THEN** no `overrides` edge from that method is persisted

#### Scenario: A Rust impl method overrides a required trait item

- **GIVEN** a Rust trait `Req` with a required method `req`, implemented for two types
- **WHEN** the index is built
- **THEN** an `overrides` edge from each impl's `req` to `Req::req` is persisted

#### Scenario: A Rust impl method overrides a default trait item

- **GIVEN** a Rust trait `Dflt` with a method `dm` that has a default body, implemented for one type that declares `dm` and one that does not
- **WHEN** the index is built
- **THEN** an `overrides` edge from the declaring impl's `dm` to `Dflt::dm` is persisted, and no edge is persisted for the other type

#### Scenario: A Rust blanket impl method overrides its trait item

- **GIVEN** a Rust impl `impl<T: Display> Tr for T` that declares the trait's method `tr`
- **WHEN** the index is built
- **THEN** an `overrides` edge from that impl's `tr` to `Tr::tr` is persisted

#### Scenario: A Rust impl for a reference type overrides its trait item

- **GIVEN** Rust impls `impl Tr for &Widget` and `impl Tr for (u8, u8)` that each declare the trait's method `tr`
- **WHEN** the index is built
- **THEN** an `overrides` edge from each impl's `tr` to `Tr::tr` is persisted

#### Scenario: A Rust file in several build units links its shared identities

- **GIVEN** one Rust source file, compiled into a library and a binary, that declares a trait `Tr` and an impl of it declaring `tr`
- **WHEN** the index is built
- **THEN** an `overrides` edge from the impl's shared `tr` to the shared `Tr::tr` is persisted

#### Scenario: A trait name that resolves to several symbols makes no edge

- **GIVEN** a Rust impl whose trait name resolves to more than one persisted symbol
- **WHEN** the index is built
- **THEN** no `overrides` edge from that impl's methods is persisted

#### Scenario: A trait in another document is reached through its use in the impl header

- **GIVEN** a Rust trait declared in one file and implemented, with a method, in another
- **WHEN** the index is built
- **THEN** an `overrides` edge from the impl's method to the trait's method is persisted

#### Scenario: A macro-generated method has no edge

- **GIVEN** a Rust impl of a workspace trait whose body holds only a macro invocation that expands to the trait's method
- **WHEN** the index is built
- **THEN** no `overrides` edge from that impl's method is persisted

#### Scenario: A Rust inherent method is not an override

- **GIVEN** a Rust type with an inherent method `name` and an impl of a trait whose method is also named `name`
- **WHEN** the index is built
- **THEN** an `overrides` edge from the trait impl's `name` to the trait's `name` is persisted, and none from the inherent `name`

#### Scenario: A method of an external base or trait is not a target

- **GIVEN** a Python class whose declared base is defined outside the workspace, and a Rust impl of a trait defined outside the workspace, each declaring a method of the base or trait
- **WHEN** the index is built
- **THEN** no `overrides` edge to a method that is not a persisted symbol is persisted

## MODIFIED Requirements

### Requirement: Dependents traversal

> Previously: dependents followed `uses`, `imports`, and `type_hierarchy` edges, and no `overrides` edge.

Given a seed symbol, the system SHALL compute the seed's dependents as the symbols whose dependency edges — `uses`, `imports`, `type_hierarchy`, or `overrides` — lead to the seed directly or transitively, SHALL NOT propagate dependence through enclosure (`contains`), and SHALL report each dependent exactly once, carrying the kind of dependency edge that connected it and its distance from the seed in hops — the shortest such distance when several paths exist.

The work the traversal performs SHALL be bounded by the reachable set rather than by the depth bound: once no symbol remains newly reachable, raising the bound SHALL neither change the answer nor materially increase the time taken to produce it.

Given several seed symbols, the system SHALL compute their combined dependents so that each dependent is reported once at its shortest distance from any of the seeds, and the work performed SHALL be bounded by the combined reachable set rather than growing with the number of seeds.

Serves: base-change-reaches-overrides

#### Scenario: Direct dependent

- **GIVEN** a function that uses the seed symbol
- **WHEN** dependents of the seed are computed
- **THEN** the function is reported at distance one with kind `uses`

#### Scenario: Transitive dependent

- **GIVEN** a function that uses another function which in turn uses the seed
- **WHEN** dependents of the seed are computed
- **THEN** the outer function is reported at distance two

#### Scenario: Dependent via imports

- **GIVEN** a module whose use statement names the seed symbol
- **WHEN** dependents of the seed are computed
- **THEN** the module is reported with kind `imports`

#### Scenario: Dependent via trait implementation

- **GIVEN** a type that implements the seed trait
- **WHEN** dependents of the seed trait are computed
- **THEN** the implementing type is reported with kind `type_hierarchy`

#### Scenario: Dependent via override

- **GIVEN** a seed method overridden by a method that a function calls
- **WHEN** dependents of the seed are computed
- **THEN** the overriding method is reported at distance one with kind `overrides`, and the calling function at distance two

#### Scenario: An override's dependents exclude the method it overrides

- **GIVEN** a seed method that overrides a base method, where the base method has callers
- **WHEN** dependents of the seed are computed
- **THEN** neither the base method nor its callers are reported by virtue of the override

#### Scenario: Enclosure never propagates dependence

- **GIVEN** a seed symbol whose containing module has no dependency edge to it
- **WHEN** dependents of the seed are computed
- **THEN** the containing module is not reported as a dependent by virtue of containment

#### Scenario: Multiple paths report the shortest distance

- **GIVEN** a symbol that reaches the seed both directly and through an intermediate symbol
- **WHEN** dependents of the seed are computed
- **THEN** that symbol is reported exactly once at distance one

#### Scenario: Cyclic dependencies terminate

- **GIVEN** two symbols that each use the other, one of them the seed
- **WHEN** dependents of the seed are computed
- **THEN** each symbol is reported at most once and the computation completes

#### Scenario: A closed reachable set does not pay for the remaining depth bound

- **GIVEN** a densely connected graph whose reachable set from the seed is closed well before the depth bound
- **WHEN** dependents of the seed are computed at that bound
- **THEN** the answer is produced within a bounded time rather than in time growing with the bound

#### Scenario: Raising the bound past closure changes nothing

- **GIVEN** a seed whose reachable set is closed at some distance
- **WHEN** dependents of the seed are computed at that distance and again at a much larger bound
- **THEN** the two answers are identical in membership, distances, connecting kinds, and order

#### Scenario: Several seeds cost no more than their combined reach

- **GIVEN** several seeds whose dependents overlap heavily
- **WHEN** their combined dependents are computed
- **THEN** each dependent is reported once at its shortest distance from any seed, and the answer is produced within a bounded time rather than in time growing with the number of seeds

#### Scenario: A shared dependent takes its shortest distance from any seed

- **GIVEN** a symbol reachable at distance one from one seed and at distance three from another
- **WHEN** the combined dependents of both seeds are computed
- **THEN** that symbol is reported exactly once, at distance one

#### Scenario: A seed reached through a saturated intermediate keeps its true distance

- **GIVEN** three seeds where the third reaches the other two only through an intermediate symbol that depends on both of them directly
- **WHEN** the combined dependents of the three seeds are computed
- **THEN** the third seed is reported exactly once, at distance two
