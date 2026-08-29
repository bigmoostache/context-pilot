//! The modem side — `ModemManager`, driven through `mmcli -J`.
//!
//! # Why `mmcli` and not the AT port
//!
//! MEASURED on the box (Armbian, Photonicat 2, `mmcli 1.24.0`, Quectel
//! RM520N-GL in **MBIM** composition): `mmcli -m 0 --messaging-status` answers
//! `supported storages: mt`, and a create → list → read → delete round trip
//! carries accents, an emoji and a newline back unchanged. So messaging works
//! over MBIM, and the AT/PDU path is not needed.
//!
//! That matters because the alternative is expensive. `ModemManager` owns
//! `/dev/ttyUSB2`, so an AT implementation would have to take the port away from
//! it (a udev `ID_MM_PORT_IGNORE` rule) and then re-implement GSM-7/UCS2
//! decoding and multipart reassembly — the ~700 lines photonicat's own
//! `pysms_tool.py` carries. `mmcli` hands us decoded UTF-8 with the segments
//! already recombined.
//!
//! # The gate
//!
//! `CP_MMCLI_BIN`, on the same terms as every other tool in this feature's
//! neighbourhood: **unset, or naming a path that does not exist, means the whole
//! module is inert** — no subprocess, no error, the feature simply reports
//! itself unavailable. That is what lets `cargo test` run on a laptop, and what
//! makes "no `ModemManager` here" and "not configured for messaging" the same
//! honest state (the lesson `nmcli_bin` already learned).

use std::ffi::OsString;
use std::path::Path;

use serde_json::Value;

use super::super::apply::run;

/// `mmcli`'s path, **only if that path exists** — see the module doc.
fn mmcli_bin() -> Option<OsString> {
    let bin = std::env::var_os("CP_MMCLI_BIN")?;
    Path::new(&bin).exists().then_some(bin)
}

/// `mmcli`'s spelling of "this field has no value". Anything carrying it is
/// absent, not empty — MEASURED on a created message, whose every unset
/// property reads `"--"`.
const ABSENT: &str = "--";

/// Read a JSON string field, mapping `mmcli`'s `"--"` sentinel to `None`.
fn field(value: &Value, path: &[&str]) -> Option<String> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(key)?;
    }
    let text = cursor.as_str()?;
    (text != ABSENT).then(|| text.to_owned())
}

/// One message sitting in the modem's storage, as the ingester needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Incoming {
    /// The D-Bus path — the handle to read and then delete it by.
    pub handle: String,
    /// The sender. May be alphanumeric (a short-code brand name), so it is
    /// never validated as a dialling number on the way in.
    pub peer: String,
    /// The text, decoded to UTF-8 and de-segmented by `ModemManager`.
    pub body: String,
    /// The network's timestamp in epoch seconds, when it gave one.
    pub sent_at: Option<i64>,
}

/// A detected modem: the tool and the D-Bus path to drive it with.
pub(crate) struct Modem {
    /// `mmcli`'s absolute path.
    bin: OsString,
    /// The modem's D-Bus path, e.g. `/org/freedesktop/ModemManager1/Modem/0`.
    path: String,
}

impl Modem {
    /// Find the first modem `ModemManager` knows about, or `None` when the tool
    /// is absent or no modem is assembled.
    ///
    /// The path is **discovered**, never assumed to be index `0`:
    /// `ModemManager` increments the index across re-enumerations, which is
    /// exactly how the signal read-out silently broke after a modem reset until
    /// `signal_dbm` was fixed to do this.
    pub(crate) fn detect() -> Option<Self> {
        let bin = mmcli_bin()?;
        let listed = run(&bin, &["-J".to_owned(), "-L".to_owned()]).ok()?;
        let parsed: Value = serde_json::from_str(&listed).ok()?;
        let path = parsed.get("modem-list")?.as_array()?.first()?.as_str()?.to_owned();
        Some(Self { bin, path })
    }

    /// Every message currently in the modem's storage that the **network
    /// delivered**.
    ///
    /// Outbound messages (`pdu-type: submit`) are skipped: this box created
    /// them, they are already archived, and re-ingesting them would file our own
    /// sent messages as if they had arrived. Photonicat's `linuxFallback.go`
    /// draws the same line.
    ///
    /// A message that cannot be read individually is skipped rather than
    /// failing the sweep — one unreadable entry must not stop the others from
    /// being archived and freeing their slot.
    ///
    /// # Errors
    ///
    /// Returns `mmcli`'s stderr when the list itself cannot be obtained.
    pub(crate) fn list_incoming(&self) -> Result<Vec<Incoming>, String> {
        let listed =
            run(&self.bin, &["-J".to_owned(), "-m".to_owned(), self.path.clone(), "--messaging-list-sms".to_owned()])?;
        let parsed: Value = serde_json::from_str(&listed).map_err(|e| format!("mmcli sms list: {e}"))?;
        let handles = parsed
            .get("modem.messaging.sms")
            .and_then(Value::as_array)
            .ok_or_else(|| "mmcli sms list: no modem.messaging.sms array".to_owned())?;
        Ok(handles.iter().filter_map(Value::as_str).filter_map(|handle| self.read(handle)).collect())
    }

    /// Read one message, or `None` when it is unreadable or not a delivery.
    fn read(&self, handle: &str) -> Option<Incoming> {
        let shown = run(&self.bin, &["-J".to_owned(), "-s".to_owned(), handle.to_owned()]).ok()?;
        let parsed: Value = serde_json::from_str(&shown).ok()?;
        parse_incoming(handle, &parsed)
    }

    /// Remove a message from the modem's storage, freeing its slot.
    ///
    /// # Errors
    ///
    /// Returns `mmcli`'s stderr.
    pub(crate) fn delete(&self, handle: &str) -> Result<(), String> {
        let args = ["-m".to_owned(), self.path.clone(), format!("--messaging-delete-sms={handle}")];
        run(&self.bin, &args).map(|_out| ())
    }

    /// Create and send one message.
    ///
    /// **The body goes through a file, never through the option string.**
    /// `--messaging-create-sms="text='…'"` has no escaping story: an apostrophe,
    /// a comma or a newline in the text breaks the parse, and the failure is
    /// silent truncation rather than an error. `--messaging-create-sms-with-text`
    /// takes a path, which sidesteps the whole problem — VERIFIED on the box
    /// with accents, an emoji and an embedded newline.
    ///
    /// # Errors
    ///
    /// Returns `mmcli`'s stderr from whichever step failed. The created message
    /// is cleaned up when the send fails, so a rejected send does not leave a
    /// draft occupying a storage slot.
    pub(crate) fn send(&self, to: &str, body: &str) -> Result<(), String> {
        let scratch = BodyFile::write(body)?;
        let created = run(
            &self.bin,
            &[
                "-m".to_owned(),
                self.path.clone(),
                format!("--messaging-create-sms=number={to}"),
                format!("--messaging-create-sms-with-text={}", scratch.path.display()),
            ],
        )?;
        let handle = created
            .split_whitespace()
            .find(|token| token.starts_with("/org/freedesktop/ModemManager1/SMS/"))
            .ok_or_else(|| format!("mmcli did not name the created message: {created}"))?
            .to_owned();
        match run(&self.bin, &["-s".to_owned(), handle.clone(), "--send".to_owned()]) {
            Ok(_out) => Ok(()),
            Err(failure) => {
                // Best-effort: the send already failed, and a failure to clean
                // up must not replace the reason with a less useful one.
                drop(self.delete(&handle));
                Err(failure)
            }
        }
    }
}

/// The pure half of [`Modem::read`]: one `mmcli -J -s <path>` document to an
/// [`Incoming`], or `None` when it is not a delivery.
///
/// Split from the subprocess so the shape `mmcli` actually emits can be tested
/// against captured fixtures, with no modem and no `ModemManager` anywhere.
///
/// Outbound messages are rejected here rather than filtered by the caller: a
/// `submit` is one this box created, it is already archived, and re-ingesting it
/// would file our own sent message as if it had arrived.
pub(super) fn parse_incoming(handle: &str, document: &Value) -> Option<Incoming> {
    let sms = document.get("sms")?;
    if field(sms, &["properties", "pdu-type"]).as_deref() != Some("deliver") {
        return None;
    }
    Some(Incoming {
        handle: handle.to_owned(),
        peer: field(sms, &["content", "number"]).unwrap_or_default(),
        // An empty body is a real message (a delivery report, a ping); only an
        // absent one is skipped, hence `unwrap_or_default` here too.
        body: field(sms, &["content", "text"]).unwrap_or_default(),
        sent_at: field(sms, &["properties", "timestamp"]).as_deref().and_then(epoch_seconds),
    })
}

/// A message body on disk, deleted when it goes out of scope.
///
/// It holds the text of someone's SMS, so it is created `0600` and removed on
/// every path — including the error paths, which is why this is a guard type
/// rather than two calls around the `mmcli` invocation.
struct BodyFile {
    /// Where it lives.
    path: std::path::PathBuf,
}

impl BodyFile {
    /// Write `body` to a fresh private file.
    ///
    /// # Errors
    ///
    /// Returns the I/O message when the file cannot be created or written.
    fn write(body: &str) -> Result<Self, String> {
        let unique = format!(
            "cp-sms-{}-{}.txt",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |dur| dur.as_nanos())
        );
        let path = std::env::temp_dir().join(unique);
        let mut options = std::fs::OpenOptions::new();
        let _mode = options.write(true).create_new(true);
        private(&mut options);
        let mut file = options.open(&path).map_err(|e| format!("create {}: {e}", path.display()))?;
        std::io::Write::write_all(&mut file, body.as_bytes()).map_err(|e| format!("write {}: {e}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for BodyFile {
    /// Remove the scratch file. A failure here is not actionable — the file is
    /// in the temp dir and carries no further consequence — so it is dropped.
    fn drop(&mut self) {
        drop(std::fs::remove_file(&self.path));
    }
}

/// Create the scratch file `0600`, so the body is never world-readable even for
/// the moment it exists.
#[cfg(unix)]
fn private(options: &mut std::fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt as _;
    let _mode = options.mode(0o600);
}

/// No-op on non-Unix (local dev on a platform without POSIX modes).
#[cfg(not(unix))]
fn private(_options: &mut std::fs::OpenOptions) {}

use std::time::{SystemTime, UNIX_EPOCH};

/// RFC 3339 (what `ModemManager` emits, offset included) to epoch **seconds**.
///
/// `None` for anything unparseable, which the archive stores as "no network
/// timestamp" and orders by ingestion instead. A wrong time is worse than none:
/// a modem whose clock never got set would otherwise file today's message under
/// 1970 and bury it at the bottom of the list.
fn epoch_seconds(raw: &str) -> Option<i64> {
    const MILLIS_PER_SECOND: i64 = 1000;
    let millis = cp_mod_utilities::time::parse_rfc3339_to_epoch_ms(raw)?;
    millis.checked_div(MILLIS_PER_SECOND)
}

/// Whether messaging is reachable at all on this box — the `CP_MMCLI_BIN` gate,
/// answered without spawning anything.
///
/// Cheap on purpose: it is read on every `GET /api/it/network`, which the
/// cockpit polls every five seconds.
pub(crate) fn available() -> bool {
    mmcli_bin().is_some()
}
