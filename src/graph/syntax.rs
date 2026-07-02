//! The syntax oracle: tree-sitter-rust structure and enclosure over a source file.
//!
//! This oracle is always fresh, error-tolerant, and needs no build. It owns structure and
//! enclosure; the semantic index owns cross-file identity. The join reconciles the two.
//!
//! The oracle exposes exactly what the join needs: the identifier (name) node whose span contains a
//! given byte span, and the chain of persisted declarations that enclose a byte offset (a closure is
//! not a persisted declaration, so it is transparent to the chain — a reference inside a closure
//! attributes to the enclosing function).

use tree_sitter::{Node, Parser, Tree};

use super::range::ByteSpan;

/// A parsed syntax tree over one source file, retaining the source for byte slicing.
pub struct SyntaxTree {
    tree: Tree,
    source: String,
}

/// A declaration node the graph persists, identified by the byte span of its name and its full
/// span. The name span is what a semantic occurrence at the declaration site matches against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxDeclaration {
    /// The declaration's kind, as the tree-sitter node kind (e.g. `function_item`, `struct_item`).
    pub node_kind: String,
    /// The byte span of the declaration's name identifier.
    pub name_span: ByteSpan,
    /// The byte span of the whole declaration (its full source body).
    pub full_span: ByteSpan,
}

/// tree-sitter node kinds that correspond to persisted Rust declarations.
const DECLARATION_KINDS: &[&str] = &[
    "mod_item",
    "struct_item",
    "enum_item",
    "union_item",
    "trait_item",
    "impl_item",
    "function_item",
    "const_item",
    "static_item",
    "type_item",
    "function_signature_item",
];

fn span_of(node: Node) -> ByteSpan {
    ByteSpan {
        start: node.start_byte(),
        end: node.end_byte(),
    }
}

impl SyntaxTree {
    /// Parse Rust `source`. Returns `None` only if the parser cannot be initialized.
    pub fn parse(source: &str) -> Option<Self> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok()?;
        let tree = parser.parse(source, None)?;
        Some(Self {
            tree,
            source: source.to_string(),
        })
    }

    /// The source text this tree was parsed from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The bytes at a span, or `None` if the span is out of bounds.
    pub fn text_at(&self, span: ByteSpan) -> Option<&str> {
        self.source.get(span.start..span.end)
    }

    /// The smallest identifier (name) node whose span contains `span`.
    ///
    /// Returns the node's byte span. A semantic occurrence range covers a name token, so containment
    /// to an identifier node is the join's structural match; the caller then applies the
    /// text-equality guard.
    pub fn name_node_containing(&self, span: ByteSpan) -> Option<ByteSpan> {
        let root = self.tree.root_node();
        let mut best: Option<ByteSpan> = None;
        let mut cursor = root.walk();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let node_span = span_of(node);
            if !node_span.contains(&span) {
                continue;
            }
            if is_name_node(node) {
                // Prefer the tightest containing identifier.
                best = Some(match best {
                    Some(b) if (b.end - b.start) <= (node_span.end - node_span.start) => b,
                    _ => node_span,
                });
            }
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        best
    }

    /// Whether any syntactic construct exists at `span` at all (identifier or otherwise).
    ///
    /// Used to distinguish the "semantic occurrence with no syntax at its location" outcome (macro
    /// expansion, coordinate drift) from a genuine text mismatch.
    pub fn has_construct_at(&self, span: ByteSpan) -> bool {
        let root = self.tree.root_node();
        root.descendant_for_byte_range(span.start, span.end.max(span.start + 1).min(root.end_byte()))
            .is_some()
    }

    /// The chain of persisted declarations enclosing `offset`, innermost first.
    ///
    /// Closures are not declaration kinds, so they never appear — a reference inside a closure
    /// resolves to the enclosing function. When the chain is empty, the enclosing construct is the
    /// module/file itself (the outermost attribution).
    pub fn enclosing_declarations(&self, offset: usize) -> Vec<SyntaxDeclaration> {
        let root = self.tree.root_node();
        let mut chain = Vec::new();
        let mut node = root.descendant_for_byte_range(offset, offset);
        while let Some(n) = node {
            if DECLARATION_KINDS.contains(&n.kind())
                && let Some(name_span) = declaration_name_span(n, &self.source)
            {
                chain.push(SyntaxDeclaration {
                    node_kind: n.kind().to_string(),
                    name_span,
                    full_span: span_of(n),
                });
            }
            node = n.parent();
        }
        chain
    }

    /// The `(type_name, trait_name)` pairs for every `impl Trait for Type` block in the file.
    ///
    /// Feeds the uncontracted `type_hierarchy` edges. A plain inherent `impl Type` (no trait) is not
    /// a hierarchy relationship and is skipped.
    pub fn trait_impls(&self) -> Vec<(String, String)> {
        let root = self.tree.root_node();
        let mut out = Vec::new();
        let mut cursor = root.walk();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "impl_item"
                && let Some(trait_node) = node.child_by_field_name("trait")
                && let Some(type_node) = node.child_by_field_name("type")
                && let (Some(trait_name), Some(type_name)) =
                    (self.text_at(span_of(trait_node)), self.text_at(span_of(type_node)))
            {
                out.push((type_name.to_string(), trait_name.to_string()));
            }
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        out
    }

    /// Every persisted declaration in the file, each with its name and full span.
    pub fn all_declarations(&self) -> Vec<SyntaxDeclaration> {
        let root = self.tree.root_node();
        let mut out = Vec::new();
        let mut cursor = root.walk();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if DECLARATION_KINDS.contains(&node.kind())
                && let Some(name_span) = declaration_name_span(node, &self.source)
            {
                out.push(SyntaxDeclaration {
                    node_kind: node.kind().to_string(),
                    name_span,
                    full_span: span_of(node),
                });
            }
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        out
    }
}

/// Whether a node is an identifier-like name node.
fn is_name_node(node: Node) -> bool {
    matches!(
        node.kind(),
        "identifier" | "type_identifier" | "field_identifier" | "shorthand_field_identifier"
    )
}

/// The byte span of a declaration node's `name` field, if it has one.
fn declaration_name_span(node: Node, _source: &str) -> Option<ByteSpan> {
    // Most Rust item nodes expose their name under the `name` field.
    if let Some(name) = node.child_by_field_name("name") {
        return Some(span_of(name));
    }
    // `impl_item` has no `name` field; use its `type` field's identifier as the attribution anchor.
    if node.kind() == "impl_item"
        && let Some(ty) = node.child_by_field_name("type")
    {
        return Some(span_of(ty));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "\
mod net {
    pub struct Client;
    impl Client {
        pub fn connect(&self) {}
    }
    pub fn open() {
        let f = || connect();
    }
}
";

    /// The byte span of the `occurrence`-th (1-based) appearance of `token` in `SRC`.
    fn span_of_token(token: &str, occurrence: usize) -> ByteSpan {
        let abs = SRC.match_indices(token).nth(occurrence - 1).expect("token present").0;
        ByteSpan {
            start: abs,
            end: abs + token.len(),
        }
    }

    #[test]
    fn name_node_containing_finds_identifier() {
        let tree = SyntaxTree::parse(SRC).unwrap();
        let connect_def = span_of_token("connect", 1);
        let name = tree.name_node_containing(connect_def).unwrap();
        assert_eq!(tree.text_at(name), Some("connect"));
    }

    #[test]
    fn enclosing_declarations_skips_closures() {
        let tree = SyntaxTree::parse(SRC).unwrap();
        // The `connect()` call inside the closure in `open`.
        let call = span_of_token("connect", 2);
        let chain = tree.enclosing_declarations(call.start);
        // Innermost persisted declaration is the function `open`, not the closure.
        assert_eq!(chain.first().map(|d| tree.text_at(d.name_span)), Some(Some("open")));
    }

    #[test]
    fn module_level_offset_has_module_as_outermost() {
        let tree = SyntaxTree::parse(SRC).unwrap();
        let client_ref = span_of_token("Client", 2); // in `impl Client`
        let chain = tree.enclosing_declarations(client_ref.start);
        // The outermost declaration is the module `net`.
        assert_eq!(chain.last().map(|d| tree.text_at(d.name_span)), Some(Some("net")));
    }
}
