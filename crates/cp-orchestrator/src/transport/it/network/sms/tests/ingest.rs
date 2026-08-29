//! The ingestion sweep — the archive-then-delete contract in isolation.
//!
//! Driven through a fake [`MessageSource`](super::super::super::modem::MessageSource)
//! that records its deletions and can be told to refuse them, so the ordering
//! invariant is asserted with no modem and no `ModemManager` in sight.

use super::super::db::Inbound;
use super::{only, store};

/// A modem that answers from a script, records what was deleted, and can be
/// told to refuse its deletions.
///
/// This is the seam that makes the archive-then-delete invariant testable at
/// all: with no fake, `poll::ingest` could only ever be exercised against real
/// hardware, and the ordering it depends on would be asserted nowhere.
struct FakeModem {
    /// What `list_incoming` returns, mutable so a test can stage a second sweep.
    waiting: std::cell::RefCell<Vec<super::super::modem::Incoming>>,
    /// Handles the ingester asked to delete, in order.
    deleted: std::cell::RefCell<Vec<String>>,
    /// Whether every deletion fails — the case the digest design exists for.
    refuse_deletes: bool,
}

impl FakeModem {
    /// A modem holding `waiting`, whose deletions succeed.
    fn holding(waiting: Vec<super::super::modem::Incoming>) -> Self {
        Self {
            waiting: std::cell::RefCell::new(waiting),
            deleted: std::cell::RefCell::new(Vec::new()),
            refuse_deletes: false,
        }
    }

    /// The same, but every deletion fails.
    fn refusing(waiting: Vec<super::super::modem::Incoming>) -> Self {
        Self { refuse_deletes: true, ..Self::holding(waiting) }
    }
}

impl super::super::modem::MessageSource for FakeModem {
    fn list_incoming(&self) -> Result<Vec<super::super::modem::Incoming>, String> {
        Ok(self.waiting.borrow().clone())
    }

    fn delete(&self, handle: &str) -> Result<(), String> {
        if self.refuse_deletes {
            return Err("modem busy".to_owned());
        }
        self.deleted.borrow_mut().push(handle.to_owned());
        self.waiting.borrow_mut().retain(|message| message.handle != handle);
        Ok(())
    }
}

/// Build one message as the parser would hand it over.
fn incoming(handle: &str, peer: &str, body: &str, sent_at: Option<i64>) -> super::super::modem::Incoming {
    super::super::modem::Incoming { handle: handle.to_owned(), peer: peer.to_owned(), body: body.to_owned(), sent_at }
}

/// The happy path: archived, then removed from the modem, in that order.
#[test]
fn ingest_archives_then_frees_the_slot() {
    let (_dir, store) = store();
    let radio = FakeModem::holding(vec![incoming("/SMS/0", "+33612345678", "Bonjour", Some(100))]);
    super::super::poll::ingest(&store, &radio);
    assert_eq!(store.list(None, 10).expect("list").len(), 1, "archived");
    assert_eq!(radio.deleted.borrow().as_slice(), ["/SMS/0"], "slot freed");
}

/// A refused deletion must NOT lose the message and must NOT duplicate it: the
/// next sweep re-reads the same modem entry, recognises it, and retries.
///
/// This is the failure the content digest was designed for, and it is a normal
/// occurrence — a busy modem, a `ModemManager` restart — not an exotic one.
#[test]
fn a_refused_delete_retries_without_duplicating() {
    let (_dir, store) = store();
    let radio = FakeModem::refusing(vec![incoming("/SMS/0", "+33612345678", "Bonjour", Some(100))]);
    super::super::poll::ingest(&store, &radio);
    super::super::poll::ingest(&store, &radio);
    super::super::poll::ingest(&store, &radio);
    assert_eq!(store.list(None, 10).expect("list").len(), 1, "three sweeps, still one message");
}

/// Once the modem's copy is gone, an identical message that arrives LATER is a
/// new message and must be archived as one.
///
/// The regression test for the bug a plain `UNIQUE(digest)` caused: a carrier
/// short-code sending the same alert twice with no network timestamp — a real,
/// tested shape — had its second copy silently swallowed AND deleted from the
/// modem. Message loss, invisible to everyone.
#[test]
fn an_identical_message_arriving_later_is_a_new_message() {
    let (_dir, store) = store();
    let alert = |handle: &str| incoming(handle, "Bouygues", "Votre conso atteint 80%", None);

    let monday = FakeModem::holding(vec![alert("/SMS/0")]);
    super::super::poll::ingest(&store, &monday);
    assert_eq!(store.list(None, 10).expect("list").len(), 1, "the first alert");

    // A different D-Bus path, because ModemManager mints a new object; same
    // sender, same text, no timestamp — so the same content digest.
    let friday = FakeModem::holding(vec![alert("/SMS/7")]);
    super::super::poll::ingest(&store, &friday);
    assert_eq!(store.list(None, 10).expect("list").len(), 2, "the second alert is not swallowed");
}

/// While the modem's copy is still pending, a re-read is the SAME message even
/// if `ModemManager` renumbered it — the handle is refreshed, not treated as new.
#[test]
fn a_renumbered_pending_message_is_still_the_same_message() {
    let (_dir, store) = store();
    let text = |handle: &str| incoming(handle, "Bouygues", "Votre conso atteint 80%", None);
    let before = FakeModem::refusing(vec![text("/SMS/0")]);
    super::super::poll::ingest(&store, &before);
    // `ModemManager` restarted; the same undeleted message now answers to /SMS/0
    // under a fresh numbering, then finally deletes.
    let after = FakeModem::holding(vec![text("/SMS/3")]);
    super::super::poll::ingest(&store, &after);
    assert_eq!(store.list(None, 10).expect("list").len(), 1, "still one message");
    assert_eq!(after.deleted.borrow().as_slice(), ["/SMS/3"], "deleted under its new handle");
}

/// A hidden message still counts against the send ceiling.
///
/// Otherwise the ceiling is decorative: the same `can_manage_it` caller who may
/// send may also call DELETE, so a hard delete would let an operator send ten,
/// remove the rows, and send ten more, indefinitely — on the vendor's plan, and
/// erasing the `sent_by` audit trail on the way out.
#[test]
fn hiding_a_sent_message_does_not_refund_the_quota() {
    let (_dir, store) = store();
    let id = store.record_outgoing("+33612345678", "Coucou", Some("user-7")).expect("record");
    let window = super::super::db::now_s().saturating_sub(3_600);
    assert_eq!(store.sent_since(window, Some("user-7")).expect("count"), 1);
    assert!(store.delete(id).expect("delete"), "hidden");
    assert!(store.list(None, 10).expect("list").is_empty(), "gone from the listing");
    assert_eq!(store.sent_since(window, Some("user-7")).expect("count"), 1, "still counted");
}

/// A hidden message stops counting as unread.
#[test]
fn hiding_clears_the_unread_badge() {
    let (_dir, store) = store();
    let _row = store
        .insert_incoming(&Inbound { handle: "/h1", peer: "+33612345678", body: "Bonjour", sent_at: Some(1) })
        .expect("insert");
    let id = only(&store.list(None, 1).expect("list")).id;
    assert_eq!(store.unread_count().expect("count"), 1);
    assert!(store.delete(id).expect("delete"), "hidden");
    assert_eq!(store.unread_count().expect("count"), 0, "hidden messages are not unread");
}
