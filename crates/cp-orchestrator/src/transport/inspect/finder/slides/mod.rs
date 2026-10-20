//! `PowerPoint` preview: `pptx` → a resolved slide model for the Finder's
//! slide viewer (`GET /api/fleet/{id}/fs/slides?path=`).
//!
//! Every slide is flattened with its master and layout: decorations from the
//! master and layout (unless `showMasterSp="0"`), then the slide's own shapes,
//! each placeholder resolved through the inheritance cascade. Colours, fonts,
//! bullets and geometry are computed here so the webapp only draws boxes.
//!
//! * [`xml`] — small owned DOM over `quick-xml`.
//! * [`package`] — the zip, relationships and embedded images.
//! * [`color`] — theme palette, colour map and transforms.
//! * [`geom`] — preset and custom geometry → SVG paths.
//! * [`text`] — text bodies through the list-style cascade.
//! * [`shapes`] — the shape tree, paint, placeholders and tables.
//! * [`model`] — the serialised output.

mod color;
mod geom;
mod model;
mod package;
mod shapes;
mod text;
mod xml;

use std::path::Path;
use std::sync::Mutex;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

use super::support::{confined_path, extract_param, fleet_folder};
use color::Theme;
use model::{Deck, Shape, Slide};
use package::Package;
use shapes::{Ctx, Layers, Paint};
use xml::Node;

/// Largest pptx previewed (100 MiB).
const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

/// Most slides converted; the rest are dropped and `truncated` is set.
const MAX_SLIDES: usize = 300;

/// Default 16:9 slide size (EMU) when `p:sldSz` is missing.
const DEFAULT_SIZE: (i64, i64) = (12_192_000, 6_858_000);

/// `GET /api/fleet/{id}/fs/slides?path=` — pptx → slide model JSON.
///
/// Confined to the agent realm (`403` on escape), `404` for a non-file, `415`
/// for a non-pptx or unreadable package, `413` over [`MAX_FILE_BYTES`].
pub(crate) fn fs_slides(state: &Mutex<Backend>, fleet_id: &str, query: &str) -> HttpReply {
    let folder = match fleet_folder(state, fleet_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let relative = match extract_param(query, "path") {
        Some(p) if !p.is_empty() => p,
        _ => return HttpReply::error(400, "missing path parameter"),
    };
    let Some(target) = confined_path(&folder, &relative) else {
        return HttpReply::error(403, "path outside agent realm");
    };
    if !target.is_file() {
        return HttpReply::error(404, "file not found");
    }
    let ext = target.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !matches!(ext.as_str(), "pptx" | "pptm" | "ppsx" | "potx") {
        return HttpReply::error(415, "not a pptx presentation");
    }
    if target.metadata().map_or(0, |m| m.len()) > MAX_FILE_BYTES {
        return HttpReply::error(413, "presentation too large to preview");
    }
    convert(&target).map_or_else(|| HttpReply::error(415, "could not parse presentation"), |deck| HttpReply::ok(&deck))
}

/// Convert a pptx file; `None` when it is not a readable presentation.
fn convert(path: &Path) -> Option<Deck> {
    let pkg = Package::open(path)?;
    convert_package(&pkg)
}

/// Convert an opened package.
fn convert_package(pkg: &Package) -> Option<Deck> {
    let pres_part = "ppt/presentation.xml";
    let pres = pkg.xml(pres_part)?;
    let size = pres.child("p:sldSz");
    let width = size.and_then(|node| node.int("cx")).unwrap_or(DEFAULT_SIZE.0);
    let height = size.and_then(|node| node.int("cy")).unwrap_or(DEFAULT_SIZE.1);
    let rels = pkg.rels(pres_part);
    let parts: Vec<String> = pres
        .path("p:sldIdLst")
        .map(|list| list.all("p:sldId").filter_map(|id| rels.get(id.attr("r:id")?).cloned()).collect())
        .unwrap_or_default();
    let table_styles = pkg.xml("ppt/tableStyles.xml");
    let deck = DeckCtx { pkg, defaults: pres.child("p:defaultTextStyle"), table_styles: table_styles.as_ref() };
    let slides = parts
        .iter()
        .take(MAX_SLIDES)
        .enumerate()
        .filter_map(|(idx, part)| deck.slide(part, idx.saturating_add(1)))
        .collect();
    Some(Deck { width, height, slides, truncated: parts.len() > MAX_SLIDES })
}

/// Deck-wide inputs shared by every slide.
struct DeckCtx<'doc> {
    /// The package.
    pkg: &'doc Package,
    /// `p:defaultTextStyle`.
    defaults: Option<&'doc Node>,
    /// `a:tblStyleLst` root.
    table_styles: Option<&'doc Node>,
}

impl DeckCtx<'_> {
    /// Convert slide `part` (1-based `index`).
    fn slide(&self, part: &str, index: usize) -> Option<Slide> {
        let pkg = self.pkg;
        let slide = pkg.xml(part)?;
        let layout_part = pkg.rel_of_type(part, "/slideLayout");
        let layout = layout_part.as_deref().and_then(|lp| pkg.xml(lp));
        let master_part = layout_part.as_deref().and_then(|lp| pkg.rel_of_type(lp, "/slideMaster"));
        let master = master_part.as_deref().and_then(|mp| pkg.xml(mp)).unwrap_or_default();
        let theme = Theme::load(pkg, master_part.as_deref().unwrap_or(""), &master);
        let parts = [
            (master_part.as_deref(), Some(&master)),
            (layout_part.as_deref(), layout.as_ref()),
            (Some(part), Some(&slide)),
        ];
        let mut background = None;
        let mut shapes: Vec<Shape> = Vec::new();
        let show = |node: Option<&Node>| node.and_then(|n| n.flag("showMasterSp")).unwrap_or(true);
        for (level, &(maybe_part, maybe_root)) in parts.iter().enumerate() {
            let (Some(part_name), Some(root)) = (maybe_part, maybe_root) else { continue };
            let skip =
                (level == 0 && !(show(layout.as_ref()) && show(Some(&slide)))) || (level == 1 && !show(Some(&slide)));
            let rels = pkg.rels(part_name);
            let paint = Paint { theme: &theme, pkg, rels: &rels };
            let c_sld = root.child("p:cSld");
            background = c_sld.and_then(|node| paint.background(node)).or(background);
            if skip {
                continue;
            }
            let layers = Layers {
                master: &master,
                layout: if level == 2 { layout.as_ref() } else { None },
                defaults: self.defaults,
            };
            let ctx = Ctx { paint, layers, table_styles: self.table_styles };
            if let Some(tree) = c_sld.and_then(|node| node.child("p:spTree")) {
                shapes::walk(&ctx, tree, level < 2, &mut shapes);
            }
        }
        Some(Slide { index, hidden: slide.flag("show") == Some(false), background, shapes, notes: self.notes(part) })
    }

    /// Speaker notes: the body placeholder of the linked notes slide.
    fn notes(&self, part: &str) -> Option<String> {
        let notes = self.pkg.xml(&self.pkg.rel_of_type(part, "/notesSlide")?)?;
        let tree = notes.path("p:cSld/p:spTree")?;
        let body = tree
            .all("p:sp")
            .find(|sp| sp.path("p:nvSpPr/p:nvPr/p:ph").and_then(|ph| ph.attr("type")) == Some("body"))?;
        Some(text::plain(body.child("p:txBody")?)).filter(|text| !text.is_empty())
    }
}

#[cfg(test)]
mod tests;
