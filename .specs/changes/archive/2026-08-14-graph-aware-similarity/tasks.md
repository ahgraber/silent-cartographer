# Tasks: graph-aware-similarity

> Every task below was completed and then reverted; see the status note in `proposal.md`.
> The checkmarks record that the work was done, not that it ships.

## Graph-signal store queries (code-navigation)

- [x] Implement the set-based store queries the signal needs: the subject's one-hop dependency neighborhood (`uses`/`imports`/`type_hierarchy`, both directions); the distance-2 candidates with their shared neighbors and each shared neighbor's degree, in one aggregation; and the surviving candidates' neighborhood sizes, in one aggregation — never a per-candidate query loop.
- [x] Write store-level tests over a fixture graph: the neighborhood is direction-inclusive; a distance-2 symbol appears as a candidate with the correct shared neighbors and degrees; a symbol with no shared neighbor does not appear.

## Similarity ranking (code-navigation)

- [x] Implement the damped-overlap score in Rust — Resource Allocation numerator over Jaccard union denominator — summed over rows fetched in a fixed order, ranked descending with canonical-identity tie-break, emitted as the graph rank list (candidates with zero overlap absent).
- [x] Fuse the graph rank list into `similar`'s existing reciprocal-rank fusion as a third list, leaving the clone-certainty tier, subject exclusion, and detail projection untouched.
- [x] Update the fused-order wiring test to the three-list composition: the answer's order must equal the fusion recomputed from the store's three rank lists. (The two-list assertion is obsoleted by this change's ordering clause; the update is the evidence the third list is actually fused.)
- [x] Write the test for the structurally-adjacent scenario: two candidates with identical content similarity (consistently renamed copies of one source), one sharing the subject's callers and dependencies in the fixture graph — the sharing candidate precedes the other.
- [x] Write the test for the isolated-candidate scenario: a content-similar candidate with no dependency edges still appears in the answer.
- [x] Write the test for the ubiquity scenario: a fixture symbol referenced by every other symbol, with content unlike the subject's, does not precede the content-similar candidates.
- [x] Write the test for the near-copy-wired-elsewhere scenario: a near-copy sharing none of the subject's dependency relationships precedes structurally-adjacent but content-dissimilar candidates.
- [x] Write the determinism test: two identical stores yield byte-identical `similar` answers for the same subject (fixed fetch order and identity tie-breaks under floating-point summation).

## Continuation-token version binding (code-navigation)

- [x] Add the similarity-ranking version constant, starting at 1, sealed into the `similar` page identity through the same sealed-pair mechanism the dependents ordering uses, and thread it through the `similar` command's identity construction.
- [x] Write the refusal test: a well-formed token hashed under a different similarity-ranking version is refused as a usage error naming the recovery, on the `similar` pagination path.
- [x] Write the sealed-pair hash test: two `similar` page identities differing only in the similarity-ranking version hash differently.

## Documentation

- [x] Update the README's `similar` section: candidates rank on content and on dependency-graph position; ubiquity alone never ranks; the estimation marker and clone markers are unchanged.

## Dogfood

- [x] Rebuild the persistent dogfood clones and probe the three consolidation shapes over them: a co-called sibling family enters the answer's head; a cross-implementation twin stays at rank 1; a hub symbol stays out of heads it does not belong in (both as subject and as candidate).
  Record hits and misses in the change notes.
- [x] Run the pre-registered A/B: the chosen score against the bare Resource Allocation numerator (no union denominator), judged on the hub probes; record which form the evidence keeps and fold the outcome into the DegreeDampedOverlap decision.
