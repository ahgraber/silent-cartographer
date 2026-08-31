# Manual evidence: non-Unicode-path exclusion

Recorded 2026-08-31.
Backs the Verification Waiver in `design.md` for the "non-Unicode path" clause of the _Source discovery excludes undecodable files_ requirement, which has no automated execution evidence in this repository's development environment.

## Why automated evidence is unavailable here

The condition — a discovered file whose relative path is not valid Unicode — can only be constructed on a filesystem that accepts arbitrary path bytes.
This sandbox is macOS; APFS/HFS+ enforces valid-UTF-8 file names at the OS level and refuses the write outright (`std::fs::write` returns `Illegal byte sequence (os error 92)`).
The repository carries no CI configuration, so no other automated run exists to supply the evidence either.
The paired test, `commands::tests::a_non_unicode_path_is_excluded_not_fatal` (`src/commands.rs`), is written and gated at runtime rather than by platform — it attempts the fixture write and discloses why it cannot proceed when the filesystem refuses it, confirmed live in this session:

```text
skipped a_non_unicode_path_is_excluded_not_fatal: this filesystem refuses a non-Unicode file name
outright (Illegal byte sequence (os error 92)), so the exclusion this test targets cannot be
constructed here — expected on macOS (APFS/HFS+); run on Linux to exercise it
```

That confirms the test harness is honest about its own limits.
It does not confirm the guard itself is correct — that is what this record establishes by inspection.

## Code trace of the guard

`src/commands.rs`, inside `collect_dir`, immediately before the already-proven content-decode guard:

```rust
let rel_path = path.strip_prefix(root).unwrap_or(&path);
let Some(rel) = rel_path.to_str() else {
    eprintln!(
        "{}",
        crate::render::sanitize(&format!(
            "warning: {} has a path that is not valid Unicode; excluded from the build",
            rel_path.to_string_lossy()
        ))
    );
    continue;
};
```

- `Path::to_str`/`to_string_lossy` are standard-library primitives, not code this change wrote; their behavior (`to_str` returns `None` exactly when the path is not valid Unicode; `to_string_lossy` never panics and always returns a `Cow<str>`) is part of the standard library's own tested contract, not something this change needs to separately verify.
- The branch this change adds is a `let...else` dispatch on that `Option`: on `None`, emit the diagnostic and `continue` past this entry — a `continue` inside the same `for entry in std::fs::read_dir(dir)?` loop the content-decode guard's own `continue` already exercises live, on the real sphinx dogfood build (see `dogfood-evidence.md`).
  Both branches use the identical control-flow primitive (loop-`continue`) that has already run, successfully, against a real multi-hundred-file workspace.
- The only novel logic is _which_ condition selects the branch — `rel_path.to_str().is_none()` versus `read_to_string(&path)`'s `ErrorKind::InvalidData` — and that condition is a direct call to a standard-library predicate, not custom parsing.
- `commands::tests::a_file_not_valid_utf8_is_excluded_not_fatal` (adjacent in the same file) proves the sibling branch — same loop, same `continue`, same diagnostic shape, different guard predicate — end to end, including that collection still returns `Ok` with the decodable sibling intact.

## Residual risk this waiver accepts

- The `let...else` control flow itself (correct early-exit, no fallthrough into the content read for a path that already failed the Unicode check) is unverified by execution — accepted, on the strength of the trace above: it is the same shape as the adjacent, execution-proven branch, differing only in the guard predicate.
- A Linux run remains the way to close this for real; see the open follow-up task in `tasks.md`.
