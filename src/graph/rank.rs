//! Global PageRank over the rank graph projection: the codebase-wide structural-importance score
//! that orders dependents within a distance layer under the `ranked` ordering.
//!
//! The score is a heuristic proxy for how load-bearing a symbol is — mass flows from dependent to
//! dependency, so widely-depended-upon symbols accumulate rank transitively. Scores are internal:
//! they order rows and are then discarded, never serialized into any answer.

use super::store::RankProjection;

/// The ranking model's version, bound into every continuation token issued for an orderable answer
/// so a token never resumes across a scoring change.
///
/// It covers the whole model: the algorithm (global PageRank by power iteration), the projection
/// (in-workspace symbols; `uses`/`imports`/`type_hierarchy` edges collapsed per pair), the damping
/// factor, the iteration count, and the uniform edge weights. Changing any of them is a
/// retrieval-scoring change under the repository's governance rule: bump this constant and record a
/// migration note in the changelog.
pub const RANK_VERSION: u32 = 1;

/// The damping factor: the standard value from the PageRank literature.
const DAMPING: f64 = 0.85;

/// The fixed iteration count. Fixed rather than convergence-thresholded so the computation is
/// deterministic by construction; 40 iterations bounds the residual near 0.85⁴⁰ ≈ 1.5×10⁻³, far
/// below the separations that make one symbol observably more load-bearing than another. Near-equal
/// scores order deterministically through the comparator's trailing keys, and no score is ever
/// published, so residual numerical noise is never presented as a meaningful distinction.
const ITERATIONS: u32 = 40;

/// Compute the global PageRank of every projected node, returned aligned with `projection.nodes`.
///
/// Uniform teleport, uniform edge weights, dangling mass redistributed uniformly. Determinism by
/// construction: the projection's nodes and edges arrive in canonical-identity order and are
/// visited in that order, so floating-point summation order is fixed and identical inputs produce
/// bitwise-identical score vectors.
pub fn global_rank(projection: &RankProjection) -> Vec<f64> {
    let n = projection.nodes.len();
    if n == 0 {
        return Vec::new();
    }

    let mut out_degree = vec![0u64; n];
    for &(src, _) in &projection.edges {
        out_degree[src] += 1;
    }

    let uniform = 1.0 / n as f64;
    let mut rank = vec![uniform; n];
    for _ in 0..ITERATIONS {
        // Dangling nodes (no outgoing projected edge) redistribute their mass uniformly; the sum
        // accumulates in node order.
        let mut dangling = 0.0;
        for (i, &degree) in out_degree.iter().enumerate() {
            if degree == 0 {
                dangling += rank[i];
            }
        }
        let base = (1.0 - DAMPING) * uniform + DAMPING * dangling * uniform;
        let mut next = vec![base; n];
        // Edges are ordered by (src, dst) canonical identity, so each destination's contributions
        // accumulate in a fixed order.
        for &(src, dst) in &projection.edges {
            next[dst] += DAMPING * rank[src] / out_degree[src] as f64;
        }
        rank = next;
    }
    rank
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::CanonicalId;

    /// A projection over nodes named by letter, with edges given as `(src, dst)` names.
    fn projection(nodes: &[&str], edges: &[(&str, &str)]) -> RankProjection {
        let mut names: Vec<&str> = nodes.to_vec();
        names.sort_unstable();
        let index_of = |name: &str| names.iter().position(|n| *n == name).expect("edge names a node");
        let mut idx_edges: Vec<(usize, usize)> = edges.iter().map(|(s, d)| (index_of(s), index_of(d))).collect();
        idx_edges.sort_unstable();
        RankProjection {
            nodes: names.iter().map(|n| CanonicalId::from_raw((*n).to_string())).collect(),
            edges: idx_edges,
        }
    }

    fn rank_of(projection: &RankProjection, name: &str) -> f64 {
        let scores = global_rank(projection);
        let i = projection
            .nodes
            .iter()
            .position(|n| n.as_str() == name)
            .expect("named node is projected");
        scores[i]
    }

    // A hub many symbols depend on accumulates more rank than a leaf nothing depends on.
    #[test]
    fn a_hub_outranks_a_leaf() {
        let p = projection(
            &["hub", "leaf", "u1", "u2", "u3"],
            &[("u1", "hub"), ("u2", "hub"), ("u3", "hub")],
        );
        assert!(rank_of(&p, "hub") > rank_of(&p, "leaf"));
    }

    // Transitive weight: a symbol used by two hubs outranks one used by three leaves, because the
    // hubs' own accumulated rank flows onward to what they depend on.
    #[test]
    fn a_symbol_used_by_two_hubs_outranks_one_used_by_three_leaves() {
        let mut edges: Vec<(&str, &str)> = vec![("h1", "x"), ("h2", "x"), ("l1", "y"), ("l2", "y"), ("l3", "y")];
        // Each hub is itself depended upon by four callers; the leaves by nothing.
        let callers = ["c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8"];
        for (i, c) in callers.iter().enumerate() {
            edges.push((c, if i < 4 { "h1" } else { "h2" }));
        }
        let mut nodes = vec!["x", "y", "h1", "h2", "l1", "l2", "l3"];
        nodes.extend(callers);
        let p = projection(&nodes, &edges);
        assert!(rank_of(&p, "x") > rank_of(&p, "y"));
    }

    // Determinism: two runs over the same projection produce bitwise-identical score vectors.
    #[test]
    fn identical_inputs_produce_bitwise_identical_scores() {
        let p = projection(
            &["a", "b", "c", "d", "isolated"],
            &[("a", "b"), ("b", "c"), ("c", "a"), ("d", "a"), ("d", "b")],
        );
        let first = global_rank(&p);
        let second = global_rank(&p);
        let first_bits: Vec<u64> = first.iter().map(|f| f.to_bits()).collect();
        let second_bits: Vec<u64> = second.iter().map(|f| f.to_bits()).collect();
        assert_eq!(first_bits, second_bits);
    }

    // An empty projection ranks nothing rather than dividing by zero.
    #[test]
    fn an_empty_projection_yields_no_scores() {
        let p = RankProjection {
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        assert!(global_rank(&p).is_empty());
    }
}
