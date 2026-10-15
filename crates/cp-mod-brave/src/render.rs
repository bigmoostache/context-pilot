//! Compact text rendering of Brave responses for result panels.
//!
//! The raw serde dump carried `null` fields, echo fields, Brave's `<strong>` highlight
//! tags and (for LLM context) a `sources` map duplicating every URL/title. The model
//! pays for every panel token on every turn, so only the useful fields are rendered.

use std::fmt::Write as _;

use crate::types::{BraveSearchResponse, LLMContextResponse};

/// Render web results (and optional rich data) as a numbered list.
pub(crate) fn search(resp: &BraveSearchResponse, rich: Option<&serde_json::Value>) -> String {
    let mut out = String::new();
    if let Some(data) = rich {
        out.push_str("# Rich result\n");
        out.push_str(&serde_yaml::to_string(data).unwrap_or_default());
        out.push('\n');
    }
    let results = resp.web.as_ref().map_or(&[][..], |w| w.results.as_slice());
    if !results.is_empty() {
        out.push_str("# Web results\n");
    }
    for (i, r) in results.iter().enumerate() {
        line(&mut out, format_args!("{}. {}", i.saturating_add(1), clean(r.title.as_deref().unwrap_or("(untitled)"))));
        let url = r.url.as_deref().unwrap_or("");
        match r.age.as_deref() {
            Some(age) => line(&mut out, format_args!("   {url} ({age})")),
            None => line(&mut out, format_args!("   {url}")),
        }
        if let Some(d) = r.description.as_deref() {
            line(&mut out, format_args!("   {}", clean(d)));
        }
        for s in r.extra_snippets.iter().flatten() {
            line(&mut out, format_args!("   - {}", clean(s)));
        }
    }
    out
}

/// Render grounding items, one section per source page. `sources` is dropped (it repeats grounding).
pub(crate) fn llm_context(resp: &LLMContextResponse) -> String {
    let mut out = String::new();
    let items = resp.grounding.as_ref().and_then(|g| g.generic.as_ref()).map_or(&[][..], Vec::as_slice);
    for item in items {
        line(&mut out, format_args!("## {}", clean(item.title.as_deref().unwrap_or("(untitled)"))));
        line(&mut out, format_args!("{}", item.url.as_deref().unwrap_or("")));
        for snippet in item.snippets.iter().flatten() {
            match snippet.as_str() {
                Some(text) => line(&mut out, format_args!("{}", clean(text))),
                None => out.push_str(&serde_yaml::to_string(snippet).unwrap_or_default()),
            }
        }
        out.push('\n');
    }
    out
}

/// Append one formatted line. Writing to a `String` cannot fail, so the `fmt::Result` is discarded once, here.
fn line(out: &mut String, args: std::fmt::Arguments<'_>) {
    let _infallible = out.write_fmt(args);
    out.push('\n');
}

/// Strip HTML tags and decode the few entities Brave emits in snippets.
pub(crate) fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::{clean, llm_context, search};
    use crate::types::{BraveSearchResponse, LLMContextResponse};

    #[test]
    fn clean_strips_tags_and_entities() {
        assert_eq!(clean("<strong>Rust</strong> &amp; C &lt;3"), "Rust & C <3");
        assert_eq!(clean("a > b"), "a > b");
    }

    #[test]
    fn search_is_compact() {
        let resp: BraveSearchResponse = serde_json::from_str(
            r#"{"type":"search","query":{"original":"q"},"web":{"results":[
                {"title":"<strong>T</strong>","url":"https://x.dev","description":"d","age":null},
                {"title":"U","url":"https://y.dev","extra_snippets":["s1"],"age":"2 days ago"}]}}"#,
        )
        .expect("fixture parses");
        let out = search(&resp, None);
        assert_eq!(out, "# Web results\n1. T\n   https://x.dev\n   d\n2. U\n   https://y.dev (2 days ago)\n   - s1\n");
        assert!(!out.contains("null"));
    }

    #[test]
    fn llm_context_drops_sources() {
        let resp: LLMContextResponse = serde_json::from_str(
            r#"{"grounding":{"generic":[{"url":"https://x.dev","title":"X","snippets":["one",{"k":"v"}]}]},
                "sources":{"https://x.dev":{"title":"X"}}}"#,
        )
        .expect("fixture parses");
        let out = llm_context(&resp);
        assert_eq!(out, "## X\nhttps://x.dev\none\nk: v\n\n");
    }
}
