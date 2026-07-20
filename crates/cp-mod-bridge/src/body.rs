//! [`Store`] — the agent's immutable, content-addressed body store and the
//! I13 body-before-reference durability barrier (design doc §3.1 / I13,
//! roadmap P? / X781, validates V12).
//!
//! A message body is referenced from the oplog only by its [`ContentHash`]
//! (design doc I3). This store owns the bytes behind those hashes. The single
//! invariant it exists to uphold is **I13**:
//!
//! > A content hash `H` may appear in a **durable** oplog entry only **after**
//! > the body for `H` is itself durable.
//!
//! # Inline-small / spill-large
//!
//! [`Store::put`] classifies a body by size:
//!
//! * **Small** (`len <= inline_threshold`) → [`Stored::Inline`]. The bytes are
//!   handed back for the caller to embed **inside the same oplog entry** that
//!   references them. The body then *is* part of that entry's own `fdatasync`,
//!   so the barrier is satisfied with **zero extra write and zero extra file** —
//!   the common case.
//! * **Large** (`len > inline_threshold`) → [`Stored::Spilled`]. The bytes are
//!   written to `bodies/{hash}` via `tmp → fdatasync → rename → fsync(dir)`
//!   **before [`put`](Store::put) returns**. The caller journals the
//!   referencing entry only after `put` returns, so the spilled body is provably
//!   durable first.
//!
//! Because bodies are content-addressed, a spill is **idempotent**: writing the
//! same content twice is a no-op once the file exists, and a crash *inside* the
//! barrier window (body durable, referencing entry never committed) leaves a
//! harmless **orphan body** — unreferenced, identical to any other copy of that
//! content, reclaimed by [`gc`](Store::gc). No durable entry ever references
//! a missing body; that is the V12 guarantee.
//!
//! # Garbage collection (GAP 3 grace rule)
//!
//! [`gc`](Store::gc) deletes only bodies that are **both** unreferenced and
//! older than a grace window, reusing the tested predicate
//! [`cp_oplog::compact::body_gc_eligible`]. The grace must exceed the longest
//! possible barrier window so an *in-flight* spill — durable but not yet
//! referenced — is never mistaken for a crash-orphan and deleted out from under
//! the entry about to name it. Use [`cp_oplog::compact::DEFAULT_GC_GRACE`].

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cp_oplog::compact::body_gc_eligible;
use cp_wire::types::ContentHash;

use crate::error::{BootResult, Error};

/// Default inline/spill cutoff (4 KiB).
///
/// A body of at most this many bytes rides its referencing oplog entry's own
/// `fdatasync` (inline) rather than paying for a separate spilled file. Above
/// it, the separate-file cost is worth it and the body spills.
pub const DEFAULT_INLINE_THRESHOLD: usize = 4096;

/// Subdirectory of the oplog directory that holds spilled body files.
const BODIES_DIR: &str = "bodies";

/// The disposition of a body after [`Store::put`].
///
/// Either way the [`ContentHash`] is the body's content address; the variant
/// tells the caller *how* to make the reference durable (embed the bytes vs.
/// reference the already-durable file).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stored {
    /// A small body: embed `bytes` in the same oplog entry that references it,
    /// so the body shares that entry's `fdatasync` (the barrier is trivial).
    Inline {
        /// Content address of `bytes`.
        hash: ContentHash,
        /// The body bytes to embed in the referencing entry.
        bytes: Vec<u8>,
    },

    /// A large body: already durable at `bodies/{hash}` before `put` returned.
    /// The caller references `hash` from a later oplog entry.
    Spilled {
        /// Content address of the spilled body.
        hash: ContentHash,
    },
}

impl Stored {
    /// The body's content address, regardless of disposition.
    #[must_use]
    pub const fn hash(&self) -> ContentHash {
        match *self {
            Self::Inline { hash, .. } | Self::Spilled { hash } => hash,
        }
    }

    /// Whether the body was spilled to its own durable file (vs. inlined).
    #[must_use]
    pub const fn is_spilled(&self) -> bool {
        matches!(*self, Self::Spilled { .. })
    }
}

/// The agent's content-addressed body store, rooted at `<oplog_dir>/bodies`.
#[derive(Debug)]
pub struct Store {
    /// Directory holding the `{hash}` body files.
    dir: PathBuf,

    /// Bodies at most this many bytes are inlined; larger ones spill.
    inline_threshold: usize,
}

impl Store {
    /// Open (creating if absent) the body store under `oplog_dir`, with the
    /// default inline threshold.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the `bodies` directory cannot be created.
    pub fn open(oplog_dir: &Path) -> BootResult<Self> {
        Self::open_with_threshold(oplog_dir, DEFAULT_INLINE_THRESHOLD)
    }

    /// Open with an explicit `inline_threshold` (tests force spills with a tiny
    /// threshold without writing kilobytes).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the `bodies` directory cannot be created.
    pub fn open_with_threshold(oplog_dir: &Path, inline_threshold: usize) -> BootResult<Self> {
        let dir = oplog_dir.join(BODIES_DIR);
        fs::create_dir_all(&dir).map_err(|e| Error::io(format!("create body store {}", dir.display()), e))?;
        Ok(Self { dir, inline_threshold })
    }

    /// Store `bytes`, returning how the caller must make the reference durable.
    ///
    /// A small body is returned [`Inline`](Stored::Inline) for the caller to
    /// embed in the referencing entry. A large body is **spilled durably**
    /// (`tmp → fdatasync → rename → fsync(dir)`) before this returns, upholding
    /// the I13 barrier; a re-`put` of identical content is an idempotent no-op.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if a spilled body cannot be written, `fdatasync`'d,
    /// renamed, or the directory `fsync`'d.
    pub fn put(&self, bytes: &[u8]) -> BootResult<Stored> {
        let hash = ContentHash::of(bytes);
        if bytes.len() <= self.inline_threshold {
            return Ok(Stored::Inline { hash, bytes: bytes.to_vec() });
        }

        let path = self.body_path(hash);
        // Content-addressed: the bytes are identical to any prior copy, so an
        // existing file is already the correct, durable content.
        if path.exists() {
            return Ok(Stored::Spilled { hash });
        }

        let staging = self.tmp_path(hash);
        write_durable(&staging, bytes)?;
        fs::rename(&staging, &path).map_err(|e| {
            let _ignored = fs::remove_file(&staging); // leave no stale tmp behind
            Error::io(format!("rename body {} into place", staging.display()), e)
        })?;
        sync_dir(&self.dir)?;
        Ok(Stored::Spilled { hash })
    }

    /// Fetch a spilled body by hash, verifying its integrity.
    ///
    /// Returns `Ok(None)` if no such body exists (an inline body never lives
    /// here — it is carried in its oplog entry). The read-back bytes are
    /// re-hashed and compared against `hash`; a mismatch (bit-rot, a wrong
    /// file) is an [`Error::Io`] rather than silently returning corrupt data.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on a read fault or a content-hash mismatch.
    pub fn get(&self, hash: ContentHash) -> BootResult<Option<Vec<u8>>> {
        let path = self.body_path(hash);
        match fs::read(&path) {
            Ok(bytes) => {
                if ContentHash::of(&bytes) == hash {
                    Ok(Some(bytes))
                } else {
                    Err(Error::io(
                        format!("body {} failed integrity check", hash.to_hex()),
                        std::io::Error::new(std::io::ErrorKind::InvalidData, "content hash mismatch"),
                    ))
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::io(format!("read body {}", path.display()), e)),
        }
    }

    /// Garbage-collect crash-orphan bodies (design doc GAP 3).
    ///
    /// Deletes every spilled body that is **both** absent from `referenced`
    /// **and** older than `grace`. The `referenced` set is the live head hashes
    /// (from oplog replay); `grace` must exceed the longest barrier window
    /// (use [`cp_oplog::compact::DEFAULT_GC_GRACE`]) so an in-flight spill, which
    /// is referenced within that window, is never collected. Returns the number
    /// of bodies removed.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the directory cannot be listed or a removal
    /// fails. A body whose age cannot be determined is conservatively kept.
    pub fn gc(&self, referenced: &HashSet<ContentHash>, grace: Duration) -> BootResult<u64> {
        let referenced_hex: HashSet<String> = referenced.iter().map(|h| h.to_hex()).collect();
        let mut removed: u64 = 0;

        let read_dir = match fs::read_dir(&self.dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(Error::io(format!("list bodies {}", self.dir.display()), e)),
        };

        for dirent in read_dir {
            let entry = dirent.map_err(|e| Error::io("read body dir entry", e))?;
            let name_os = entry.file_name();
            let Some(name) = name_os.to_str() else { continue };
            // Only 64-char lowercase-hex names are bodies; skip tmp files and
            // anything else.
            if name.len() != 64 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            if referenced_hex.contains(name) {
                continue;
            }
            let Some(age) = file_age(&entry.path()) else { continue };
            if body_gc_eligible(age, grace) {
                fs::remove_file(entry.path()).map_err(|e| Error::io(format!("gc body {name}"), e))?;
                removed = removed.wrapping_add(1);
            }
        }
        Ok(removed)
    }

    /// The on-disk path of the spilled body for `hash`.
    fn body_path(&self, hash: ContentHash) -> PathBuf {
        self.dir.join(hash.to_hex())
    }

    /// The temp path a spill is written to before its atomic rename. The pid
    /// suffix keeps a crashed predecessor's leftover tmp from colliding.
    fn tmp_path(&self, hash: ContentHash) -> PathBuf {
        self.dir.join(format!("{}.tmp.{}", hash.to_hex(), std::process::id()))
    }
}

/// Write `bytes` to `path` and `fdatasync` so the file's data is durable before
/// the caller renames it into place.
fn write_durable(path: &Path, bytes: &[u8]) -> BootResult<()> {
    let mut file: File = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|e| Error::io(format!("open body tmp {}", path.display()), e))?;
    file.write_all(bytes).map_err(|e| Error::io(format!("write body tmp {}", path.display()), e))?;
    file.sync_data().map_err(|e| Error::io(format!("fdatasync body tmp {}", path.display()), e))?;
    Ok(())
}

/// `fsync` a directory so a freshly `rename`d child entry is durable.
fn sync_dir(dir: &Path) -> BootResult<()> {
    let handle = File::open(dir).map_err(|e| Error::io(format!("open dir {}", dir.display()), e))?;
    handle.sync_all().map_err(|e| Error::io(format!("fsync dir {}", dir.display()), e))?;
    Ok(())
}

/// The age of the file at `path`, or `None` if its modification time cannot be
/// read (so the caller can conservatively skip GC for it).
fn file_age(path: &Path) -> Option<Duration> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified.elapsed().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_oplog::compact::DEFAULT_GC_GRACE;
    use tempfile::tempdir;

    /// A store with a tiny threshold so a few bytes already spill.
    fn store(dir: &Path) -> Store {
        Store::open_with_threshold(dir, 4).expect("open")
    }

    #[test]
    fn small_body_is_inlined_with_no_file() {
        let dir = tempdir().expect("dir");
        let s = store(dir.path());
        let stored = s.put(b"hi").expect("put");
        assert_eq!(stored, Stored::Inline { hash: ContentHash::of(b"hi"), bytes: b"hi".to_vec() });
        assert!(!stored.is_spilled());
        // Nothing was written to the bodies directory.
        let files: Vec<_> = fs::read_dir(dir.path().join(BODIES_DIR)).expect("ls").collect();
        assert!(files.is_empty(), "an inline body must not create a file");
    }

    #[test]
    fn large_body_spills_durably_and_reads_back() {
        let dir = tempdir().expect("dir");
        let s = store(dir.path());
        let body = b"a body large enough to spill past the tiny threshold";
        let stored = s.put(body).expect("put");
        let hash = ContentHash::of(body);
        assert_eq!(stored, Stored::Spilled { hash });
        assert!(stored.is_spilled());
        // The file exists (durable) and reads back, integrity-checked.
        assert!(dir.path().join(BODIES_DIR).join(hash.to_hex()).exists(), "spilled file present");
        assert_eq!(s.get(hash).expect("get"), Some(body.to_vec()));
    }

    #[test]
    fn spill_is_idempotent_for_identical_content() {
        let dir = tempdir().expect("dir");
        let s = store(dir.path());
        let body = b"identical content spilled twice";
        let first = s.put(body).expect("first");
        let second = s.put(body).expect("second");
        assert_eq!(first, second, "same content yields the same Spilled hash");
        // Exactly one body file on disk.
        let count = fs::read_dir(dir.path().join(BODIES_DIR))
            .expect("ls")
            .filter(|e| e.as_ref().map(|e| e.file_name().to_string_lossy().len() == 64).unwrap_or(false))
            .count();
        assert_eq!(count, 1, "idempotent spill keeps a single file");
    }

    #[test]
    fn get_missing_returns_none() {
        let dir = tempdir().expect("dir");
        let s = store(dir.path());
        assert_eq!(s.get(ContentHash::of(b"never stored")).expect("get"), None);
    }

    #[test]
    fn get_detects_corruption() {
        let dir = tempdir().expect("dir");
        let s = store(dir.path());
        let body = b"a body that will be corrupted on disk";
        let hash = s.put(body).expect("put").hash();
        // Overwrite the file with different bytes — the hash no longer matches.
        fs::write(dir.path().join(BODIES_DIR).join(hash.to_hex()), b"tampered").expect("tamper");
        assert!(s.get(hash).is_err(), "a hash mismatch must be reported, not returned");
    }

    #[test]
    fn gc_collects_unreferenced_orphan_past_grace() {
        // I13 crash-orphan: a spilled body with no referencing entry, aged past
        // the grace, is a provable orphan and is reclaimed.
        let dir = tempdir().expect("dir");
        let s = store(dir.path());
        let hash = s.put(b"orphaned crash body").expect("put").hash();
        let referenced = HashSet::new(); // nothing references it
        let removed = s.gc(&referenced, Duration::ZERO).expect("gc");
        assert_eq!(removed, 1, "an aged, unreferenced orphan is collected");
        assert_eq!(s.get(hash).expect("get"), None, "the orphan file is gone");
    }

    #[test]
    fn gc_keeps_referenced_body() {
        let dir = tempdir().expect("dir");
        let s = store(dir.path());
        let hash = s.put(b"a referenced body").expect("put").hash();
        let referenced: HashSet<ContentHash> = std::iter::once(hash).collect();
        let removed = s.gc(&referenced, Duration::ZERO).expect("gc");
        assert_eq!(removed, 0, "a referenced body is never collected");
        assert!(s.get(hash).expect("get").is_some(), "referenced body survives gc");
    }

    #[test]
    fn gc_keeps_in_flight_spill_within_grace() {
        // The GAP 3 race guard: a freshly-spilled body (an in-flight spill whose
        // referencing entry is about to commit) is younger than the grace, so
        // gc must not delete it even though it is not yet referenced.
        let dir = tempdir().expect("dir");
        let s = store(dir.path());
        let hash = s.put(b"an in-flight spilled body").expect("put").hash();
        let referenced = HashSet::new(); // not referenced *yet*
        let removed = s.gc(&referenced, DEFAULT_GC_GRACE).expect("gc");
        assert_eq!(removed, 0, "a young in-flight spill within grace is protected");
        assert!(s.get(hash).expect("get").is_some(), "in-flight body survives gc");
    }
}
