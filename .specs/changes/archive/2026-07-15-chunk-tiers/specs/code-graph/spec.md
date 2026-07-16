# Delta for code-graph

## ADDED Requirements

### Requirement: Per-symbol tier content

The system SHALL persist, for every in-workspace symbol whose definition span is persisted, content at fixed tiers derived from the symbol's own source: a signature tier — the declaration form without its body — and an interface tier — the signature together with the symbol's own documentation.
A symbol carrying no documentation SHALL have an interface tier equal to its signature tier, and a declaration with no distinct body SHALL have a signature tier equal to its full declaration.
A module defined by a document rather than an in-document declaration carries no declaration form in source, so its signature tier SHALL be its qualified name.
Tier content SHALL be extracted from the same build's fresh syntax tree that anchors the guarded join, and SHALL be persisted with the build, wholly superseded by each subsequent build.

Serves: fetch-at-depth, search-ready-substrate

#### Scenario: Documented Rust function tiers

- **GIVEN** an indexed Rust function carrying a doc comment and a body
- **WHEN** its tier content is retrieved
- **THEN** the signature tier is the function's declaration without its body, and the interface tier carries both the doc comment and the signature and not the body

#### Scenario: Undocumented symbol falls back to signature

- **GIVEN** an indexed symbol with no documentation of its own
- **WHEN** its interface tier is retrieved
- **THEN** it equals the symbol's signature tier

#### Scenario: Python function tiers carry the docstring

- **GIVEN** an indexed Python function with a docstring
- **WHEN** its tier content is retrieved
- **THEN** the signature tier is the function's header, and the interface tier carries the header together with the docstring

#### Scenario: Rust module interface carries its module documentation

- **GIVEN** an indexed Rust module whose document opens with inner doc comments
- **WHEN** the module's interface tier is retrieved
- **THEN** it carries the module documentation

#### Scenario: Python module interface carries its module docstring

- **GIVEN** an indexed Python module whose document opens with a module docstring
- **WHEN** the module's interface tier is retrieved
- **THEN** it carries the module docstring

#### Scenario: Declaration without a distinct body

- **GIVEN** an indexed declaration with no body distinct from its declaration form
- **WHEN** its signature tier is retrieved
- **THEN** it equals the full declaration

### Requirement: Module bodies span their documents

The system SHALL persist, for every in-workspace module symbol whose definition is a document itself rather than an in-document declaration, that whole defining document as the module's definition span, such that the module's body is the document's full source text, uniformly across supported languages; a module defined by an in-document module declaration SHALL keep that declaration's span as its body.

Serves: orient-by-summary, fetch-at-depth

#### Scenario: Rust module body equals its document

- **GIVEN** an indexed Rust workspace
- **WHEN** a module symbol's body is retrieved
- **THEN** the returned text equals the module's document byte-for-byte

#### Scenario: Python module body equals its document

- **GIVEN** an indexed Python module whose definition occurrence is the zero-width span at its document's origin
- **WHEN** the module symbol is persisted and its body is retrieved
- **THEN** the returned text equals the module's document byte-for-byte

#### Scenario: Inline module declarations keep their declaration spans

- **GIVEN** an indexed Rust workspace containing an inline `mod` declaration with a body
- **WHEN** that module symbol's body is retrieved
- **THEN** the returned text equals the inline declaration's source text, not the whole document
