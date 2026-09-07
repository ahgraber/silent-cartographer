# Evidence notes: eval-localization

## Static c10r binary build (2026-08-15)

- Command: `evals/scripts/build-c10r-musl.sh aarch64` (cargo-zigbuild cross-compile in a native container; arch is a script parameter).
- Artifact: `evals/vendor/c10r/aarch64/c10r` — ELF 64-bit LSB executable, ARM aarch64, statically linked, stripped, 42,756,840 bytes (embedded model payload included).
- SHA-256: `a018bc20b2390c73d24fa8bb69084fa0371147ba680b24fc0269acdcece5544d`.
- Offline verification: the script's final step ran `c10r --version` in a bare `alpine:3.20` container with `--network none`; the script completes only if that call succeeds (it did).
- The `x86_64` variant is the same command with `x86_64` as the argument; it is built at frozen-run provisioning time (compile is a native cross-compile on any host; only its verification container needs x86 execution).

## c10r ingest memory finding (2026-08-16, dogfood via treatment image builds)

- `c10r build --language python` on Faker (`joke2k/faker`, 814 files, ~60k aligned symbols) peaks at **≈27.8 GB memory footprint** on the host (`/usr/bin/time -l`, full successful build; the scip-python phase alone is only ~2.4 GB, so the ingest/embedding phase owns the rest).
- Consequence here: `RUN c10r build` inside treatment image builds is OOM-killed (exit 137) at any sane podman-VM allotment; reproduced at 8 GiB.
- This is a concrete reproducer for the O(corpus) ingest/hydration cost deferred to the `internal-efficiency` stub during semantic-investigation; Faker's hundreds of near-duplicate locale-provider files are the pathological corpus shape.
- Index portability probe: an index built at one workspace path serves queries from a copy at another path — exit 0, answers correct, prefixed with `warning: this index describes a different workspace (built for <path>)`.
  Host-built indexes baked into images are therefore mechanically viable; the per-query warning is the trade-off (a treatment-arm-only stimulus that could depress agent trust/uptake).
- Concurrent `c10r build` runs against one store refuse cleanly: `ingest failed: store error: database is locked`.
- **Resolved 2026-08-25**: after a c10r-side ingest memory fix (user change, outside this eval change), the same fresh Faker build peaks at **2.57 GB child RSS** (142 s, exit 0) — the ceiling is now scip-python's own ~2.4 GB phase.
  In-image index builds are viable again; IndexBakedAtBuild stands as designed.
  Alignment stats shifted slightly vs the pre-fix run on an identical checkout (aligned 59,913 → 59,741; text_mismatch 179 → 144; exact 54,740 → 54,745) — flagged to the user for their byte-identity gate on the c10r change.

## c10r non-UTF-8 source finding (2026-08-25, dogfood via treatment image builds)

- `c10r build --language python` on sphinx (`sphinx-doc/sphinx`) aborts with `reading ./tests/roots/test-pycode/cp_1251_coded.py: stream did not contain valid UTF-8` (exit 1).
- The file is a deliberate CP-1251-encoded test fixture with a PEP 263 encoding declaration — legitimate repo content; one unreadable file currently kills the entire index build.
- Affects 3 of 20 dev instances (all sphinx); the other 17 build clean.
  15 images built before the failure, including Faker (post-memory-fix, in-container) and matplotlib.
- c10r-side fix indicated: tolerate non-UTF-8 files as a typed skip with a warning instead of a fatal error.
- **Resolved 2026-08-31**: shipped on c10r main as the `undecodable-sources` change (`51b4818` fix(build) + `6a010b4` docs sync/archive) — undecodable file contents and non-Unicode paths are excluded with a warning instead of failing the build.

## Treatment provisioning evidence (2026-08-25)

- Dev-subset treatment images: **17/20 built** (`build-manifest.json` written; index sizes 6.6–155.6 MB; per-image wall clock recorded).
  3 sphinx instances fail on the non-UTF-8 finding above and are recorded by the build script as failures.
- Offline query check: `docker run --rm --network none c10r-eval-treatment-jazzband__tablib-613 sh -c 'c10r status && c10r find Dataset'` — `status` reports `"freshness": "fresh"`, `"stale": false`, provenance `scip-python 0.6.6`, semantic index with pinned model identity; `find Dataset` returns 25 of 41 results with a resume cursor.
  No foreign-workspace warning: the index is built and served at `/repo`.
- Faker built in-container post-memory-fix (the same instance that OOM'd pre-fix).

## Runner-compatible task format evidence (2026-08-25)

- Command: `uv run pier run -p tasks/dev/baseline -i 'jazzband__tablib-613' --agent nop --env docker`.
- Result: 1 trial, 0 exceptions, 17 s end to end; Pier built the environment, ran the `nop` agent, executed `tests/test.sh` as verifier, and parsed the reward — reporting all grading metrics with the empty-answer signature (`Reward 0.0`, `Unparsed 1.0`), which is grading totality behaving under the real harness (the nop agent writes no answer).
- Contract corrections discovered against Pier 0.3.1's actual parser and folded into the generator:
  - `-p` takes a dataset directory whose children are task dirs; tasks filter via `-i`/`--include-task-name` globs.
  - `[task]` is registry metadata requiring `name` in `org/name` form (we use `<arm>/<instance_id>`); harness fields (instance, arm, dataset revision) live in Pier's free-form `[metadata]` table.
  - The verifier is `tests/test.sh` (discovered by convention, run in the shared agent environment); the reward file must be a flat numeric mapping written to `/logs/verifier/reward.json`; non-numeric grading detail goes to a `grade-details.json` sidecar.

## Build environment findings, recorded for reruns

- sqlite-vec 0.1.9's C amalgamation needs `-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int64_t=uint64_t` under musl (BSD type names that glibc leaks transitively but musl headers never provide); wired into the script.
- Compiling the main crate needs more RAM than podman machines allot by default (rustc SIGKILL = VM OOM); fixed with `podman machine set --memory 8192 --cpus 6`.
- The nix-packaged podman (5.8.2) ships no Rosetta support (no `--rosetta` flags, no `/mnt/rosetta` mount); x86 containers on this machine run under qemu, which handles light workloads but segfaulted rustc — hence native cross-compilation, never emulated compilation.
