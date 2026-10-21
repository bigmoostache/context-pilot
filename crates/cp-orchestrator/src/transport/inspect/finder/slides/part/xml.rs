//! Minimal owned XML tree over `quick-xml`, enough for OOXML parts.
//!
//! `PresentationML` is navigated by nesting (`p:spPr/a:xfrm/a:off`), and the
//! style cascade revisits the same nodes many times, so a small DOM is simpler
//! and cheaper than re-streaming. Element names keep their prefix (`a:off`):
//! Office always writes the canonical `a`/`p`/`r` prefixes, so matching on the
//! qualified name is unambiguous in practice.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// Deepest element nesting accepted; deeper input is treated as malformed.
const MAX_DEPTH: usize = 256;

/// One XML element: qualified name, attributes, child elements, direct text.
#[derive(Debug, Default, Clone)]
pub(in crate::transport::inspect::finder::slides) struct Node {
    /// Qualified element name (`a:off`).
    pub name: String,
    /// Attributes as (qualified name, unescaped value), document order.
    pub attrs: Vec<(String, String)>,
    /// Child elements, document order.
    pub children: Vec<Self>,
    /// Concatenated character data directly inside this element.
    pub text: String,
}

impl Node {
    /// Attribute value by qualified name.
    pub(in crate::transport::inspect::finder::slides) fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|pair| pair.0 == key).map(|pair| pair.1.as_str())
    }

    /// Attribute parsed as an integer.
    pub(in crate::transport::inspect::finder::slides) fn int(&self, key: &str) -> Option<i64> {
        self.attr(key)?.trim().parse().ok()
    }

    /// Boolean attribute (`1`/`true`).
    pub(in crate::transport::inspect::finder::slides) fn flag(&self, key: &str) -> Option<bool> {
        self.attr(key).map(|raw| matches!(raw, "1" | "true"))
    }

    /// First child with this name.
    pub(in crate::transport::inspect::finder::slides) fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|node| node.name == name)
    }

    /// Every child with this name.
    pub(in crate::transport::inspect::finder::slides) fn all<'node>(
        &'node self,
        name: &'node str,
    ) -> impl Iterator<Item = &'node Self> + 'node {
        self.children.iter().filter(move |node| node.name == name)
    }

    /// Descend a `/`-separated child path (`p:spPr/a:xfrm`).
    pub(in crate::transport::inspect::finder::slides) fn path(&self, route: &str) -> Option<&Self> {
        route.split('/').try_fold(self, |node, step| node.child(step))
    }

    /// Name without its prefix (`a:off` → `off`).
    pub(in crate::transport::inspect::finder::slides) fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }
}

/// Parse a document; `None` when it is not well-formed.
pub(in crate::transport::inspect::finder::slides) fn parse(bytes: &[u8]) -> Option<Node> {
    let mut reader = Reader::from_reader(bytes);
    let mut stack: Vec<Node> = vec![Node::default()];
    let mut buf = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf).ok()?;
        if !apply(&mut stack, event)? {
            break;
        }
        buf.clear();
    }
    let root = stack.pop()?;
    root.children.into_iter().next()
}

/// Fold one event into the open-element stack; `Some(false)` at end of input.
fn apply(stack: &mut Vec<Node>, event: Event<'_>) -> Option<bool> {
    match event {
        Event::Start(tag) => {
            if stack.len() > MAX_DEPTH {
                return None;
            }
            stack.push(element(&tag));
        }
        Event::Empty(tag) => stack.last_mut()?.children.push(element(&tag)),
        Event::End(_) => {
            let done = stack.pop()?;
            stack.last_mut()?.children.push(done);
        }
        Event::Text(text) => stack.last_mut()?.text.push_str(&text.decode().ok()?),
        Event::CData(data) => stack.last_mut()?.text.push_str(&data.decode().ok()?),
        Event::GeneralRef(entity) => push_entity(stack.last_mut()?, &entity),
        Event::Eof => return Some(false),
        Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
    }
    Some(true)
}

/// Append a resolved `&…;` reference (character or predefined entity).
fn push_entity(node: &mut Node, entity: &quick_xml::events::BytesRef<'_>) {
    if let Ok(Some(ch)) = entity.resolve_char_ref() {
        node.text.push(ch);
        return;
    }
    let name = entity.decode().unwrap_or_default();
    let resolved = match name.as_ref() {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "apos" => '\'',
        "quot" => '"',
        _ => return,
    };
    node.text.push(resolved);
}

/// Build an element (no children yet) from a start tag.
fn element(tag: &BytesStart<'_>) -> Node {
    let name = String::from_utf8_lossy(tag.name().as_ref()).into_owned();
    let attrs = tag
        .attributes()
        .filter_map(Result::ok)
        .filter_map(|attr| {
            let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
            Some((key, attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.into_owned()))
        })
        .collect();
    Node { name, attrs, children: Vec::new(), text: String::new() }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_nested_attrs_and_entities() {
        let doc = parse(br#"<p:sld><a:off x="10" y="2"/><a:t>A &amp; B&#233;</a:t></p:sld>"#).unwrap();
        assert_eq!(doc.name, "p:sld");
        assert_eq!(doc.path("a:off").and_then(|n| n.int("x")), Some(10));
        assert_eq!(doc.child("a:t").map(|n| n.text.as_str()), Some("A & B\u{e9}"));
        assert_eq!(doc.child("a:off").map(super::Node::local), Some("off"));
    }

    #[test]
    fn rejects_malformed() {
        assert!(parse(b"<a><b></a>").is_none());
    }
}
