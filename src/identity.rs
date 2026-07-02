//! Canonical symbol identity.
//!
//! A canonical identity is a deterministic, workspace-namespaced projection of a symbol's resolved
//! semantic descriptor, of the conceptual form `<workspace>::<module_path>::<qualname>[#disambiguator]`.
//! It is the join key across the syntax and semantic oracles and the machine round-trip key exposed
//! to consumers.
//!
//! The projection is a pure function of the workspace identity and the resolved descriptor, so
//! re-indexing unchanged sources yields byte-identical identities and discovery order is irrelevant.
//! A disambiguator is appended only when a within-workspace collision is actually observed
//! (parse-don't-validate: respond to an observed collision, not a hypothetical one). Rust
//! descriptors do not collide and therefore carry no suffix.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Identity of the workspace a symbol was indexed under.
///
/// Namespacing every identity with its workspace keeps identical descriptors from two different
/// workspaces distinct, so the multi-repo management surface is never designed out.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    /// Construct a workspace identity from an owned or borrowed string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The workspace identity as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The role a descriptor segment plays, mirroring the SCIP descriptor suffix taxonomy but kept
/// engine-neutral so a future non-SCIP backend produces comparable keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SegmentKind {
    /// A namespace or module (package) path element.
    Module,
    /// A type.
    Type,
    /// A term (value, constant, field).
    Term,
    /// A method.
    Method,
    /// A type parameter.
    TypeParameter,
    /// A macro.
    Macro,
    /// A metadata segment.
    Meta,
}

/// One segment of a resolved descriptor: a name and the role it plays in the path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DescriptorSegment {
    /// The segment's name token (its terminal spelling in source).
    pub name: String,
    /// The role the segment plays.
    pub kind: SegmentKind,
}

impl DescriptorSegment {
    /// Construct a descriptor segment.
    pub fn new(name: impl Into<String>, kind: SegmentKind) -> Self {
        Self {
            name: name.into(),
            kind,
        }
    }
}

/// A resolved, engine-neutral descriptor: the ordered path of segments a semantic backend resolved
/// for a global symbol.
///
/// Only global symbols have descriptors here; file-local symbols (parameters, let-bindings) carry
/// no global descriptor and are outside the projection's domain — they are represented as `None`
/// by the backend and excluded from the persisted base.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Descriptor {
    /// The package the symbol belongs to (a stable name independent of the workspace path).
    pub package: String,
    /// The ordered descriptor segments, outermost first.
    pub segments: Vec<DescriptorSegment>,
}

impl Descriptor {
    /// Construct a descriptor from a package and its ordered segments.
    pub fn new(package: impl Into<String>, segments: Vec<DescriptorSegment>) -> Self {
        Self {
            package: package.into(),
            segments,
        }
    }

    /// The terminal name segment — the name token the join's text-equality guard compares against.
    ///
    /// A SCIP occurrence range covers the name token, not the qualified path, so the guard target is
    /// the last segment's name. Returns `None` for a descriptor with no segments (degenerate input).
    pub fn terminal_name(&self) -> Option<&str> {
        self.segments.last().map(|s| s.name.as_str())
    }

    /// The base (pre-disambiguation) projection body: package and segment names joined by `::`.
    fn projection_body(&self) -> String {
        let mut parts = Vec::with_capacity(self.segments.len() + 1);
        parts.push(self.package.as_str());
        for seg in &self.segments {
            parts.push(seg.name.as_str());
        }
        parts.join("::")
    }
}

/// A canonical, workspace-namespaced symbol identity.
///
/// The wrapped string is the source of truth for round-tripping and joining; a human-readable name
/// is rendered separately by the query layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CanonicalId(String);

impl CanonicalId {
    /// Wrap an already-formed canonical identity string (e.g. one read back from the store or
    /// supplied by a consumer for resolution).
    pub fn from_raw(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// The identity as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CanonicalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The base projection of a single descriptor under a workspace, before collision-only
/// disambiguation. Deterministic and independent of any other symbol.
fn base_projection(workspace: &WorkspaceId, descriptor: &Descriptor) -> String {
    format!("{}::{}", workspace.as_str(), descriptor.projection_body())
}

/// Project a set of resolved descriptors to canonical identities within one workspace, applying
/// collision-only disambiguation.
///
/// The input order does not affect any resulting identity: descriptors that share a base projection
/// are disambiguated in a canonical (sorted) order, so the assignment is a pure function of the
/// descriptor set. A disambiguator is appended only to members of a colliding group; a descriptor
/// whose base projection is unique carries no suffix.
///
/// Returns the identity for each input descriptor, in the same order as the input.
pub fn project_all(workspace: &WorkspaceId, descriptors: &[Descriptor]) -> Vec<CanonicalId> {
    // Group input indices by their base projection.
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, descriptor) in descriptors.iter().enumerate() {
        groups
            .entry(base_projection(workspace, descriptor))
            .or_default()
            .push(idx);
    }

    let mut out: Vec<Option<CanonicalId>> = vec![None; descriptors.len()];
    for (base, mut members) in groups {
        if members.len() == 1 {
            // No observed collision: emit the base projection with no suffix.
            out[members[0]] = Some(CanonicalId(base));
            continue;
        }
        // Observed collision: disambiguate in a canonical order so the assignment is
        // order-independent. Members are ranked by full descriptor content first, then by original
        // index to keep true duplicates stable.
        members.sort_by(|&a, &b| descriptors[a].cmp(&descriptors[b]).then(a.cmp(&b)));
        for (rank, &idx) in members.iter().enumerate() {
            out[idx] = Some(CanonicalId(format!("{base}#{rank}")));
        }
    }

    out.into_iter().map(|id| id.expect("every index assigned")).collect()
}

/// Project a single descriptor with no collision context. Used where the caller has already
/// established uniqueness (e.g. resolving a supplied identity); it yields the base projection.
pub fn project_one(workspace: &WorkspaceId, descriptor: &Descriptor) -> CanonicalId {
    CanonicalId(base_projection(workspace, descriptor))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn seg(name: &str, kind: SegmentKind) -> DescriptorSegment {
        DescriptorSegment::new(name, kind)
    }

    /// A small corpus of distinct Rust-shaped descriptors (no overloading, so none collide).
    fn rust_corpus() -> Vec<Descriptor> {
        vec![
            Descriptor::new(
                "crate",
                vec![
                    seg("net", SegmentKind::Module),
                    seg("Client", SegmentKind::Type),
                    seg("connect", SegmentKind::Method),
                ],
            ),
            Descriptor::new(
                "crate",
                vec![
                    seg("net", SegmentKind::Module),
                    seg("Client", SegmentKind::Type),
                    seg("disconnect", SegmentKind::Method),
                ],
            ),
            Descriptor::new(
                "crate",
                vec![seg("util", SegmentKind::Module), seg("parse", SegmentKind::Term)],
            ),
            Descriptor::new("crate", vec![seg("net", SegmentKind::Module)]),
        ]
    }

    // _(Deterministic canonical identity)_
    #[test]
    fn reprojection_is_byte_identical() {
        let ws = WorkspaceId::new("ws");
        let corpus = rust_corpus();
        let first = project_all(&ws, &corpus);
        let second = project_all(&ws, &corpus);
        assert_eq!(first, second);
    }

    // _(Deterministic canonical identity)_ — order independence.
    #[test]
    fn projection_is_order_independent() {
        let ws = WorkspaceId::new("ws");
        let corpus = rust_corpus();

        let forward = project_all(&ws, &corpus);

        // Visit the same descriptors in reverse order and map each identity back to its descriptor.
        let mut reversed: Vec<Descriptor> = corpus.clone();
        reversed.reverse();
        let reverse_ids = project_all(&ws, &reversed);

        for (i, descriptor) in corpus.iter().enumerate() {
            let rev_pos = reversed.iter().position(|d| d == descriptor).unwrap();
            assert_eq!(
                forward[i], reverse_ids[rev_pos],
                "identity for {descriptor:?} changed with visitation order"
            );
        }
    }

    // _(Workspace-namespaced identity)_
    #[test]
    fn identical_descriptors_in_two_workspaces_are_distinct() {
        let descriptor = Descriptor::new(
            "crate",
            vec![seg("net", SegmentKind::Module), seg("Client", SegmentKind::Type)],
        );
        let a = project_one(&WorkspaceId::new("workspace-a"), &descriptor);
        let b = project_one(&WorkspaceId::new("workspace-b"), &descriptor);
        assert_ne!(a, b);
    }

    // _(Identity uniqueness within a workspace)_
    #[test]
    fn distinct_symbols_yield_no_duplicate_identities() {
        let ws = WorkspaceId::new("ws");
        let corpus = rust_corpus();
        let ids = project_all(&ws, &corpus);
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), corpus.len(), "identities are not unique: {ids:?}");
    }

    // _(Identity uniqueness within a workspace)_ — no vacuous suffix for non-colliding Rust symbols.
    #[test]
    fn rust_symbols_carry_no_disambiguator_suffix() {
        let ws = WorkspaceId::new("ws");
        let corpus = rust_corpus();
        let ids = project_all(&ws, &corpus);
        for id in &ids {
            assert!(
                !id.as_str().contains('#'),
                "non-colliding Rust symbol carries a disambiguator: {id}"
            );
        }
    }

    // _(Identity uniqueness within a workspace)_ — disambiguation is detected, not assumed.
    #[test]
    fn observed_collision_is_disambiguated() {
        let ws = WorkspaceId::new("ws");
        // Two distinct descriptors sharing the same base body `crate::m::f`, differing only by the
        // segment kind the base projection does not encode (a synthetic overloading case). This is
        // the collision the disambiguator exists for.
        let as_term = Descriptor::new(
            "crate",
            vec![seg("m", SegmentKind::Module), seg("f", SegmentKind::Term)],
        );
        let as_method = Descriptor::new(
            "crate",
            vec![seg("m", SegmentKind::Module), seg("f", SegmentKind::Method)],
        );
        assert_ne!(as_term, as_method, "test setup: descriptors must be distinct");

        let ids = project_all(&ws, &[as_term, as_method]);
        assert_ne!(ids[0], ids[1], "colliding descriptors were not disambiguated");
        assert!(
            ids.iter().all(|id| id.as_str().contains('#')),
            "disambiguator not emitted: {ids:?}"
        );
    }
}
