//! Unit tests for the SMS archive, the `mmcli` parser, and the gate.
//!
//! **None of these touch a modem.** The fixtures below were captured verbatim
//! from the real box (Armbian Photonicat 2, `mmcli 1.24.0`, Quectel RM520N-GL in
//! MBIM composition) — the shape is measured, not guessed — and every archive
//! test runs against its own temp-dir database, so they hold no global state and
//! can run in parallel.
//!
//! Split in two for the 500-line file cap: this file covers the parser, the
//! archive and the gate; [`ingest`] covers the sweep loop.

mod ingest;

use std::path::Path;

use serde_json::{Value, json};

use super::db::{Delivery, Direction, Inbound, Message, SmsStore};
use super::modem::parse_incoming;

// ── Fixtures, captured from the box ─────────────────────────────────────────

/// The text of the captured message, escaped because the workspace forbids
/// non-ASCII literals. It reads `Test accentue: eaeu <grinning face> <em dash>
/// ligne2` and ends in a newline — an emoji outside the BMP, a punctuation mark
/// outside Latin-1, and a control character, all three of which survived the
/// round trip through `mmcli` on the box.
const SUBMIT_TEXT: &str = "Test accentue: eaeu \u{1f600} \u{2014} ligne2\n";

/// A message this box created — `pdu-type: submit`, every property still the
/// `"--"` sentinel because nothing has been sent yet.
///
/// Captured from the box; only the text is lifted into [`SUBMIT_TEXT`] so the
/// literal here stays ASCII.
fn submit() -> Value {
    json!({"sms": {
        "content": { "data": "--", "number": "+33600000000", "text": SUBMIT_TEXT },
        "dbus-path": "/org/freedesktop/ModemManager1/SMS/0",
        "properties": {
            "class": "--", "delivery-report": "not requested", "delivery-state": "--",
            "discharge-timestamp": "--", "message-reference": "--", "pdu-type": "submit",
            "service-category": "--", "smsc": "--", "state": "--", "storage": "--",
            "teleservice-id": "--", "timestamp": "--", "validity": "--"
        }
    }})
}

/// The same document shape with the two fields that make it a delivery: the
/// `pdu-type` and a real network timestamp.
fn delivered(number: &str, text: &str, timestamp: &str) -> Value {
    json!({"sms": {
        "content": { "data": "--", "number": number, "text": text },
        "dbus-path": "/org/freedesktop/ModemManager1/SMS/3",
        "properties": {
            "class": "--", "delivery-report": "not requested", "delivery-state": "--",
            "discharge-timestamp": "--", "message-reference": "--", "pdu-type": "deliver",
            "service-category": "--", "smsc": "+33609001390", "state": "received",
            "storage": "mt", "teleservice-id": "--", "timestamp": timestamp, "validity": "--"
        }
    }})
}

/// The newest row of a listing.
///
/// A named helper rather than `list[0]`: the workspace forbids
/// `clippy::indexing_slicing` everywhere, tests included, and a panic message
/// naming what was missing beats an index-out-of-bounds.
fn only(messages: &[Message]) -> &Message {
    messages.first().expect("the listing has at least one message")
}

/// An archive on a fresh temp dir. The `TempDir` is returned so the caller binds
/// it for the test's lifetime — it is deleted on drop.
fn store() -> (tempfile::TempDir, SmsStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SmsStore::open(&dir.path().join("sms.db")).expect("open archive");
    (dir, store)
}

// ── The parser ──────────────────────────────────────────────────────────────

/// A delivery is parsed, with its number, its text and its timestamp.
#[test]
fn parses_a_delivery() {
    let document = delivered("+33612345678", "Bonjour", "2026-08-29T21:12:33+02:00");
    let parsed = parse_incoming("/org/freedesktop/ModemManager1/SMS/3", &document).expect("a deliver parses");
    assert_eq!(parsed.peer, "+33612345678");
    assert_eq!(parsed.body, "Bonjour");
    assert_eq!(parsed.handle, "/org/freedesktop/ModemManager1/SMS/3");
    // 2026-08-29T21:12:33+02:00 == 2026-08-29T19:12:33Z. The offset must be
    // applied, not ignored: mmcli reports the box's local zone.
    assert_eq!(parsed.sent_at, Some(1_788_030_753), "offset applied");
}

/// An outbound message is NOT ingested — it is already archived, and filing it
/// as inbound would show the operator their own message as if it had arrived.
#[test]
fn rejects_a_submit() {
    assert!(parse_incoming("/org/freedesktop/ModemManager1/SMS/0", &submit()).is_none(), "submit is not ingested");
}

/// `"--"` is mmcli's spelling of "no value", not a timestamp. A message that
/// carries it must archive with no network time rather than with a bogus one.
#[test]
fn absent_timestamp_is_none() {
    let document = delivered("+33612345678", "Sans horodatage", "--");
    let parsed = parse_incoming("/handle", &document).expect("still a delivery");
    assert_eq!(parsed.sent_at, None, "the sentinel is absence, not a date");
}

/// An unparseable timestamp is absence too. A modem whose clock was never set
/// would otherwise file today's message under 1970 and bury it.
#[test]
fn junk_timestamp_is_none() {
    let document = delivered("+33612345678", "Horodatage casse", "not-a-date");
    assert_eq!(parse_incoming("/handle", &document).expect("delivery").sent_at, None);
}

/// A short-code sender is alphanumeric, not a dialling number, and must survive
/// ingestion untouched — carriers use them for exactly the alerts this feature
/// is for.
#[test]
fn alphanumeric_sender_survives() {
    let document = delivered("Bouygues", "Votre conso...", "2026-08-29T10:00:00Z");
    assert_eq!(parse_incoming("/handle", &document).expect("delivery").peer, "Bouygues");
}

/// A delivery whose text is not populated yet is NOT ingested.
///
/// The regression test for a message destroyed in production on the deployed
/// box's very first receipt: `ModemManager` publishes the SMS object before its
/// content necessarily is, so a sweep landing in that window read `"--"`,
/// archived an empty body, and then deleted the modem's only copy. Skipping it
/// costs one storage slot for one poll interval; taking it costs the message.
#[test]
fn a_delivery_with_no_text_yet_is_left_alone() {
    let document = delivered("+33612345678", "--", "2026-08-30T10:01:00Z");
    assert!(parse_incoming("/handle", &document).is_none(), "not ours to take yet");
}

/// A genuinely EMPTY text is still a message, and is ingested.
///
/// The distinction the fix rests on: `"--"` is mmcli's spelling of absence,
/// `""` is a real (if odd) body. Collapsing the two would swap one bug for
/// another.
#[test]
fn an_empty_text_is_still_ingested() {
    let document = delivered("+33612345678", "", "2026-08-30T10:01:00Z");
    let parsed = parse_incoming("/handle", &document).expect("an empty body is a message");
    assert_eq!(parsed.body, "");
}

/// A document that is not an SMS at all yields `None` rather than panicking.
#[test]
fn malformed_document_is_none() {
    assert!(parse_incoming("/handle", &json!({})).is_none(), "no sms key");
    assert!(parse_incoming("/handle", &json!({ "sms": 3i32 })).is_none(), "sms is not an object");
}

// ── Idempotent ingestion — the reason `digest` exists ────────────────────────

/// The same message offered twice lands once.
///
/// This is the test that justifies the whole digest design: the ingester
/// re-offers a message every time a modem delete fails, which is a normal
/// occurrence, not an error path.
#[test]
fn re_ingesting_the_same_message_is_a_no_op() {
    let (_dir, store) = store();
    let first = store
        .insert_incoming(&Inbound {
            handle: "/h1",
            peer: "+33612345678",
            body: "Bonjour",
            sent_at: Some(1_787_080_353),
        })
        .expect("first");
    let again = store
        .insert_incoming(&Inbound {
            handle: "/h1",
            peer: "+33612345678",
            body: "Bonjour",
            sent_at: Some(1_787_080_353),
        })
        .expect("second");
    assert_eq!(first, again, "the same message maps to the same row");
    assert_eq!(store.list(None, 10).expect("list").len(), 1, "exactly one row");
}

/// Two genuinely different messages are two rows, even from the same sender at
/// the same second — the digest must not over-collapse.
#[test]
fn different_bodies_are_different_messages() {
    let (_dir, store) = store();
    let _first_row = store
        .insert_incoming(&Inbound { handle: "/h1", peer: "+33612345678", body: "Un", sent_at: Some(100) })
        .expect("first");
    let _second_row = store
        .insert_incoming(&Inbound { handle: "/h2", peer: "+33612345678", body: "Deux", sent_at: Some(100) })
        .expect("second");
    assert_eq!(store.list(None, 10).expect("list").len(), 2);
}

/// The peer's length is framed into the digest, so a body cannot be crafted to
/// impersonate a different sender by shifting the boundary.
#[test]
fn peer_boundary_cannot_be_shifted() {
    let (_dir, store) = store();
    let _first_row = store
        .insert_incoming(&Inbound { handle: "/h1", peer: "+3361", body: "234Bonjour", sent_at: Some(100) })
        .expect("first");
    let _second_row = store
        .insert_incoming(&Inbound { handle: "/h2", peer: "+3361234", body: "Bonjour", sent_at: Some(100) })
        .expect("second");
    assert_eq!(store.list(None, 10).expect("list").len(), 2, "concatenation must not collide");
}

// ── Listing ─────────────────────────────────────────────────────────────────

/// Newest first, and `before` walks backwards through the pages.
#[test]
fn lists_newest_first_and_paginates() {
    let (_dir, store) = store();
    for index in 0..5i64 {
        let _row = store
            .insert_incoming(&Inbound {
                handle: &format!("/h{index}"),
                peer: "+33612345678",
                body: &format!("message {index}"),
                sent_at: Some(index),
            })
            .expect("insert");
    }
    let first_page = store.list(None, 2).expect("page one");
    assert_eq!(first_page.len(), 2);
    assert_eq!(only(&first_page).body, "message 4", "newest first");
    let second_id = first_page.get(1).expect("a second row on page one").id;
    let next = store.list(Some(second_id), 2).expect("page two");
    assert_eq!(only(&next).body, "message 2", "continues where page one stopped");
}

/// An inbound message starts unread and stops counting once marked.
#[test]
fn unread_count_tracks_marking() {
    let (_dir, store) = store();
    let _row = store
        .insert_incoming(&Inbound { handle: "/h1", peer: "+33612345678", body: "Bonjour", sent_at: Some(1) })
        .expect("insert");
    assert_eq!(store.unread_count().expect("count"), 1);
    let id = only(&store.list(None, 1).expect("list")).id;
    assert!(store.mark_read(id).expect("mark"), "marking an existing message succeeds");
    assert_eq!(store.unread_count().expect("count"), 0);
}

/// Re-marking a message read is not a `404`: the row exists, the caller's
/// intent is already satisfied.
#[test]
fn re_marking_read_still_reports_found() {
    let (_dir, store) = store();
    let _row = store
        .insert_incoming(&Inbound { handle: "/h1", peer: "+33612345678", body: "Bonjour", sent_at: Some(1) })
        .expect("insert");
    let id = only(&store.list(None, 1).expect("list")).id;
    assert!(store.mark_read(id).expect("first"), "first mark");
    assert!(store.mark_read(id).expect("second"), "already read is still found");
}

/// A missing id is reported as missing by both by-id operations.
#[test]
fn missing_id_is_reported() {
    let (_dir, store) = store();
    assert!(!store.mark_read(4242).expect("mark"), "no such message");
    assert!(!store.delete(4242).expect("delete"), "no such message");
}

/// Deleting removes the row.
#[test]
fn delete_removes_the_row() {
    let (_dir, store) = store();
    let _row = store
        .insert_incoming(&Inbound { handle: "/h1", peer: "+33612345678", body: "Bonjour", sent_at: Some(1) })
        .expect("insert");
    let id = only(&store.list(None, 1).expect("list")).id;
    assert!(store.delete(id).expect("delete"), "deleted");
    assert!(store.list(None, 10).expect("list").is_empty());
}

// ── Outbound ────────────────────────────────────────────────────────────────

/// An outbound message is archived before the modem is touched, and its
/// terminal state and audit trail are recorded.
#[test]
fn outbound_records_state_and_sender() {
    let (_dir, store) = store();
    let id = store.record_outgoing("+33612345678", "Coucou", Some("user-7")).expect("record");
    let pending = store.list(None, 1).expect("list");
    assert_eq!(only(&pending).delivery, Delivery::Sending, "written before the send");
    assert_eq!(only(&pending).direction, Direction::Sent);
    assert_eq!(only(&pending).sent_by.as_deref(), Some("user-7"), "audit trail");
    store.finish_outgoing(id, None).expect("finish");
    assert_eq!(only(&store.list(None, 1).expect("list")).delivery, Delivery::Sent);
}

/// A refused send is recorded as failed, with the modem's own words kept.
#[test]
fn failed_send_keeps_the_reason() {
    let (_dir, store) = store();
    let id = store.record_outgoing("+33612345678", "Coucou", Some("user-7")).expect("record");
    store.finish_outgoing(id, Some("no network coverage")).expect("finish");
    let rows = store.list(None, 1).expect("list");
    let row = only(&rows);
    assert_eq!(row.delivery, Delivery::Failed);
    assert_eq!(row.error.as_deref(), Some("no network coverage"));
}

/// An outbound message is never counted as unread — the badge is about what
/// arrived, not about what we sent.
#[test]
fn outbound_is_not_unread() {
    let (_dir, store) = store();
    let _id = store.record_outgoing("+33612345678", "Coucou", None).expect("record");
    assert_eq!(store.unread_count().expect("count"), 0);
}

/// The rate limiter counts only what this sender sent inside the window.
#[test]
fn sent_since_counts_per_sender_and_window() {
    let (_dir, store) = store();
    let _first = store.record_outgoing("+33612345678", "Un", Some("user-7")).expect("record");
    let _second = store.record_outgoing("+33612345678", "Deux", Some("user-9")).expect("record");
    let now = super::db::now_s();
    let window = now.saturating_sub(3_600);
    assert_eq!(store.sent_since(window, Some("user-7")).expect("count"), 1, "one sender");
    assert_eq!(store.sent_since(window, None).expect("count"), 2, "the whole box");
    assert_eq!(store.sent_since(now.saturating_add(60), None).expect("count"), 0, "a future window is empty");
}

// ── Retention ───────────────────────────────────────────────────────────────

/// The volume ceiling keeps the newest rows and drops the rest.
#[test]
fn retention_bounds_by_volume() {
    let (_dir, store) = store();
    for index in 0..10i64 {
        let _row = store
            .insert_incoming(&Inbound {
                handle: &format!("/h{index}"),
                peer: "+33612345678",
                body: &format!("message {index}"),
                sent_at: Some(index),
            })
            .expect("insert");
    }
    assert_eq!(store.prune(super::db::now_s(), 365, 4).expect("prune"), 6, "six removed");
    let kept = store.list(None, 100).expect("list");
    assert_eq!(kept.len(), 4);
    assert_eq!(only(&kept).body, "message 9", "the newest survived");
}

/// Age and volume are independent bounds: a message inside the volume ceiling
/// still goes once it falls out of the retention window.
///
/// The clock is supplied, not read — `prune` takes `now` precisely so this can
/// be asserted without backdating a row behind the store's back.
#[test]
fn retention_bounds_by_age() {
    const DAY_S: i64 = 86_400;
    let (_dir, store) = store();
    let _row = store
        .insert_incoming(&Inbound { handle: "/h1", peer: "+33612345678", body: "Vieux", sent_at: Some(1) })
        .expect("insert");
    let now = super::db::now_s();
    assert_eq!(store.prune(now, 30, 5_000).expect("prune"), 0, "inside the window, kept");
    let in_two_months = now.saturating_add(DAY_S.saturating_mul(60));
    assert_eq!(store.prune(in_two_months, 30, 5_000).expect("prune"), 1, "outside the window, dropped");
    assert!(store.list(None, 10).expect("list").is_empty());
}

// ── The gate ────────────────────────────────────────────────────────────────

/// With no `CP_MMCLI_BIN` — a laptop, CI, any box with no `ModemManager` — the
/// feature reports itself absent and spawns nothing.
///
/// This is the test that keeps `cargo test` from touching a modem, and it is why
/// the whole module hangs off one env gate.
#[test]
fn no_tool_means_no_sms() {
    assert!(!super::modem::available(), "no CP_MMCLI_BIN in the test environment");
    assert_eq!(super::status_json(Path::new("/nonexistent"), true), Value::Null, "no tool, so no sms status");
}

/// A box with no 5G module reports `null` even where the tool exists — the
/// hardware fact outranks the tool.
#[test]
fn no_modem_means_no_sms() {
    assert_eq!(super::status_json(Path::new("/nonexistent"), false), Value::Null, "no modem, so no sms status");
}
