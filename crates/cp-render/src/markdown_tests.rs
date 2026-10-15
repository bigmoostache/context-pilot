//! Table-driven tests for the markdown-to-IR parser.

use crate::markdown::{parse_inline, parse_line, to_blocks};
use crate::{Align, Block, Cell, Column, Semantic, Span};

/// Concatenated text of a span list.
fn text(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

/// `(text, bold)` pairs, the shape most inline tests assert on.
fn shape(spans: &[Span]) -> Vec<(String, bool)> {
    spans.iter().map(|s| (s.text.clone(), s.bold)).collect()
}

/// Element `i` of a slice, panicking with context when out of range.
fn at<T>(items: &[T], i: usize) -> &T {
    items.get(i).expect("index in range")
}

/// Spans of a `Block::Line`, panicking on any other variant.
fn line_spans(block: &Block) -> Vec<Span> {
    let Block::Line(spans) = block.clone() else { panic!("expected Block::Line, got {block:?}") };
    spans
}

/// Columns and rows of the first block, which must be a `Block::Table`.
fn first_table(blocks: &[Block]) -> (Vec<Column>, Vec<Vec<Cell>>) {
    let Some(Block::Table { columns, rows }) = blocks.first().cloned() else { panic!("expected table") };
    (columns, rows)
}

#[test]
fn headers_strip_hashes_and_style_by_level() {
    let cases = [
        ("# Title", "Title", Semantic::Accent, true),
        ("## Sub", "Sub", Semantic::Accent, false),
        ("### Third", "Third", Semantic::Accent, false),
        ("#### Deep", "Deep", Semantic::Code, false),
        ("  # Indented", "Indented", Semantic::Accent, true),
    ];
    for (input, want, semantic, bold) in cases {
        let spans = parse_line(input);
        assert_eq!(spans.len(), 1, "{input}");
        let span = at(&spans, 0);
        assert_eq!((span.text.as_str(), span.semantic, span.bold), (want, semantic, bold), "{input}");
    }
}

#[test]
fn hash_without_space_is_not_a_header() {
    for input in ["#tag", "#123 issue", "#!/bin/sh"] {
        let spans = parse_line(input);
        assert_eq!(text(&spans), input, "{input}");
        assert!(spans.iter().all(|s| s.semantic == Semantic::Default && !s.bold), "{input}");
    }
}

#[test]
fn bullets_keep_indent_and_parse_inline() {
    for (input, indent) in [("- item **b**", ""), ("  * item **b**", "  ")] {
        let spans = parse_line(input);
        assert_eq!(at(&spans, 0).text, indent, "{input}");
        let bullet = at(&spans, 1);
        assert_eq!((bullet.text.as_str(), bullet.semantic), ("\u{2022} ", Semantic::AccentDim), "{input}");
        assert_eq!(text(spans.get(2..).unwrap_or_default()), "item b", "{input}");
        assert!(spans.last().is_some_and(|s| s.bold), "{input}");
    }
}

#[test]
fn double_star_line_is_bold_not_bullet() {
    assert_eq!(shape(&parse_line("**bold** rest")), vec![("bold".into(), true), (" rest".into(), false)]);
}

#[test]
fn inline_formatting() {
    let cases = [
        ("plain", vec![("plain", false)]),
        ("a **b** c", vec![("a ", false), ("b", true), (" c", false)]),
        ("a __b__ c", vec![("a ", false), ("b", true), (" c", false)]),
        ("single *star* and _under_", vec![("single *star* and _under_", false)]),
        ("x `code` y", vec![("x ", false), ("code", false), (" y", false)]),
        ("see [docs](http://x) now", vec![("see ", false), ("docs", false), (" now", false)]),
    ];
    for (input, want) in cases {
        let expected: Vec<(String, bool)> = want.into_iter().map(|(t, b)| (t.to_owned(), b)).collect();
        assert_eq!(shape(&parse_inline(input)), expected, "{input}");
    }
}

#[test]
fn code_and_link_semantics() {
    let code = parse_inline("x `c` y");
    assert_eq!(at(&code, 1).semantic, Semantic::Warning);
    let link = parse_inline("[docs](u)");
    let first = at(&link, 0);
    assert_eq!((first.text.as_str(), first.semantic), ("docs", Semantic::Accent));
}

#[test]
fn bracket_without_url_stays_literal() {
    for input in ["[x] done", "array[0", "a [b] c"] {
        assert_eq!(text(&parse_inline(input)), input, "{input}");
    }
}

#[test]
fn unclosed_markers_stay_literal() {
    for input in ["a `b c", "a **b c", "x __y", "`", "**"] {
        let spans = parse_inline(input);
        assert_eq!(text(&spans), input, "{input}");
        assert!(spans.iter().all(|s| !s.bold && s.semantic == Semantic::Default), "{input}");
    }
}

#[test]
fn intraword_double_underscore_is_literal() {
    for input in ["call __init__ now", "foo__bar__", "snake_case_name"] {
        let spans = parse_inline(input);
        assert_eq!(text(&spans), input.replace("__init__", "init"), "{input}");
    }
    assert!(parse_inline("foo__bar__").iter().all(|s| !s.bold));
}

#[test]
fn empty_input_yields_one_empty_span() {
    assert_eq!(shape(&parse_inline("")), vec![(String::new(), false)]);
}

#[test]
fn to_blocks_empty_lines_and_text() {
    let blocks = to_blocks("a\n\nb");
    assert_eq!(blocks.len(), 3);
    assert_eq!(text(&line_spans(at(&blocks, 0))), "a");
    assert_eq!(blocks.get(1), Some(&Block::Empty));
    assert!(to_blocks("").is_empty());
}

#[test]
fn fenced_code_is_verbatim() {
    let blocks = to_blocks("```rust\n# not header\n**not bold**\n```\nafter **b**");
    let semantics: Vec<Semantic> = blocks.iter().map(|b| at(&line_spans(b), 0).semantic).collect();
    assert_eq!(
        semantics.get(..4).unwrap_or_default(),
        [Semantic::Muted, Semantic::Code, Semantic::Code, Semantic::Muted]
    );
    assert_eq!(text(&line_spans(at(&blocks, 1))), "# not header");
    assert_eq!(text(&line_spans(at(&blocks, 2))), "**not bold**");
    assert!(line_spans(at(&blocks, 4)).last().is_some_and(|s| s.bold));
}

#[test]
fn unterminated_fence_runs_to_end() {
    let blocks = to_blocks("```\ncode\n# still code");
    assert!(blocks.iter().skip(1).all(|b| at(&line_spans(b), 0).semantic == Semantic::Code));
}

#[test]
fn table_with_alignment_row() {
    let blocks = to_blocks("| L | C | R |\n|:--|:-:|--:|\n| a | **b** | c |\ntail");
    assert_eq!(blocks.len(), 2);
    let (columns, rows) = first_table(&blocks);
    let heads: Vec<(&str, Align)> = columns.iter().map(|c| (c.header.as_str(), c.align)).collect();
    assert_eq!(heads, [("L", Align::Left), ("C", Align::Center), ("R", Align::Right)]);
    assert_eq!(rows.len(), 1);
    let cell = at(at(&rows, 0), 1);
    assert_eq!(cell.align, Align::Center);
    assert!(at(&cell.spans, 0).bold);
}

#[test]
fn table_without_separator_and_short_rows() {
    let (columns, rows) = first_table(&to_blocks("| h1 | h2 |\n| only |\n| x | y | extra |"));
    assert!(columns.iter().all(|c| c.align == Align::Left));
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.len() == 2), "rows padded/truncated to column count");
    assert_eq!(text(&at(at(&rows, 0), 1).spans), "");
}
