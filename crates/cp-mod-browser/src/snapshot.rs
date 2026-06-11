//! Page snapshot → compact e-ref representation.
//!
//! A JS DOM walk collects interactive elements (links, buttons, inputs…),
//! assigns each a stable short ref (`e1`, `e2`, …) and a unique CSS selector.
//! The agent then acts on refs (`browser_click {ref: "e12"}`) instead of
//! guessing brittle selectors. The verbose tree lives in the Browser panel,
//! never inline in the conversation.

use std::fmt::Write as _;

use crate::types::Eref;

/// JS executed in-page to enumerate interactive elements.
///
/// Returns `[{role, label, selector}]`. Selector preference: `#id` →
/// `tag[name=…]` → nth-of-type path (always unique, stable enough between
/// snapshots of the same DOM).
pub const SNAPSHOT_JS: &str = r#"
(() => {
  const out = [];
  const seen = new Set();
  const cssPath = (el) => {
    if (el.id) return '#' + CSS.escape(el.id);
    const name = el.getAttribute && el.getAttribute('name');
    if (name) {
      const sel = el.tagName.toLowerCase() + '[name="' + CSS.escape(name) + '"]';
      if (document.querySelectorAll(sel).length === 1) return sel;
    }
    const parts = [];
    let n = el;
    while (n && n.nodeType === 1 && n !== document.body) {
      let i = 1, sib = n;
      while ((sib = sib.previousElementSibling)) if (sib.tagName === n.tagName) i++;
      parts.unshift(n.tagName.toLowerCase() + ':nth-of-type(' + i + ')');
      n = n.parentElement;
    }
    return 'body > ' + parts.join(' > ');
  };
  const label = (el) => {
    const t = (el.getAttribute('aria-label') || el.innerText || el.value
      || el.getAttribute('placeholder') || el.getAttribute('title') || el.getAttribute('alt') || '')
      .trim().replace(/\s+/g, ' ');
    return t.slice(0, 80);
  };
  const role = (el) => {
    const tag = el.tagName.toLowerCase();
    if (tag === 'input') return 'input:' + (el.type || 'text');
    if (tag === 'a') return 'link';
    return tag;
  };
  const els = document.querySelectorAll(
    'a[href], button, input:not([type=hidden]), select, textarea, [role=button], [role=link], [onclick]');
  for (const el of els) {
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) continue;
    const sel = cssPath(el);
    if (seen.has(sel)) continue;
    seen.add(sel);
    out.push({ role: role(el), label: label(el), selector: sel });
    if (out.length >= 200) break;
  }
  return JSON.stringify(out);
})()
"#;

/// Parse the snapshot JSON into an e-ref table.
#[must_use]
pub fn parse(value: &serde_json::Value) -> Vec<Eref> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let selector = item.get("selector")?.as_str()?.to_string();
            Some(Eref {
                id: format!("e{}", i.saturating_add(1)),
                selector,
                role: item.get("role").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
                label: item.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// Render the e-ref table as compact aligned text for the Browser panel.
#[must_use]
pub fn render_erefs(erefs: &[Eref]) -> String {
    let mut out = String::new();
    for e in erefs {
        let _w = writeln!(out, "  {:<5} {:<12} {}", e.id, e.role, compact(&e.label, &e.selector));
    }
    out
}

/// Label if present, otherwise the selector as fallback identification.
const fn compact<'txt>(label: &'txt str, selector: &'txt str) -> &'txt str {
    if label.is_empty() { selector } else { label }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_snapshot_items_into_erefs() {
        let v = serde_json::json!([
            {"role": "link", "label": "Home", "selector": "#home"},
            {"role": "input:text", "label": "Email", "selector": "input[name=\"email\"]"}
        ]);
        let erefs = parse(&v);
        assert_eq!(erefs.len(), 2, "two items parsed");
        let first = erefs.first().map(|e| (e.id.as_str(), e.selector.as_str()));
        assert_eq!(first, Some(("e1", "#home")), "ids start at e1 and selector preserved");
    }

    #[test]
    fn skips_items_without_selector() {
        let v = serde_json::json!([{"role": "button", "label": "x"}]);
        assert!(parse(&v).is_empty(), "items without selector are dropped");
    }
}
