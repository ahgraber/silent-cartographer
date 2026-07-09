# Design: python-adapter

## Context

- The backend contract, identity projection, normalization/twin machinery, locality attribution, and store are all language-neutral after `duplicate-identity`; this change adds a second producer, not new consumers.
- scip-python (Sourcegraph, built on Pyright) is the only production Python SCIP indexer; it runs on Node.js and emits the same SCIP protobuf the Rust adapter already parses.
- Settled scope (proposal, 2026-07-07): full parity from the batch index, no live overlay; single-language per store; typed environment failure + provenance now, doctor command later; dogfood target `pydantic/httpx2`.
- The guarded-join delta pins Python's launch posture: default name-token rule only, everything else refused; Python rule families are earned by dogfood evidence, mirroring how Rust's four were.

## Decisions

### Decision: scip-python invoked as an external tool, version-pinned by provenance, no fork

**Chosen:** invoke `scip-python index` from PATH exactly as rust-analyzer is invoked; record the tool's reported version as analyzer provenance; document a known-good version; do not fork or vendor.

**Rationale:** adopt-over-build; provenance-recorded versions already gate staleness, so a tool upgrade honestly invalidates the store rather than silently shifting descriptors.
The maintenance risk from the research (Sourcegraph abandonment, medium likelihood) is accepted for now — the codebase is small and forkable the day the risk lands, and forking preemptively is speculative work.
This settles the proposal's last open question.

**Alternatives considered:**

- Vendoring/forking scip-python now: YAGNI; adds a Node build product to a Rust repo before any breakage justifies it.
- Waiting for scip-ty/scip-pyrefly: confirmed wishlist-only (2026-07-07); a waiting strategy with no ship date.

### Decision: Environment detection order with no silent fallback

**Chosen:** resolve the Python environment as — explicit CLI flag → `$VIRTUAL_ENV` if set → a `.venv`/`venv` directory in the workspace root → typed refusal.
No silent fallback to a system interpreter.

**Rationale:** indexing against the wrong environment produces confidently wrong third-party resolution — the exact failure class the calibration principle forbids; refusal with a named remedy ("activate or pass the environment") is the honest floor.

**Alternatives considered:**

- System-python fallback: silent wrong-environment indexes; rejected.
- Requiring the flag always: hostile ergonomics for the overwhelmingly common activated-venv and `.venv` cases.

### Decision: Environment provenance = interpreter version + environment path + installed-package fingerprint

**Chosen:** the declared environment facts are the interpreter version, the resolved environment path, and a fingerprint of the installed package set (sorted distribution name+version list from the environment's `*.dist-info` directory names, hashed — a directory listing, no pip invocation).
Staleness compares all three.

**Rationale:** interpreter path/version alone misses the common drift (`pip install` into the same venv changes what imports resolve to); the dist-info listing catches installs, upgrades, and removals for the cost of one readdir.

**Alternatives considered:**

- Interpreter path+version only: blind to package drift, the likeliest real staleness source.
- Hashing site-packages contents: accurate but slow and I/O-heavy on large environments.

### Decision: Shared SCIP translation, thin per-language adapters

**Chosen:** extract the SCIP-protobuf translation (`translate_index`, symbol parsing, range reading) from the Rust adapter into a language-neutral module; each adapter becomes tool invocation + provenance + language-specific enrichment (Rust: `cargo metadata` library roots; Python: environment facts, empty `library_roots` at launch).

**Rationale:** the translation is already descriptor-grammar-generic; duplicating it per adapter would fork bug-fix surface.
Python supplies no library-roots map in v1 — Python twin groups fall back to the refusal path by construction until dogfood shows which package facts matter.

**Alternatives considered:**

- Copying translate_index into the Python adapter: divergence risk for zero benefit.

### Decision: tree-sitter-python mirrors the Rust syntax layer

**Chosen:** a Python syntax module over tree-sitter-python providing the same queries the join consumes: declaration kinds (module, class, function — including decorated and async forms), name-node containment, enclosing-declaration chains, whole-document module spans.
Descriptor terminal names are taken from scip-python as-is; no custom descriptor normalization layer (re-exports through `__init__`, `<locals>` names, overloads, `TYPE_CHECKING` conditionals land in refusal buckets or the twin machinery until dogfood evidence justifies specific handling).

**Rationale:** parse-don't-guess: the hazards from the research are real but their on-the-ground shapes are unknown; the refusal default makes them visible (typed, counted) instead of silently mishandled, which is exactly the evidence the follow-up rule families need.

**Alternatives considered:**

- Building a qualname normalization layer up front: speculative — normalizes cases nobody has measured yet, against the change's own launch-posture contract.

### Decision: Language selection by manifest, pyproject.toml as the Python marker

**Chosen:** `Cargo.toml` marks Rust, `pyproject.toml` marks Python; both present + no `--language` flag → typed refusal naming the flag; the flag overrides detection; the selected backend is recorded via analyzer provenance (no new schema column for language — the analyzer identity carries it).

**Rationale:** the two manifests are the modern canonical markers; setup.py-only legacy layouts are served by the explicit flag rather than a detection heuristic that ages badly.

**Alternatives considered:**

- Detecting setup.py/setup.cfg/requirements.txt: heuristic sprawl for shrinking layouts; the flag covers them.
- A dedicated `language` metadata column: redundant with analyzer identity.

### Decision: SCHEMA_VERSION 6 → 7

**Chosen:** bump for the environment-provenance facts persisted on `index_metadata` (nullable JSON text — absent for the Rust adapter).

**Rationale:** the staleness contract now reads these facts; a v6 store read by the new binary would silently lack them.
The existing replace-on-build / typed-refusal-on-query contract handles migration; per the repo governance rule this is the migration plan (pre-1.0, replace-and-rebuild).

**Alternatives considered:**

- No bump with a nullable column bolted on: v6 stores would report fresh while missing staleness inputs — a silent calibration hole.

### Decision: Conformance suite parameterized per backend, Python fixture committed

**Chosen:** the existing conformance suite becomes backend-parameterized; a minimal committed Python fixture project (a package with a nested module, a class with a method and a base class, a cross-module import and call) plus a checked-in expected-shape record; the suite runs per adapter and gates usability independently.
CI-shaped runs without scip-python installed skip the live-tool leg with an explicit skip marker, never a silent pass; the SCIP-translation and join legs run from a committed fixture index, tool-free.

**Rationale:** the delta contract requires per-backend gating; committing the fixture index keeps the suite deterministic where the tool is absent while the live leg still exists for machines that have it.

**Alternatives considered:**

- Requiring scip-python for the whole suite: turns every unrelated test run into a Node dependency.
- Mocking scip-python's output entirely: the live leg is the only place real-tool drift would ever surface.

## Architecture

```text
build --language? ──manifest detection──► backend selection (typed refusal on ambiguity)
        │
        ├─ RustAdapter    (rust-analyzer scip; cargo metadata → library_roots)
        └─ PythonAdapter  (env resolution: flag → $VIRTUAL_ENV → .venv → typed refusal;
                           scip-python index; env facts → provenance)
        │
        ▼
shared SCIP translation ──► ExtractedIndex (+library_roots, +environment facts)
        ▼
normalize (twin split)  ──► identity projection ──► guarded join
                                                    │  syntax oracle per language:
                                                    │  tree-sitter-rust | tree-sitter-python
                                                    │  Python: default rule only, refusals typed
                                                    ▼
                                            store (schema v7: env provenance on metadata)
                                                    ▼
                              status / get / trace — language-neutral, unchanged consumers
```

## Risks

- **Python alignment rate will start low**: decorators, properties, re-exports, and dynamic constructs will land in `text_mismatch`/`semantic_only` until Python rule families are earned; the dogfood loop on httpx2 is an explicit task group, and the launch-posture contract makes the interim honest rather than wrong.
  Do not loosen the default rule to inflate the number.
- **scip-python output shape surprises** (position encoding, module/whole-file spans, `local` symbol prevalence): the conformance fixture pins the shapes we depend on; deviations surface there first, not in production joins.
- **Environment fingerprint false negatives**: editable installs and non-dist-info distributions may drift undetected; recorded as a known limitation — the fingerprint is a staleness floor, not a proof.
- **scip-python maintenance risk**: accepted (decision above); the version in provenance plus the conformance suite means an upgrade that shifts descriptors is caught as staleness + conformance failure, not silent drift.
- **httpx2 tag drift**: the dogfood pins an exact release tag at implementation time and records it in `dogfood.md` so ground truth is reproducible.
