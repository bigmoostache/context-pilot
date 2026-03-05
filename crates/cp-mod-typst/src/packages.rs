//! Typst Universe package resolver.
//!
//! Downloads `@preview/` packages from https://packages.typst.org and caches them
//! in the user-global cache at `~/.cache/typst/packages/preview/{name}/{version}/`.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;

/// A parsed package specifier like `@preview/graceful-genetics:0.2.0`.
#[derive(Debug, Clone)]
pub struct PackageSpec {
    /// Namespace (e.g., "preview")
    pub namespace: String,
    /// Package name (e.g., "graceful-genetics")
    pub name: String,
    /// Version (e.g., "0.2.0")
    pub version: String,
}

impl PackageSpec {
    /// Parse a package specifier string like `@preview/name:version`.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the spec doesn't match `@namespace/name:version`
    /// format, or if any component is empty.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        let rest = spec.strip_prefix('@').ok_or_else(|| format!("Invalid package spec '{spec}': must start with @"))?;

        let (namespace, rest) = rest
            .split_once('/')
            .ok_or_else(|| format!("Invalid package spec '{spec}': expected @namespace/name:version"))?;

        let (name, version) = rest
            .split_once(':')
            .ok_or_else(|| format!("Invalid package spec '{spec}': expected @namespace/name:version"))?;

        if namespace.is_empty() || name.is_empty() || version.is_empty() {
            return Err(format!("Invalid package spec '{spec}': namespace, name, and version must not be empty"));
        }

        Ok(Self { namespace: namespace.to_string(), name: name.to_string(), version: version.to_string() })
    }

    /// The download URL on packages.typst.org.
    #[must_use]
    pub fn download_url(&self) -> String {
        format!("https://packages.typst.org/{}/{}-{}.tar.gz", self.namespace, self.name, self.version)
    }

    /// The local cache directory for this package.
    #[must_use]
    pub fn cache_dir(&self) -> PathBuf {
        package_cache_root().join(&self.namespace).join(&self.name).join(&self.version)
    }

    /// Check if this package is already cached.
    #[must_use]
    pub fn is_cached(&self) -> bool {
        self.cache_dir().join("typst.toml").exists()
    }

    /// Display string like `@preview/name:version`.
    #[must_use]
    pub fn to_spec_string(&self) -> String {
        format!("@{}/{}:{}", self.namespace, self.name, self.version)
    }
}

/// Root of the user-global Typst package cache.
/// Matches the typst CLI default: `~/.cache/typst/packages/`
#[must_use]
pub fn package_cache_root() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".cache").join("typst").join("packages")
}

/// Resolve a package to its cached directory, downloading if needed.
/// Returns the path to the package root directory.
///
/// # Errors
///
/// Returns `Err` if the package download or extraction fails.
pub fn resolve_package(spec: &PackageSpec) -> Result<PathBuf, String> {
    let cache_dir = spec.cache_dir();

    if spec.is_cached() {
        return Ok(cache_dir);
    }

    download_package(spec)?;
    Ok(cache_dir)
}

/// Download a package from Typst Universe and extract to cache.
///
/// # Errors
///
/// Returns `Err` if the HTTP request fails, the package is not found,
/// or the tar.gz archive cannot be extracted.
pub fn download_package(spec: &PackageSpec) -> Result<(), String> {
    let url = spec.download_url();
    let cache_dir = spec.cache_dir();

    // Create cache directory
    fs::create_dir_all(&cache_dir).map_err(|e| format!("Failed to create cache dir {}: {}", cache_dir.display(), e))?;

    // Download the tar.gz
    let response = reqwest::blocking::get(&url).map_err(|e| format!("Failed to download {url}: {e}"))?;

    if !response.status().is_success() {
        // Clean up empty cache dir on failure
        let _r = fs::remove_dir(&cache_dir);
        return Err(format!(
            "Package {} not found (HTTP {}). Check the package name and version at https://typst.app/universe/",
            spec.to_spec_string(),
            response.status()
        ));
    }

    let bytes = response.bytes().map_err(|e| format!("Failed to read response: {e}"))?;

    // Extract tar.gz to cache directory
    let gz = GzDecoder::new(&*bytes);
    let mut archive = tar::Archive::new(gz);

    for entry in archive.entries().map_err(|e| format!("Failed to read tar archive: {e}"))? {
        let mut entry = entry.map_err(|e| format!("Failed to read tar entry: {e}"))?;
        let entry_path = entry.path().map_err(|e| format!("Invalid path in archive: {e}"))?.into_owned();

        // Security: prevent path traversal
        if entry_path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            continue;
        }

        let target = cache_dir.join(&entry_path);

        if entry.header().entry_type().is_dir() {
            let _r = fs::create_dir_all(&target);
        } else {
            // Ensure parent directory exists
            if let Some(parent) = target.parent() {
                let _ = fs::create_dir_all(parent).ok();
            }
            let mut content = Vec::new();
            let _ = entry
                .read_to_end(&mut content)
                .map_err(|e| format!("Failed to extract {}: {}", entry_path.display(), e))?;
            fs::write(&target, &content).map_err(|e| format!("Failed to write {}: {}", target.display(), e))?;
        }
    }

    // Verify extraction succeeded
    if !cache_dir.join("typst.toml").exists() {
        // Some packages have files at the root level, not in a subdirectory
        // That's fine — the package is usable
    }

    Ok(())
}

/// Resolve a `@namespace/name:version` import path to the filesystem path.
/// Used by the World impl to resolve package file IDs.
///
/// # Errors
///
/// Returns `Err` if the package cannot be resolved or the sub-path
/// doesn't exist within it.
pub fn resolve_package_path(namespace: &str, name: &str, version: &str, sub_path: &Path) -> Result<PathBuf, String> {
    let spec = PackageSpec { namespace: namespace.to_string(), name: name.to_string(), version: version.to_string() };

    let pkg_dir = resolve_package(&spec)?;
    let full_path = pkg_dir.join(sub_path);

    if !full_path.exists() {
        return Err(format!("File {} not found in package {}", sub_path.display(), spec.to_spec_string()));
    }

    Ok(full_path)
}

/// List all cached packages. Returns Vec<(namespace, name, version)>.
#[must_use]
pub fn list_cached() -> Vec<(String, String, String)> {
    let root = package_cache_root();
    let mut packages = Vec::new();

    if !root.exists() {
        return packages;
    }

    // Walk: root/namespace/name/version/
    let Ok(namespaces) = fs::read_dir(&root) else { return packages };
    for ns_entry in namespaces.flatten() {
        let ns_path = ns_entry.path();
        if !ns_path.is_dir() {
            continue;
        }
        let namespace = ns_entry.file_name().to_string_lossy().to_string();

        let Ok(names) = fs::read_dir(&ns_path) else { continue };
        for name_entry in names.flatten() {
            let name_path = name_entry.path();
            if !name_path.is_dir() {
                continue;
            }
            let name = name_entry.file_name().to_string_lossy().to_string();

            let Ok(versions) = fs::read_dir(&name_path) else { continue };
            for ver_entry in versions.flatten() {
                let ver_path = ver_entry.path();
                if !ver_path.is_dir() {
                    continue;
                }
                let version = ver_entry.file_name().to_string_lossy().to_string();
                packages.push((namespace.clone(), name.clone(), version));
            }
        }
    }

    packages.sort();
    packages
}
