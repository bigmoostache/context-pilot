//! The background ingester — one thread for the life of the process.
//!
//! # Why this is a thread and not a fetch when the panel opens
//!
//! The modem's storage is small (`mt`, a few dozen slots on the RM520N-GL) and
//! **overflows in silence**: once it is full the network's next message is
//! dropped with no error anywhere. A box whose cockpit nobody opens for a week
//! would therefore lose messages, and nothing would say so. Ingesting on a
//! timer is what makes the feature a service rather than a viewer.
//!
//! # The order of the two writes
//!
//! Archive first, delete from the modem second — never the reverse. A crash
//! between them costs nothing: the message is still in the modem, the next
//! sweep re-reads it, and [`SmsStore::insert_incoming`] recognises it by digest
//! and does nothing. The reverse order loses the message outright.
//!
//! That is also why a message already in the archive is **still** deleted from
//! the modem: seeing it again means the previous delete did not take, and
//! retrying is the whole repair. Photonicat's own ingester does the same.
//!
//! # What this thread will not do
//!
//! It never enables the modem, changes the uplink mode, or touches
//! `ModemManager`'s state. Whether the modem is registered is a fact about the
//! world; the cockpit reports it. Repairing connectivity is `apply`'s job, and
//! two writers of network configuration is the failure this module exists
//! beside, not one it joins.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::db::{Inbound, SmsStore, sms_path};
use super::modem::{MessageSource, Modem};
use super::{Backend, RETENTION_ROWS, enabled, retention_days};

/// Settle delay before the first sweep — lets the transport bind and
/// `ModemManager` finish probing after a boot, so the first tick is not a
/// guaranteed miss.
const BOOT_DELAY: Duration = Duration::from_secs(20);

/// Seconds between sweeps. `CP_SMS_POLL_S`, default 30 — the same cadence
/// photonicat's own ingester runs at.
fn interval() -> Duration {
    let seconds = super::env_number("CP_SMS_POLL_S", 30);
    Duration::from_secs(u64::try_from(seconds).unwrap_or(30))
}

/// Spawn the ingester. One thread for the process lifetime.
///
/// Returns a handle that is never joined — like the driver and the update
/// scheduler, the loop ends when the process does.
pub(crate) fn spawn(backend: Arc<Mutex<Backend>>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        run_loop(&backend);
    })
}

/// Settle, then sweep forever. Split into a `-> !` helper so the divergence is
/// explicit while the spawned closure still yields `()`.
fn run_loop(backend: &Arc<Mutex<Backend>>) -> ! {
    thread::sleep(BOOT_DELAY);
    loop {
        tick(backend);
        thread::sleep(interval());
    }
}

/// One sweep. Every failure is logged and swallowed: this thread must outlive a
/// modem that is busy, a database that is momentarily locked, and a
/// `ModemManager` restart.
fn tick(backend: &Arc<Mutex<Backend>>) {
    // `modem::available()` is part of the gate, not just an optimisation:
    // `modem_present()` answers TRUE wherever the applier is inert (a laptop, CI,
    // any box with no NetworkManager), so without this a dev machine would
    // create and chmod an sms.db every 30 s for a modem that does not exist.
    if !enabled() || !super::modem::available() || !super::super::apply::modem_present() {
        return;
    }
    let Some(dir) = backend.lock().ok().map(|guard| guard.agents_dir.clone()) else {
        return;
    };
    let store = match SmsStore::open(&sms_path(&dir)) {
        Ok(store) => store,
        Err(failure) => {
            crate::oerr!("sms: cannot open the archive ({failure}) — messages stay on the modem");
            return;
        }
    };
    if let Some(radio) = Modem::detect() {
        ingest(&store, &radio);
    }
    prune(&store);
}

/// Move everything the network delivered into the archive, then free its slot.
pub(super) fn ingest(store: &SmsStore, radio: &dyn MessageSource) {
    let waiting = match radio.list_incoming() {
        Ok(waiting) => waiting,
        Err(failure) => {
            crate::oerr!("sms: cannot list the modem's messages ({failure})");
            return;
        }
    };
    for message in waiting {
        let inbound =
            Inbound { handle: &message.handle, peer: &message.peer, body: &message.body, sent_at: message.sent_at };
        match store.insert_incoming(&inbound) {
            // Archived now, or archived by an earlier sweep whose delete did not
            // take — either way the modem's copy is redundant and its slot is
            // wanted back.
            Ok(id) => reclaim(store, radio, &message.handle, id),
            Err(failure) => {
                // Deliberately NOT deleted: the archive does not have it, so the
                // modem is the only copy left. Better a full storage than a lost
                // message — the next sweep retries.
                crate::oerr!("sms: cannot archive {} ({failure}) — left on the modem", message.handle);
            }
        }
    }
}

/// Delete the modem's copy and, only if that succeeded, release the row.
///
/// The order is the whole point. `forget_modem_copy` is what lets a LATER
/// identical message be archived as its own message; calling it before the
/// modem's copy is actually gone would make the next sweep re-archive this one
/// as a duplicate. So a failed delete leaves the row pending, which is exactly
/// the state that makes the retry idempotent.
fn reclaim(store: &SmsStore, radio: &dyn MessageSource, handle: &str, id: i64) {
    if let Err(failure) = radio.delete(handle) {
        crate::oerr!("sms: archived {handle} but could not delete it from the modem ({failure}) — will retry");
        return;
    }
    if let Err(failure) = store.forget_modem_copy(id) {
        crate::oerr!("sms: deleted {handle} from the modem but could not release row {id} ({failure})");
    }
}

/// Apply the retention policy, logging what it removed when it removed anything.
fn prune(store: &SmsStore) {
    match store.prune(super::db::now_s(), retention_days(), RETENTION_ROWS) {
        Ok(0) => {}
        Ok(removed) => crate::oerr!("sms: retention removed {removed} message(s)"),
        Err(failure) => crate::oerr!("sms: retention pass failed ({failure})"),
    }
}
