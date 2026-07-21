//! The bridge boot error type and its `Result` alias.
//!
//! Boot is the one place the bridge can fail loudly and refuse to run: a
//! second instance contending for the same folder lock (design doc I1/D2), a
//! filesystem fault while binding the stream socket or writing the registry,
//! or an oplog that will not open. [`Error`] keeps these distinguishable so
//! a caller can tell "another agent already owns this folder" (expected,
//! recoverable — surface it to the user) from "the disk is broken" (fatal).
//!
//! Like [`cp_oplog::error::OplogError`], this type deliberately does **not**
//! implement [`std::error::Error`]: that trait's unstable `provide` method
//! trips the workspace's `missing_trait_methods` lint, and the bridge never
//! needs `dyn Error` erasure — callers match the variants directly.

use std::fmt;
use std::io;

/// An error raised while booting the agent-side bridge.
#[derive(Debug)]
pub enum Error {
    /// Another live agent already holds the exclusive lock on this folder, so
    /// this instance must not run (single-process exclusion, design doc I1/D2).
    AlreadyRunning {
        /// The folder whose lock could not be acquired.
        folder: String,
    },

    /// A filesystem operation failed (lock file, socket, registry, or oplog
    /// directory). The `context` names which step, the `source` carries the
    /// underlying OS error.
    Io {
        /// Human-readable name of the boot step that failed.
        context: String,
        /// The underlying I/O error.
        source: io::Error,
    },
}

impl Error {
    /// Build an [`Error::Io`] tagging the failing step with `context`.
    pub(crate) fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io { context: context.into(), source }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        cp_base::deref_match!(self, {
            Self::AlreadyRunning { ref folder } => {
                write!(f, "another agent already owns the folder {folder}")
            }
            Self::Io { ref context, ref source } => write!(f, "bridge boot failed ({context}): {source}"),
        })
    }
}

/// A `Result` whose error is a [`Error`].
pub type BootResult<T> = Result<T, Error>;
