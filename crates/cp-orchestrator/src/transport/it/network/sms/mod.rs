//! SMS — read, send and archive the messages of the box's own SIM.
//!
//! **Only ever present on a 5G variant.** The gate is not new: it is
//! [`modem_present`](super::apply::modem_present), the same sysfs probe that
//! already decides whether the cockpit offers the 5G uplink modes at all. A box
//! with no M.2 module reports `status.sms: null` and the panel does not render.
//!
//! Three pieces, and the seam between them is the point:
//!
//! * [`modem`] — `mmcli`, and nothing else knows the modem exists.
//! * [`db`] — the archive, which outlives the modem's few storage slots.
//! * [`poll`] — the background ingester that moves messages from the first to
//!   the second, then frees the slot.
//!
//! # Two boundaries this module does not cross
//!
//! **It never writes network configuration.** Not the uplink mode, not the
//! `cp-wwan` profile, not `ModemManager`'s state. `apply` is the only writer of
//! system network config, and a comfort feature must not be able to disturb the
//! box's connectivity. A modem that is not registered is reported, not repaired.
//!
//! **Sending costs the vendor money.** The SIM and the data plan are ours — the
//! same reason the APN sits behind `can_manage_secrets`. So an operator may send
//! (it is their site to run), but every send is rate-limited and carries the
//! user id that ordered it. See [`RATE_PER_USER_HOURLY`].

pub(crate) mod db;
mod modem;
pub(crate) mod poll;
mod records;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Backend, HttpReply};
use db::{Delivery, Direction, Message, SmsStore};

// ── Configuration, all env-gated like the rest of this neighbourhood ─────────

/// Whether the feature is armed at all. `CP_SMS_ENABLED=0` disarms it
/// completely — the poller never starts and every route answers `503`.
///
/// A client who does not want message content stored on their appliance gets
/// one switch, not a documentation note.
pub(crate) fn enabled() -> bool {
    std::env::var("CP_SMS_ENABLED").map_or(true, |raw| raw != "0")
}

/// Read a positive integer from the environment, falling back on anything
/// missing or unparseable.
fn env_number(key: &str, fallback: i64) -> i64 {
    std::env::var(key).ok().and_then(|raw| raw.trim().parse::<i64>().ok()).filter(|n| *n > 0).unwrap_or(fallback)
}

/// Days a message is kept. `CP_SMS_RETENTION_DAYS`, default 90.
fn retention_days() -> i64 {
    env_number("CP_SMS_RETENTION_DAYS", 90)
}

/// Hard ceiling on archived messages, whatever their age. Age alone would let a
/// flood fill the disk inside the window.
const RETENTION_ROWS: i64 = 5_000;

/// Messages one operator may send per hour.
const RATE_PER_USER_HOURLY: i64 = 10;
/// Messages the whole box may send per day, across every operator.
const RATE_GLOBAL_DAILY: i64 = 50;
/// Seconds in the per-user window.
const HOUR_S: i64 = 3_600;
/// Seconds in the global window.
const DAY_S: i64 = 86_400;

/// Longest body we will hand the modem: ten UCS-2 segments.
///
/// Not a round number for looks — a concatenated SMS carries 67 UCS-2
/// characters per segment once the UDH is subtracted, and ten segments is where
/// carriers stop being reliable about reassembly.
const MAX_BODY_CHARS: usize = 670;

/// Default page size for the archive listing.
const DEFAULT_LIMIT: i64 = 50;
/// Largest page a caller may ask for.
const MAX_LIMIT: i64 = 200;

// ── Store access ────────────────────────────────────────────────────────────

/// The agents dir, or the reply to send when the backend lock is poisoned.
fn agents_dir(state: &Mutex<Backend>) -> Result<PathBuf, HttpReply> {
    match state.lock() {
        Ok(backend) => Ok(backend.agents_dir.clone()),
        Err(_poisoned) => Err(HttpReply::error(500, "backend lock poisoned")),
    }
}

/// Open the archive for one request.
///
/// Per-call rather than a long-lived handle behind a mutex, deliberately:
/// `SQLite` opens in microseconds, the poller and the handlers then share no
/// state at all, and — the reason that decided it — every test gets its own
/// database from its own temp dir with no global to reset between them.
fn store_of(dir: &Path) -> Result<SmsStore, HttpReply> {
    SmsStore::open(&db::sms_path(dir)).map_err(|failure| HttpReply::error(500, &failure))
}

/// The `503` every route answers when the feature is disarmed.
fn disarmed() -> HttpReply {
    HttpReply::error(503, "SMS is disabled on this box")
}

// ── Status, folded into `GET /api/it/network` ───────────────────────────────

/// The `status.sms` object, or `null` on a box that cannot do SMS.
///
/// `null` for the same two reasons the bearer is: no modem, or no tool. It
/// deliberately does **not** spawn `mmcli` — `status::probe` already spawns
/// several per poll, and "is there a modem" is answered from sysfs, which needs
/// no subprocess and cannot flap while `ModemManager` restarts.
pub(crate) fn status_json(dir: &Path, has_modem: bool) -> Value {
    if !has_modem || !enabled() || !modem::available() {
        return Value::Null;
    }
    let unread = SmsStore::open(&db::sms_path(dir)).and_then(|store| store.unread_count()).unwrap_or(0);
    json!({ "available": true, "unread": unread })
}

// ── Read routes ─────────────────────────────────────────────────────────────

/// `GET /api/it/sms` — one page of the archive, newest first.
pub(crate) fn list(state: &Mutex<Backend>, query: &str) -> HttpReply {
    if !enabled() {
        return disarmed();
    }
    let dir = match agents_dir(state) {
        Ok(dir) => dir,
        Err(reply) => return reply,
    };
    let store = match store_of(&dir) {
        Ok(store) => store,
        Err(reply) => return reply,
    };
    let before = param(query, "before").and_then(|raw| raw.parse::<i64>().ok());
    let limit =
        param(query, "limit").and_then(|raw| raw.parse::<i64>().ok()).unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    match store.list(before, limit) {
        Ok(messages) => HttpReply::ok(&json!({ "messages": messages.iter().map(as_json).collect::<Vec<_>>() })),
        Err(failure) => HttpReply::error(500, &failure),
    }
}

/// One message, as the cockpit reads it.
fn as_json(message: &Message) -> Value {
    json!({
        "id": message.id,
        "direction": match message.direction {
            Direction::Received => "received",
            Direction::Sent => "sent",
        },
        "peer": message.peer,
        "body": message.body,
        "sent_at": message.sent_at,
        "ingested_at": message.ingested_at,
        "delivery": match message.delivery {
            Delivery::Received => "received",
            Delivery::Sending => "sending",
            Delivery::Sent => "sent",
            Delivery::Failed => "failed",
        },
        "read": message.read_at.is_some(),
        "sent_by": message.sent_by,
        "error": message.error,
    })
}

/// Read one `key=value` out of a raw query string.
///
/// A local parser rather than a dependency: this route has exactly two
/// parameters, both plain integers, and neither can contain an escape.
fn param(query: &str, key: &str) -> Option<String> {
    // `find_map`, not `find`: it hands the pair over BY VALUE, so the closure
    // binds two `&str` rather than a reference to a tuple of them — which is
    // what `clippy::pattern_type_mismatch` (forbid) objects to.
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(name, value)| (name == key).then(|| value.to_owned()))
}

// ── Write routes ────────────────────────────────────────────────────────────

/// `POST /api/it/sms/{id}/read` — mark one message read.
pub(crate) fn mark_read(state: &Mutex<Backend>, raw_id: &str) -> HttpReply {
    with_id(state, raw_id, SmsStore::mark_read)
}

/// `DELETE /api/it/sms/{id}` — drop one message from the archive.
///
/// Our copy only: the modem's was removed at ingestion, which is what keeps its
/// storage from filling.
pub(crate) fn remove(state: &Mutex<Backend>, raw_id: &str) -> HttpReply {
    with_id(state, raw_id, SmsStore::delete)
}

/// The shared shape of the two by-id routes: parse, open, act, and turn a
/// `false` into the `404` it means.
fn with_id<F>(state: &Mutex<Backend>, raw_id: &str, act: F) -> HttpReply
where
    F: FnOnce(&SmsStore, i64) -> Result<bool, String>,
{
    if !enabled() {
        return disarmed();
    }
    let Ok(id) = raw_id.parse::<i64>() else {
        return HttpReply::error(400, "id must be an integer");
    };
    let dir = match agents_dir(state) {
        Ok(dir) => dir,
        Err(reply) => return reply,
    };
    let store = match store_of(&dir) {
        Ok(store) => store,
        Err(reply) => return reply,
    };
    match act(&store, id) {
        Ok(true) => HttpReply::ok(&json!({ "ok": true })),
        Ok(false) => HttpReply::error(404, "no such message"),
        Err(failure) => HttpReply::error(500, &failure),
    }
}

/// Request body for a send.
#[derive(Deserialize)]
struct SendReq {
    /// Destination number. Named `recipient` in Rust and `to` on the wire.
    #[serde(rename = "to")]
    recipient: String,
    /// The text to send.
    body: String,
}

/// `POST /api/it/sms` — send one message.
///
/// `sender` is the authenticated user id, recorded before the modem is touched
/// so a send that dies mid-flight still leaves an audit trail.
pub(crate) fn send(state: &Mutex<Backend>, body: &[u8], sender: Option<&str>) -> HttpReply {
    if !enabled() {
        return disarmed();
    }
    let Ok(req) = serde_json::from_slice::<SendReq>(body) else {
        return HttpReply::error(400, "expected {\"to\": string, \"body\": string}");
    };
    if let Err(reason) = validate(&req) {
        return HttpReply::error(400, reason);
    }
    let dir = match agents_dir(state) {
        Ok(dir) => dir,
        Err(reply) => return reply,
    };
    let store = match store_of(&dir) {
        Ok(store) => store,
        Err(reply) => return reply,
    };
    if let Some(reason) = over_rate_limit(&store, sender) {
        return HttpReply::error(429, &reason);
    }
    dispatch(&store, &req, sender)
}

/// Reject a body the modem or the carrier would reject anyway, with a reason
/// the operator can act on.
///
/// The server is the authority here; the cockpit mirrors these rules only so
/// the operator gets the message without a round trip.
fn validate(req: &SendReq) -> Result<(), &'static str> {
    let digits = req.recipient.strip_prefix('+').unwrap_or(&req.recipient);
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("number must be digits, optionally prefixed with +");
    }
    if digits.chars().count() < 6 || digits.chars().count() > 15 {
        return Err("number must be 6 to 15 digits (E.164)");
    }
    if req.body.is_empty() {
        return Err("body must not be empty");
    }
    if req.body.chars().count() > MAX_BODY_CHARS {
        return Err("body is too long (max 670 characters)");
    }
    Ok(())
}

/// Why this send is refused, or `None` when it is within both ceilings.
fn over_rate_limit(store: &SmsStore, sender: Option<&str>) -> Option<String> {
    let now = db::now_s();
    // FAIL CLOSED. A read error here used to count as zero, which turned a
    // momentarily locked database into unlimited sending on someone else's
    // bill. A ceiling that opens when it cannot see is not a ceiling.
    let hourly = match store.sent_since(now.saturating_sub(HOUR_S), sender) {
        Ok(count) => count,
        Err(failure) => return Some(format!("cannot verify the send ceiling ({failure})")),
    };
    if hourly >= RATE_PER_USER_HOURLY {
        return Some(format!("rate limit: {RATE_PER_USER_HOURLY} messages per hour"));
    }
    let daily = match store.sent_since(now.saturating_sub(DAY_S), None) {
        Ok(count) => count,
        Err(failure) => return Some(format!("cannot verify the send ceiling ({failure})")),
    };
    if daily >= RATE_GLOBAL_DAILY {
        return Some(format!("rate limit: {RATE_GLOBAL_DAILY} messages per day for this box"));
    }
    None
}

/// Archive the attempt, hand it to the modem, then record how it went.
///
/// The row is written **first**: if the modem call hangs and the process is
/// killed, the archive still shows what was attempted and by whom.
fn dispatch(store: &SmsStore, req: &SendReq, sender: Option<&str>) -> HttpReply {
    let id = match store.record_outgoing(&req.recipient, &req.body, sender) {
        Ok(id) => id,
        Err(failure) => return HttpReply::error(500, &failure),
    };
    let Some(radio) = modem::Modem::detect() else {
        let _recorded = store.finish_outgoing(id, Some("no modem available"));
        return HttpReply::error(503, "no modem available");
    };
    match radio.send(&req.recipient, &req.body) {
        Ok(()) => {
            let _recorded = store.finish_outgoing(id, None);
            HttpReply::ok(&json!({ "id": id, "delivery": "sent" }))
        }
        Err(failure) => {
            let _recorded = store.finish_outgoing(id, Some(&failure));
            HttpReply::error(502, &failure)
        }
    }
}
