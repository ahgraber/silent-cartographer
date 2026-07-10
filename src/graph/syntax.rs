//! The syntax oracle: tree-sitter structure and enclosure over a source file, for every language
//! the join consumes.
//!
//! This oracle is always fresh, error-tolerant, and needs no build. It owns structure and
//! enclosure; the semantic index owns cross-file identity. The join reconciles the two.
//!
//! The oracle exposes exactly what the join needs: the identifier (name) node whose span contains a
//! given byte span, and the chain of persisted declarations that enclose a byte offset (a closure is
//! not a persisted declaration, so it is transparent to the chain — a reference inside a closure
//! attributes to the enclosing function).
//!
//! Structure differs by language, so declaration kinds, name-node kinds, and the module-declaration
//! query are each a per-[`Language`] table; the query surface itself ([`SyntaxTree`]'s methods) is
//! the same for every language a caller selects.

use tree_sitter::{Node, Parser, Tree};

use super::range::ByteSpan;

/// The source language a [`SyntaxTree`] is parsed as, selecting its declaration-kind and name-node
/// tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
}

/// A parsed syntax tree over one source file, retaining the source for byte slicing.
pub struct SyntaxTree {
    tree: Tree,
    source: String,
    language: Language,
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

/// The name-token spans of the trait and the implementing type in one `impl Trait for Type` block.
///
/// Each span points at the terminal identifier token — the identifier inside a generic type
/// (`From<DetailArg>` → `From`), the terminal segment of a qualified path (`fmt::Display` →
/// `Display`), the bare identifier otherwise — so edge derivation can resolve it through the aligned
/// occurrence sitting at exactly that location, never by matching display names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraitImpl {
    /// The name-token span of the implementing type.
    pub type_name_span: ByteSpan,
    /// The name-token span of the implemented trait.
    pub trait_name_span: ByteSpan,
}

/// The subclass/base name-token spans of one Python `class Sub(Base1, Base2):` definition.
///
/// Each base span points at the terminal identifier token (a dotted base `module.Base` → `Base`;
/// the bare identifier otherwise), so `type_hierarchy` edge derivation resolves it through the
/// aligned occurrence sitting at exactly that location — the Python analog of [`TraitImpl`]. A base
/// shape carrying no resolvable name token (a call, a subscript) contributes no span: skip, never
/// guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassBases {
    /// The name-token span of the class being defined.
    pub class_name_span: ByteSpan,
    /// The name-token spans of the declared bases, in declaration order.
    pub base_name_spans: Vec<ByteSpan>,
}

/// One declared alias binding: a document-local name bound to a target token, from an
/// `import ... as ...`-shaped construct.
///
/// The join verifies a binding by checking whether an aligned occurrence already sits at
/// `target_token_span` (or at the whole `binding_span`, the shape scip-python emits at from-import
/// binding sites) — the binding is only usable evidence once that occurrence's symbol identity is
/// known, never by comparing names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AliasBinding {
    /// The byte span of the alias identifier — the local name a reference token must spell to use
    /// this binding.
    pub alias_name_span: ByteSpan,
    /// The byte span of the target token the alias binds to: the aliased name's own span (Python) or
    /// the terminal path segment being renamed (Rust).
    pub target_token_span: ByteSpan,
    /// The byte span of the whole binding construct — the `aliased_import` (Python) or
    /// `use_as_clause` (Rust) node, covering `target as alias`.
    pub binding_span: ByteSpan,
}

/// The syntactic construct located at a byte span: its node kind, full span, and — for
/// operator-shaped expressions — the operator token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructAt {
    /// The tree-sitter node kind (e.g. `binary_expression`, `try_expression`, `crate`).
    pub kind: String,
    /// The construct's full byte span.
    pub span: ByteSpan,
    /// The operator token for binary / compound-assignment / unary expressions, else `None`.
    pub operator: Option<String>,
}

/// tree-sitter node kinds that correspond to persisted Rust declarations.
const RUST_DECLARATION_KINDS: &[&str] = &[
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

/// tree-sitter node kinds that correspond to persisted Python declarations: the module (the whole
/// document), classes, and functions (async functions are `function_definition` nodes too, so they
/// need no separate entry).
const PYTHON_DECLARATION_KINDS: &[&str] = &["module", "class_definition", "function_definition"];

/// The persisted-declaration node kinds for `language`.
fn declaration_kinds(language: Language) -> &'static [&'static str] {
    match language {
        Language::Rust => RUST_DECLARATION_KINDS,
        Language::Python => PYTHON_DECLARATION_KINDS,
    }
}

/// The identifier-like (name) node kinds for `language`.
fn name_node_kinds(language: Language) -> &'static [&'static str] {
    match language {
        Language::Rust => &[
            "identifier",
            "type_identifier",
            "field_identifier",
            "shorthand_field_identifier",
        ],
        Language::Python => &["identifier"],
    }
}

fn span_of(node: Node) -> ByteSpan {
    ByteSpan {
        start: node.start_byte(),
        end: node.end_byte(),
    }
}

impl SyntaxTree {
    /// Parse `source` as `language`. Returns `None` only if the parser cannot be initialized.
    pub fn parse(source: &str, language: Language) -> Option<Self> {
        let mut parser = Parser::new();
        let grammar = match language {
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
        };
        parser.set_language(&grammar).ok()?;
        let tree = parser.parse(source, None)?;
        Some(Self {
            tree,
            source: source.to_string(),
            language,
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
            if is_name_node(node, self.language) {
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
        let kinds = declaration_kinds(self.language);
        let mut chain = Vec::new();
        let mut node = root.descendant_for_byte_range(offset, offset);
        while let Some(n) = node {
            if kinds.contains(&n.kind())
                && let Some(name_span) = declaration_name_span(n)
            {
                chain.push(SyntaxDeclaration {
                    node_kind: n.kind().to_string(),
                    name_span,
                    full_span: declaration_full_span(n),
                });
            }
            node = n.parent();
        }
        chain
    }

    /// The trait/type name-token spans for every `impl Trait for Type` block in the file.
    ///
    /// Each entry carries the terminal identifier span of the type and the trait (see [`TraitImpl`]),
    /// so `type_hierarchy` edge derivation resolves each span through the aligned occurrence at that
    /// location. A plain inherent `impl Type` (no trait) is not a hierarchy relationship and is
    /// skipped, as is any header whose trait or type has no resolvable name token.
    pub fn trait_impls(&self) -> Vec<TraitImpl> {
        let root = self.tree.root_node();
        let mut out = Vec::new();
        let mut cursor = root.walk();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "impl_item"
                && let Some(trait_node) = node.child_by_field_name("trait")
                && let Some(type_node) = node.child_by_field_name("type")
                && let (Some(trait_tok), Some(type_tok)) = (name_token(trait_node), name_token(type_node))
            {
                out.push(TraitImpl {
                    type_name_span: span_of(type_tok),
                    trait_name_span: span_of(trait_tok),
                });
            }
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        out
    }

    /// The subclass/base name-token spans for every `class Sub(Base1, Base2):` definition in a
    /// Python file — the Python analog of [`Self::trait_impls`].
    ///
    /// Each entry carries the class's own name-token span and one span per declared base's terminal
    /// identifier, so `type_hierarchy` edge derivation resolves each through the aligned occurrence
    /// at that location. A class without a superclass list contributes an entry with no base spans;
    /// keyword arguments in the header (`metaclass=...`) and base shapes without a name token are
    /// skipped, never guessed at.
    pub fn class_bases(&self) -> Vec<ClassBases> {
        let root = self.tree.root_node();
        let mut out = Vec::new();
        let mut cursor = root.walk();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "class_definition"
                && let Some(name) = node.child_by_field_name("name")
            {
                let mut base_name_spans = Vec::new();
                if let Some(superclasses) = node.child_by_field_name("superclasses") {
                    let mut args_cursor = superclasses.walk();
                    for arg in superclasses.named_children(&mut args_cursor) {
                        if let Some(token) = python_base_name_token(arg) {
                            base_name_spans.push(span_of(token));
                        }
                    }
                }
                out.push(ClassBases {
                    class_name_span: span_of(name),
                    base_name_spans,
                });
            }
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        out
    }

    /// Every declared alias binding in the file: Python `import a.b as c` / `from m import n as c`
    /// (`aliased_import` nodes), and Rust `use path as name;` / `pub use path as name;`
    /// (`use_as_clause` nodes).
    ///
    /// A plain `import x` or `use a::b;` (no `as` clause) declares no binding and contributes
    /// nothing — the join's import-alias rule has no evidence without an explicit local rename.
    pub fn alias_bindings(&self) -> Vec<AliasBinding> {
        match self.language {
            Language::Python => self.python_alias_bindings(),
            Language::Rust => self.rust_alias_bindings(),
        }
    }

    /// Python alias bindings, read from `aliased_import` nodes under `import`/`from ... import`
    /// statements.
    ///
    /// The target token span is the aliased name's own span as tree-sitter reports it: the full
    /// dotted name for `import a.b as c`, the plain name for `from m import n as c`. The alias span
    /// is the `alias` field's identifier.
    fn python_alias_bindings(&self) -> Vec<AliasBinding> {
        let root = self.tree.root_node();
        let mut out = Vec::new();
        let mut cursor = root.walk();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "aliased_import"
                && let (Some(target), Some(alias)) =
                    (node.child_by_field_name("name"), node.child_by_field_name("alias"))
            {
                out.push(AliasBinding {
                    alias_name_span: span_of(alias),
                    target_token_span: span_of(target),
                    binding_span: span_of(node),
                });
            }
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        out
    }

    /// Rust alias bindings, read from `use_as_clause` nodes under `use`/`pub use` declarations.
    ///
    /// The target token span is the terminal segment of the `path` field: the whole node for a bare
    /// `identifier` path, or the `name` field's identifier for a `scoped_identifier` path
    /// (`a::b as c` → `b`). A path shape with no resolvable terminal segment contributes nothing.
    fn rust_alias_bindings(&self) -> Vec<AliasBinding> {
        let root = self.tree.root_node();
        let mut out = Vec::new();
        let mut cursor = root.walk();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "use_as_clause"
                && let Some(path) = node.child_by_field_name("path")
                && let Some(alias) = node.child_by_field_name("alias")
                && let Some(target) = rust_use_path_terminal(path)
            {
                out.push(AliasBinding {
                    alias_name_span: span_of(alias),
                    target_token_span: span_of(target),
                    binding_span: span_of(node),
                });
            }
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        out
    }

    /// The syntactic construct at `span`: the smallest **named** node containing it, with the
    /// operator token for operator-shaped expressions.
    ///
    /// Anonymous token nodes (a bare `?`, `==`, `[`) resolve to their named parent, and a span on
    /// whitespace inside an expression resolves to that expression — which is what lets the
    /// operator-desugar alignment rule match by construct instead of raw bytes (live operator spans
    /// are observed sitting adjacent to the sigil). A span inside an operand resolves to the
    /// operand's own node, never the surrounding expression, so the construct match stays exact.
    pub fn construct_at(&self, span: ByteSpan) -> Option<ConstructAt> {
        let root = self.tree.root_node();
        let end = span.end.max(span.start + 1).min(root.end_byte());
        let mut node = root.descendant_for_byte_range(span.start, end)?;
        while !node.is_named() {
            node = node.parent()?;
        }
        let operator = match node.kind() {
            // Binary and compound-assignment expressions expose the sigil as the `operator` field.
            "binary_expression" | "compound_assignment_expr" => node
                .child_by_field_name("operator")
                .and_then(|op| self.text_at(span_of(op)))
                .map(str::to_string),
            // A unary expression's sigil is its leading token (`-`, `!`, `*`).
            "unary_expression" => node.child(0).and_then(|c| self.text_at(span_of(c))).map(str::to_string),
            _ => None,
        };
        Some(ConstructAt {
            kind: node.kind().to_string(),
            span: span_of(node),
            operator,
        })
    }

    /// The self-type text of the nearest `impl` block enclosing `offset`, if any.
    ///
    /// Walks the ancestor chain to the innermost `impl_item` and reads its `type` field verbatim
    /// (generic arguments included — the caller strips them for base-name comparison). `Self`
    /// inside a trait body has no enclosing `impl` and yields `None`.
    pub fn enclosing_impl_self_type(&self, offset: usize) -> Option<String> {
        let root = self.tree.root_node();
        let mut node = root.descendant_for_byte_range(offset, offset);
        while let Some(n) = node {
            if n.kind() == "impl_item"
                && let Some(ty) = n.child_by_field_name("type")
            {
                return self.text_at(span_of(ty)).map(str::to_string);
            }
            node = n.parent();
        }
        None
    }

    /// Whether `span` sits on the name token of a module declaration — the identifier that is the
    /// `name` field of a `mod_item` (`mod name;` or `mod name { .. }`).
    ///
    /// A module symbol is referenced from every `use`/path segment naming it across the workspace;
    /// only the declaration site carries parent-module evidence, so callers deriving module
    /// containment must gate reference occurrences through this check rather than trusting any
    /// reference.
    pub fn is_module_declaration_name(&self, span: ByteSpan) -> bool {
        // Python has no `mod`-style declaration, so the module-chain locality rule this check backs
        // is inert for Python: every Python span reports no module-declaration site.
        if self.language != Language::Rust {
            return false;
        }
        let root = self.tree.root_node();
        let end = span.end.max(span.start + 1).min(root.end_byte());
        let Some(mut node) = root.descendant_for_byte_range(span.start, end) else {
            return false;
        };
        while !node.is_named() {
            let Some(parent) = node.parent() else {
                return false;
            };
            node = parent;
        }
        if node.kind() != "identifier" {
            return false;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        parent.kind() == "mod_item"
            && parent
                .child_by_field_name("name")
                .is_some_and(|name| span_of(name) == span_of(node))
    }

    /// The chain of `attribute`/`dotted_name` node spans enclosing `span`, innermost first (Python).
    ///
    /// A dotted construct is the parser's own grouping of a qualified reference (`h2.connection` is
    /// the enclosing construct of the `h2` token inside `h2.connection.H2Connection(...)`), so a
    /// prefix-token occurrence can retry evidence against the wider text the parser sees, without
    /// re-deriving dotted-name grouping by hand. A span outside any dotted construct yields an empty
    /// chain.
    pub fn enclosing_dotted_constructs(&self, span: ByteSpan) -> Vec<ByteSpan> {
        let root = self.tree.root_node();
        let end = span.end.max(span.start + 1).min(root.end_byte());
        let mut chain = Vec::new();
        let mut node = root.descendant_for_byte_range(span.start, end);
        while let Some(n) = node {
            if matches!(n.kind(), "attribute" | "dotted_name") {
                chain.push(span_of(n));
            }
            node = n.parent();
        }
        chain
    }

    /// Every persisted declaration in the file, each with its name and full span.
    pub fn all_declarations(&self) -> Vec<SyntaxDeclaration> {
        let root = self.tree.root_node();
        let kinds = declaration_kinds(self.language);
        let mut out = Vec::new();
        let mut cursor = root.walk();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if kinds.contains(&node.kind())
                && let Some(name_span) = declaration_name_span(node)
            {
                out.push(SyntaxDeclaration {
                    node_kind: node.kind().to_string(),
                    name_span,
                    full_span: declaration_full_span(node),
                });
            }
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        out
    }
}

/// The terminal identifier token of a type or trait node: the identifier inside a generic type
/// (`From<Arg>` → `From`), the terminal segment of a qualified path (`fmt::Display` → `Display`), or
/// the bare identifier itself. Returns `None` for a shape carrying no resolvable name token.
fn name_token(node: Node) -> Option<Node> {
    match node.kind() {
        "type_identifier" | "identifier" => Some(node),
        "generic_type" => node.child_by_field_name("type").and_then(name_token),
        "scoped_type_identifier" | "scoped_identifier" => node.child_by_field_name("name").and_then(name_token),
        _ => None,
    }
}

/// Whether a node is an identifier-like name node for `language`.
fn is_name_node(node: Node, language: Language) -> bool {
    name_node_kinds(language).contains(&node.kind())
}

/// The terminal segment of a Rust `use_as_clause`'s `path` field: the node itself for a bare
/// `identifier` or the path keyword `crate`/`self`/`super`, or the `name` field's identifier for a
/// `scoped_identifier` (`a::b` → `b`). Returns `None` for a path shape with no resolvable terminal
/// segment.
fn rust_use_path_terminal(node: Node) -> Option<Node> {
    match node.kind() {
        "identifier" | "crate" | "self" | "super" => Some(node),
        "scoped_identifier" => node.child_by_field_name("name"),
        _ => None,
    }
}

/// The terminal identifier token of a Python base-class expression: the bare identifier, or the
/// terminal segment of a dotted name (`module.Base` → `Base`). Returns `None` for a shape carrying
/// no resolvable name token (a call, a subscript, a keyword argument).
fn python_base_name_token(node: Node) -> Option<Node> {
    match node.kind() {
        "identifier" => Some(node),
        "attribute" => node.child_by_field_name("attribute").and_then(python_base_name_token),
        _ => None,
    }
}

/// The byte span of a declaration node's `name` field, if it has one.
///
/// Most Rust item nodes and both Python declaration nodes (`class_definition`,
/// `function_definition`) expose their name under the `name` field directly. `impl_item` has no
/// `name` field; its `type` field's identifier is the attribution anchor instead. Python's `module`
/// node has no name field at all — it is the document itself, so it is excluded from both the
/// enclosing-declaration chain and the enumeration this gates, exactly as Rust's top level (no
/// wrapping declaration) is.
fn declaration_name_span(node: Node) -> Option<ByteSpan> {
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

/// The full byte span persisted for a declaration node.
///
/// A Python declaration wrapped in `decorated_definition` (`@deco\ndef f(): ...`) persists the
/// wrapper's span so decorators are included in the declaration's body; every other declaration
/// persists its own span.
fn declaration_full_span(node: Node) -> ByteSpan {
    match node.parent() {
        Some(parent) if parent.kind() == "decorated_definition" => span_of(parent),
        _ => span_of(node),
    }
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
        let tree = SyntaxTree::parse(SRC, Language::Rust).unwrap();
        let connect_def = span_of_token("connect", 1);
        let name = tree.name_node_containing(connect_def).unwrap();
        assert_eq!(tree.text_at(name), Some("connect"));
    }

    #[test]
    fn enclosing_declarations_skips_closures() {
        let tree = SyntaxTree::parse(SRC, Language::Rust).unwrap();
        // The `connect()` call inside the closure in `open`.
        let call = span_of_token("connect", 2);
        let chain = tree.enclosing_declarations(call.start);
        // Innermost persisted declaration is the function `open`, not the closure.
        assert_eq!(chain.first().map(|d| tree.text_at(d.name_span)), Some(Some("open")));
    }

    #[test]
    fn module_level_offset_has_module_as_outermost() {
        let tree = SyntaxTree::parse(SRC, Language::Rust).unwrap();
        let client_ref = span_of_token("Client", 2); // in `impl Client`
        let chain = tree.enclosing_declarations(client_ref.start);
        // The outermost declaration is the module `net`.
        assert_eq!(chain.last().map(|d| tree.text_at(d.name_span)), Some(Some("net")));
    }

    /// The construct at the first occurrence of `token` in `src`.
    fn construct_at_token(src: &str, token: &str) -> ConstructAt {
        let tree = SyntaxTree::parse(src, Language::Rust).unwrap();
        let start = src.find(token).expect("token present");
        tree.construct_at(ByteSpan {
            start,
            end: start + token.len(),
        })
        .expect("construct present")
    }

    // The construct kinds the operator-desugar correspondence dispatches on, one per family.
    #[test]
    fn construct_at_resolves_operator_families() {
        let try_c = construct_at_token("fn f(x: Option<u8>) -> Option<u8> { Some(x?) }\n", "?");
        assert_eq!(try_c.kind, "try_expression");

        let eq = construct_at_token("fn f(a: u8, b: u8) -> bool { a == b }\n", "==");
        assert_eq!(
            (eq.kind.as_str(), eq.operator.as_deref()),
            ("binary_expression", Some("=="))
        );

        let add_assign = construct_at_token("fn f(mut a: u8) { a += 1; }\n", "+=");
        assert_eq!(
            (add_assign.kind.as_str(), add_assign.operator.as_deref()),
            ("compound_assignment_expr", Some("+="))
        );

        let neg = construct_at_token("fn f(a: i8) -> i8 { -a }\n", "-a");
        assert_eq!(
            (neg.kind.as_str(), neg.operator.as_deref()),
            ("unary_expression", Some("-"))
        );

        let index = construct_at_token("fn f(v: &[u8]) -> u8 { v[0] }\n", "[0]");
        assert_eq!(index.kind, "index_expression");

        let call = construct_at_token("fn f(g: fn()) { g(); }\n", "g()");
        assert_eq!(call.kind, "call_expression");

        let for_loop = construct_at_token("fn f(v: Vec<u8>) { for _x in v {} }\n", "for");
        assert_eq!(for_loop.kind, "for_expression");
    }

    // A single-byte span on whitespace beside a sigil resolves to the surrounding expression (the
    // live rust-analyzer adjacency case), while a span on an operand resolves to the operand.
    #[test]
    fn construct_at_whitespace_beside_sigil_is_the_expression() {
        let src = "fn f(a: u8, b: u8) -> u8 { a + b }\n";
        let tree = SyntaxTree::parse(src, Language::Rust).unwrap();
        let space_before_plus = src.find(" + ").unwrap(); // the space between `a` and `+`
        let c = tree
            .construct_at(ByteSpan {
                start: space_before_plus,
                end: space_before_plus + 1,
            })
            .unwrap();
        assert_eq!(
            (c.kind.as_str(), c.operator.as_deref()),
            ("binary_expression", Some("+"))
        );

        // A span on the operand `a` itself is the identifier, never the surrounding expression.
        let a_pos = src.find("{ a ").unwrap() + 2;
        let operand = tree
            .construct_at(ByteSpan {
                start: a_pos,
                end: a_pos + 1,
            })
            .unwrap();
        assert_eq!(operand.kind, "identifier");
    }

    // The `crate` path keyword is its own named node kind, distinct from identifiers.
    #[test]
    fn construct_at_crate_keyword() {
        let c = construct_at_token("use crate::thing::Thing;\n", "crate");
        assert_eq!(c.kind, "crate");
    }

    // A module-declaration name token is recognized; a use-style path segment spelling the same
    // module name, and non-module identifiers, are not.
    #[test]
    fn module_declaration_name_is_distinguished_from_use_style_references() {
        let src = "\
mod sub;
mod inline_mod {
    pub fn f() {}
}
use sub::thing;
fn not_a_mod() {}
";
        let tree = SyntaxTree::parse(src, Language::Rust).unwrap();
        let span = |token: &str, occurrence: usize| {
            let abs = src.match_indices(token).nth(occurrence - 1).expect("token present").0;
            ByteSpan {
                start: abs,
                end: abs + token.len(),
            }
        };

        // Declaration sites: `mod sub;` and `mod inline_mod { .. }`.
        assert!(tree.is_module_declaration_name(span("sub", 1)), "`mod sub;` name");
        assert!(
            tree.is_module_declaration_name(span("inline_mod", 1)),
            "inline module name"
        );

        // A use-style path segment naming the module is not a declaration site.
        assert!(
            !tree.is_module_declaration_name(span("sub", 2)),
            "`use sub::thing` path segment is not a declaration"
        );

        // A non-module identifier is not a declaration site.
        assert!(!tree.is_module_declaration_name(span("not_a_mod", 1)), "fn name");
    }

    // `Self` is a name node in both type position and path-segment position, and the nearest
    // enclosing impl's self type is readable for the cross-check; a trait body has none.
    #[test]
    fn self_token_is_a_name_node_and_impl_self_type_is_readable() {
        let src = "\
struct A;
impl<T> A {
    fn f() -> Self { Self::g() }
}
trait Tr {
    fn t() -> Self;
}
";
        let tree = SyntaxTree::parse(src, Language::Rust).unwrap();

        // Both `Self` positions resolve through the name-node path with text `Self`.
        for pos in [src.find("Self").unwrap(), src.find("Self::").unwrap()] {
            let name = tree
                .name_node_containing(ByteSpan {
                    start: pos,
                    end: pos + 4,
                })
                .expect("Self is a name node");
            assert_eq!(tree.text_at(name), Some("Self"));
        }

        // Inside the impl: the self type (verbatim, generics included where written).
        let in_impl = src.find("Self").unwrap();
        assert_eq!(tree.enclosing_impl_self_type(in_impl).as_deref(), Some("A"));

        // Inside the trait body: no impl to cross-check against.
        let in_trait = src.rfind("Self").unwrap();
        assert_eq!(tree.enclosing_impl_self_type(in_trait), None);
    }

    const PY_SRC: &str = "\
class Widget:
    def method(self):
        pass

@decorator
def decorated():
    pass

async def async_fn():
    pass

TOP_LEVEL = 1
";

    /// The byte span of the `occurrence`-th (1-based) appearance of `token` in `src`.
    fn py_span_of(src: &str, token: &str, occurrence: usize) -> ByteSpan {
        let abs = src.match_indices(token).nth(occurrence - 1).expect("token present").0;
        ByteSpan {
            start: abs,
            end: abs + token.len(),
        }
    }

    // Every persisted Python declaration enumerates with its kind, name text, and span; a
    // decorated declaration's span is the wrapper's span (decorators included), while its name node
    // is the inner definition's `name` field.
    #[test]
    fn python_declarations_enumerate_with_names_and_spans() {
        let tree = SyntaxTree::parse(PY_SRC, Language::Python).unwrap();
        let decls = tree.all_declarations();

        let class = decls.iter().find(|d| d.node_kind == "class_definition").unwrap();
        assert_eq!(tree.text_at(class.name_span), Some("Widget"));
        assert_eq!(
            tree.text_at(class.full_span),
            Some("class Widget:\n    def method(self):\n        pass")
        );

        let method = decls
            .iter()
            .find(|d| d.node_kind == "function_definition" && tree.text_at(d.name_span) == Some("method"))
            .unwrap();
        assert_eq!(tree.text_at(method.full_span), Some("def method(self):\n        pass"));

        let decorated = decls
            .iter()
            .find(|d| tree.text_at(d.name_span) == Some("decorated"))
            .unwrap();
        assert_eq!(decorated.node_kind, "function_definition");
        // The decorated span includes the decorator line, not just the `def` header.
        let decorated_start = py_span_of(PY_SRC, "@decorator", 1).start;
        assert_eq!(decorated.full_span.start, decorated_start);
        assert_eq!(
            tree.text_at(decorated.full_span),
            Some("@decorator\ndef decorated():\n    pass")
        );

        let async_fn = decls
            .iter()
            .find(|d| tree.text_at(d.name_span) == Some("async_fn"))
            .unwrap();
        assert_eq!(async_fn.node_kind, "function_definition");
        assert_eq!(
            tree.text_at(async_fn.full_span),
            Some("async def async_fn():\n    pass")
        );
    }

    // A position inside a method body encloses [method, class]; a position at module top level
    // encloses nothing (Python's `module` has no name field, so it never appears in the chain — the
    // same "no wrapping declaration" shape as Rust's top level).
    #[test]
    fn python_enclosing_declarations_innermost_first() {
        let tree = SyntaxTree::parse(PY_SRC, Language::Python).unwrap();

        let in_method_body = PY_SRC.find("pass").unwrap();
        let chain = tree.enclosing_declarations(in_method_body);
        let names: Vec<Option<&str>> = chain.iter().map(|d| tree.text_at(d.name_span)).collect();
        assert_eq!(names, vec![Some("method"), Some("Widget")]);

        let at_module_top = PY_SRC.find("TOP_LEVEL").unwrap();
        assert_eq!(tree.enclosing_declarations(at_module_top), Vec::new());
    }

    // A span inside an identifier resolves to that identifier's own span; a span on a keyword (not
    // an identifier-kind node in the Python grammar) resolves to nothing.
    #[test]
    fn python_name_node_containment() {
        let tree = SyntaxTree::parse(PY_SRC, Language::Python).unwrap();

        let widget_span = py_span_of(PY_SRC, "Widget", 1);
        let name = tree.name_node_containing(widget_span).unwrap();
        assert_eq!(tree.text_at(name), Some("Widget"));

        let class_keyword = py_span_of(PY_SRC, "class", 1);
        assert_eq!(tree.name_node_containing(class_keyword), None);
    }

    // Both Python alias forms yield their declared bindings with correct target/alias spans; a plain
    // `import x` (no `as` clause) yields none.
    #[test]
    fn python_alias_bindings_enumerate() {
        let src = "import a.b as c\nfrom m import n as c2\nimport x\n";
        let tree = SyntaxTree::parse(src, Language::Python).unwrap();
        let bindings = tree.alias_bindings();
        assert_eq!(
            bindings.len(),
            2,
            "only the two `as`-aliased imports bind: {bindings:?}"
        );

        let dotted_target = py_span_of(src, "a.b", 1);
        let dotted_alias = py_span_of(src, "c", 1);
        let dotted_binding = py_span_of(src, "a.b as c", 1);
        assert!(
            bindings.iter().any(|b| b.target_token_span == dotted_target
                && b.alias_name_span == dotted_alias
                && b.binding_span == dotted_binding),
            "`import a.b as c` binds alias `c` to target `a.b` across binding span `a.b as c`: {bindings:?}"
        );

        let from_target = py_span_of(src, "n", 1);
        let from_alias = py_span_of(src, "c2", 1);
        let from_binding = py_span_of(src, "n as c2", 1);
        assert!(
            bindings.iter().any(|b| b.target_token_span == from_target
                && b.alias_name_span == from_alias
                && b.binding_span == from_binding),
            "`from m import n as c2` binds alias `c2` to target `n` across binding span `n as c2`: {bindings:?}"
        );
    }

    // Both Rust `use ... as` forms (bare and `pub`) yield their declared bindings; a plain `use a::b;`
    // (no `as` clause) yields none.
    #[test]
    fn rust_use_as_bindings_enumerate() {
        let src = "use alpha::beta as gamma;\npub use delta as epsilon;\nuse alpha::beta;\n";
        let tree = SyntaxTree::parse(src, Language::Rust).unwrap();
        let bindings = tree.alias_bindings();
        assert_eq!(bindings.len(), 2, "only the two `as`-aliased uses bind: {bindings:?}");

        let span = |token: &str, occurrence: usize| {
            let abs = src.match_indices(token).nth(occurrence - 1).expect("token present").0;
            ByteSpan {
                start: abs,
                end: abs + token.len(),
            }
        };

        let scoped_target = span("beta", 1); // the `beta` in `alpha::beta as gamma`
        let scoped_alias = span("gamma", 1);
        let scoped_binding = span("alpha::beta as gamma", 1);
        assert!(
            bindings.iter().any(|b| b.target_token_span == scoped_target
                && b.alias_name_span == scoped_alias
                && b.binding_span == scoped_binding),
            "`use alpha::beta as gamma;` binds alias `gamma` to the terminal segment `beta` across the whole \
             clause: {bindings:?}"
        );

        let bare_target = span("delta", 1);
        let bare_alias = span("epsilon", 1);
        let bare_binding = span("delta as epsilon", 1);
        assert!(
            bindings.iter().any(|b| b.target_token_span == bare_target
                && b.alias_name_span == bare_alias
                && b.binding_span == bare_binding),
            "`pub use delta as epsilon;` binds alias `epsilon` to target `delta` across the whole clause: \
             {bindings:?}"
        );
    }

    // The chain at a prefix token inside a multi-segment dotted construct contains every enclosing
    // dotted span, innermost first; a bare identifier outside any dotted construct yields an empty
    // chain.
    #[test]
    fn python_dotted_construct_chain_at_token() {
        let src = "h2.connection.H2Connection(1)\nx\n";
        let tree = SyntaxTree::parse(src, Language::Python).unwrap();

        let h2_token = py_span_of(src, "h2", 1);
        let chain = tree.enclosing_dotted_constructs(h2_token);
        let texts: Vec<Option<&str>> = chain.iter().map(|s| tree.text_at(*s)).collect();
        assert!(
            texts.contains(&Some("h2.connection")),
            "chain contains the inner dotted span: {texts:?}"
        );
        assert!(
            texts.contains(&Some("h2.connection.H2Connection")),
            "chain contains the wider dotted span: {texts:?}"
        );
        assert_eq!(
            texts.first().copied(),
            Some(Some("h2.connection")),
            "innermost enclosing construct comes first: {texts:?}"
        );

        let bare = py_span_of(src, "x", 1);
        assert_eq!(
            tree.enclosing_dotted_constructs(bare),
            Vec::new(),
            "a bare identifier outside any dotted construct has no enclosing chain"
        );
    }
}
