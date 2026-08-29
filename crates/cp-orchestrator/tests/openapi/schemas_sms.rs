//! `OpenAPI` schemas for the SMS surface.
//!
//! Split from [`schemas_net`](super::schemas_net) so both stay inside the
//! 500-line file cap; merged alongside it in the spec builder.

use serde_json::{Value, json};

use super::{arr, r};

/// The SMS schemas: the status block folded into `GET /api/it/network`, one
/// archived message, the listing, and the send result.
///
/// Split in two halves purely for the 60-line function cap.
pub(super) fn sms() -> Value {
    super::merge(status_schema(), message_schemas())
}

/// The `status.sms` block.
fn status_schema() -> Value {
    json!({
        "ItSmsStatus": {
            "type": "object",
            "description": "Whether this box can do SMS at all, plus the unread badge. NULL \u{2014} not an object \
                with `available: false` \u{2014} for either of two reasons: the box is not a 5G variant \
                (`status.modem_present` false), or no `mmcli` is configured. The cockpit renders the SMS panel \
                only when this is an object, so a box with no modem does not show a disabled panel, it shows \
                none. Deliberately carries NO storage counts: `mmcli --messaging-status` reports which storages \
                the modem supports (`mt` on the RM520N-GL) and not how full they are, so a used/total pair \
                would have to be invented.",
            "properties": {
                "available": { "type": "boolean" },
                "unread": {
                    "type": "integer",
                    "description": "Inbound messages nobody has opened yet. Counts the ARCHIVE, not the modem \u{2014} \
                        the ingester empties the modem on every sweep, which is what keeps its handful of \
                        storage slots from filling and silently dropping the next message."
                }
            },
            "required": ["available", "unread"]
        }
    })
}

/// One archived message, the listing envelope, and the send result.
fn message_schemas() -> Value {
    json!({
        "ItSmsMessage": {
            "type": "object",
            "description": "One archived message. Inbound messages are ingested from the modem by a background \
                sweep and then deleted from it; outbound ones are written here BEFORE the modem is asked to \
                send, so a send that dies mid-flight still leaves a record of what was attempted and by whom.",
            "properties": {
                "id": { "type": "integer" },
                "direction": {
                    "type": "string",
                    "enum": ["received", "sent"],
                    "description": "`received` is `pdu-type: deliver` from the network; `sent` is one this box submitted."
                },
                "peer": {
                    "type": "string",
                    "description": "The other end. NOT always a dialling number: carriers send alerts from \
                        alphanumeric short names (`Bouygues`), so this is never validated as E.164 on the way in \u{2014} \
                        only on the way out, where we are the sender."
                },
                "body": { "type": "string", "description": "UTF-8, decoded and de-segmented by ModemManager." },
                "sent_at": {
                    "type": "integer",
                    "nullable": true,
                    "description": "The NETWORK's timestamp, epoch seconds. Null when the modem reported none \
                        (`mmcli` spells that `--`) or reported something unparseable. A wrong time is worse than \
                        none, so listings order by `ingested_at`, which this box can vouch for."
                },
                "ingested_at": { "type": "integer", "description": "When this box first saw it. Never null; the sort key." },
                "delivery": {
                    "type": "string",
                    "enum": ["received", "sending", "sent", "failed"],
                    "description": "`received` is terminal for inbound. Outbound walks `sending` \u{2192} `sent` | `failed`."
                },
                "read": { "type": "boolean" },
                "sent_by": {
                    "type": "string",
                    "nullable": true,
                    "description": "The user id that ordered an outbound message \u{2014} the audit trail, since sending \
                        spends the vendor's data plan. Null for inbound."
                },
                "error": { "type": "string", "nullable": true, "description": "The modem's own words when a send failed." }
            },
            "required": ["id", "direction", "peer", "body", "sent_at", "ingested_at", "delivery", "read", "sent_by", "error"]
        },
        "ItSmsList": {
            "type": "object",
            "properties": { "messages": arr(r("ItSmsMessage")) },
            "required": ["messages"]
        },
        "ItSmsSendResult": {
            "type": "object",
            "properties": {
                "id": { "type": "integer", "description": "The archived row, written before the modem was touched." },
                "delivery": { "type": "string", "enum": ["sent"] }
            },
            "required": ["id", "delivery"]
        }
    })
}

/// Body of `POST /api/it/sms`.
pub(super) fn send_body() -> Value {
    json!({
        "type": "object",
        "properties": {
            "to": {
                "type": "string",
                "description": "Destination in E.164: 6\u{2013}15 digits, optionally prefixed with `+`. Rejected with a \
                    400 otherwise \u{2014} unlike an inbound `peer`, which may be an alphanumeric short name."
            },
            "body": {
                "type": "string",
                "description": "1 to 670 characters. The ceiling is ten UCS-2 segments (67 characters each once the \
                    concatenation header is subtracted), which is where carriers stop being reliable about \
                    reassembly. The text is handed to `mmcli` through a FILE, never interpolated into an option \
                    string, so apostrophes, commas and newlines survive."
            }
        },
        "required": ["to", "body"]
    })
}
