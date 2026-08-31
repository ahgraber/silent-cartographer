# Proposal: undecodable-sources

## Intent

A workspace file that is not valid UTF-8 takes c10r down entirely.
Source discovery reads every discovered file as a UTF-8 string, and a decode failure propagates as an I/O error that aborts the whole invocation.
One stray fixture — a test file deliberately written in CP-1251 — is enough to make `build`, `get`, `trace`, `find`, and `impact` all fail on an otherwise healthy repository.
The file itself is not the problem: Python tooling honors PEP 263 encoding declarations and would have indexed it, but discovery dies before any indexer starts.

## User Stories

### Story: workspace-survives-undecodable-file

As a developer or agent working in a repository that contains a file c10r cannot decode, I want the index to build and answer over every other file, so that one stray fixture does not cost me the tool.

Ladders to north-star outcome 1 (Precise locate): the rest of the workspace stays navigable.

### Story: exclusion-is-disclosed

As a developer or agent, I want each excluded file named when it is excluded, so that I can tell an honest gap from a defect in c10r and judge whether the gap matters to my question.

Ladders to north-star outcome 5 (Calibrated trust) and the guiding principle _calibration over coverage_: absence is typed and disclosed, never implied.

## Scope

**In scope** (one capability — `code-graph`):

- Source discovery excludes a file whose bytes are not valid UTF-8, reports the exclusion, and completes over the rest.
- The exclusion applies wherever discovery is consumed — the build, the currency check every query performs, and `impact`'s pre-change hash — so the discovered set stays a function of the workspace alone.
- Every other read failure on a discovered file still aborts the invocation.
- A fixture test and a dogfood run over a real repository that carries such a file.

**Out of scope:**

- Honoring PEP 263 declarations (or any other in-band encoding declaration) to decode and index such files.
  Real work, near-zero payoff: these files are overwhelmingly test fixtures.
- Lossy decoding of an undecodable file into the syntax layer.
  See `design.md` § Decision: SkipRatherThanLossyDecode.
- Any status or manifest schema addition carrying a count of exclusions.
  The diagnostic suffices until a consumer needs more.
- The Rust backend equivalent as a separate concern.
  Rust source is UTF-8 by definition of the language, so a `.rs` file `rustc` accepts cannot hit this — the guard is language-agnostic and covers it for free, but no Rust-specific work is implied.
- The non-UTF-8 **patch** abort in `impact`.
  `GitRepo::diff` decodes the whole patch as UTF-8, so a diff that changes a line carrying non-UTF-8 bytes aborts the assessment even after this change.
  Reproduced with `git diff --unified=0` over a modified CP-1251 line: the patch bytes do not decode.
  The fix has a different shape — a patch cannot be skipped file-by-file at the read site — so it belongs in its own change.

## Approach

The failure has a single origin: the syntax-layer source walk reads each discovered file with `std::fs::read_to_string`, whose decode failure arrives as an I/O error of kind `InvalidData` and is propagated by `?`.

Discriminate on the error kind at that read site.
`InvalidData` means the bytes are not UTF-8: report the file and continue the walk.
Every other kind — a permission failure, a file that vanished mid-walk — still propagates, because those are operational failures that must stay loud.

Because the walk is the one place any consumer obtains sources, the guard placed there is inherited by all three consumers with no per-caller work, and cannot drift between them.

An excluded file's semantic occurrences already have a home: the join counts an occurrence in a document with no source as `semantic_only` and records it as unaligned.
That is an existing, representable state in the alignment model, so the degraded case needs no new machinery.

## Open Questions

None.
