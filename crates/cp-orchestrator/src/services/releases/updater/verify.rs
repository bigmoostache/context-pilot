//! Manifest verification — signature, freshness, anti-rollback (§5.4/§5.6).
//!
//! The order is deliberate: the **signature is checked before the JSON is even
//! parsed** — nothing inside an unsigned or tampered manifest is ever
//! believed, not even enough of it to produce a friendlier error. Then
//! freshness (a stale signed manifest cannot be replayed forever), then the
//! monotonic-version and `min_from` guards.

use super::super::manifest::Manifest;
use super::super::{UPDATE_PUBKEY, semver_sort_key};

/// Outcome of a successful verification.
#[derive(Debug)]
pub enum UpdateEvaluation {
    /// The channel's version equals the running one — nothing to do.
    UpToDate,
    /// A newer, applicable version is on offer.
    Available(Manifest),
}

/// A failed verification — every variant means "do not act on this manifest".
#[derive(Debug)]
pub enum VerifyError {
    /// The minisign signature does not verify against [`UPDATE_PUBKEY`].
    Signature(String),
    /// The (signed) JSON does not parse into the frozen [`Manifest`] schema.
    Parse(String),
    /// A timestamp field is malformed.
    Timestamp {
        /// Which manifest field failed to parse.
        field: &'static str,
        /// The offending raw value.
        value: String,
    },
    /// The manifest's `expires_at` is in the past — a stale replay.
    Expired {
        /// The manifest's expiry instant.
        expires_at: String,
    },
    /// The manifest offers a version at or below the running one.
    Rollback {
        /// The version the manifest offers.
        offered: String,
        /// The version this box runs.
        current: String,
    },
    /// The running version is below the manifest's `min_from` floor.
    TooOldForJump {
        /// The version this box runs.
        current: String,
        /// The manifest's minimum direct-jump version.
        min_from: String,
    },
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Signature(e) => write!(f, "manifest signature verification failed: {e}"),
            Self::Parse(e) => write!(f, "manifest does not match the frozen schema: {e}"),
            Self::Timestamp { field, value } => write!(f, "manifest {field} is not a valid timestamp: {value}"),
            Self::Expired { expires_at } => write!(f, "manifest expired at {expires_at} (stale replay?)"),
            Self::Rollback { offered, current } => {
                write!(f, "manifest offers {offered} which is not newer than the running {current}")
            }
            Self::TooOldForJump { current, min_from } => {
                write!(f, "running {current} is older than the manifest's min_from {min_from} — update refused")
            }
        }
    }
}

/// Verify a fetched manifest end-to-end and decide what it means for a box
/// running version `current` at `now_epoch_secs` (UTC seconds).
///
/// # Errors
///
/// Returns the first failed check as a [`VerifyError`] — on any error the
/// caller must keep its last-known state and **never** download anything.
pub fn evaluate_manifest(
    manifest_bytes: &[u8],
    signature: &str,
    current: &str,
    now_epoch_secs: u64,
) -> Result<UpdateEvaluation, VerifyError> {
    // 1. Signature over the exact bytes, against the embedded trust anchor.
    let key = minisign_verify::PublicKey::from_base64(UPDATE_PUBKEY)
        .map_err(|e| VerifyError::Signature(format!("embedded public key: {e}")))?;
    let sig = minisign_verify::Signature::decode(signature)
        .map_err(|e| VerifyError::Signature(format!("signature decode: {e}")))?;
    key.verify(manifest_bytes, &sig, false).map_err(|e| VerifyError::Signature(e.to_string()))?;

    // 2. Only now is the content worth parsing.
    let manifest: Manifest = serde_json::from_slice(manifest_bytes).map_err(|e| VerifyError::Parse(e.to_string()))?;

    // 3. Freshness (§5.6): refuse a signed-but-stale manifest.
    let expires = iso8601_to_epoch(&manifest.expires_at)
        .ok_or_else(|| VerifyError::Timestamp { field: "expires_at", value: manifest.expires_at.clone() })?;
    if expires <= now_epoch_secs {
        return Err(VerifyError::Expired { expires_at: manifest.expires_at });
    }

    // 4. Anti-rollback (§5.6): monotonic version + min_from floor.
    let offered_key = semver_sort_key(&manifest.version);
    let current_key = semver_sort_key(current);
    if offered_key == current_key {
        return Ok(UpdateEvaluation::UpToDate);
    }
    if offered_key < current_key {
        return Err(VerifyError::Rollback { offered: manifest.version, current: current.to_owned() });
    }
    if current_key < semver_sort_key(&manifest.min_from) {
        return Err(VerifyError::TooOldForJump { current: current.to_owned(), min_from: manifest.min_from });
    }

    Ok(UpdateEvaluation::Available(manifest))
}

/// Parse a `YYYY-MM-DDTHH:MM:SSZ` UTC timestamp into epoch seconds.
///
/// The manifest pipeline emits exactly this shape (`date -u
/// +%Y-%m-%dT%H:%M:%SZ` in CI); anything else is rejected rather than
/// guessed at. Days-from-civil per Howard Hinnant's algorithm — no chrono
/// dependency for one fixed format.
pub(crate) fn iso8601_to_epoch(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let hour: u64 = s.get(11..13)?.parse().ok()?;
    let minute: u64 = s.get(14..16)?.parse().ok()?;
    let second: u64 = s.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    // days_from_civil (proleptic Gregorian → days since 1970-01-01).
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = u64::try_from(era * 146_097 + doe - 719_468).ok()?;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}
