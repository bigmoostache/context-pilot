//! The shape tree (`p:spTree`) flattened into positioned [`Shape`]s.
//!
//! Groups are flattened by mapping each child's frame through the group's
//! `chOff`/`chExt` → `off`/`ext` transform, so the frontend never nests.
//! Placeholder shapes inherit frame, geometry, paint and text style from the
//! layout and master (see [`inherit`]).

mod fill;
mod inherit;
mod table;

pub(super) use fill::Paint;
pub(super) use inherit::Layers;

use super::color::Rgba;
use super::geom;
use super::model::Shape;
use super::text::{self, TextCtx};
use super::xml::Node;

/// Most shapes emitted per slide; the rest are dropped.
const MAX_SHAPES: usize = 2_000;

/// Deepest group nesting followed.
const MAX_GROUP_DEPTH: usize = 32;

/// Everything shape conversion reads.
pub(super) struct Ctx<'doc> {
    /// Paint lookups for the part whose tree is walked.
    pub paint: Paint<'doc>,
    /// Layout/master inheritance.
    pub layers: Layers<'doc>,
    /// `ppt/tableStyles.xml` root (`a:tblStyleLst`).
    pub table_styles: Option<&'doc Node>,
}

/// Child → parent coordinate mapping of the enclosing groups.
#[derive(Clone, Copy)]
struct Xform {
    /// Per axis: (child origin, parent origin, child extent, parent extent).
    x: (i64, i64, i64, i64),
    /// Vertical counterpart of `x`.
    y: (i64, i64, i64, i64),
}

impl Xform {
    /// Identity mapping.
    const ID: Self = Self { x: (0, 0, 1, 1), y: (0, 0, 1, 1) };

    /// Map a position on one axis.
    fn pos(axis: (i64, i64, i64, i64), value: i64) -> i64 {
        let scaled = value.saturating_sub(axis.0).saturating_mul(axis.3).checked_div(axis.2).unwrap_or(0);
        axis.1.saturating_add(scaled)
    }

    /// Map a length on one axis.
    fn len(axis: (i64, i64, i64, i64), value: i64) -> i64 {
        value.saturating_mul(axis.3).checked_div(axis.2).unwrap_or(0)
    }

    /// Map a frame `(x, y, w, h)`.
    fn frame(self, frame: (i64, i64, i64, i64)) -> (i64, i64, i64, i64) {
        (Self::pos(self.x, frame.0), Self::pos(self.y, frame.1), Self::len(self.x, frame.2), Self::len(self.y, frame.3))
    }
}

/// Walk `tree`'s children into `out`. `decor` skips placeholders: on a master
/// or layout they are prompts for the slide, not content.
pub(super) fn walk(ctx: &Ctx<'_>, tree: &Node, decor: bool, out: &mut Vec<Shape>) {
    walk_at(ctx, tree, Walk { decor, xf: Xform::ID, depth: 0 }, out);
}

/// Where a recursive [`walk_at`] stands.
#[derive(Clone, Copy)]
struct Walk {
    /// Skip placeholders (master/layout prompts).
    decor: bool,
    /// Enclosing group transform.
    xf: Xform,
    /// Group nesting depth.
    depth: usize,
}

/// [`walk`] with the enclosing transform and depth.
fn walk_at(ctx: &Ctx<'_>, tree: &Node, at: Walk, out: &mut Vec<Shape>) {
    let Walk { decor, xf, depth } = at;
    for node in &tree.children {
        if out.len() >= MAX_SHAPES {
            return;
        }
        if decor && inherit::ph_of(node).is_some() {
            continue;
        }
        match node.name.as_str() {
            "p:sp" | "p:cxnSp" | "p:pic" => out.extend(shape(ctx, node, xf)),
            "p:graphicFrame" => out.extend(frame_table(ctx, node, xf)),
            "p:grpSp" if depth < MAX_GROUP_DEPTH => {
                let inner = group_xform(node, xf);
                walk_at(ctx, node, Walk { decor, xf: inner, depth: depth.saturating_add(1) }, out);
            }
            _ => {}
        }
    }
}

/// The transform a group applies to its children (composed with `outer`).
fn group_xform(group: &Node, outer: Xform) -> Xform {
    let Some(xfrm) = group.path("p:grpSpPr/a:xfrm") else { return outer };
    let pair = |name: &str, kx: &str, ky: &str| {
        xfrm.child(name).map_or((0, 0), |node| (node.int(kx).unwrap_or(0), node.int(ky).unwrap_or(0)))
    };
    let (off, ext) = (pair("a:off", "x", "y"), pair("a:ext", "cx", "cy"));
    let (ch_off, ch_ext) = (pair("a:chOff", "x", "y"), pair("a:chExt", "cx", "cy"));
    let (ox, oy, ow, oh) = outer.frame((off.0, off.1, ext.0, ext.1));
    Xform { x: (ch_off.0, ox, ch_ext.0.max(1), ow), y: (ch_off.1, oy, ch_ext.1.max(1), oh) }
}

/// Frame, rotation and flips from the first `a:xfrm` among `props`.
fn placement(props: &[&Node], xf: Xform, out: &mut Shape) -> bool {
    let Some(xfrm) = props.iter().find_map(|sp_pr| sp_pr.child("a:xfrm").filter(|node| node.child("a:off").is_some()))
    else {
        return false;
    };
    let off = xfrm.child("a:off");
    let ext = xfrm.child("a:ext");
    let raw = (
        off.and_then(|node| node.int("x")).unwrap_or(0),
        off.and_then(|node| node.int("y")).unwrap_or(0),
        ext.and_then(|node| node.int("cx")).unwrap_or(0),
        ext.and_then(|node| node.int("cy")).unwrap_or(0),
    );
    (out.x, out.y, out.w, out.h) = xf.frame(raw);
    out.rot = xfrm.int("rot").unwrap_or(0);
    out.flip_h = xfrm.flag("flipH").unwrap_or(false);
    out.flip_v = xfrm.flag("flipV").unwrap_or(false);
    true
}

/// `p:sp`, `p:cxnSp` or `p:pic`.
fn shape(ctx: &Ctx<'_>, node: &Node, xf: Xform) -> Option<Shape> {
    let ph = inherit::ph_of(node);
    let chain = ctx.layers.chain(ph);
    let props = inherit::sp_props(node, &chain);
    let name =
        node.children.first().and_then(|nv| nv.child("p:cNvPr")).and_then(|c| c.attr("name")).unwrap_or("").to_owned();
    let mut out = Shape { name, ..Shape::default() };
    if !placement(&props, xf, &mut out) {
        return None;
    }
    let style = node.child("p:style");
    let geometry =
        props.iter().copied().find(|sp_pr| sp_pr.child("a:prstGeom").is_some() || sp_pr.child("a:custGeom").is_some());
    out.path = Some(
        geometry
            .map_or_else(|| geom::outline(&Node::default(), out.w, out.h), |sp_pr| geom::outline(sp_pr, out.w, out.h)),
    );
    let paint =
        props.iter().find_map(|sp_pr| ctx.paint.fill(sp_pr, None)).unwrap_or_else(|| ctx.paint.shape_fill(None, style));
    let is_line = geometry.is_some_and(geom::is_line);
    out.fill = paint.css.filter(|_| !paint.none && !is_line);
    out.image = node.child("p:blipFill").and_then(|blip| ctx.paint.blip(blip)).or(paint.image);
    out.stroke = ctx.paint.stroke(props.first().copied(), style);
    out.text = node
        .child("p:txBody")
        .map(|tx| text_of(ctx, tx, (ph, &chain), style))
        .filter(|body| !body.paragraphs.is_empty());
    Some(out)
}

/// A shape's text, with the style's `fontRef` as the run colour/face default.
/// `inherit` is the placeholder and its inherited shapes (layout, master).
fn text_of(
    ctx: &Ctx<'_>,
    tx: &Node,
    inherit: (Option<&Node>, &[&Node]),
    style: Option<&Node>,
) -> super::model::TextBody {
    let (ph, chain) = inherit;
    let font_ref = style.and_then(|node| node.child("a:fontRef"));
    let font_color: Option<Rgba> = font_ref.and_then(|node| ctx.paint.theme.color(node, None));
    let font_face = font_ref
        .and_then(|node| node.attr("idx"))
        .map(|idx| ctx.paint.theme.font(if idx == "major" { "+mj-lt" } else { "+mn-lt" }));
    let lists = ctx.layers.lists(ph, chain);
    let body_props = inherit::body_props(chain);
    let tctx = TextCtx { theme: ctx.paint.theme, rels: ctx.paint.rels, font_color, font_face };
    text::body(tx, &lists, &body_props, &tctx)
}

/// A `p:graphicFrame` holding a table; charts/diagrams are skipped.
fn frame_table(ctx: &Ctx<'_>, node: &Node, xf: Xform) -> Option<Shape> {
    let tbl = node.path("a:graphic/a:graphicData/a:tbl")?;
    let name = node.path("p:nvGraphicFramePr/p:cNvPr").and_then(|c| c.attr("name")).unwrap_or("").to_owned();
    let mut out = Shape { name, ..Shape::default() };
    let wrapper = Node {
        name: "p:spPr".to_owned(),
        children: node.child("p:xfrm").map(|x| Node { name: "a:xfrm".to_owned(), ..x.clone() }).into_iter().collect(),
        ..Node::default()
    };
    if !placement(&[&wrapper], xf, &mut out) {
        return None;
    }
    let lists = ctx.layers.lists(None, &[]);
    let styles = ctx.table_styles;
    let want = tbl
        .path("a:tblPr/a:tableStyleId")
        .map(|id| id.text.trim().to_owned())
        .or_else(|| styles.and_then(|lst| lst.attr("def")).map(str::to_owned));
    let style = styles.and_then(|lst| lst.all("a:tblStyle").find(|st| st.attr("styleId") == want.as_deref()));
    let tctx = table::TableCtx { paint: &ctx.paint, style, lists: &lists };
    out.table = Some(table::table(tbl, &tctx));
    Some(out)
}
