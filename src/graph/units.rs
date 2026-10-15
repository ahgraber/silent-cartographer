//! The build unit each declaration belongs to, derived from the target inventory and the `mod`
//! chain.
//!
//! A document belongs to every target whose root reaches it through `mod` declarations. Units are
//! applied to the identities the build projects, never to the index's own descriptors, so twin
//! groups stay keyed by the backend's symbol.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::identity::{Descriptor, UnitMarker};
use crate::semantic::model::{
    BuildTarget, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, InventoryFailure, OccurrenceRole, SymbolClass,
    SymbolKind, TargetInventory,
};

use super::prepared::{PreparedCorpus, PreparedDocument};
use super::range::{ByteSpan, range_to_span};
use super::syntax::ModuleFileRequest;

/// The build unit a declaration belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unit {
    /// The package's default unit, or a symbol with no unit to speak of (external, no definition).
    Default,
    /// A named unit other than the default unit, or source compiled into several units.
    Marked(UnitMarker),
    /// The unit could not be determined. Its identity carries no unit marker, as for [`Unit::Default`],
    /// and it is never matched to the package's default unit.
    Undetermined,
}

impl Unit {
    /// The marker the unit contributes to a projected identity.
    pub fn unit_marker(&self) -> Option<UnitMarker> {
        match self {
            Unit::Default | Unit::Undetermined => None,
            Unit::Marked(marker) => Some(marker.clone()),
        }
    }
}

/// A package whose targets could not be listed, so its declarations carry no unit marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndeterminedPackage {
    pub package: String,
    /// The cause, serialized as `reason` (and `detail` where the cause has one).
    #[serde(flatten)]
    pub cause: InventoryFailure,
}

/// Why a document is listed as belonging to no target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentReason {
    /// No `mod` declaration reachable from any target root loads the document.
    NoTargetLoadsIt,
}

/// A document no target loads, so its declarations carry no unit marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndeterminedDocument {
    pub document_path: String,
    pub reason: DocumentReason,
}

/// The declarations whose unit could not be determined, for the build answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndeterminedUnits {
    /// Packages with in-workspace declarations whose target list was unavailable, sorted by name.
    pub packages: Vec<UndeterminedPackage>,
    /// Documents reached from no target root, sorted by path.
    pub documents: Vec<UndeterminedDocument>,
}

/// For each document, the documents holding a `mod` declaration that loads it.
pub type DeclaredFrom = HashMap<String, BTreeSet<String>>;

/// The `mod`-declaration evidence in `index`: for every in-workspace module symbol, its definition
/// document maps to the documents containing a declaration of it (`mod name;` or `mod name { .. }`).
///
/// A module symbol is referenced from every `use` and path segment naming it, so a reference counts
/// as declaration evidence only when its span is the name token of a `mod` item in the parsed
/// document.
///
/// When several modules share one descriptor the index holds their references in a duplicate group,
/// not on the modules. A `mod name;` declaration in such a group is matched to the module whose
/// definition document is the file the declaration asks for: the `#[path]` attribute's file, or the
/// sibling `name.rs` / `name/mod.rs` under the declaring file's module directory. A declaration that
/// fits no module, or fits more than one, is no evidence.
pub fn declared_from(index: &ExtractedIndex, prepared: &PreparedCorpus) -> DeclaredFrom {
    let mut declared_from = DeclaredFrom::new();
    for symbol in &index.symbols {
        if symbol.kind != SymbolKind::Module {
            continue;
        }
        let Some(def) = symbol.definition() else {
            continue;
        };
        for occ in &symbol.occurrences {
            if declaration_name_span(index, prepared, occ).is_some() {
                declared_from
                    .entry(def.document_path.clone())
                    .or_default()
                    .insert(occ.document_path.clone());
            }
        }
    }
    add_duplicate_module_declarations(index, prepared, &mut declared_from);
    declared_from
}

/// The prepared document and name span of `occ` when it is a reference at the name token of a `mod`
/// item.
fn declaration_name_span<'a>(
    index: &ExtractedIndex,
    prepared: &'a PreparedCorpus,
    occ: &ExtractedOccurrence,
) -> Option<(&'a PreparedDocument, ByteSpan)> {
    if occ.role != OccurrenceRole::Reference {
        return None;
    }
    let doc = prepared.get(occ.document_path.as_str())?;
    let encoding = index.encoding_for(&occ.document_path)?;
    let span = range_to_span(doc.tree.source(), &doc.line_index, occ.range, encoding)?;
    doc.tree.is_module_declaration_name(span).then_some((doc, span))
}

/// Add the declarations held in duplicate groups of modules (see [`declared_from`]).
fn add_duplicate_module_declarations(
    index: &ExtractedIndex,
    prepared: &PreparedCorpus,
    declared_from: &mut DeclaredFrom,
) {
    let mut definition_documents: HashMap<&Descriptor, Vec<&str>> = HashMap::new();
    for symbol in &index.symbols {
        if symbol.kind != SymbolKind::Module {
            continue;
        }
        if let (Some(descriptor), Some(def)) = (&symbol.descriptor, symbol.definition()) {
            definition_documents
                .entry(descriptor)
                .or_default()
                .push(def.document_path.as_str());
        }
    }
    let roots: HashSet<&str> = match &index.targets {
        TargetInventory::Available { targets, .. } => targets.iter().map(|target| target.root.as_str()).collect(),
        _ => HashSet::new(),
    };
    for group in &index.duplicate_groups {
        let Some(documents) = definition_documents.get(&group.descriptor) else {
            continue;
        };
        for occ in &group.occurrences {
            let Some((doc, span)) = declaration_name_span(index, prepared, occ) else {
                continue;
            };
            let Some(request) = doc.tree.module_file_request(span) else {
                continue;
            };
            let Some(wanted) = requested_files(&occ.document_path, &request, &roots) else {
                continue;
            };
            let loaded = documents
                .iter()
                .filter(|document| wanted.iter().any(|file| file == *document));
            if let Some(document) = super::exactly_one(loaded) {
                declared_from
                    .entry((*document).to_string())
                    .or_default()
                    .insert(occ.document_path.clone());
            }
        }
    }
}

/// The files a `mod name;` declaration in `declaring` may load, or `None` when they cannot be told.
///
/// With a `#[path]` attribute the file is that path relative to the declaring file's directory.
/// Otherwise it is `name.rs` or `name/mod.rs` in the declaring file's module directory, extended by
/// the enclosing inline modules. A crate root, `lib.rs`, `main.rs` and `mod.rs` keep their children
/// beside them; any other file `a.rs` keeps them in `a/`. An attribute under an inline module has
/// different base rules and is not interpreted.
fn requested_files(declaring: &str, request: &ModuleFileRequest, roots: &HashSet<&str>) -> Option<Vec<String>> {
    let (directory, file) = declaring.rsplit_once('/').unwrap_or(("", declaring));
    if let Some(path) = &request.path_attribute {
        if !request.inline_parents.is_empty() {
            return None;
        }
        return Some(vec![join_path(directory, path)?]);
    }
    let mut base = if matches!(file, "lib.rs" | "main.rs" | "mod.rs") || roots.contains(declaring) {
        directory.to_string()
    } else {
        join_path(directory, file.strip_suffix(".rs")?)?
    };
    for parent in &request.inline_parents {
        base = join_path(&base, parent)?;
    }
    Some(vec![
        join_path(&base, &format!("{}.rs", request.name))?,
        join_path(&base, &format!("{}/mod.rs", request.name))?,
    ])
}

/// `relative` under `base`, with `.` and `..` components resolved; `None` when it climbs above the
/// workspace root.
fn join_path(base: &str, relative: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// The declaring document of every document that exactly one document declares.
///
/// A document declared by zero documents (a crate root) or by several (conflicting evidence) has no
/// entry: a walk up the module chain stops there.
pub fn single_parent_map(declared_from: &DeclaredFrom) -> HashMap<String, String> {
    declared_from
        .iter()
        .filter_map(|(document, declaring)| {
            super::exactly_one(declaring).map(|parent| (document.clone(), parent.clone()))
        })
        .collect()
}

/// One unit per `index.symbols` entry, and the disclosure of every unit that could not be
/// determined.
///
/// A symbol that is in-workspace and defined belongs to the targets of its package whose root
/// reaches its definition document through `mod` declarations: one target gives that target's unit
/// (the default unit for a library kind), several give [`UnitMarker::Shared`], none gives
/// [`Unit::Undetermined`]. When the inventory is unavailable every such symbol is undetermined.
/// Every other symbol, and every symbol of a language with no inventory, is [`Unit::Default`].
pub fn assign_units(index: &ExtractedIndex, declared_from: &DeclaredFrom) -> (Vec<Unit>, UndeterminedUnits) {
    match &index.targets {
        TargetInventory::NotApplicable => (vec![Unit::Default; index.symbols.len()], UndeterminedUnits::default()),
        TargetInventory::Unavailable(failure) => units_without_inventory(index, failure),
        TargetInventory::Available { targets, .. } => units_from_targets(index, targets, declared_from),
    }
}

/// The `(package, definition document)` of an in-workspace symbol that has a definition: the only
/// symbols that belong to a unit.
fn unit_site(symbol: &ExtractedSymbol) -> Option<(&str, &str)> {
    if symbol.class != SymbolClass::InWorkspace {
        return None;
    }
    let package = symbol.descriptor.as_ref()?.package.as_str();
    Some((package, symbol.definition()?.document_path.as_str()))
}

fn units_without_inventory(index: &ExtractedIndex, failure: &InventoryFailure) -> (Vec<Unit>, UndeterminedUnits) {
    let mut packages = BTreeSet::new();
    let units = index
        .symbols
        .iter()
        .map(|symbol| match unit_site(symbol) {
            Some((package, _)) => {
                packages.insert(package);
                Unit::Undetermined
            }
            None => Unit::Default,
        })
        .collect();
    let disclosure = UndeterminedUnits {
        packages: packages
            .into_iter()
            .map(|package| UndeterminedPackage {
                package: package.to_string(),
                cause: failure.clone(),
            })
            .collect(),
        documents: Vec::new(),
    };
    (units, disclosure)
}

/// The unit of the declarations only `target` compiles.
fn unit_of_target(target: &BuildTarget) -> Unit {
    if target.is_library() {
        Unit::Default
    } else {
        Unit::Marked(UnitMarker::Unit {
            kind: target.kind.clone(),
            name: target.name.clone(),
        })
    }
}

/// The unit of every document that exactly one target reaches, by the same reach that decides
/// declaration units. A document reached by several targets, or by none, has no entry; so does every
/// document when the inventory is unavailable.
pub fn document_units(index: &ExtractedIndex, declared_from: &DeclaredFrom) -> HashMap<String, Unit> {
    let TargetInventory::Available { targets, .. } = &index.targets else {
        return HashMap::new();
    };
    let mut reaching: HashMap<&str, Vec<&BuildTarget>> = HashMap::new();
    for ((_, document), reached_by) in target_reach(targets, declared_from) {
        reaching.entry(document).or_default().extend(reached_by);
    }
    reaching
        .into_iter()
        .filter_map(|(document, targets)| match targets.as_slice() {
            [target] => Some((document.to_string(), unit_of_target(target))),
            _ => None,
        })
        .collect()
}

fn units_from_targets(
    index: &ExtractedIndex,
    targets: &[BuildTarget],
    declared_from: &DeclaredFrom,
) -> (Vec<Unit>, UndeterminedUnits) {
    let reach = target_reach(targets, declared_from);
    let mut orphans = BTreeSet::new();
    let units = index
        .symbols
        .iter()
        .map(|symbol| {
            let Some((package, document)) = unit_site(symbol) else {
                return Unit::Default;
            };
            match reach.get(&(package, document)).map(Vec::as_slice) {
                Some([target]) => unit_of_target(target),
                Some([_, _, ..]) => Unit::Marked(UnitMarker::Shared),
                _ => {
                    orphans.insert(document);
                    Unit::Undetermined
                }
            }
        })
        .collect();
    let disclosure = UndeterminedUnits {
        packages: Vec::new(),
        documents: orphans
            .into_iter()
            .map(|document| UndeterminedDocument {
                document_path: document.to_string(),
                reason: DocumentReason::NoTargetLoadsIt,
            })
            .collect(),
    };
    (units, disclosure)
}

/// The targets of each package that reach each document, by walking the `mod` declarations down
/// from every target root.
fn target_reach<'a>(
    targets: &'a [BuildTarget],
    declared_from: &'a DeclaredFrom,
) -> BTreeMap<(&'a str, &'a str), Vec<&'a BuildTarget>> {
    let mut declares: HashMap<&str, Vec<&str>> = HashMap::new();
    for (document, declaring) in declared_from {
        for parent in declaring {
            declares.entry(parent.as_str()).or_default().push(document.as_str());
        }
    }
    let mut reach: BTreeMap<(&str, &str), Vec<&BuildTarget>> = BTreeMap::new();
    for target in targets {
        // The visited set also ends a cycle of `mod` declarations.
        let mut visited: HashSet<&str> = HashSet::new();
        let mut pending = vec![target.root.as_str()];
        while let Some(document) = pending.pop() {
            if !visited.insert(document) {
                continue;
            }
            reach
                .entry((target.package.as_str(), document))
                .or_default()
                .push(target);
            pending.extend(declares.get(document).into_iter().flatten().copied());
        }
    }
    reach
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::identity::{Descriptor, DescriptorSegment, SegmentKind};
    use crate::semantic::model::{
        AnalyzerProvenance, ExtractedOccurrence, PositionEncoding, SourceDocument, SourceRange,
    };

    fn target(kind: &str, name: &str, root: &str) -> BuildTarget {
        BuildTarget {
            package: "libbin".to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            root: root.to_string(),
            manifest: "Cargo.toml".to_string(),
        }
    }

    /// An in-workspace type of `libbin` defined in `document`.
    fn symbol_in(document: &str) -> ExtractedSymbol {
        ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "libbin",
                vec![DescriptorSegment::new("Config", SegmentKind::Type)],
            )),
            kind: SymbolKind::Type,
            class: SymbolClass::InWorkspace,
            occurrences: vec![ExtractedOccurrence {
                document_path: document.to_string(),
                range: SourceRange::new(0, 0, 0, 6),
                role: OccurrenceRole::Definition,
            }],
        }
    }

    fn index_with(symbols: Vec<ExtractedSymbol>, targets: TargetInventory) -> ExtractedIndex {
        ExtractedIndex {
            provenance: AnalyzerProvenance {
                analyzer_name: "rust-analyzer".to_string(),
                analyzer_version: "0".to_string(),
            },
            documents: symbols
                .iter()
                .map(|s| SourceDocument {
                    path: s.definition().unwrap().document_path.clone(),
                    encoding: PositionEncoding::Utf8,
                })
                .collect(),
            symbols,
            duplicate_groups: Vec::new(),
            targets,
            environment: None,
        }
    }

    fn declared(edges: &[(&str, &str)]) -> DeclaredFrom {
        let mut declared = DeclaredFrom::new();
        for (document, parent) in edges {
            declared
                .entry(document.to_string())
                .or_default()
                .insert(parent.to_string());
        }
        declared
    }

    fn bin(name: &str) -> Unit {
        Unit::Marked(UnitMarker::Unit {
            kind: "bin".to_string(),
            name: name.to_string(),
        })
    }

    // _(Rust library declarations are unmarked)_
    #[test]
    fn library_root_documents_are_unmarked() {
        let index = index_with(
            vec![symbol_in("src/lib.rs"), symbol_in("src/util.rs")],
            TargetInventory::Available {
                targets: vec![target("lib", "libbin", "src/lib.rs")],
                manifests: Default::default(),
            },
        );
        let (units, disclosure) = assign_units(&index, &declared(&[("src/util.rs", "src/lib.rs")]));
        assert_eq!(units, vec![Unit::Default, Unit::Default]);
        assert_eq!(disclosure, UndeterminedUnits::default());
    }

    // _(Rust binary declarations are marked by kind and name)_
    #[test]
    fn binary_documents_marked_by_kind_and_name() {
        let index = index_with(
            vec![
                symbol_in("src/lib.rs"),
                symbol_in("src/bin/tool.rs"),
                symbol_in("src/cli.rs"),
            ],
            TargetInventory::Available {
                targets: vec![
                    target("lib", "libbin", "src/lib.rs"),
                    target("bin", "tool", "src/bin/tool.rs"),
                ],
                manifests: Default::default(),
            },
        );
        let (units, _) = assign_units(&index, &declared(&[("src/cli.rs", "src/bin/tool.rs")]));
        assert_eq!(units, vec![Unit::Default, bin("tool"), bin("tool")]);
    }

    // _(Default binary with the package's name is distinguished by kind)_
    #[test]
    fn default_binary_marked_by_kind() {
        let index = index_with(
            vec![symbol_in("src/lib.rs"), symbol_in("src/main.rs")],
            TargetInventory::Available {
                targets: vec![
                    target("lib", "libbin", "src/lib.rs"),
                    target("bin", "libbin", "src/main.rs"),
                ],
                manifests: Default::default(),
            },
        );
        let (units, _) = assign_units(&index, &DeclaredFrom::new());
        assert_eq!(units, vec![Unit::Default, bin("libbin")]);
        assert_eq!(
            units[1].unit_marker(),
            Some(UnitMarker::Unit {
                kind: "bin".to_string(),
                name: "libbin".to_string()
            })
        );
    }

    // _(Source compiled into two units has one shared identity)_
    #[test]
    fn document_reached_from_two_roots_is_shared() {
        let index = index_with(
            vec![symbol_in("tests/common/mod.rs")],
            TargetInventory::Available {
                targets: vec![target("test", "a", "tests/a.rs"), target("test", "b", "tests/b.rs")],
                manifests: Default::default(),
            },
        );
        let (units, disclosure) = assign_units(
            &index,
            &declared(&[
                ("tests/common/mod.rs", "tests/a.rs"),
                ("tests/common/mod.rs", "tests/b.rs"),
            ]),
        );
        assert_eq!(units, vec![Unit::Marked(UnitMarker::Shared)]);
        assert_eq!(disclosure, UndeterminedUnits::default());
    }

    // _(File that no unit loads has no marker)_
    #[test]
    fn document_reached_from_no_root_is_disclosed() {
        let index = index_with(
            vec![
                symbol_in("src/lib.rs"),
                symbol_in("src/old.rs"),
                symbol_in("src/old.rs"),
            ],
            TargetInventory::Available {
                targets: vec![target("lib", "libbin", "src/lib.rs")],
                manifests: Default::default(),
            },
        );
        let (units, disclosure) = assign_units(&index, &DeclaredFrom::new());
        assert_eq!(units, vec![Unit::Default, Unit::Undetermined, Unit::Undetermined]);
        assert_eq!(
            disclosure.documents,
            vec![UndeterminedDocument {
                document_path: "src/old.rs".to_string(),
                reason: DocumentReason::NoTargetLoadsIt,
            }],
            "one entry per document"
        );
        assert!(disclosure.packages.is_empty());
    }

    // A cycle of `mod` declarations ends the walk.
    #[test]
    fn module_cycle_terminates() {
        let index = index_with(
            vec![symbol_in("src/lib.rs"), symbol_in("src/a.rs")],
            TargetInventory::Available {
                targets: vec![target("lib", "libbin", "src/lib.rs")],
                manifests: Default::default(),
            },
        );
        let declared = declared(&[("src/a.rs", "src/lib.rs"), ("src/lib.rs", "src/a.rs")]);
        let (units, _) = assign_units(&index, &declared);
        assert_eq!(units, vec![Unit::Default, Unit::Default]);
    }

    // _(Package whose units cannot be listed has no markers)_
    #[test]
    fn missing_inventory_leaves_undetermined_units_and_discloses() {
        let mut external = symbol_in("src/lib.rs");
        external.class = SymbolClass::External;
        let index = index_with(
            vec![symbol_in("src/lib.rs"), symbol_in("src/bin/tool.rs"), external],
            TargetInventory::Unavailable(InventoryFailure::MetadataTimeout),
        );
        let (units, disclosure) = assign_units(&index, &DeclaredFrom::new());
        assert_eq!(units, vec![Unit::Undetermined, Unit::Undetermined, Unit::Default]);
        assert_eq!(
            disclosure.packages,
            vec![UndeterminedPackage {
                package: "libbin".to_string(),
                cause: InventoryFailure::MetadataTimeout,
            }],
            "one entry per package"
        );
        assert!(units.iter().all(|o| o.unit_marker().is_none()));
    }

    // A language with no inventory has no units: every unit is the default and nothing is disclosed.
    #[test]
    fn not_applicable_inventory_owns_everything_by_default() {
        let index = index_with(vec![symbol_in("a.py")], TargetInventory::NotApplicable);
        let (units, disclosure) = assign_units(&index, &DeclaredFrom::new());
        assert_eq!(units, vec![Unit::Default]);
        assert_eq!(disclosure, UndeterminedUnits::default());
    }

    /// An index of two modules `util` (defined in `definitions`) whose only references sit in a
    /// duplicate group at `references` (a `(document, line, column)` of each `util` name token).
    fn duplicate_util_modules(definitions: &[&str], references: &[(&str, u32, u32)]) -> ExtractedIndex {
        let descriptor = Descriptor::new("libbin", vec![DescriptorSegment::new("util", SegmentKind::Module)]);
        let symbols = definitions
            .iter()
            .map(|document| ExtractedSymbol {
                descriptor: Some(descriptor.clone()),
                kind: SymbolKind::Module,
                ..symbol_in(document)
            })
            .collect();
        let mut index = index_with(symbols, TargetInventory::NotApplicable);
        index.duplicate_groups.push(crate::semantic::model::DuplicateGroup {
            descriptor,
            occurrences: references
                .iter()
                .map(|(document, line, column)| ExtractedOccurrence {
                    document_path: document.to_string(),
                    range: SourceRange::new(*line, *column, *line, column + 4),
                    role: OccurrenceRole::Reference,
                })
                .collect(),
        });
        for (document, ..) in references {
            index.documents.push(SourceDocument {
                path: document.to_string(),
                encoding: PositionEncoding::Utf8,
            });
        }
        index
    }

    fn declared_from_sources(index: &ExtractedIndex, sources: &[(&str, &str)]) -> DeclaredFrom {
        let corpus = crate::graph::join::SourceCorpus::new(sources.iter().copied());
        let prepared = PreparedCorpus::prepare(&corpus, crate::graph::syntax::Language::Rust);
        declared_from(index, &prepared)
    }

    // The declarations of two modules that share a descriptor sit in their group; each `mod util;` is
    // matched to the module whose file it loads: the sibling `util.rs` for the library root, and the
    // `#[path]` file for the binary.
    #[test]
    fn duplicate_module_declarations_match_the_file_they_load() {
        let index = duplicate_util_modules(
            &["src/util.rs", "src/tool_util.rs"],
            &[("src/lib.rs", 0, 8), ("src/bin/tool.rs", 1, 4)],
        );
        let declared = declared_from_sources(
            &index,
            &[
                ("src/lib.rs", "pub mod util;\n"),
                ("src/bin/tool.rs", "#[path = \"../tool_util.rs\"]\nmod util;\n"),
                ("src/util.rs", ""),
                ("src/tool_util.rs", ""),
            ],
        );
        assert_eq!(
            declared,
            self::declared(&[("src/util.rs", "src/lib.rs"), ("src/tool_util.rs", "src/bin/tool.rs")])
        );
    }

    // A declaration that asks for a file no module is defined in, or for a file two modules are
    // defined in, is no evidence.
    #[test]
    fn duplicate_module_declaration_without_a_unique_file_is_no_evidence() {
        let index = duplicate_util_modules(&["src/other.rs", "src/other.rs"], &[("src/lib.rs", 0, 8)]);
        let declared = declared_from_sources(&index, &[("src/lib.rs", "pub mod util;\n")]);
        assert!(declared.is_empty(), "{declared:?}");

        let index = duplicate_util_modules(&["src/util.rs", "src/util.rs"], &[("src/lib.rs", 0, 8)]);
        let declared = declared_from_sources(&index, &[("src/lib.rs", "pub mod util;\n")]);
        assert!(declared.is_empty(), "{declared:?}");
    }

    // A module loaded from a non-root file looks in that file's own directory.
    #[test]
    fn requested_files_follow_the_declaring_file() {
        let request = |inline: &[&str]| ModuleFileRequest {
            name: "util".to_string(),
            inline_parents: inline.iter().map(|name| name.to_string()).collect(),
            path_attribute: None,
        };
        let roots: HashSet<&str> = ["src/bin/tool.rs"].into();
        let files = |declaring: &str, request: &ModuleFileRequest| requested_files(declaring, request, &roots);
        assert_eq!(
            files("src/a.rs", &request(&[])),
            Some(vec!["src/a/util.rs".to_string(), "src/a/util/mod.rs".to_string()])
        );
        assert_eq!(
            files("src/bin/tool.rs", &request(&[])),
            Some(vec!["src/bin/util.rs".to_string(), "src/bin/util/mod.rs".to_string()])
        );
        assert_eq!(
            files("src/lib.rs", &request(&["outer"])),
            Some(vec![
                "src/outer/util.rs".to_string(),
                "src/outer/util/mod.rs".to_string()
            ])
        );
    }

    // A document that one target reaches has that target's unit; a document that two targets reach has none.
    #[test]
    fn document_units_cover_only_single_target_documents() {
        let index = index_with(
            vec![symbol_in("src/lib.rs")],
            TargetInventory::Available {
                targets: vec![
                    target("lib", "libbin", "src/lib.rs"),
                    target("bin", "tool", "src/bin/tool.rs"),
                    target("bin", "other", "src/bin/other.rs"),
                ],
                manifests: Default::default(),
            },
        );
        let units = document_units(
            &index,
            &declared(&[
                ("src/caller.rs", "src/lib.rs"),
                ("src/both.rs", "src/bin/tool.rs"),
                ("src/both.rs", "src/bin/other.rs"),
            ]),
        );
        assert_eq!(units.get("src/caller.rs"), Some(&Unit::Default));
        assert_eq!(units.get("src/bin/tool.rs"), Some(&bin("tool")));
        assert_eq!(units.get("src/both.rs"), None);
        assert_eq!(units.get("src/elsewhere.rs"), None);
    }

    // The stored form reads back to the same disclosure, causes with and without detail included.
    #[test]
    fn disclosure_round_trips_through_json() {
        let disclosure = UndeterminedUnits {
            packages: vec![
                UndeterminedPackage {
                    package: "a".to_string(),
                    cause: InventoryFailure::NoManifest,
                },
                UndeterminedPackage {
                    package: "b".to_string(),
                    cause: InventoryFailure::MetadataFailed {
                        detail: "boom".to_string(),
                    },
                },
            ],
            documents: vec![UndeterminedDocument {
                document_path: "src/old.rs".to_string(),
                reason: DocumentReason::NoTargetLoadsIt,
            }],
        };
        let json = serde_json::to_string(&disclosure).unwrap();
        assert_eq!(serde_json::from_str::<UndeterminedUnits>(&json).unwrap(), disclosure);
    }

    // The disclosure serializes with the cause as `reason`, and `detail` only where the cause has one.
    #[test]
    fn disclosure_serializes_reason_and_detail() {
        let disclosure = UndeterminedUnits {
            packages: vec![
                UndeterminedPackage {
                    package: "a".to_string(),
                    cause: InventoryFailure::NoManifest,
                },
                UndeterminedPackage {
                    package: "b".to_string(),
                    cause: InventoryFailure::MetadataFailed {
                        detail: "boom".to_string(),
                    },
                },
            ],
            documents: vec![UndeterminedDocument {
                document_path: "src/old.rs".to_string(),
                reason: DocumentReason::NoTargetLoadsIt,
            }],
        };
        assert_eq!(
            serde_json::to_value(&disclosure).unwrap(),
            serde_json::json!({
                "packages": [
                    {"package": "a", "reason": "no_manifest"},
                    {"package": "b", "reason": "metadata_failed", "detail": "boom"}
                ],
                "documents": [
                    {"document_path": "src/old.rs", "reason": "no_target_loads_it"}
                ]
            })
        );
    }
}
