use core::ops::Range;

/// Why `old_string` could not be pinned to exactly one place in the file.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MatchError {
    /// `old_string` is empty: it would match everywhere.
    Empty,
    /// No literal or whitespace-tolerant occurrence.
    NotFound,
    /// Several occurrences; carries their 1-based start lines.
    Ambiguous(Vec<usize>),
}

impl MatchError {
    /// One-line, model-facing explanation (`NotFound` is enriched by the caller).
    pub(crate) fn describe(self) -> String {
        match self {
            Self::Empty => "old_string is empty".to_owned(),
            Self::NotFound => "old_string not found".to_owned(),
            Self::Ambiguous(lines) => {
                let list = lines.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
                format!(
                    "old_string matches {} places (lines {list}). Include more surrounding lines to make it unique.",
                    lines.len()
                )
            }
        }
    }
}

/// Byte layout of one haystack line.
struct LineSpan {
    /// Offset of the first byte of the line.
    start: usize,
    /// Offset just past the content, before any `\r\n` / `\n`.
    content_end: usize,
    /// Offset just past the line break (equals `content_end` on an unterminated last line).
    end: usize,
}

/// Split `hay` into lines with exact byte offsets. Unlike `str::lines`, the
/// `\r` of a CRLF break is accounted for, so offsets never drift.
fn line_spans(hay: &str) -> Vec<LineSpan> {
    let mut spans = Vec::new();
    let mut start = 0usize;
    for raw in hay.split_inclusive('\n') {
        let body = raw.strip_suffix('\n').map_or(raw, |line| line.strip_suffix('\r').unwrap_or(line));
        let end = start.saturating_add(raw.len());
        spans.push(LineSpan { start, content_end: start.saturating_add(body.len()), end });
        start = end;
    }
    spans
}

/// 1-based line number of byte offset `pos`.
fn line_of(hay: &str, pos: usize) -> usize {
    hay.get(..pos).map_or(0, |head| head.matches('\n').count()).saturating_add(1)
}

/// `true` when every line break in `hay` is CRLF (and there is at least one).
fn is_crlf(hay: &str) -> bool {
    let crlf = hay.matches("\r\n").count();
    crlf > 0 && crlf == hay.matches('\n').count()
}

/// Rewrite every line break of `text` as CRLF.
fn to_crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// Literal occurrences of `needle`, also trying its CRLF form in a CRLF file.
fn exact_spans(hay: &str, needle: &str) -> Vec<Range<usize>> {
    let mut found: Vec<Range<usize>> =
        hay.match_indices(needle).map(|(pos, _)| pos..pos.saturating_add(needle.len())).collect();
    let crlf = to_crlf(needle);
    if crlf != needle && is_crlf(hay) {
        found.extend(hay.match_indices(crlf.as_str()).map(|(pos, _)| pos..pos.saturating_add(crlf.len())));
    }
    found
}

/// Whether haystack lines from `first` on equal `needle` lines modulo trailing whitespace.
fn window_matches(hay: &str, spans: &[LineSpan], first: usize, needle: &[&str]) -> bool {
    needle.iter().enumerate().all(|(offset, want)| {
        spans
            .get(first.saturating_add(offset))
            .and_then(|span| hay.get(span.start..span.content_end))
            .is_some_and(|got| got.trim_end() == *want)
    })
}

/// Whole-line occurrences of `needle`, ignoring trailing whitespace and line-ending style.
///
/// The span starts at the first matched line and ends after its content, or
/// after its line break when `needle` itself ends with a newline (so the
/// break is replaced, never silently kept or dropped).
fn fuzzy_spans(hay: &str, needle: &str) -> Vec<Range<usize>> {
    let unified = needle.replace("\r\n", "\n");
    let wants_break = unified.ends_with('\n');
    let body = unified.strip_suffix('\n').unwrap_or(&unified);
    let wanted: Vec<&str> = body.split('\n').map(str::trim_end).collect();
    let spans = line_spans(hay);
    let window = wanted.len();
    let last_offset = window.saturating_sub(1);
    (0..spans.len().saturating_sub(window).saturating_add(1))
        .filter(|&first| window_matches(hay, &spans, first, &wanted))
        .filter_map(|first| {
            let head = spans.get(first)?;
            let tail = spans.get(first.saturating_add(last_offset))?;
            let end = if wants_break { (tail.end > tail.content_end).then_some(tail.end)? } else { tail.content_end };
            Some(head.start..end)
        })
        .collect()
}

/// Drop candidates overlapping an earlier one (same occurrence found twice).
fn distinct(mut found: Vec<Range<usize>>) -> Vec<Range<usize>> {
    found.sort_by_key(|span| span.start);
    let mut kept: Vec<Range<usize>> = Vec::new();
    for span in found {
        if kept.last().is_none_or(|prev| span.start >= prev.end) {
            kept.push(span);
        }
    }
    kept
}

/// Locate the single occurrence of `needle` in `hay`.
///
/// Literal matches win; whole-line whitespace-tolerant matching is the
/// fallback. Zero or several occurrences are errors: an edit never guesses.
pub(crate) fn locate(hay: &str, needle: &str) -> Result<Range<usize>, MatchError> {
    if needle.is_empty() {
        return Err(MatchError::Empty);
    }
    let exact = distinct(exact_spans(hay, needle));
    let found = if exact.is_empty() { distinct(fuzzy_spans(hay, needle)) } else { exact };
    let mut iter = found.into_iter();
    match (iter.next(), iter.next()) {
        (None, _) => Err(MatchError::NotFound),
        (Some(only), None) => Ok(only),
        (Some(first), Some(second)) => {
            let mut lines = vec![line_of(hay, first.start), line_of(hay, second.start)];
            lines.extend(iter.map(|span| line_of(hay, span.start)));
            Err(MatchError::Ambiguous(lines))
        }
    }
}

/// Replace the single occurrence of `old` with `new` at its exact position.
/// In a CRLF file, `new`'s line breaks are written as CRLF.
pub(crate) fn apply(hay: &str, old: &str, new: &str) -> Result<String, MatchError> {
    let span = locate(hay, old)?;
    let replacement = if is_crlf(hay) { to_crlf(new) } else { new.to_owned() };
    let (Some(before), Some(after)) = (hay.get(..span.start), hay.get(span.end..)) else {
        return Err(MatchError::NotFound);
    };
    Ok([before, replacement.as_str(), after].concat())
}

/// Matcher regression tests (one per bug in the cool-meter review).
#[cfg(test)]
mod tests {
    use super::{MatchError, apply, locate};

    /// A mid-line fragment matches literally.
    #[test]
    fn fragment_inside_line() {
        assert_eq!(apply("let x = 1;\n", "= 1", "= 2"), Ok("let x = 2;\n".to_owned()));
    }

    /// The replacement lands at the matched line, not an earlier substring hit.
    #[test]
    fn replaces_at_match_not_first_substring() {
        let hay = "  call();   \ncall();\n";
        // Literal "call();   " exists only on line 1 → unique.
        assert_eq!(apply(hay, "call();   \n", "x();\n"), Ok("  x();\ncall();\n".to_owned()));
        // Whitespace-tolerant whole-line match on line 1 only.
        let hay2 = "foo(a)  \nbar\n";
        assert_eq!(apply(hay2, "foo(a)\nbar", "baz"), Ok("baz\n".to_owned()));
    }

    /// Two occurrences are refused with both line numbers.
    #[test]
    fn ambiguous_is_refused() {
        assert_eq!(locate("a();\nb();\na();\n", "a();"), Err(MatchError::Ambiguous(vec![1, 3])));
    }

    /// Fuzzy ambiguity is refused too.
    #[test]
    fn fuzzy_ambiguous_is_refused() {
        assert_eq!(locate("x  \ny\nx \ny\n", "x\ny"), Err(MatchError::Ambiguous(vec![1, 3])));
    }

    /// CRLF file: offsets don't drift and new lines are written as CRLF.
    #[test]
    fn crlf_file_is_safe() {
        let hay = "one\r\ntwo\r\nthree\r\nfour\r\n";
        assert_eq!(apply(hay, "three\nfour\n", "3\n4\n"), Ok("one\r\ntwo\r\n3\r\n4\r\n".to_owned()));
        assert_eq!(apply(hay, "three  \nfour", "3"), Ok("one\r\ntwo\r\n3\r\n".to_owned()));
    }

    /// A trailing newline in `old_string` is consumed and replaced, never kept or merged.
    #[test]
    fn trailing_newline_kept_consistent() {
        let hay = "a\nb  \nc\n";
        assert_eq!(apply(hay, "b\n", "B\n"), Ok("a\nB\nc\n".to_owned()));
        assert_eq!(apply(hay, "b\n", ""), Ok("a\nc\n".to_owned()));
        // Literal hit: only the matched bytes change, trailing spaces stay.
        assert_eq!(apply(hay, "b", "B"), Ok("a\nB  \nc\n".to_owned()));
    }

    /// Wanting a newline after an unterminated last line is not a match.
    #[test]
    fn trailing_newline_at_eof_without_break() {
        assert_eq!(locate("a\nb  ", "b\n"), Err(MatchError::NotFound));
    }

    /// Empty needle is rejected.
    #[test]
    fn empty_needle() {
        assert_eq!(locate("abc", ""), Err(MatchError::Empty));
    }

    /// Multi-byte text keeps char boundaries.
    #[test]
    fn utf8_text() {
        assert_eq!(apply("\u{e9}\u{2192}\u{fc}\nzz\n", "\u{2192}\u{fc}", "!"), Ok("\u{e9}!\nzz\n".to_owned()));
    }
}
