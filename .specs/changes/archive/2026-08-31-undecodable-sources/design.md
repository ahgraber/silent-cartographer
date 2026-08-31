# Design: undecodable-sources

## Context

Source discovery walks the workspace by file extension and reads each match as a UTF-8 string.
It is the sole supplier of source text to the syntax layer, and it has three consumers:

- the build, which parses the collected text and hashes it as the index's content identity;
- the currency check every query performs, which recollects and rehashes to decide fresh vs. stale;
- `impact`, which recollects to compute the pre-change hash it grades exactness against.

All three obtain sources through the same walk, so a decode failure inside the walk is a failure of all three.
That shared origin is also what makes the fix cheap: the guard goes in the walk, and the three consumers inherit it without any per-caller work.

The join already has a representation for a document the semantic layer covers and the syntax layer does not.
An occurrence whose document is absent from the prepared corpus is counted `semantic_only` and recorded as an unaligned occurrence, so an excluded file's semantic coverage degrades into an existing state rather than requiring a new one.

The git boundary already agrees with the policy this change adopts.
`GitRepo::show` returns "no content" for a blob that is not valid UTF-8, on the stated grounds that such a blob is not a source the index holds.
This change makes the filesystem walk say the same thing.

## Decisions

### Decision: SkipRatherThanLossyDecode

**Chosen:** Exclude the file from the syntax layer entirely.

**Rationale:** Lossy decoding looks more generous and is a trap for the join.
Tree-sitter would parse text carrying replacement characters while the semantic indexer decodes the file's real encoding — Python tooling honors PEP 263 declarations and would read it correctly.
The two layers would then disagree about byte positions in the same file, and the join would align occurrences against the wrong spans, producing misaligned or silently wrong locations.
A skipped file degrades honestly instead: its symbols surface as `semantic_only` refusals if the analyzer covers it, which is a state the alignment model already represents.
This is _calibration over coverage_ — honest absence beats corrupted presence.

**Alternatives considered:**

- Lossy decode (`String::from_utf8_lossy`): produces the span disagreement above.
  Worse than the abort it replaces, because the abort at least fails loudly.
- Decode by honoring the file's declared encoding: real work for near-zero payoff; these files are overwhelmingly test fixtures.
  Out of scope, and nothing here forecloses it — a later change can turn an exclusion into an indexed file without altering this contract's other clauses.

### Decision: DiscriminateOnErrorKind

**Chosen:** Match the read error's kind at the read site.
`std::io::ErrorKind::InvalidData` is the decode failure: report and continue.
Every other kind propagates as it does today.

**Rationale:** The contract distinguishes "these bytes are not a source we can hold" from "this file could not be read."
The first is a property of the content and is survivable; the second is an operational failure — a permission problem, a file removed mid-walk, a failing disk — and swallowing it would let a build silently index a subset of the workspace and call it complete.
`read_to_string` reports exactly one kind for a decode failure, so the discrimination is precise rather than a message match.

**Alternatives considered:**

- Skip on any read error: turns an operational failure into a silent partial index.
  Directly contrary to _calibration over coverage_.
- Read bytes then validate explicitly (`fs::read` + `String::from_utf8`): equivalent behavior, more code, and it discards the error kind the standard library already computed.

### Decision: ReportAtTheReadSite

**Chosen:** Emit the diagnostic inside the walk, on standard error, routed through the existing `render::sanitize` guard.
Discovery's return value is unchanged: callers receive the surviving sources and nothing else.

**Rationale:** One emission point cannot drift.
Returning an exclusion list instead would make each of the three consumers decide independently whether to report it, and a consumer that forgot would silently drop the disclosure — the exact failure the `exclusion-is-disclosed` story exists to prevent.
Sanitizing matches every other diagnostic in the codebase and matters here specifically: the excluded file's _path_ is attacker-influenced text being echoed to a terminal.
Stderr is where the existing output-stream contract puts every diagnostic, so a warning cannot corrupt a `--json` answer on stdout.

**Alternatives considered:**

- Return the exclusions and let callers report: drift risk above, and it widens a signature three call sites depend on.
- Carry the exclusions in the machine answer or the manifest: a schema addition the proposal rules out; nothing consumes a count yet.

### Decision: OmitSymbolRowsWithNoSurvivingDocument

**Chosen:** A declared in-workspace symbol every one of whose occurrences names a document absent from the prepared corpus is not persisted as a symbol row at all, alongside its existing `semantic_only` occurrence accounting.
The check reuses the same `PreparedCorpus::get` lookup the join already applies per occurrence — no new plumbing, no new signature on `ingest`/`ingest_with_params`, and no change to any call site.
A symbol that keeps at least one occurrence in a document the corpus does hold is untouched, even when that occurrence itself failed to align — the pre-existing unaligned-content case (a drifted or out-of-range location) is a different condition and is not this one.

**Rationale:** Symbol persistence is unconditional over `index.symbols` and predates this change: every declared symbol with a projected identity gets a row, whether or not a definition aligned, and `definition_content` already falls back to an all-empty payload when nothing aligned.
Before this decision, an excluded file's own symbols still got such a row, and `get`/`find` returned it as `Found` with every content field empty — a confidently-wrong answer shape, not the honest absence `SkipRatherThanLossyDecode` intended.
Distinguishing "no source for this document at all" from "source present but this occurrence didn't align" keeps the fix scoped to the new failure mode without touching the pre-existing lossless-persistence behavior for a symbol whose document is genuinely on hand.

**Alternatives considered:**

- Thread an explicit excluded-path set from discovery through `ingest`: strictly more precise about _why_ a document is missing, but it grows the `ingest`/`ingest_with_params` signature and every call site (production and test) for a distinction the corpus-absence check does not need — a document collect_dir excluded and a document a test simply never supplied look identical from inside `ingest`, and the row should be omitted either way.
- Omit any symbol whose definition did not align (drop the corpus-absence condition, key off `def_name_span` alone): broader and simpler, but it would also stop persisting the pre-existing "ghost" case (an out-of-range SCIP location in a document that _is_ present) — a behavior change to the general lossless-persistence contract this change does not have authorization to make.

### Decision: ExcludeNonUnicodePaths

**Chosen:** A discovered file whose relative path is not valid Unicode is excluded and reported, the same shape as the content-encoding exclusion: `rel_path.to_str()` gates the read, and a `None` skips the file with a best-effort diagnostic (`to_string_lossy()`, for the message only — never used as a key).

**Rationale:** The relative path is the document-path key used everywhere downstream — SCIP document matching, the store's symbol rows, every diagnostic.
Before this decision it was always built with `to_string_lossy()`, which repairs invalid bytes into `U+FFFD` before anything else sees the string; two distinct paths whose invalid bytes both fall in the same position can render identically, making the key ambiguous rather than merely approximate.
That ambiguity is a property of the path-key representation used throughout the codebase, not something this change's diagnostic introduces, but a file it cannot name unambiguously is a file this change should not silently fold into the same "best-effort identified and excluded" bucket as a content-encoding failure — it is excluded outright instead.
Fixing non-Unicode path representation system-wide (a reversible, terminal-safe encoding for every document-path key) is out of scope for this change; this decision only stops a non-Unicode path from being ingested at all.

**Alternatives considered:**

- Leave `to_string_lossy()` in place and accept the ambiguity: the status quo; declined because it lets two distinct files collide onto the same document-path key, which corrupts identity rather than merely losing coverage.
- Reject the whole build on a non-Unicode path: inconsistent with every other exclusion this change makes — a workspace-local anomaly should not cost the rest of the workspace its index.

### Decision: NoSchemaOrMigrationImpact

**Chosen:** No schema version bump and no store migration.

**Rationale:** The change alters which files enter the content hash, which normally would invalidate existing indexes.
It cannot here: a workspace containing an undecodable file has no index today, because every build over it aborts.
A workspace with no undecodable file computes a byte-identical hash before and after.
So no existing store's identity moves.

## Architecture

```text
              workspace files
                     |
                     v
        +--------------------------------+
        |  source discovery walk         |
        |                                |
        |  path not Unicode --> report   |   <-- gate 1: the path key itself
        |                       + skip   |
        |  read each match as UTF-8      |
        |  InvalidData ---> report       |   <-- gate 2: the content
        |                   + skip       |
        |  other error ---> abort        |
        +--------------------------------+
                     |
       surviving (path, text) pairs
                     |
        +------------+------------+
        |            |            |
        v            v            v
      build     currency      impact
                 check      pre-change
                              hash

  excluded file, semantic side:

      analyzer covers it  -->  occurrence's document
                               absent from corpus
                                      |
                                      v
                          semantic_only refusal (existing)
                                      |
                                      v
                    every occurrence of this symbol absent
                    from the corpus --> row not persisted
                    (get/find report absent, not a hollow Found)
```

## Risks

- **A workspace with many undecodable files emits many diagnostic lines.**
  Bounded by the discovered set, and it only fires in a workspace that cannot build at all today.
  Accepted; if it ever becomes noise, the schema addition the proposal defers is the answer, not truncation.
- **The non-encoding read failure is awkward to provoke portably in a test.**
  The usual lever is removing read permission from a file, which does not hold when the suite runs as root.
  Mitigation: gate that test on a runtime check that the permission actually denies a read, and skip it otherwise, rather than asserting on a condition the environment does not provide.
- **The dogfood repository must be fetched.**
  The reproducer is a real repository that ships an undecodable fixture.
  If the network denies the clone, the fixture tests still carry the requirement's runnable evidence and the dogfood is reported as unavailable with the exact command, per the definition-of-done rule for unavailable checks.
- **The non-Unicode-path test cannot exercise its condition on every development platform.** macOS's filesystem (APFS/HFS+) enforces valid-UTF-8 file names at the OS level, so a non-Unicode name cannot be created there at all.
  Gated at runtime, not by `target_os`: the test attempts the fixture write and, on failure, prints why and returns rather than asserting on a condition the environment does not provide — the same shape as the permission-probe test just above it.
  A sibling-project survey (`../ai-zettelkasten`, `../blackwall`) informed this: both gate integration-style tests on the actual capability being probed (a tool on `PATH`, a VM driver) rather than the host OS, and both make an unmet capability a visible, reasoned skip rather than an absent test — `../blackwall/cage/tests/conftest.py`'s policy explicitly rejects OS-gating ("a product claim, not a reason to refuse to execute") and treats an opted-in-but-missing capability as a hard error, never a silent skip.
  Rust's test harness has no native runtime-skip status, so the closest match is the early-return-with-printed-reason this test now uses; it compiles and its skip reason is visible in every run's captured output, including this macOS session, rather than the function not existing at all on a platform that cannot exercise it.
  This closes the "invisible elsewhere" half of the gap; it does not by itself supply execution evidence for the guard on a capable filesystem — see § Verification Waivers.

## Verification Waivers

- **Requirement:** _Source discovery excludes undecodable files_ — the "or whose path is not valid Unicode" clause and its paired scenario ("A file whose path is not valid Unicode is excluded, not fatal").
  **Reason:** The condition can only be constructed on a filesystem that accepts arbitrary path bytes; this development environment is macOS, whose filesystem (APFS/HFS+) refuses a non-Unicode file name outright, and the repository has no CI to supply the run elsewhere.
  The paired test (`commands::tests::a_non_unicode_path_is_excluded_not_fatal`) is written and gated at runtime rather than by platform, and confirms live that it cannot construct the fixture here — it does not confirm the guard itself.
  **Manual evidence:** [manual-evidence-non-unicode-path.md](manual-evidence-non-unicode-path.md) — a code trace showing the guard's only novel logic is a `let...else` dispatch on `Path::to_str()` (a standard-library predicate), reusing the identical loop-`continue` control flow the adjacent content-decode guard already exercises successfully against a real workspace (the sphinx dogfood run).
  **Recorded:** 2026-08-31.
