//! Text frames: `p:txBody` resolved through the list-style cascade.
//!
//! A paragraph at level *n* takes each property from the first source that
//! sets it: its own `a:pPr`/`a:rPr`, then the shape's `a:lstStyle`, then the
//! inherited lists the caller passes (layout placeholder, master placeholder,
//! master `p:txStyles`, presentation `defaultTextStyle`). The shape's
//! `p:style/a:fontRef` slots in between the local and inherited sources,
//! which is where `PowerPoint` ranks it.

use std::collections::BTreeMap;

use super::color::{Rgba, Theme};
use super::model::{Bullet, Paragraph, Run, TextBody};
use super::part::xml::Node;

/// Default insets (EMU): 0.1" left/right, 0.05" top/bottom.
const INSETS: [i64; 4] = [91_440, 45_720, 91_440, 45_720];

/// Default run size (1/100 pt).
const DEFAULT_SIZE: i64 = 1_800;

/// What a text frame needs from its surroundings.
pub(super) struct TextCtx<'doc> {
    /// Palette and fonts.
    pub theme: &'doc Theme,
    /// Relationships of the part holding the text (hyperlinks).
    pub rels: &'doc BTreeMap<String, String>,
    /// `p:style/a:fontRef` colour.
    pub font_color: Option<Rgba>,
    /// `p:style/a:fontRef` face (`+mj-lt` / `+mn-lt` resolved).
    pub font_face: Option<String>,
}

/// Property sources for one level, most specific first; the first `local`
/// entries come from the shape itself.
struct Chain<'doc> {
    /// Level nodes (`a:pPr`, `a:lvlNpPr`).
    nodes: Vec<&'doc Node>,
    /// How many leading nodes are shape-local.
    local: usize,
}

impl<'doc> Chain<'doc> {
    /// First value of attribute `key`.
    fn attr(&self, key: &str) -> Option<&'doc str> {
        self.nodes.iter().find_map(|node| node.attr(key))
    }

    /// First child named `name`.
    fn child(&self, name: &str) -> Option<&'doc Node> {
        self.nodes.iter().find_map(|node| node.child(name))
    }

    /// First child named `name` among the local, then the inherited, nodes.
    fn split_child(&self, name: &str) -> (Option<&'doc Node>, Option<&'doc Node>) {
        let (local, inherited) = self.nodes.split_at(self.local.min(self.nodes.len()));
        (local.iter().find_map(|node| node.child(name)), inherited.iter().find_map(|node| node.child(name)))
    }

    /// The `a:defRPr` of every node (run-property fallbacks), keeping order.
    fn run_defaults(&self, own: Option<&'doc Node>) -> Self {
        let local = usize::from(own.is_some())
            .saturating_add(self.nodes.iter().take(self.local).filter(|n| n.child("a:defRPr").is_some()).count());
        let nodes = own.into_iter().chain(self.nodes.iter().filter_map(|node| node.child("a:defRPr"))).collect();
        Self { nodes, local }
    }
}

/// Resolve a whole text body. `lists` are the inherited list styles and
/// `body_prs` the inherited `a:bodyPr`s, most specific first.
pub(super) fn body(tx: &Node, lists: &[&Node], body_prs: &[&Node], ctx: &TextCtx<'_>) -> TextBody {
    let own_list = tx.child("a:lstStyle");
    let all_lists: Vec<&Node> = own_list.into_iter().chain(lists.iter().copied()).collect();
    let local = usize::from(own_list.is_some());
    let props = Chain { nodes: tx.child("a:bodyPr").into_iter().chain(body_prs.iter().copied()).collect(), local: 1 };
    let mut counters = [0i64; 9];
    let paragraphs = tx
        .all("a:p")
        .map(|para| {
            let level = para.path("a:pPr").and_then(|p| p.int("lvl")).unwrap_or(0).clamp(0, 8);
            let chain = level_chain(para.child("a:pPr"), &all_lists, local, level);
            paragraph(para, &chain, ctx, (&mut counters, level))
        })
        .collect();
    let int = |key: &str, slot: usize| {
        props.attr(key).and_then(|raw| raw.parse().ok()).unwrap_or_else(|| INSETS.get(slot).copied().unwrap_or(0))
    };
    TextBody {
        anchor: props.attr("anchor").unwrap_or("t").to_owned(),
        insets: [int("lIns", 0), int("tIns", 1), int("rIns", 2), int("bIns", 3)],
        wrap: props.attr("wrap") != Some("none"),
        font_scale: props.child("a:normAutofit").and_then(|fit| fit.int("fontScale")).unwrap_or(100_000),
        vert: props.attr("vert").filter(|vert| *vert != "horz").map(str::to_owned),
        paragraphs,
    }
}

/// Plain text of a body: paragraphs joined by newlines (notes, tooltips).
pub(super) fn plain(tx: &Node) -> String {
    let lines: Vec<String> = tx
        .all("a:p")
        .map(|para| {
            para.children
                .iter()
                .map(|run| {
                    if run.name == "a:br" {
                        "\n".to_owned()
                    } else {
                        run.child("a:t").map(|t| t.text.clone()).unwrap_or_default()
                    }
                })
                .collect()
        })
        .collect();
    lines.join("\n").trim().to_owned()
}

/// The level sources for `level`: the paragraph's own `a:pPr`, then each list.
fn level_chain<'doc>(ppr: Option<&'doc Node>, lists: &[&'doc Node], local: usize, level: i64) -> Chain<'doc> {
    let key = format!("a:lvl{}pPr", level.saturating_add(1));
    let mut nodes: Vec<&Node> = ppr.into_iter().collect();
    let mut local_count = nodes.len();
    for (idx, list) in lists.iter().enumerate() {
        if let Some(node) = list.child(&key) {
            nodes.push(node);
            if idx < local {
                local_count = local_count.saturating_add(1);
            }
        }
    }
    Chain { nodes, local: local_count }
}

/// Resolve one paragraph.
/// `numbering` is the deck-wide auto-number counters and this paragraph's level.
fn paragraph(para: &Node, chain: &Chain<'_>, ctx: &TextCtx<'_>, numbering: (&mut [i64; 9], i64)) -> Paragraph {
    let (counters, level) = numbering;
    let end = chain.run_defaults(para.child("a:endParaRPr"));
    let end_size = end.attr("sz").and_then(|raw| raw.parse().ok()).unwrap_or(DEFAULT_SIZE);
    let runs: Vec<Run> = para.children.iter().filter_map(|child| run_of(child, chain, ctx)).collect();
    Paragraph {
        align: chain.attr("algn").unwrap_or("l").to_owned(),
        margin_left: chain.attr("marL").and_then(|raw| raw.parse().ok()).unwrap_or(0),
        indent: chain.attr("indent").and_then(|raw| raw.parse().ok()).unwrap_or(0),
        line_spacing: spacing(chain.child("a:lnSpc"), 100_000, end_size),
        space_before: spacing(chain.child("a:spcBef"), 0, end_size).abs(),
        space_after: spacing(chain.child("a:spcAft"), 0, end_size).abs(),
        bullet: bullet(chain, ctx.theme, counters, level).filter(|_| !runs.is_empty()),
        end_size,
        runs,
    }
}

/// `a:lnSpc`/`a:spcBef`/`a:spcAft`: `spcPct` → positive 1/1000 percent,
/// `spcPts` → negative hundredths of a point. Before/after percentages are
/// converted to points against `size` (one line ≈ 1.2 × size).
fn spacing(spc: Option<&Node>, dflt: i64, size: i64) -> i64 {
    let Some(node) = spc else { return dflt };
    if let Some(pts) = node.child("a:spcPts").and_then(|pts| pts.int("val")) {
        return pts.saturating_neg();
    }
    let pct = node.child("a:spcPct").and_then(|pct| pct.int("val")).unwrap_or(dflt);
    if dflt == 0 {
        size.saturating_mul(6).saturating_mul(pct).checked_div(500_000).unwrap_or(0).saturating_neg()
    } else {
        pct
    }
}

/// One `a:r` / `a:fld` / `a:br` child as a run; other children are skipped.
fn run_of(child: &Node, chain: &Chain<'_>, ctx: &TextCtx<'_>) -> Option<Run> {
    let text = match child.name.as_str() {
        "a:r" | "a:fld" => child.child("a:t").map(|t| t.text.clone()).unwrap_or_default(),
        "a:br" => "\n".to_owned(),
        _ => return None,
    };
    let props = chain.run_defaults(child.child("a:rPr"));
    Some(Run { text, ..run_props(&props, ctx) })
}

/// Resolve character properties from a run-property chain.
fn run_props(props: &Chain<'_>, ctx: &TextCtx<'_>) -> Run {
    let flag = |key: &str| props.attr(key).is_some_and(|raw| matches!(raw, "1" | "true"));
    Run {
        text: String::new(),
        size: props.attr("sz").and_then(|raw| raw.parse().ok()).unwrap_or(DEFAULT_SIZE),
        bold: flag("b"),
        italic: flag("i"),
        underline: props.attr("u").is_some_and(|raw| raw != "none"),
        strike: props.attr("strike").is_some_and(|raw| raw != "noStrike"),
        color: run_color(props, ctx).unwrap_or(Rgba { red: 0.0, green: 0.0, blue: 0.0, alpha: 1.0 }).css(),
        font: run_font(props, ctx),
        highlight: props.child("a:highlight").and_then(|hl| ctx.theme.color(hl, None)).map(Rgba::css),
        baseline: props.attr("baseline").and_then(|raw| raw.parse().ok()).unwrap_or(0),
        caps: props.attr("cap").filter(|cap| *cap != "none").map(str::to_owned),
        link: props.child("a:hlinkClick").and_then(|link| link.attr("r:id")).and_then(|id| ctx.rels.get(id)).cloned(),
    }
}

/// Local fill → `fontRef` colour → inherited fill.
fn run_color(props: &Chain<'_>, ctx: &TextCtx<'_>) -> Option<Rgba> {
    let (local, inherited) = props.split_child("a:solidFill");
    local.and_then(|fill| ctx.theme.color(fill, None)).or(ctx.font_color).or_else(|| ctx.theme.color(inherited?, None))
}

/// Local face → `fontRef` face → inherited face → theme body font.
fn run_font(props: &Chain<'_>, ctx: &TextCtx<'_>) -> String {
    let (local, inherited) = props.split_child("a:latin");
    let face = |node: Option<&Node>| node.and_then(|latin| latin.attr("typeface")).map(|face| ctx.theme.font(face));
    face(local).or_else(|| ctx.font_face.clone()).or_else(|| face(inherited)).unwrap_or_else(|| ctx.theme.minor.clone())
}

/// The paragraph's bullet, advancing the auto-number counters.
fn bullet(chain: &Chain<'_>, theme: &Theme, counters: &mut [i64; 9], level: i64) -> Option<Bullet> {
    let slot = usize::try_from(level).unwrap_or(0);
    for deeper in counters.iter_mut().skip(slot.saturating_add(1)) {
        *deeper = 0;
    }
    let kind = chain.nodes.iter().find_map(|node| {
        node.children.iter().find(|c| matches!(c.name.as_str(), "a:buNone" | "a:buChar" | "a:buAutoNum"))
    });
    let text = match kind.map(|node| (node.name.as_str(), node)) {
        Some(("a:buChar", node)) => node.attr("char").unwrap_or("\u{2022}").to_owned(),
        Some(("a:buAutoNum", node)) => {
            let count = counters.get_mut(slot)?;
            *count = count.saturating_add(1).max(node.int("startAt").unwrap_or(1));
            autonum(node.attr("type").unwrap_or("arabicPeriod"), *count)
        }
        _ => {
            if let Some(count) = counters.get_mut(slot) {
                *count = 0;
            }
            return None;
        }
    };
    let color = chain
        .nodes
        .iter()
        .find_map(|node| node.children.iter().find(|c| matches!(c.name.as_str(), "a:buClr" | "a:buClrTx")));
    Some(Bullet {
        text,
        color: color.filter(|node| node.name == "a:buClr").and_then(|node| theme.color(node, None)).map(Rgba::css),
        font: chain.child("a:buFont").and_then(|font| font.attr("typeface")).map(|face| theme.font(face)),
        scale: chain.child("a:buSzPct").and_then(|pct| pct.int("val")).unwrap_or(100_000),
    })
}

/// Auto-number suffix → (prefix, suffix) wrapping.
const SUFFIXES: [(&str, &str, &str); 3] = [("ParenBoth", "(", ")"), ("ParenR", "", ")"), ("Plain", "", "")];

/// Format an auto-number (`arabicPeriod`, `alphaLcParenR`, `romanUcPeriod`…).
fn autonum(kind: &str, value: i64) -> String {
    let body = match kind.get(..7).unwrap_or("") {
        "alphaLc" => alpha(value).to_ascii_lowercase(),
        "alphaUc" => alpha(value),
        "romanLc" => roman(value).to_ascii_lowercase(),
        "romanUc" => roman(value),
        _ => value.to_string(),
    };
    let (open, close) =
        SUFFIXES.iter().find(|entry| kind.ends_with(entry.0)).map_or(("", "."), |entry| (entry.1, entry.2));
    format!("{open}{body}{close}")
}

/// 1 → A, 27 → AA.
fn alpha(value: i64) -> String {
    let mut left = value.max(1);
    let mut out = Vec::new();
    while left > 0 {
        let rem = left.saturating_sub(1).rem_euclid(26);
        out.push(char::from(b'A'.saturating_add(u8::try_from(rem).unwrap_or(0))));
        left = left.saturating_sub(1).checked_div(26).unwrap_or(0);
    }
    out.iter().rev().collect()
}

/// Upper-case Roman numeral (values above 3999 fall back to digits).
fn roman(value: i64) -> String {
    if !(1..4000).contains(&value) {
        return value.to_string();
    }
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut left = value;
    let mut out = String::new();
    for (unit, glyph) in table {
        while left >= unit {
            out.push_str(glyph);
            left = left.saturating_sub(unit);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{autonum, plain};
    use crate::transport::inspect::finder::slides::part::xml::parse;

    #[test]
    fn autonum_formats() {
        assert_eq!(autonum("arabicPeriod", 3), "3.");
        assert_eq!(autonum("alphaLcParenR", 28), "ab)");
        assert_eq!(autonum("romanUcPeriod", 14), "XIV.");
        assert_eq!(autonum("arabicParenBoth", 2), "(2)");
    }

    #[test]
    fn plain_text_joins_paragraphs() {
        let tx = parse(b"<p:txBody><a:p><a:r><a:t>Hi</a:t></a:r><a:br/><a:r><a:t>there</a:t></a:r></a:p><a:p><a:r><a:t>!</a:t></a:r></a:p></p:txBody>").unwrap();
        assert_eq!(plain(&tx), "Hi\nthere\n!");
    }
}
