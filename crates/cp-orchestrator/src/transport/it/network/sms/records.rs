//! The value types the archive stores and hands back.
//!
//! Split from [`db`](super::db) so both stay inside the 500-line file cap: this
//! half is what a message *is*, that half is how it is persisted and queried.

use std::time::{SystemTime, UNIX_EPOCH};

/// Which way a message travelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    /// Delivered to this box by the network (`pdu-type: deliver`).
    Received,
    /// Submitted by an operator through the cockpit (`pdu-type: submit`).
    Sent,
}

impl Direction {
    /// The column value. Stored as an integer because it is a closed two-valued
    /// fact, and because the read paths filter on it.
    pub(super) const fn as_i64(self) -> i64 {
        match self {
            Self::Received => 0,
            Self::Sent => 1,
        }
    }

    /// The inverse of [`Self::as_i64`]; anything unexpected reads as
    /// `Received`, which is the direction that cannot be confused with an
    /// action this box took.
    pub(super) const fn from_i64(raw: i64) -> Self {
        if raw == 1 { Self::Sent } else { Self::Received }
    }
}

/// Where a message is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// Ingested from the modem. Terminal for an inbound message.
    Received,
    /// Handed to the modem, no confirmation yet.
    Sending,
    /// The modem accepted it for delivery.
    Sent,
    /// The modem refused it, or the send never completed.
    Failed,
}

impl Delivery {
    /// The column value — a string, so the table stays readable to a human
    /// holding `sqlite3` during an incident.
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Failed => "failed",
        }
    }

    /// The inverse of [`Self::as_str`]. An unknown spelling reads as
    /// [`Failed`](Self::Failed): a row we cannot interpret must not be
    /// presented as delivered.
    pub(super) fn from_str(raw: &str) -> Self {
        match raw {
            "received" => Self::Received,
            "sending" => Self::Sending,
            "sent" => Self::Sent,
            _unknown => Self::Failed,
        }
    }
}

/// One inbound message on its way into the archive.
///
/// A struct rather than four parameters: the workspace caps functions at four
/// arguments, and three consecutive `&str` are exactly the shape a caller
/// transposes silently.
pub(crate) struct Inbound<'msg> {
    /// The modem's D-Bus path — what makes the archived row *pending*.
    pub handle: &'msg str,
    /// The sender, as the network gave it.
    pub peer: &'msg str,
    /// The decoded text.
    pub body: &'msg str,
    /// The network's timestamp in epoch seconds, when it gave one.
    pub sent_at: Option<i64>,
}

/// One archived message, as the read paths hand it out.
#[derive(Clone, Debug)]
pub(crate) struct Message {
    /// Row id — the handle the cockpit uses to mark read or delete.
    pub id: i64,
    /// Which way it travelled.
    pub direction: Direction,
    /// The other end, in E.164 where the network gave us one. Senders may also
    /// be alphanumeric short names, which no dialling plan validates.
    pub peer: String,
    /// The text, already decoded to UTF-8 by `ModemManager`.
    pub body: String,
    /// The network's own timestamp, epoch seconds — `None` when the modem
    /// reported none (`mmcli` spells that `"--"`).
    pub sent_at: Option<i64>,
    /// When this box first saw it. Never null, so the list has a stable order
    /// even for messages the network did not timestamp.
    pub ingested_at: i64,
    /// Where it is in its life.
    pub delivery: Delivery,
    /// When an operator marked it read, or `None` while unread.
    pub read_at: Option<i64>,
    /// The user id that sent it — the audit trail for outbound messages, which
    /// cost money on the vendor's data plan. `None` for inbound.
    pub sent_by: Option<String>,
    /// Why a send failed, verbatim from the modem.
    pub error: Option<String>,
}

/// Epoch seconds, saturating rather than panicking on a pre-1970 clock.
pub(crate) fn now_s() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |dur| i64::try_from(dur.as_secs()).unwrap_or(i64::MAX))
}
