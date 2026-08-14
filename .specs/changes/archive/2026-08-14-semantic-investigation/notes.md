# Change notes: semantic-investigation

## Dogfood (2026-08-11)

Rebuilt all four persistent dogfood clones (ripgrep, fd, httpx2, Flask) with the new schema and ran task-grounded passes with `search` and `similar` in simplify/refactor/code-review mindsets.
The Python clones were rebuilt in a second pass, once `scip-python` was installed: httpx2 35,249 aligned symbols with ~4,200 corpus entries; Flask 16,710 aligned.

Build cost: the semantic pass is invisible at repo scale — ripgrep (50,391 aligned symbols, 5,092 corpus entries) and fd rebuilt in seconds, embedding included.
Query latency: ~1.0 s wall per query, dominated by binary startup (model parse plus freshness probe); the retrieval itself is sub-millisecond.

`search` judgment: **useful**.
Every query tried ranked the on-target symbol in the top 5:

- "convert a glob pattern into a regular expression" → `Glob`, `Tokens::to_regex_with` (the actual conversion).
- "decide whether a file should be ignored by gitignore rules" → `WalkBuilder`, `GitignoreBuilder::add_str`, `IgnoreBuilder::require_git`.
- "write search results as JSON" → `JSONBuilder::build`, `Printer::JSON`, `SearchResult`.

Python passes, same verdict:

- Flask, "register a function to run before each request" → `Scaffold.before_request` first, then `Blueprint.before_app_request` and `after_request`.
- httpx2, "decode chunked transfer encoding from the response body" → `Response._get_content_decoder` first.
- httpx2, `similar Client::_send_single_request` → `AsyncClient._send_single_request` **first** — the sync/async parallel implementation, the exact consolidation signal the assess-similarity story exists for — followed by the transport `handle_request` family.

Python-corpus noise observation: scip-python persists function parameters as symbols with tier content, so one-token parameter entries (`follow_redirects`, a bare `request`) entered the corpus as leaves and polluted some answers — a redirect-phrased query surfaced five `follow_redirects` parameter rows before the redirect logic itself.
Fixed in round 2 below: name-only symbols are excluded from the corpus, and containment counts contributing children only.

`similar` judgment: **useful**.
`similar pattern_from_bytes` ranked its near-twin `pattern_from_os` first — the consolidation candidate a refactor pass wants — and surfaced the pcre2/regex parallel implementations together.
Clone markers fire correctly in the wild: grep-pcre2's `RegexMatcher::new` is marked an **exact clone** of grep-regex's (token-identical parallel implementation), and ripgrep carries 237 substitution-key groups with two or more members (repetitive flag structs, numbered test variants, duplicated helpers) — real duplication, detected deterministically.

Fallback-architecture signal: **none**.
The static hybrid met every query tried; the late-interaction fallback stays parked.

Finding fixed during dogfood: sqlite-vec caps a KNN query's `k` at 4096, and the vector signal passed the full corpus size — ripgrep's 5,092-entry corpus was the first to hit it.
The signal now clamps to the cap (an honest candidate pool under the candidates-not-completeness framing) with a store-level regression test.

## Dogfood round 2 (2026-08-11, after review)

Round 1's queries were leading — they shared vocabulary with the definitions they found.
They are kept as the regression floor; round 2 used non-leading queries (vocabulary disjoint from the code, abstract descriptions), after two fixes landed:

**Fix 1 — name-only corpus exclusion.**
A symbol whose every tier is its bare name token (scip-python's parameter symbols) now contributes no corpus entry and no clone keys.
Fixing it exposed a second defect: containment counted _persisted_ children, so every Python function containing its parameter symbols read as a container and contributed only its interface — its body was never indexed.
Containment now counts _contributing_ children only. httpx2's corpus dropped from ~4,200 entries to 2,240, function bodies entered the Python corpus for the first time, and the previously polluted redirect query now answers with `Client.send`, `_send_handling_auth`, and `AsyncClient._send_handling_redirects` — no parameter rows.

**Fix 2 — `similar` is the two-signal hybrid.** `similar` now ranks by the same RRF fusion `search` uses (vector signal + BM25 over the subject's own render words), beneath the unchanged clone-certainty tier; the sync/async twin result still ranks first.

**Round-2 results (hits and misses, verbatim tally):**

- HIT — httpx2, "reuse open sockets between requests instead of reconnecting" (no "pool" vocabulary) → `ConnectionPool.__init__` / `AsyncConnectionPool.__init__` top-2.
- HIT — Flask, "turn an unhandled crash into a 500 error page" → `Flask.handle_exception` first.
- HIT — ripgrep, "show a few lines before and after each hit" (no "context") → `ContextMode::set_before` / `set_after`.
- HIT — ripgrep, "coordinate worker threads walking directories at the same time" → `Worker::get_work`, `WalkParallel::visit`.
- PARTIAL — Flask, "store data that lives only for the duration of one request" → the `flask.ctx` neighborhood (`after_this_request`, `RequestContext.copy`) but not `g` itself.
- PARTIAL — ripgrep, "avoid printing garbage from files that are not text" → the binary-handling _flag documentation_ methods, not the detection logic in grep-searcher.
- MISS — httpx2, "give up if the server takes too long to answer" (no "timeout") → TLS/SOCKS constructors; timeout logic absent from the head.
- MISS — fd, "run a program on every file that matches" → completions/test helpers; the `exec` module absent from the head.

Judgment: strong on abstract queries that share _any_ domain vocabulary with names or docs; weak when the paraphrase shares none (the two misses).
That weakness is the first concrete signal relevant to the fallback architecture — logged as a watch item, not a trigger: agents typically issue vocabulary-bearing queries, and the misses degrade to a browsable ranked list, never a confident wrong answer.

## Spike: composite similarity ranking (2026-08-11)

Time-boxed mini-spike: RRF-fused a keyword-overlap rank (FTS5 over the subject's own render words) with the vector rank, on two ripgrep subjects, against vector-only.

- `pattern_from_bytes`: top-8 overlap 6/8; fusion promoted same-module neighbors (`patterns_from_reader`, `tests::bytes`).
- `Tokens::to_regex_with`: top-8 overlap 4/8; fusion promoted same-domain functional neighbors (`new_regex`, `new_regex_set`, `GlobBuilder::build`) over cross-crate incidental matches.

First-round verdict, superseded by review: the keyword-signal lift was judged promising but deferred whole.
The review set the two-signal hybrid as this change's floor for `similar` (landed — see round 2 above), leaving the remaining signals as the open spike question.

## Spike round 2: the remaining signals (2026-08-11)

Signature-shape (sequence similarity over signature text), graph-interaction (Jaccard over dependency-edge neighborhoods), and token edit-distance (over the floor's top 100 candidates, a banded-computation stand-in) rank lists, RRF-fused against the shipped two-signal floor on ripgrep subjects.
Approximation: the floor enters the fusion as one signal rather than being re-split into its two constituents, so this measures head-reshaping, not exact four-signal RRF.

- `pattern_from_bytes`: top-8 overlap 1/8 — the fused head becomes the sibling family (`pattern_from_os`, `patterns_from_stdin`, `patterns_from_path`) plus the `RegexMatcher` constructor family, displacing `Display::fmt` / `valid_up_to` / printer-type noise from the floor's head.
- `Tokens::to_regex_with`: top-8 overlap 3/8 — the fused head becomes the `Glob::literal` / `prefix` / `suffix` / `ext` / `required_ext` family: same-shaped sibling methods, exactly the Type-3 near-clones the deterministic keys cannot certify.

**Per-signal ablation** (floor, floor+one signal, floor+all; ripgrep `pattern_from_bytes` and `Tokens::to_regex_with`, httpx2 `Client::_send_single_request`):

- **Graph-interaction gave the greatest lift.**
  It alone surfaced the true sibling family (`patterns_from_path`, `patterns_from_stdin` — co-called from the same flag-parsing code) and, on `to_regex_with`, filled the head with the same-`impl` `Glob` family while keeping the functional top hit at rank 1.
  Droop modes: hub symbols leak in through shared-neighbor mass (`grep-cli::crate`, `Response::request`), and parallel implementations with disjoint neighborhoods (sync/async twins) score zero on this signal alone — fusion rescued the twin in every trial.
- **Signature-shape lifts same-API families but is the droop-prone signal.**
  On `pattern_from_bytes` it pulled the `RegexMatcher::new` / `Builder::build` constructor family (same `fn(&str) -> Result<_, Error>` shape); on `to_regex_with` it demoted the most functionally-related row (`tokens_to_regex`) from rank 1 to 3 in favor of shape-siblings.
  Expect droop whenever the subject's signature is generic (constructors, `fn(&self) -> Option<_>` getters): same-shape-but-unrelated rows flood the head.
- **Edit-distance promotes true structural siblings and cross-implementation twins** (`MockTransport.handle_request`, the `_send_handling_redirects` pair, `escape`/`unescape` loop bodies) but can hand rank 1 to "structurally similar, semantically different" code (`InnerLiterals::one_regex` over `tokens_to_regex`), and it is the expensive signal.
- **The all-signals fusion was cleaner than any single addition in all three trials**: RRF's consensus keeps rows two or more signals agree on and suppresses each signal's private noise, and it never displaced a true twin from rank 1.

**Verdict: the remaining signals demonstrate real further lift, led by graph-interaction, with signature-shape the most droop-prone and edit-distance the most expensive.**
They still land as a follow-up change: each needs production design (signature tokenization, per-candidate graph-neighborhood queries with hub damping, banded edit distance in Rust with an early-exit bound), and the two-signal floor plus clone tier already serves the story.
The follow-up now starts from per-signal evidence, not conjecture.

## Round-1/round-2 comparability note (2026-08-11)

The containment fix changed the Python corpus under the round-1 results (bodies were missing from parameterized functions), so both round-1 Python queries were re-run on the fixed corpus: Flask's `before_request` query still ranks `Scaffold.before_request` first, and httpx2's chunked-decode query holds `Response._get_content_decoder` at rank 4 (was 1 — bodies dilute the docstring signal on that query, still top-5).
The bug touched the semantic corpus only — `contains` edges were derived identically throughout — so no other verb's dogfood is affected.
The Rust corpora also shrank under the name-only exclusion (ripgrep 5,092 → 3,354): the excluded rows are unit enum variants, field names, and bodiless trait-method declarations, whose persisted content was already just the name token — nothing content-bearing was lost, and name lookup for such symbols is `find`'s job.

## Review remediation (2026-08-12)

An external review bot raised six findings; each was probed before acceptance.

- **Tokenizer truncation (CONFIRMED, fixed).**
  Probe: two ~600-word texts differing only in the tail embedded bit-identically — the exported `tokenizer.json` serializes a 512-token truncation the model card contradicts (its stated maximum is 1,000,000 tokens), and the tokenizer applies it inside every encode.
  Content past token 512 could never affect a vector, falsifying the no-token-limit contract; the lexical index was unaffected (it gets the full split render).
  Fix: the loader removes the truncation config before constructing the tokenizer; the probe is now the tail-sensitivity regression.
- **Equal-span container leak (CONFIRMED, fixed).**
  Probe: a file module whose span exactly equals its sole declaration's (a file with no trailing newline) classified as a leaf and its render carried the child's body verbatim.
  Two defects: span-value comparison defeated containment at equal spans, and the whole-document module span derived a fabricated `contains` edge from whichever declaration opens the document (the symmetric case of the empty-span guard).
  Fix: containment compares identities with equal spans conferring containment only on the module side, and a whole-document span derives no enclosure parent; the probe is now the equal-span regression.
- **Clone-key ordinal quadratic scan (accepted).**
  Replaced the linear scan with a hash-map ordinal; behavior identical.
- **Model licensing/provenance, merged with the decision to keep weights out of git (accepted, reshaped).**
  The payload directory is now gitignored (it was never committed); the repo carries a manifest (upstream, pinned revision, per-file SHA-256, license) and the upstream MIT text; `build.rs` fetches missing files from the pinned revision and verifies every file against the manifest on every build; the README credits the model.
- **Estimation marker on non-found human renders (VALID IN PART, resolved by user ruling).**
  Machine answers carried `classification: "estimation"` on ambiguous/absent while their human renders said nothing; the empty answer's scoped wording satisfies its scenario.
  The user ruled for the `tests`-relation carve-out: a resolution refusal terminates before any ranking is derived, so it carries neither the marker nor the semantic-index provenance.
  The delta requirement gained the carve-out clause and two scenarios, the engine dropped the marker from the ambiguous and unresolved paths, and both arms are pinned by machine-level tests — machine and human answers now agree on every outcome.
- **O(corpus) hydration before pagination (confirmed by construction, deferred).**
  The paging contract needs the full ordering but only the returned page's rows need hydration; recorded as a candidate in the `internal-efficiency` stub, not this change.

## Verify remediation (2026-08-12)

The sdd-verify run found three CRITICALs — all evidence gaps on write-sites of the semantic-labeling SHALL, with the code correct on inspection — plus five warnings.
Remediated with test evidence, spec scenarios, and design documentation; no production code changed:

- Unresolved-position subject (`similar --at` over an unknown document): now tested through the command handler — typed absence, no marker, no provenance.
- No-corpus-entry subject: an external subject over a non-empty corpus now proves the no-representation return specifically (a ranking pass would have produced found rows, so the empty answer identifies the site).
- Pagination: the marker and provenance are asserted on the truncated first page and the resumed page; the identity-carrying rebuild is now evidenced for both fields.
- The token-overlap clause gained direct evidence: a query sharing no token with any render still returns every corpus symbol (membership, the clause as written; ranking quality on vocabulary-disjoint queries remains the dogfood watch item).
- The staleness-composition test now exercises `similar`, matching its scenario.
- Clone-key supersession is asserted in the edited-symbol test; four unsampled clauses (contributor exactly-once, corpus determinism, failed-build arm, key supersession) gained delta scenarios.
- The 4096-candidate KNN ceiling is documented in the storage decision.
- Open at commit time: `models/potion-code-16M-v2.manifest.json` and `models/potion-code-16M-v2.LICENSE` are untracked and MUST ride the feat commit — a tree without the manifest cannot build.
