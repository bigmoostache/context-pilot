//! The oplog error type and its `Result` alias.
//!
//! Every fallible oplog operation — opening, appending, syncing, replaying,
//! compacting — surfaces an [`Error`]. It distinguishes a filesystem
//! failure ([`Error::Io`]) from a framing failure ([`Error::Frame`],
//! a [`cp_wire::framing::FrameError`]); a caller almost always treats both as
//! fatal-to-the-write, but keeping them separate lets a diagnostic tell a
//! genuine I/O fault from a serialisation/size bug apart.
//!
//! The type deliberately does **not** implement [`std::error::Error`]: that
//! trait's (unstable) `provide` method trips the workspace's
//! `missing_trait_methods` lint, and the oplog never needs `dyn Error` erasure
//! — callers match the two variants directly.

use std::fmt;
use std::io;

use cp_wire::framing::FrameError;

/// An error from oplog operations.
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "oplog error taxonomy is a closed two-variant set (Io/Frame) constructed within cp-oplog and matched exhaustively by callers; #[non_exhaustive] would force cross-crate wildcard arms that the forbidden wildcard_enum_match_arm lint rejects"
)]
pub enum Error {
    /// An underlying filesystem operation failed (open, write, sync, …).
    Io(io::Error),

    /// The entry could not be framed (serialisation failed or it was too
    /// large to carry a 32-bit length prefix).
    Frame(FrameError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "oplog I/O error: {e}"),
            Self::Frame(e) => write!(f, "oplog framing error: {e}"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<FrameError> for Error {
    fn from(e: FrameError) -> Self {
        Self::Frame(e)
    }
}

/// A `Result` whose error is an [`Error`].
pub type OplogResult<T> = Result<T, Error>;
