# Dogfood notes: is-tested

Spot-check of `trace <symbol> --relation tests` on the persistent clones (2026-08-01), after rebuilding each index at schema version 12 with the release binary.

## ripgrep (Rust)

`globset::glob::Glob` → 11 test sites out of 36 reference sites, answer marked `classification: "convention"`.
Rule mix: `test_attribute` ×9 (sites inside `#[test]` functions), `test_configuration` ×2 (module-scope `use` sites attributed to the `#[cfg(test)] mod tests` modules themselves).
The `references` answer over the same subject carries no marker and no per-site rule; the human render prints the convention notice above the results.

## fd (Rust)

`filter::size::SizeFilter` → 74 test sites: `test_attribute` ×4, `test_configuration` ×70.
The heavy configuration bucket is macro-generated test tables in `src/filter/size.rs` whose reference sites attribute to the gated `tests` module at module scope — plausible and inspectable through the per-site rule.

## Flask (Python)

`flask.app.Flask` → 135 test sites: `test_file` ×112, `test_directory` ×23.
The directory bucket is the sample applications under `tests/test_apps/` whose file names match no test-file form.
Includes a module-scope `conftest.py` site attributed through the document-module fallback (`enclosing: null`, rule `test_file`), and fixture functions in `conftest.py`.

## httpx2 (Python)

`httpcore2._async.connection_pool.AsyncConnectionPool` → 41 test sites, all `test_file`, enclosing real pytest functions under `tests/`.
`httpx2._models.Response` → definite empty **with** the convention marker: the suite exercises the class through the package re-export, so its direct reference set holds no test-classified site.
This is the disclosed-miss side of the calibration trade working as designed — the empty answer scopes itself to "no convention-classified reference site found" rather than claiming the class is untested.
