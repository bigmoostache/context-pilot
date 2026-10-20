//! Placeholder inheritance: a slide placeholder takes its frame, geometry,
//! paint, body properties and list styles from the matching placeholder on
//! its layout, then on the master.
//!
//! Matching follows `PowerPoint`: the layout is searched by `idx` first, then by
//! type; the master by type only, with `ctrTitle` read as `title` and
//! `subTitle`/`obj`/untyped read as `body`.

use super::super::xml::Node;

/// The parts a slide inherits from.
pub(in super::super) struct Layers<'doc> {
    /// `p:sldMaster` root.
    pub master: &'doc Node,
    /// `p:sldLayout` root (absent for master/layout decorations).
    pub layout: Option<&'doc Node>,
    /// `p:presentation/p:defaultTextStyle`.
    pub defaults: Option<&'doc Node>,
}

/// The `p:ph` element of any shape kind (`p:nvSpPr`, `p:nvPicPr`, …).
pub(in super::super) fn ph_of(shape: &Node) -> Option<&Node> {
    shape.children.iter().find(|node| node.name.starts_with("p:nv"))?.path("p:nvPr/p:ph")
}

/// A placeholder's type as the master understands it.
fn master_kind(ph: &Node) -> &str {
    match ph.attr("type").unwrap_or("body") {
        "ctrTitle" | "title" => "title",
        "subTitle" | "obj" | "body" => "body",
        other => other,
    }
}

/// The placeholder shapes of a part's `p:spTree` (top level only).
fn placeholders(part: &Node) -> impl Iterator<Item = (&Node, &Node)> {
    part.path("p:cSld/p:spTree")
        .into_iter()
        .flat_map(|tree| tree.children.iter())
        .filter_map(|shape| Some((shape, ph_of(shape)?)))
}

impl<'doc> Layers<'doc> {
    /// Inherited shapes for placeholder `ph`, nearest first (layout, master).
    pub(in super::super) fn chain(&self, placeholder: Option<&Node>) -> Vec<&'doc Node> {
        let Some(ph) = placeholder else { return Vec::new() };
        let kind = ph.attr("type").unwrap_or("body");
        let idx = ph.attr("idx");
        let from_layout = self.layout.and_then(|layout| {
            let mut found = placeholders(layout);
            let by_idx = idx.and_then(|want| placeholders(layout).find(|pair| pair.1.attr("idx") == Some(want)));
            by_idx.or_else(|| found.find(|pair| pair.1.attr("type").unwrap_or("body") == kind)).map(|pair| pair.0)
        });
        let want = from_layout.and_then(ph_of).map_or_else(|| master_kind(ph), master_kind);
        let from_master = placeholders(self.master).find(|pair| master_kind(pair.1) == want).map(|pair| pair.0);
        from_layout.into_iter().chain(from_master).collect()
    }

    /// List styles for a text body, nearest first: inherited placeholder
    /// `a:lstStyle`s, then the master text style for the placeholder kind,
    /// then the presentation defaults.
    pub(in super::super) fn lists(&self, ph: Option<&Node>, chain: &[&'doc Node]) -> Vec<&'doc Node> {
        let mut out: Vec<&Node> = chain.iter().filter_map(|shape| shape.path("p:txBody/a:lstStyle")).collect();
        let styles = self.master.child("p:txStyles");
        let master_style = match ph.map(master_kind) {
            Some("title") => "p:titleStyle",
            Some("body") => "p:bodyStyle",
            _ => "p:otherStyle",
        };
        if ph.is_some() {
            out.extend(styles.and_then(|node| node.child(master_style)));
        }
        out.extend(self.defaults);
        if ph.is_none() {
            out.extend(styles.and_then(|node| node.child("p:otherStyle")));
        }
        out
    }
}

/// Inherited `a:bodyPr` nodes, nearest first.
pub(in super::super) fn body_props<'doc>(chain: &[&'doc Node]) -> Vec<&'doc Node> {
    chain.iter().filter_map(|shape| shape.path("p:txBody/a:bodyPr")).collect()
}

/// `p:spPr` of the shape and its inherited placeholders, nearest first.
pub(in super::super) fn sp_props<'doc>(own: &'doc Node, chain: &[&'doc Node]) -> Vec<&'doc Node> {
    std::iter::once(own).chain(chain.iter().copied()).filter_map(|shape| shape.child("p:spPr")).collect()
}

#[cfg(test)]
mod tests {
    use super::Layers;
    use crate::transport::inspect::finder::slides::xml::parse;

    #[test]
    fn layout_by_idx_then_master_by_kind() {
        let master = parse(br#"<p:sldMaster><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp></p:spTree></p:cSld></p:sldMaster>"#).unwrap();
        let layout = parse(br#"<p:sldLayout><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp></p:spTree></p:cSld></p:sldLayout>"#).unwrap();
        let slide_ph = parse(br#"<p:ph idx="1"/>"#).unwrap();
        let layers = Layers { master: &master, layout: Some(&layout), defaults: None };
        assert_eq!(layers.chain(Some(&slide_ph)).len(), 2);
    }
}
