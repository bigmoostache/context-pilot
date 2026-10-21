//! The pptx package: a zip of OOXML parts linked by relationship files.
//!
//! Parts are read eagerly into memory (bounded by [`MAX_PART_BYTES`] each and
//! [`MAX_TOTAL_BYTES`] overall) so the converter can resolve any relationship
//! without re-opening the archive. Relationship targets are normalised to
//! package paths (`ppt/slides/../media/image1.png` → `ppt/media/image1.png`).

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::Path;

use base64::Engine as _;

use super::xml::{self, Node};

/// Largest single part read (32 MiB); bigger parts are skipped.
const MAX_PART_BYTES: u64 = 32 * 1024 * 1024;

/// Budget for all parts together (96 MiB): a zip bomb stops here.
const MAX_TOTAL_BYTES: u64 = 96 * 1024 * 1024;

/// Largest image inlined as a data URI (8 MiB); bigger ones render as a box.
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// Every part of the package, by its path inside the zip.
pub(in crate::transport::inspect::finder::slides) struct Package {
    /// Part bytes keyed by normalised package path (no leading `/`).
    parts: BTreeMap<String, Vec<u8>>,
}

impl Package {
    /// Open a pptx file; `None` when it is not a readable zip.
    pub(in crate::transport::inspect::finder::slides) fn open(path: &Path) -> Option<Self> {
        let file = std::fs::File::open(path).ok()?;
        let mut archive = zip::ZipArchive::new(file).ok()?;
        let mut parts = BTreeMap::new();
        let mut budget = MAX_TOTAL_BYTES;
        for idx in 0..archive.len() {
            let Ok(mut entry) = archive.by_index(idx) else { continue };
            let size = entry.size();
            if entry.is_dir() || size > MAX_PART_BYTES || size > budget {
                continue;
            }
            budget = budget.saturating_sub(size);
            let name = entry.name().trim_start_matches('/').to_owned();
            let mut bytes = Vec::new();
            if entry.by_ref().take(MAX_PART_BYTES).read_to_end(&mut bytes).is_ok() {
                let _prev = parts.insert(name, bytes);
            }
        }
        Some(Self { parts })
    }

    /// Build from in-memory parts (tests).
    #[cfg(test)]
    pub(in crate::transport::inspect::finder::slides) const fn from_parts(parts: BTreeMap<String, Vec<u8>>) -> Self {
        Self { parts }
    }

    /// Parse a part as XML.
    pub(in crate::transport::inspect::finder::slides) fn xml(&self, part: &str) -> Option<Node> {
        xml::parse(self.parts.get(part)?)
    }

    /// Relationships of `part` as id → normalised target path. External
    /// targets (hyperlinks) keep their raw URL.
    pub(in crate::transport::inspect::finder::slides) fn rels(&self, part: &str) -> BTreeMap<String, String> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let rels_path = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
        let Some(doc) = self.xml(&rels_path) else { return BTreeMap::new() };
        doc.all("Relationship")
            .filter_map(|rel| {
                let id = rel.attr("Id")?.to_owned();
                let target = rel.attr("Target")?;
                let external = rel.attr("TargetMode") == Some("External");
                Some((id, if external { target.to_owned() } else { resolve(dir, target) }))
            })
            .collect()
    }

    /// The first relationship target of `part` whose type ends with `kind`
    /// (`slideLayout`, `slideMaster`, `theme`, `notesSlide`).
    pub(in crate::transport::inspect::finder::slides) fn rel_of_type(&self, part: &str, kind: &str) -> Option<String> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let doc = self.xml(&format!("{dir}/_rels/{file}.rels"))?;
        let rel = doc.all("Relationship").find(|rel| rel.attr("Type").is_some_and(|ty| ty.ends_with(kind)))?;
        Some(resolve(dir, rel.attr("Target")?))
    }

    /// An image part as a `data:` URI; `None` for formats browsers can't show
    /// (EMF/WMF/TIFF) or images over [`MAX_IMAGE_BYTES`].
    pub(in crate::transport::inspect::finder::slides) fn data_uri(&self, part: &str) -> Option<String> {
        let bytes = self.parts.get(part)?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return None;
        }
        let ext = part.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let mime = match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            _ => return None,
        };
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        Some(format!("data:{mime};base64,{encoded}"))
    }
}

/// Resolve a relationship target against the source part's directory.
fn resolve(dir: &str, target: &str) -> String {
    let joined = target.strip_prefix('/').map_or_else(|| format!("{dir}/{target}"), str::to_owned);
    let mut out: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                let _up = out.pop();
            }
            other => out.push(other),
        }
    }
    out.join("/")
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn resolves_relative_and_absolute_targets() {
        assert_eq!(resolve("ppt/slides", "../media/image1.png"), "ppt/media/image1.png");
        assert_eq!(resolve("ppt", "slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("ppt/slides", "/ppt/media/a.png"), "ppt/media/a.png");
    }

    #[test]
    fn path_helper_is_stable() {
        assert_eq!(std::path::Path::new("a.pptx").extension().and_then(|e| e.to_str()), Some("pptx"));
    }
}
