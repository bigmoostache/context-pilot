//! Embedded typst compilation engine.
//!
//! Implements a minimal `typst::World` for compiling `.typ` files to PDF.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::layout::PagedDocument;
use typst::syntax::{FileId, Source, VirtualPath, package::PackageSpec as TypstPackageSpec};
use typst::text::{Font, FontBook, FontInfo};
use typst::utils::LazyHash;
use typst::{Library, World};

use crate::packages;
use cp_base::cast::Safe as _;
use std::fmt::Write as _;

/// Successful compilation output: PDF bytes, warning text, and accessed file paths.
pub type CompileOutput = (Vec<u8>, String, Vec<PathBuf>);

/// Compile a `.typ` file to PDF bytes.
///
/// `source_path` is relative to the project root (which is the current directory).
/// Returns `Ok((pdf_bytes`, `warnings_text`, `accessed_files`)) or `Err(error_message)`.
///
/// # Errors
///
/// Returns `Err` if the source path cannot be resolved, compilation fails,
/// or PDF export encounters errors.
pub fn compile_to_pdf(source_path: &str) -> Result<CompileOutput, String> {
    let abs_path =
        PathBuf::from(source_path).canonicalize().map_err(|e| format!("Cannot resolve path '{source_path}': {e}"))?;

    let root = abs_path.parent().ok_or_else(|| "Source file has no parent directory".to_string())?.to_path_buf();

    // For imports like "../templates/foo.typ", we need root to be the project root
    // not just the document's directory. Walk up to find .context-pilot/
    let project_root = find_project_root(&abs_path).unwrap_or_else(|| root.clone());

    let rel_path =
        abs_path.strip_prefix(&project_root).map_err(|_e| "Source path not under project root".to_string())?;

    let main_id = FileId::new(None, VirtualPath::new(rel_path));
    let world = ContextPilotWorld::new(project_root, main_id)?;

    let result = typst::compile::<PagedDocument>(&world);

    // Collect warnings
    let warnings: Vec<String> = result.warnings.iter().map(|w| format!("warning: {}", w.message)).collect();

    match result.output {
        Ok(document) => {
            let pdf_bytes = typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default()).map_err(|errors| {
                let mut msg = String::new();
                for diag in &errors {
                    let _r = writeln!(msg, "pdf error: {}", diag.message);
                }
                msg
            })?;
            // Include warnings in the success message (never eprintln — it corrupts the TUI)
            let mut result_msg = String::new();
            if !warnings.is_empty() {
                result_msg.push_str(&warnings.join("\n"));
                result_msg.push('\n');
            }
            // Extract accessed files for dependency tracking
            let deps = world.accessed_files.lock().map(|set| set.iter().cloned().collect()).unwrap_or_default();
            Ok((pdf_bytes, result_msg, deps))
        }
        Err(errors) => {
            let mut msg = String::new();
            for diag in &errors {
                let _r = writeln!(msg, "error: {}", diag.message);
                for hint in &diag.hints {
                    let _r2 = writeln!(msg, "  hint: {hint}");
                }
            }
            if !warnings.is_empty() {
                msg.push_str(&warnings.join("\n"));
                msg.push('\n');
            }
            Err(msg)
        }
    }
}

/// Compile a `.typ` file and write the PDF to the output path.
///
/// # Errors
///
/// Returns `Err` if compilation fails, the output directory cannot be
/// created, or the PDF file cannot be written.
pub fn compile_and_write(source_path: &str, output_path: &str) -> Result<String, String> {
    let (pdf_bytes, warnings, _deps) = compile_to_pdf(source_path)?;

    // Write to output path
    if let Some(parent) = Path::new(output_path).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    fs::write(output_path, &pdf_bytes).map_err(|e| format!("write {output_path}: {e}"))?;

    let mut msg = format!("✓ Compiled {} ({} bytes)", output_path, pdf_bytes.len());
    if !warnings.is_empty() {
        msg.push('\n');
        msg.push_str(&warnings);
    }
    Ok(msg)
}

/// Find the project root by walking up and looking for .context-pilot/
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".context-pilot").is_dir() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Minimal World implementation for Context Pilot.
struct ContextPilotWorld {
    /// Project root directory
    root: PathBuf,
    /// Main source file ID
    main_id: FileId,
    /// Standard library
    library: LazyHash<Library>,
    /// Font book (metadata about available fonts)
    book: LazyHash<FontBook>,
    /// Loaded fonts
    fonts: Vec<Font>,
    /// Source file cache
    sources: HashMap<FileId, Source>,
    /// All file paths accessed during compilation (for dependency tracking)
    accessed_files: Mutex<HashSet<PathBuf>>,
}

impl ContextPilotWorld {
    /// Create a new world rooted at `root` with the given main source file.
    fn new(root: PathBuf, main_id: FileId) -> Result<Self, String> {
        // Discover system fonts
        let mut book = FontBook::new();
        let mut fonts = Vec::new();

        // Search common font directories
        let font_dirs = [
            PathBuf::from("/usr/share/fonts"),
            PathBuf::from("/usr/local/share/fonts"),
            dirs_home().map(|h| h.join(".fonts")).unwrap_or_default(),
            dirs_home().map(|h| h.join(".local/share/fonts")).unwrap_or_default(),
        ];

        for dir in &font_dirs {
            if dir.is_dir() {
                load_fonts_from_dir(dir, &mut book, &mut fonts);
            }
        }

        // Also load typst's embedded fonts (from typst-assets if available)
        // For now, system fonts should be sufficient

        let mut world = Self {
            root,
            main_id,
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
            sources: HashMap::new(),
            accessed_files: Mutex::new(HashSet::new()),
        };

        // Pre-load the main source
        let _src = world.load_source(main_id)?;

        Ok(world)
    }

    /// Load (and cache) the source file identified by `id`.
    fn load_source(&mut self, id: FileId) -> Result<Source, String> {
        if let Some(source) = self.sources.get(&id) {
            return Ok(source.clone());
        }

        let path = self.resolve_path(id)?;
        let content = fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
        let source = Source::new(id, content);
        drop(self.sources.insert(id, source.clone()));
        Ok(source)
    }

    /// Resolve a `FileId` to an absolute filesystem path (local or package).
    fn resolve_path(&self, id: FileId) -> Result<PathBuf, String> {
        // Check if this FileId belongs to a package (@preview/name:version)
        if let Some(pkg_spec) = id.package() {
            return Self::resolve_package_path(id, pkg_spec);
        }

        // Local file — resolve relative to project root
        let vpath = id.vpath();
        let path = vpath.resolve(&self.root).ok_or_else(|| format!("cannot resolve virtual path: {vpath:?}"))?;
        Ok(path)
    }

    /// Resolve a file path within a Typst Universe package.
    /// Downloads the package if not already cached.
    fn resolve_package_path(id: FileId, pkg: &TypstPackageSpec) -> Result<PathBuf, String> {
        let namespace = pkg.namespace.as_str();
        let name = pkg.name.as_str();
        let version = format!("{}", pkg.version);

        let spec = packages::PackageSpec { namespace: namespace.to_string(), name: name.to_string(), version };

        let pkg_dir = packages::resolve_package(&spec)?;

        // The VirtualPath within the package (e.g., /lib.typ)
        let vpath = id.vpath();
        let sub_path = vpath
            .resolve(&pkg_dir)
            .ok_or_else(|| format!("cannot resolve {:?} in package {}", vpath, spec.to_spec_string()))?;

        Ok(sub_path)
    }
}

impl World for ContextPilotWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main_id
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if let Some(source) = self.sources.get(&id) {
            return Ok(source.clone());
        }

        // Resolve via our unified path resolver (handles local + packages)
        let path = self.resolve_path(id).map_err(|_e| FileError::AccessDenied)?;
        if let Ok(mut set) = self.accessed_files.lock() {
            let _ = set.insert(path.clone());
        }
        let content = fs::read_to_string(&path).map_err(|_e| FileError::NotFound(path))?;
        Ok(Source::new(id, content))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        // Resolve via our unified path resolver (handles local + packages)
        let path = self.resolve_path(id).map_err(|_e| FileError::AccessDenied)?;
        if let Ok(mut set) = self.accessed_files.lock() {
            let _ = set.insert(path.clone());
        }
        let data = fs::read(&path).map_err(|_e| FileError::NotFound(path))?;
        Ok(Bytes::new(data))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }
    fn today(&self, offset: Option<i64>) -> Option<Datetime> {
        use chrono::{Datelike as _, FixedOffset, Local, Timelike as _, Utc};
        let now = Local::now();
        let naive = if let Some(hours) = offset {
            let utc = Utc::now();
            let secs = hours.checked_mul(3600)?;
            let tz = FixedOffset::east_opt(secs.to_i32())?;
            utc.with_timezone(&tz).naive_local()
        } else {
            now.naive_local()
        };
        Datetime::from_ymd_hms(
            naive.year(),
            naive.month().to_u8(),
            naive.day().to_u8(),
            naive.hour().to_u8(),
            naive.minute().to_u8(),
            naive.second().to_u8(),
        )
    }
}

/// Load fonts from a directory recursively.
pub(crate) fn load_fonts_from_dir(dir: &Path, book: &mut FontBook, fonts: &mut Vec<Font>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            load_fonts_from_dir(&path, book, fonts);
        } else if is_font_file(&path)
            && let Ok(data) = fs::read(&path)
        {
            let bytes = Bytes::new(data);
            for (i, info) in FontInfo::iter(&bytes).enumerate() {
                book.push(info);
                if let Some(font) = Font::new(bytes.clone(), i.to_u32()) {
                    fonts.push(font);
                }
            }
        }
    }
}

/// Check if a file looks like a font file.
pub(crate) fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_lowercase().as_str(), "ttf" | "otf" | "ttc" | "woff" | "woff2"))
}

/// Get the home directory.
pub(crate) fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
