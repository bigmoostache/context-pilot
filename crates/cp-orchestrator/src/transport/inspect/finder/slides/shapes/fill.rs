//! Paint: fills, outlines and backgrounds as CSS values.
//!
//! An explicit `p:spPr` paint wins; otherwise the shape's `p:style` reference
//! (`fillRef`/`lnRef idx`) picks an entry of the theme's format scheme, with
//! the reference's colour standing in for `phClr`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::super::color::{Rgba, Theme};
use super::super::model::Stroke;
use super::super::part::package::Package;
use super::super::part::xml::Node;

/// Paint lookups for one part.
pub(in super::super) struct Paint<'doc> {
    /// Palette and format scheme.
    pub theme: &'doc Theme,
    /// Package (embedded images).
    pub pkg: &'doc Package,
    /// Relationships of the part being painted.
    pub rels: &'doc BTreeMap<String, String>,
}

/// What a fill resolved to.
#[derive(Debug, Default)]
pub(in super::super) struct Fill {
    /// CSS paint for the shape interior.
    pub css: Option<String>,
    /// An embedded picture (`a:blipFill`).
    pub image: Option<String>,
    /// `true` when the source said `noFill` explicitly (stops inheritance).
    pub none: bool,
}

impl Paint<'_> {
    /// The first fill element among `holder`'s children, if any.
    pub(in super::super) fn fill(&self, holder: &Node, ph: Option<Rgba>) -> Option<Fill> {
        holder.children.iter().find_map(|node| self.fill_node(node, ph))
    }

    /// One fill element.
    fn fill_node(&self, node: &Node, ph: Option<Rgba>) -> Option<Fill> {
        Some(match node.name.as_str() {
            "a:noFill" => Fill { none: true, ..Fill::default() },
            "a:solidFill" => Fill { css: self.theme.color(node, ph).map(Rgba::css), ..Fill::default() },
            "a:gradFill" => Fill { css: gradient(self.theme, node, ph), ..Fill::default() },
            "a:pattFill" => Fill {
                css: node.child("a:fgClr").and_then(|fg| self.theme.color(fg, ph)).map(Rgba::css),
                ..Fill::default()
            },
            "a:blipFill" => Fill { image: self.blip(node), ..Fill::default() },
            _ => return None,
        })
    }

    /// The image referenced by an `a:blipFill`.
    pub(in super::super) fn blip(&self, blip_fill: &Node) -> Option<String> {
        let id = blip_fill.child("a:blip")?.attr("r:embed")?;
        self.pkg.data_uri(self.rels.get(id)?)
    }

    /// Shape interior: `spPr` fill, else the style's `fillRef`.
    pub(in super::super) fn shape_fill(&self, sp_pr: Option<&Node>, style: Option<&Node>) -> Fill {
        if let Some(fill) = sp_pr.and_then(|node| self.fill(node, None)) {
            return fill;
        }
        let Some(fill_ref) = style.and_then(|node| node.child("a:fillRef")) else { return Fill::default() };
        let ph = self.theme.color(fill_ref, None);
        let idx = fill_ref.int("idx").unwrap_or(0);
        let entry = usize::try_from(idx.saturating_sub(1)).ok().and_then(|slot| self.theme.fills.get(slot));
        entry.and_then(|node| self.fill_node(node, ph)).unwrap_or_default()
    }

    /// Shape outline: `spPr/a:ln` over the style's `lnRef` theme line.
    pub(in super::super) fn stroke(&self, sp_pr: Option<&Node>, style: Option<&Node>) -> Option<Stroke> {
        let line_ref = style.and_then(|node| node.child("a:lnRef"));
        let ph = line_ref.and_then(|node| self.theme.color(node, None));
        let theme_line = line_ref
            .and_then(|node| usize::try_from(node.int("idx")?.saturating_sub(1)).ok())
            .and_then(|slot| self.theme.lines.get(slot));
        let own = sp_pr.and_then(|node| node.child("a:ln"));
        let sources: Vec<&Node> = own.into_iter().chain(theme_line).collect();
        self.line(&sources, ph)
    }

    /// Resolve a line from `a:ln` sources, most specific first.
    pub(in super::super) fn line(&self, sources: &[&Node], ph: Option<Rgba>) -> Option<Stroke> {
        let fill = sources.iter().find_map(|ln| self.fill(ln, ph))?;
        let color = fill.css.filter(|_| !fill.none)?;
        let width = sources.iter().find_map(|ln| ln.int("w")).unwrap_or(9_525);
        let child_attr =
            |name: &str, key: &str| sources.iter().find_map(|ln| ln.child(name)?.attr(key)).map(str::to_owned);
        Some(Stroke {
            color,
            width,
            dash: child_attr("a:prstDash", "val").filter(|dash| dash != "solid"),
            head: child_attr("a:tailEnd", "type").filter(|kind| kind != "none"),
            tail: child_attr("a:headEnd", "type").filter(|kind| kind != "none"),
        })
    }

    /// Background of a `p:cSld`: `p:bgPr` fill or `p:bgRef` theme entry.
    pub(in super::super) fn background(&self, c_sld: &Node) -> Option<String> {
        let bg = c_sld.child("p:bg")?;
        if let Some(props) = bg.child("p:bgPr") {
            let fill = self.fill(props, None)?;
            return fill.css.or_else(|| fill.image.map(|uri| format!("center / cover no-repeat url(\"{uri}\")")));
        }
        let bg_ref = bg.child("p:bgRef")?;
        let ph = self.theme.color(bg_ref, None);
        let idx = bg_ref.int("idx").unwrap_or(0);
        let entry = if idx >= 1001 {
            usize::try_from(idx.saturating_sub(1001)).ok().and_then(|slot| self.theme.bg_fills.get(slot))
        } else {
            usize::try_from(idx.saturating_sub(1)).ok().and_then(|slot| self.theme.fills.get(slot))
        };
        let fill = entry.and_then(|node| self.fill_node(node, ph))?;
        fill.css.or_else(|| ph.map(Rgba::css))
    }
}

/// `a:gradFill` → CSS `linear-gradient` (or `radial-gradient` for paths).
fn gradient(theme: &Theme, grad: &Node, ph: Option<Rgba>) -> Option<String> {
    let mut stops: Vec<(i64, String)> = grad
        .path("a:gsLst")?
        .all("a:gs")
        .filter_map(|stop| Some((stop.int("pos").unwrap_or(0), theme.color(stop, ph)?.css())))
        .collect();
    stops.sort_by_key(|stop| stop.0);
    let mut list = String::new();
    for stop in &stops {
        let _w = write!(list, ", {} {}%", stop.1, stop.0.checked_div(1_000).unwrap_or(0));
    }
    if grad.child("a:path").is_some() {
        return Some(format!("radial-gradient(circle{list})"));
    }
    let angle = grad.path("a:lin").and_then(|lin| lin.int("ang")).unwrap_or(0).checked_div(60_000).unwrap_or(0);
    Some(format!("linear-gradient({}deg{list})", angle.saturating_add(90)))
}

#[cfg(test)]
mod tests {
    use super::gradient;
    use crate::transport::inspect::finder::slides::color::Theme;
    use crate::transport::inspect::finder::slides::part::xml::parse;

    #[test]
    fn linear_gradient_css() {
        let theme = Theme::empty();
        let grad = parse(br#"<a:gradFill><a:gsLst><a:gs pos="100000"><a:srgbClr val="FFFFFF"/></a:gs><a:gs pos="0"><a:srgbClr val="000000"/></a:gs></a:gsLst><a:lin ang="5400000"/></a:gradFill>"#).unwrap();
        assert_eq!(gradient(&theme, &grad, None).unwrap(), "linear-gradient(180deg, #000000 0%, #ffffff 100%)");
    }
}
