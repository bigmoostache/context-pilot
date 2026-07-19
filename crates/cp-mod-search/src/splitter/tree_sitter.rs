//! Tree-sitter AST-based splitter.
//!
//! Parses source files into ASTs using tree-sitter grammars and extracts
//! top-level semantic units (functions, structs, classes, etc.) as chunks.
//! Falls through to [`FixedSizeSplitter`](super::fixed_size::FixedSizeSplitter)
//! for unsupported languages.

use std::path::Path;

use tree_sitter::{Language, Parser};

use crate::splitter::Splitter;
use crate::types::Chunk;

/// Splits source files into semantic chunks using tree-sitter AST parsing.
///
/// Walks top-level AST nodes and groups them into semantic chunks
/// (functions, structs, classes, impl blocks, etc.).  Non-semantic
/// nodes (comments, imports, whitespace) between semantic items are
/// attached to the next semantic chunk as a preamble.
pub(crate) struct TreeSitterSplitter;

impl TreeSitterSplitter {
    /// Create a new tree-sitter splitter.
    pub(crate) const fn new() -> Self {
        Self
    }
}

/// Map a file extension to a tree-sitter [`Language`] and its
/// set of top-level node kinds that constitute "semantic items".
fn language_for_ext(ext: &str) -> Option<(Language, &'static [&'static str])> {
    match ext {
        "rs" => Some((
            tree_sitter_rust::LANGUAGE.into(),
            &[
                "function_item",
                "struct_item",
                "enum_item",
                "impl_item",
                "trait_item",
                "mod_item",
                "const_item",
                "static_item",
                "type_item",
                "macro_definition",
            ],
        )),
        "py" => Some((
            tree_sitter_python::LANGUAGE.into(),
            &["function_definition", "class_definition", "decorated_definition"],
        )),
        "js" | "jsx" => Some((
            tree_sitter_javascript::LANGUAGE.into(),
            &[
                "function_declaration",
                "class_declaration",
                "export_statement",
                "lexical_declaration",
                "variable_declaration",
            ],
        )),
        "ts" => Some((
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            &[
                "function_declaration",
                "class_declaration",
                "interface_declaration",
                "type_alias_declaration",
                "enum_declaration",
                "export_statement",
                "lexical_declaration",
                "variable_declaration",
            ],
        )),
        "tsx" => Some((
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            &[
                "function_declaration",
                "class_declaration",
                "interface_declaration",
                "type_alias_declaration",
                "enum_declaration",
                "export_statement",
                "lexical_declaration",
                "variable_declaration",
            ],
        )),
        "go" => Some((
            tree_sitter_go::LANGUAGE.into(),
            &["function_declaration", "method_declaration", "type_declaration", "const_declaration", "var_declaration"],
        )),
        "java" => Some((
            tree_sitter_java::LANGUAGE.into(),
            &[
                "class_declaration",
                "interface_declaration",
                "enum_declaration",
                "annotation_type_declaration",
                "record_declaration",
            ],
        )),
        "c" | "h" => Some((
            tree_sitter_c::LANGUAGE.into(),
            &["function_definition", "struct_specifier", "enum_specifier", "type_definition", "declaration"],
        )),
        "cpp" | "hpp" | "cc" => Some((
            tree_sitter_cpp::LANGUAGE.into(),
            &[
                "function_definition",
                "class_specifier",
                "struct_specifier",
                "namespace_definition",
                "template_declaration",
                "enum_specifier",
                "type_definition",
                "declaration",
            ],
        )),
        _ => None,
    }
}

/// Extract the name of a semantic node by looking for common child
/// node kinds that carry the identifier.
fn extract_name(node: &tree_sitter::Node<'_>, source: &[u8]) -> String {
    // Try direct "name" field first (covers most grammars)
    if let Some(name_node) = node.child_by_field_name("name") {
        let range = name_node.byte_range();
        return source.get(range).map(|s| String::from_utf8_lossy(s).into_owned()).unwrap_or_default();
    }

    // For Rust impl blocks: look for "type" field
    if node.kind() == "impl_item"
        && let Some(type_node) = node.child_by_field_name("type")
    {
        let type_range = type_node.byte_range();
        let type_name = source.get(type_range).map(|s| String::from_utf8_lossy(s).into_owned()).unwrap_or_default();
        // Check for trait impl: `impl Trait for Type`
        if let Some(trait_node) = node.child_by_field_name("trait") {
            let trait_range = trait_node.byte_range();
            let trait_name =
                source.get(trait_range).map(|s| String::from_utf8_lossy(s).into_owned()).unwrap_or_default();
            return format!("{trait_name} for {type_name}");
        }
        return type_name;
    }

    // For decorated definitions (Python): look inside the inner definition
    if node.kind() == "decorated_definition"
        && let Some(def_node) = node.child_by_field_name("definition")
    {
        return extract_name(&def_node, source);
    }

    // For export statements (JS/TS): look inside the declaration child
    if node.kind() == "export_statement"
        && let Some(decl) = node.child_by_field_name("declaration")
    {
        return extract_name(&decl, source);
    }

    String::new()
}

/// Map a tree-sitter node kind to a shorter, user-friendly chunk type label.
fn chunk_type_label(kind: &str) -> &'static str {
    match kind {
        "function_item" | "function_definition" | "function_declaration" => "function",
        "method_declaration" => "method",
        "struct_item" | "struct_specifier" => "struct",
        "enum_item" | "enum_specifier" | "enum_declaration" => "enum",
        "impl_item" => "impl",
        "trait_item" => "trait",
        "mod_item" => "module",
        "class_declaration" | "class_specifier" | "class_definition" => "class",
        "interface_declaration" | "annotation_type_declaration" => "interface",
        "type_item" | "type_alias_declaration" | "type_definition" | "type_declaration" => "type",
        "const_item" | "const_declaration" | "lexical_declaration" => "const",
        "static_item" => "static",
        "var_declaration" | "variable_declaration" => "var",
        "namespace_definition" => "namespace",
        "template_declaration" => "template",
        "macro_definition" => "macro",
        "decorated_definition" => "decorated",
        "export_statement" => "export",
        "record_declaration" => "record",
        "declaration" => "declaration",
        _ => "other",
    }
}

impl Splitter for TreeSitterSplitter {
    fn supports(&self, extension: &str) -> bool {
        language_for_ext(extension).is_some()
    }

    fn split(&self, content: &str, path: &Path) -> Vec<Chunk> {
        let ext = path.extension().and_then(std::ffi::OsStr::to_str).unwrap_or("");

        let Some((language, semantic_kinds)) = language_for_ext(ext) else {
            return Vec::new();
        };

        let mut parser = Parser::new();
        if parser.set_language(&language).is_err() {
            log::warn!("tree-sitter: cannot set language for extension '{ext}'");
            return Vec::new();
        }

        let Some(tree) = parser.parse(content, None) else {
            log::warn!("tree-sitter: parse failed for {}", path.display());
            return Vec::new();
        };

        let source = content.as_bytes();
        let root = tree.root_node();
        let mut chunks = Vec::new();

        // Accumulate non-semantic preamble text (comments, use statements, etc.)
        let mut preamble_start: Option<usize> = None;
        let mut preamble_start_line: u32 = 1;
        let mut preamble_start_char: u32 = 0;

        let cursor_count = u32::try_from(root.child_count()).unwrap_or(u32::MAX);
        for i in 0..cursor_count {
            let Some(node) = root.child(i) else {
                continue;
            };

            let kind = node.kind();
            let is_semantic = semantic_kinds.contains(&kind);

            if is_semantic {
                // Flush any accumulated preamble as a "preamble" chunk
                if let Some(pre_start) = preamble_start.take() {
                    let pre_end = node.start_byte();
                    if pre_end > pre_start {
                        let pre_content = content.get(pre_start..pre_end).unwrap_or("");
                        let trimmed = pre_content.trim();
                        if !trimmed.is_empty() {
                            let pre_end_line = node.start_position().row.saturating_add(1);
                            chunks.push(Chunk {
                                content: pre_content.to_owned(),
                                kind: "preamble".to_owned(),
                                name: String::new(),
                                line_start: preamble_start_line,
                                line_end: u32::try_from(pre_end_line).unwrap_or(u32::MAX),
                                char_start: preamble_start_char,
                                char_end: u32::try_from(pre_end).unwrap_or(u32::MAX),
                            });
                        }
                    }
                }

                // Extract the semantic chunk
                let start_byte = node.start_byte();
                let end_byte = node.end_byte();
                let node_content = content.get(start_byte..end_byte).unwrap_or("");
                let name = extract_name(&node, source);
                let label = chunk_type_label(kind);
                let start_line = node.start_position().row.saturating_add(1);
                let end_line = node.end_position().row.saturating_add(1);

                chunks.push(Chunk {
                    content: node_content.to_owned(),
                    kind: label.to_owned(),
                    name,
                    line_start: u32::try_from(start_line).unwrap_or(u32::MAX),
                    line_end: u32::try_from(end_line).unwrap_or(u32::MAX),
                    char_start: u32::try_from(start_byte).unwrap_or(u32::MAX),
                    char_end: u32::try_from(end_byte).unwrap_or(u32::MAX),
                });
            } else {
                // Non-semantic node — start or extend preamble
                if preamble_start.is_none() {
                    preamble_start = Some(node.start_byte());
                    preamble_start_line = u32::try_from(node.start_position().row.saturating_add(1)).unwrap_or(1);
                    preamble_start_char = u32::try_from(node.start_byte()).unwrap_or(0);
                }
                // The preamble extends to wherever the next semantic node starts
                // (or end of file if none)
            }
        }

        // Flush trailing preamble
        if let Some(pre_start) = preamble_start {
            let pre_content = content.get(pre_start..).unwrap_or("");
            let trimmed = pre_content.trim();
            if !trimmed.is_empty() {
                let total_lines = content.lines().count();
                chunks.push(Chunk {
                    content: pre_content.to_owned(),
                    kind: "preamble".to_owned(),
                    name: String::new(),
                    line_start: preamble_start_line,
                    line_end: u32::try_from(total_lines).unwrap_or(u32::MAX),
                    char_start: preamble_start_char,
                    char_end: u32::try_from(content.len()).unwrap_or(u32::MAX),
                });
            }
        }

        chunks
    }
}
