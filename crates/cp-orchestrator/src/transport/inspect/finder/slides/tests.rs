//! End-to-end conversion of a minimal in-memory deck.

use std::collections::BTreeMap;

use super::convert_package;
use super::package::Package;

/// Relationship file with one entry.
fn rel(id: &str, kind: &str, target: &str) -> Vec<u8> {
    format!(
        r#"<Relationships><Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{target}"/></Relationships>"#
    )
    .into_bytes()
}

/// One slide on a layout on a master with a theme; the master has a
/// decoration and a title placeholder the slide inherits from.
fn deck() -> Package {
    let mut parts: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut put = |path: &str, body: Vec<u8>| {
        let _prev = parts.insert(path.to_owned(), body);
    };
    put("ppt/presentation.xml", br#"<p:presentation><p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#.to_vec());
    put("ppt/_rels/presentation.xml.rels", rel("rId2", "slide", "slides/slide1.xml"));
    put("ppt/slides/_rels/slide1.xml.rels", rel("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"));
    put("ppt/slideLayouts/_rels/slideLayout1.xml.rels", rel("rId1", "slideMaster", "../slideMasters/slideMaster1.xml"));
    put("ppt/slideMasters/_rels/slideMaster1.xml.rels", rel("rId1", "theme", "../theme/theme1.xml"));
    put("ppt/theme/theme1.xml", br#"<a:theme><a:themeElements><a:clrScheme><a:dk1><a:srgbClr val="111111"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:accent1><a:srgbClr val="6C5CE7"/></a:accent1></a:clrScheme><a:fontScheme><a:majorFont><a:latin typeface="Georgia"/></a:majorFont><a:minorFont><a:latin typeface="Arial"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#.to_vec());
    put("ppt/slideMasters/slideMaster1.xml", br#"<p:sldMaster><p:cSld><p:bg><p:bgPr><a:solidFill><a:schemeClr val="bg1"/></a:solidFill></p:bgPr></p:bg><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Bar"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="5143500"/></a:xfrm><a:prstGeom prst="rect"/><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></p:spPr></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Title"/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="10" y="20"/><a:ext cx="300" cy="40"/></a:xfrm></p:spPr></p:sp></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1"/><p:txStyles><p:titleStyle><a:lvl1pPr algn="ctr"><a:defRPr sz="4400" b="1"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mj-lt"/></a:defRPr></a:lvl1pPr></p:titleStyle></p:txStyles></p:sldMaster>"#.to_vec());
    put("ppt/slideLayouts/slideLayout1.xml", b"<p:sldLayout><p:cSld><p:spTree/></p:cSld></p:sldLayout>".to_vec());
    put("ppt/slides/slide1.xml", br#"<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="T"/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang="fr"/><a:t>Bonjour</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#.to_vec());
    Package::from_parts(parts)
}

#[test]
fn converts_master_decor_and_inherited_title() {
    let out = convert_package(&deck()).unwrap();
    assert_eq!((out.width, out.height), (9_144_000, 5_143_500));
    let slide = out.slides.first().unwrap();
    assert_eq!(slide.background.as_deref(), Some("#ffffff"));
    assert_eq!(slide.shapes.len(), 2, "master bar + slide title (master title prompt skipped)");
    let (Some(bar), Some(title)) = (slide.shapes.first(), slide.shapes.last()) else { panic!("two shapes") };
    assert_eq!(bar.fill.as_deref(), Some("#6c5ce7"));
    assert_eq!((title.x, title.y, title.w, title.h), (10, 20, 300, 40));
    check_title_run(title);
}

/// The slide title inherits alignment, size, weight, colour and face from the master.
fn check_title_run(title: &super::model::Shape) {
    let para = title.text.as_ref().unwrap().paragraphs.first().unwrap();
    assert_eq!(para.align, "ctr");
    let run = para.runs.first().unwrap();
    assert_eq!((run.text.as_str(), run.size, run.bold), ("Bonjour", 4400, true));
    assert_eq!((run.color.as_str(), run.font.as_str()), ("#111111", "Georgia"));
}

/// Manual smoke: `CP_PPTX=<file> cargo test -p cp-orchestrator --lib slides::tests::smoke -- --ignored` writes `<file>.json`.
#[test]
#[ignore = "needs a real deck via CP_PPTX"]
fn smoke() {
    let path = std::env::var("CP_PPTX").unwrap();
    let deck = super::convert(std::path::Path::new(&path)).unwrap();
    std::fs::write(format!("{path}.json"), serde_json::to_string_pretty(&deck).unwrap()).unwrap();
}
