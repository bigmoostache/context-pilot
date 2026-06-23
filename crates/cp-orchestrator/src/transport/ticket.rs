//! Single-use SSE upgrade tickets (design doc I9b).
//!
//! The browser endpoint cannot be protected by filesystem permissions — its
//! attacker is a **malicious website** in the user's own browser reaching
//! `http://localhost`, a confused-deputy / DNS-rebind threat that `Origin` and
//! CORS cannot be trusted to stop. The defence is a short-lived, **single-use**
//! ticket: the frontend mints one over a same-origin request (a cross-origin
//! site can *trigger* the mint but, blocked by CORS, can never *read* the
//! returned token) and immediately redeems it on the streaming `GET`. A ticket
//! is valid exactly once and only within its TTL.
//!
//! The store is a plain in-memory map guarded by the caller's lock; it owns no
//! threads. Expired tickets are swept lazily on every mint and redeem, so a
//! flood of un-redeemed mints cannot grow the map without bound past one TTL
//! window of churn.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default ticket lifetime — long enough for the client to mint-then-connect,
/// short enough that a leaked ticket is useless within a round-trip.
pub const DEFAULT_TTL: Duration = Duration::from_secs(30);

/// Number of random bytes in a ticket token (256-bit, hex-encoded).
const TOKEN_BYTES: usize = 32;

/// Data stored alongside a minted ticket.
#[derive(Debug, Clone)]
struct TicketEntry {
    /// Expiry timestamp (ms since the Unix epoch).
    expiry_ms: u64,
    /// The authenticated user who minted this ticket, if any. `None` when auth
    /// is disabled — the ticket still gates the SSE upgrade but carries no
    /// identity.
    user_id: Option<String>,
}

/// The payload returned on a successful single-use redemption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedeemedTicket {
    /// The user who minted the ticket (`None` when auth is disabled).
    pub user_id: Option<String>,
}

/// A mint-once, redeem-once SSE upgrade ticket store.
#[derive(Debug)]
pub struct TicketStore {
    /// token → entry (expiry + optional user identity).
    live: HashMap<String, TicketEntry>,
    /// Ticket lifetime.
    ttl: Duration,
}

impl TicketStore {
    /// Create a store with the default TTL.
    #[must_use]
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_TTL)
    }

    /// Create a store with an explicit ticket TTL.
    #[must_use]
    pub fn with_ttl(ttl: Duration) -> Self {
        Self { live: HashMap::new(), ttl }
    }

    /// Mint a fresh single-use ticket, returning its opaque token.
    ///
    /// `user_id` is the authenticated caller's identity — stored in the ticket
    /// so the SSE redeem path can enforce per-agent ACL without a Bearer header
    /// (Phase 7, design doc §7). Pass `None` when auth is disabled.
    ///
    /// Sweeps expired tickets first so the map stays bounded.
    pub fn mint(&mut self, user_id: Option<&str>) -> String {
        let now = now_ms();
        self.sweep(now);
        let token = random_token();
        let expiry_ms = now.saturating_add(saturating_millis(self.ttl));
        let entry = TicketEntry { expiry_ms, user_id: user_id.map(str::to_owned) };
        let _previous = self.live.insert(token.clone(), entry);
        token
    }

    /// Redeem a ticket. Returns `Some(RedeemedTicket)` exactly once per
    /// minted, unexpired token — carrying the user identity embedded at mint
    /// time. Every subsequent or unknown redemption returns `None`.
    pub fn redeem(&mut self, token: &str) -> Option<RedeemedTicket> {
        let now = now_ms();
        self.sweep(now);
        match self.live.remove(token) {
            Some(entry) if entry.expiry_ms >= now => {
                Some(RedeemedTicket { user_id: entry.user_id })
            }
            _ => None,
        }
    }

    /// Number of live (un-redeemed, unexpired) tickets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// Whether the store holds no live tickets.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// Drop every ticket whose expiry is in the past.
    fn sweep(&mut self, now_ms: u64) {
        self.live.retain(|_token, entry| entry.expiry_ms >= now_ms);
    }
}

impl Default for TicketStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Milliseconds since the Unix epoch, saturating at 0 on a pre-epoch clock.
fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| saturating_millis(d))
}

/// A `Duration` as `u64` milliseconds, saturating on overflow.
fn saturating_millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// A 256-bit random token, lowercase-hex, sourced from `/dev/urandom`.
///
/// Mirrors the bridge's `cap_token` minting (no `rand` dependency). A read
/// failure falls back to a time-seeded value — degraded but never panicking;
/// tickets are a defence-in-depth layer, not the sole authority.
fn random_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes)).is_err() {
        // Fallback: spread a nanosecond clock across the buffer.
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0u128, |d| d.as_nanos());
        for (i, slot) in bytes.iter_mut().enumerate() {
            let shift = u32::try_from((i % 16).saturating_mul(8)).unwrap_or(0);
            *slot = u8::try_from(seed.wrapping_shr(shift) & 0xff).unwrap_or(0);
        }
    }
    to_hex(&bytes)
}

/// Lowercase-hex encode a byte slice.
fn to_hex(bytes: &[u8]) -> String {
    use core::fmt::Write as _;
    let mut hex = String::with_capacity(bytes.len().saturating_mul(2));
    for &byte in bytes {
        _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn mint_then_redeem_succeeds_once() {
        let mut store = TicketStore::new();
        let token = store.mint(None);
        assert_eq!(store.len(), 1);
        assert!(store.redeem(&token).is_some(), "first redemption succeeds");
        assert!(store.redeem(&token).is_none(), "second redemption fails (single-use)");
        assert!(store.is_empty(), "redeemed ticket is consumed");
    }

    #[test]
    fn unknown_token_is_rejected() {
        let mut store = TicketStore::new();
        assert!(store.redeem("never-minted").is_none());
    }

    #[test]
    fn expired_ticket_is_rejected() {
        let mut store = TicketStore::with_ttl(Duration::from_millis(10));
        let token = store.mint(None);
        sleep(Duration::from_millis(30));
        assert!(store.redeem(&token).is_none(), "expired ticket must not redeem");
        assert!(store.is_empty(), "sweep dropped the expired ticket");
    }

    #[test]
    fn tokens_are_unique_and_long() {
        let mut store = TicketStore::new();
        let a = store.mint(None);
        let b = store.mint(None);
        assert_ne!(a, b, "each mint yields a distinct token");
        assert_eq!(a.len(), TOKEN_BYTES * 2, "256-bit hex token");
    }

    #[test]
    fn mint_sweeps_expired() {
        let mut store = TicketStore::with_ttl(Duration::from_millis(10));
        let _stale = store.mint(None);
        sleep(Duration::from_millis(30));
        let _fresh = store.mint(None); // mint sweeps the stale one first
        assert_eq!(store.len(), 1, "only the fresh ticket survives");
    }

    #[test]
    fn ticket_carries_user_id() {
        let mut store = TicketStore::new();
        let token = store.mint(Some("user-42"));
        let ticket = store.redeem(&token).expect("valid ticket");
        assert_eq!(ticket.user_id.as_deref(), Some("user-42"));
    }

    #[test]
    fn ticket_without_user_redeems_as_none() {
        let mut store = TicketStore::new();
        let token = store.mint(None);
        let ticket = store.redeem(&token).expect("valid ticket");
        assert!(ticket.user_id.is_none());
    }
}
