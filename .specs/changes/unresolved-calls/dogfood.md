# Dogfood: unresolved-calls

Measured on 2026-10-19 with the working-tree build `target/release/c10r` 0.1.1 (release profile).
Indexers: scip-python 0.6.6, ty-scip 0.1.0, rust-analyzer 1.95.0 (59807616 2026-04-14).
Clones are under `~/.cache/silent-cartographer`: flask at 22d9247, ai-vfs (`ai-vfs-audit`) at bc8f5f1, ripgrep at 48b0c79.
Every build wrote its own store under `/tmp/claude-501/dogfood/`.
Counts come from `sqlite3` queries on `unresolved_calls` and `discrepancy_symbols`.

## Build times

| Build                  | Wall clock |
| ---------------------- | ---------- |
| Flask, scip-python     | 5 s        |
| Flask, ty-scip         | 2 s        |
| ai-vfs, scip-python    | 17 s       |
| ripgrep, rust-analyzer | 9 s        |

ai-vfs has no `.venv`, so its environment was created outside the clone with `UV_PROJECT_ENVIRONMENT=/tmp/claude-501/dogfood/aivfs-venv uv sync --frozen --all-extras --all-groups` and passed as `--environment`.
scip-python printed a `pyproject.toml` parse error for Flask (TOML line 193); indexing still completed.
Every `trace` answer printed `stale (content changed)` and a "different workspace" warning because the stores were queried from the worktree directory, not the clone.

## 1. Flask recorded calls by form

A recorded call is a call whose name has no resolved occurrence in the index.
Form: attribute call is a row with a receiver (`x.name(...)`), plain call is a row without one (`name(...)`).
The earlier figures come from `._scratch/research/scip-python/probe-unresolved/out.md`.
They count calls found by syntax in the whole clone (3,916 calls: 2,898 attribute, 976 plain) that have no occurrence at the name token.
The 2026-10-03 measurement covered scip-python only and found attribute calls alone unresolved (1,004); the 2026-10-10 measurement added ty-scip and the plain form.

| Indexer     | Form      | 2026-10-03 and 2026-10-10 | 2026-10-19 recorded |
| ----------- | --------- | ------------------------- | ------------------- |
| scip-python | attribute | 1,004 of 2,898            | 1,007               |
| scip-python | plain     | 0 of 976                  | 0                   |
| ty-scip     | attribute | 1,499                     | 1,499               |
| ty-scip     | plain     | 88                        | 88                  |

Discrepancy links in the same stores:

| Indexer     | Rows in `discrepancy_symbols` | Distinct discrepancies linked |
| ----------- | ----------------------------- | ----------------------------- |
| scip-python | 127                           | 115                           |
| ty-scip     | 1,121                         | 314                           |

The ty-scip counts match the 2026-10-10 figures exactly.
The scip-python attribute count is 3 above 1,004 (0.3%); no position-encoding inflation appears.
The 3 extra calls were not traced.
The proposal says ty-scip left 1,000 of the 1,004 scip-python-unresolved calls unresolved; that is the overlap, not the ty-scip total.

## 2. `references` answers on Flask

Subjects: `flask::flask::examples::tutorial::flaskr::blog::index`, `flask::flask::flask::config::Config::__init__`, and the three `raise_if_not_found#0`, `#1`, `#2` definitions inside `flask::flask::flask::cli::locate_app`.

| Subject                          | scip-python                                                                                                                                                                                                                  | ty-scip                                                                                                                                                                                                                                             |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `blog::index`                    | `empty: no resolved reference recorded; 0 calls named index have no resolved target and 0 uses are unconfirmed (calls by frameworks, implicit calls such as constructors and operators, and lookups by string are not seen)` | identical                                                                                                                                                                                                                                           |
| `Config::__init__`               | the same `empty` message with `0 calls named __init__`, no list                                                                                                                                                              | the same `empty` message with `6 calls named __init__`, and an `unresolved:` list of 6: five `super().__init__` calls (cli.py:587, sessions.py:80, testing.py:86, testing.py:126, testing.py:273) and `BaseEnvironment.__init__` (templating.py:49) |
| `raise_if_not_found#0`           | `empty` with `3 uses are unconfirmed`; an `unconfirmed:` list of 3 sites (cli.py:353:57, tests/test_cli.py:223:43, tests/test_cli.py:228:51), each "could mean any of 3 definitions of raise_if_not_found (#0, #1, #2)"      | identical                                                                                                                                                                                                                                           |
| `raise_if_not_found#1`           | identical to `#0`                                                                                                                                                                                                            | identical to `#0`                                                                                                                                                                                                                                   |
| `raise_if_not_found#2`           | 1 result (cli.py:254:14, the module-level use after the definitions) plus the same 3-site `unconfirmed:` list                                                                                                                | the same 1 result plus the same list                                                                                                                                                                                                                |
| `raise_if_not_found` (no suffix) | `absent: nothing resolved (a definite none, not a failure)`                                                                                                                                                                  | identical                                                                                                                                                                                                                                           |

No answer in this task says "definite empty set".
The `blog.index` view is registered with `@bp.route("/")`, and the answer's message names frameworks as a blind spot without naming route registration.
The two indexers differ only for `Config.__init__`, where ty-scip lists 6 `__init__` calls that it could not resolve.
The `#2` result is attributed to `<module>` under scip-python and to `locate_app#2` under ty-scip.

## 3. ai-vfs `MetadataStore.touch_blob` (scip-python)

Subject: `ai-vfs-audit::ai-vfs-audit::vfs::protocols::metadata::MetadataStore::touch_blob`.
Default answer: `empty: no resolved reference recorded; 33 calls named touch_blob have no resolved target and 0 uses are unconfirmed`, then `unresolved: 25 of 33 calls named touch_blob have no resolved target` with 25 listed and the line `list all 33 with --limit 0`.
JSON `--limit 0`: `unresolved_calls.total` 33, 33 calls listed, `truncated` false, `outcome` empty.
Text search `rg -n '\.touch_blob\('` in the clone found 35 call sites.
Comparing by file and line, the group lists 33 of them.
The two it does not list are `src/vfs/vfs.py:437` (`self._meta.touch_blob(content_hash)`) and `tests/integration/test_gc_cross_replica.py:83` (`vfs._meta.touch_blob(content_hash)`).
Both have a resolved occurrence at the call name, on `BaseSqlMetadataStore::touch_blob`, so they are references to a different symbol and are correctly absent from this group.
No call in the group is absent from the text search.

## 4. ripgrep with rust-analyzer

The 2026-10-16 measurement (`._scratch/research/rust-unresolved/out.md`) counted calls found by syntax outside macro invocations, in documents present in the SCIP index.
It found 127 unresolved of 12,039 (1.1%): 96 method, 25 path, 6 plain.
It also estimated 56 more inside macro token trees by a heuristic; the build's syntax pass does not see calls inside macro bodies.

| Source                                                            | Method (`.`) | Path (`::`) | Plain | Total |
| ----------------------------------------------------------------- | ------------ | ----------- | ----- | ----- |
| 2026-10-16, outside macros                                        | 96           | 25          | 6     | 127   |
| 2026-10-19 recorded, all files                                    | 114          | 35          | 6     | 155   |
| 2026-10-19 recorded, excluding `crates/globset/src/serde_impl.rs` | 96           | 25          | 6     | 127   |

Form was taken from the two source bytes before the call name (`::` path, `.` method, otherwise plain); every method and path row has a receiver and every plain row has none.
`crates/globset/src/serde_impl.rs` is a file rust-analyzer left out of the index (the 2026-10-16 measurement listed it as not in SCIP), so all 28 of its calls are recorded as unresolved.
Without it the count equals the 2026-10-16 count in every form.
Discrepancy links: 826 rows, 730 distinct discrepancies.

## 5. Size of the JSON `references` answer with `--limit 0`

For each store the subject is a workspace method whose last component is the most common unresolved name.
Each of those names has a workspace symbol.

| Store               | Most common names (count)                                                   | Subject                                                              | Calls listed | Bytes   |
| ------------------- | --------------------------------------------------------------------------- | -------------------------------------------------------------------- | ------------ | ------- |
| Flask, scip-python  | get 270, route 179, register_blueprint 57, post 34, test_request_context 32 | `flask::flask::flask::sansio::scaffold::Scaffold::get`               | 270          | 121,733 |
| Flask, scip-python  | same                                                                        | `flask::flask::flask::ctx::_AppCtxGlobals::get`                      | 270          | 119,964 |
| Flask, ty-scip      | get 326, route 179, raises 77, register_blueprint 57, parametrize 38        | `flask::flask::flask::sansio::scaffold::Scaffold::get`               | 326          | 145,865 |
| ai-vfs, scip-python | write 491, search 155, setattr 108, native_text_search 91, stat 85          | `ai-vfs-audit::ai-vfs-audit::vfs::vfs::VFS::write`                   | 491          | 266,622 |
| ai-vfs, scip-python | same                                                                        | `ai-vfs-audit::ai-vfs-audit::vfs::protocols::fs_port::FsPort::write` | 491          | 248,979 |

Sizes were taken with `wc -c` on the output of `c10r trace --relation references <subject> --json --limit 0`.
Each call entry is about 450 to 540 bytes.
