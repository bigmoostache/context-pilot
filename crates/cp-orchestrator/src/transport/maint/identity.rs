//! Box **identity** (DNS name + IP) and the `POST /api/maint/identity` handler.
//!
//! The IT operator names the box on the maintenance plane; that name + IP are
//! persisted durably and become the SANs of the private-CA leaf Caddy presents
//! (see [`super::caddy`]). Setting the identity re-renders the Caddyfile and
//! reloads Caddy, so the leaf is re-issued for the chosen name (Obj 3.2).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::Backend;
use super::HttpReply;
use super::state::write_atomic;
use crate::services::auth::types::User;

/// The box's operator-chosen identity — the cert subjects for the private leaf.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Identity {
    /// DNS name the operator gives the box (e.g. `pilot.acme.corp`). May be
    /// empty if the operator only pins an IP.
    pub(crate) name: String,
    /// The box's LAN IP (e.g. `192.168.1.116`).
    pub(crate) ip: String,
}

/// On-disk location of the identity record, beside the provisioned flag in the
/// agents dir (under `/opt/context-pilot` on the box).
pub(crate) fn identity_path(agents_dir: &Path) -> PathBuf {
    agents_dir.join(".identity.json")
}

/// Load the persisted identity, or `None` if unset / unreadable / malformed /
/// **invalid**. The on-disk record is re-validated (not just parsed) before use:
/// it is fed straight into Caddyfile generation, so a tampered file with a junk
/// name/IP must never reach the renderer (defence-in-depth — tampering already
/// needs root, but we still fail closed).
pub(crate) fn load_identity(path: &Path) -> Option<Identity> {
    let raw = std::fs::read(path).ok()?;
    let identity: Identity = serde_json::from_slice(&raw).ok()?;
    if !validate_ip(&identity.ip) {
        return None;
    }
    if !identity.name.is_empty() && !validate_name(&identity.name) {
        return None;
    }
    Some(identity)
}

/// Persist the identity atomically + durably (survives reboot).
fn save_identity(path: &Path, identity: &Identity) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(identity).unwrap_or_default();
    write_atomic(path, &json)
}

/// Validate a DNS hostname per the usual label rules: 1–253 chars total, each
/// dot-separated label 1–63 chars of `[A-Za-z0-9-]`, not starting/ending with a
/// hyphen, and at least one alphabetic character somewhere (so a bare number
/// isn't mistaken for a hostname — those go in the IP field).
#[must_use]
pub(crate) fn validate_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 253 {
        return false;
    }
    if !name.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    name.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// Validate that `ip` parses as an IPv4 or IPv6 address.
#[must_use]
pub(crate) fn validate_ip(ip: &str) -> bool {
    ip.parse::<std::net::IpAddr>().is_ok()
}

/// `POST /api/maint/finalize` (Admin) — flip the box to *provisioned* and start
/// serving the cockpit.
///
/// Pre-requisites (Obj 5.4): the seeded paper password must have been changed
/// (`must_change_password == false`) **and** the box identity (name/IP) must be
/// set — the leaf needs subjects before the cockpit can be trusted. On success it
/// persists the durable flag, then re-renders the Caddyfile in *provisioned* mode
/// and reloads Caddy so `:443` begins serving the cockpit (Obj 2.2.2). Idempotent.
pub(crate) fn finalize(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    // The Admin gate guarantees `auth_user` is `Some(Admin)` here.
    let Some(caller) = auth_user else {
        return HttpReply::error(401, "admin authorization required");
    };
    // Pre-requisite: the seeded paper password must have been changed.
    if caller.must_change_password {
        return HttpReply::error(412, "change the admin password before finalizing");
    }

    let (flag_path, identity) = match state.lock() {
        Ok(b) => (b.provision_flag_path.clone(), load_identity(&identity_path(&b.agents_dir))),
        Err(_) => return HttpReply::error(500, "backend lock poisoned"),
    };

    // Pre-requisite: the box must be named (a leaf needs subjects).
    let Some(identity) = identity else {
        return HttpReply::error(412, "set the box name/IP before finalizing");
    };

    if let Err(e) = super::state::set_provisioned(&flag_path, true) {
        eprintln!("finalize: could not persist provisioned flag: {e}");
        return HttpReply::error(500, "could not persist provisioned flag");
    }

    // Re-render Caddy in provisioned mode → :443 starts serving the cockpit.
    match super::caddy::regenerate(true, Some(&identity)) {
        Ok(reloaded) => HttpReply::ok(&serde_json::json!({ "provisioned": true, "reloaded": reloaded })),
        Err(e) => {
            eprintln!("finalize: caddy reload failed: {e}");
            // The box is provisioned (flag persisted); a retry/identity change
            // will re-apply the cockpit config.
            HttpReply::error(502, "provisioned but the TLS reload failed")
        }
    }
}

/// `GET /api/maint/identity` (Admin) — the current box identity, or `null` when
/// unset. Lets the wizard prefill the name/IP form on a re-visited box.
pub(crate) fn get_identity(state: &Mutex<Backend>) -> HttpReply {
    let identity = match state.lock() {
        Ok(b) => load_identity(&identity_path(&b.agents_dir)),
        Err(_) => return HttpReply::error(500, "backend lock poisoned"),
    };
    HttpReply::ok(&serde_json::json!({ "identity": identity }))
}

/// Render and write the Caddyfile for the current persisted state at boot (M3).
///
/// This **writes only, never reloads**: at boot Caddy's admin API isn't up yet
/// (the orchestrator starts before Caddy), and Caddy loads the freshly-written
/// file on its own start a moment later. Reloading here would always "fail"
/// (admin unreachable) and wrongly trigger a rollback that could revert a
/// legitimately-new render (e.g. a DHCP-changed IP). A no-op when Caddy isn't
/// configured (`CP_CADDYFILE` unset — local dev). Never fails startup.
pub(crate) fn apply_caddy_at_boot(state: &Mutex<Backend>) {
    let (provisioned, identity) = match state.lock() {
        Ok(b) => (super::is_provisioned(&b.provision_flag_path), load_identity(&identity_path(&b.agents_dir))),
        Err(_) => return,
    };
    match super::caddy::write_config(provisioned, identity.as_ref()) {
        Ok(true) => eprintln!("caddy: config written at boot (provisioned={provisioned})"),
        Ok(false) => {} // Caddy not configured in this environment — skipped.
        Err(e) => eprintln!("WARN: caddy boot config write failed: {e}"),
    }
}

/// `POST /api/maint/identity` (Admin) — set the box name + IP.
///
/// Body: `{ "name": "...", "ip": "..." }` (`name` may be empty to pin only an
/// IP). Validates, persists durably, then re-renders the Caddyfile and reloads
/// Caddy so the leaf is re-issued for the new subjects. A reload failure rolls
/// the Caddyfile back and is reported as a `502`, but the identity is still
/// persisted (the next finalize/identity will retry the reload).
pub(crate) fn set_identity(state: &Mutex<Backend>, body: &[u8]) -> HttpReply {
    #[derive(Deserialize)]
    struct Req {
        name: String,
        ip: String,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"name\":\"...\",\"ip\":\"...\"}");
    };
    let name = req.name.trim().to_owned();
    let ip = req.ip.trim().to_owned();

    if !validate_ip(&ip) {
        return HttpReply::error(400, "ip is not a valid IPv4/IPv6 address");
    }
    if !name.is_empty() && !validate_name(&name) {
        return HttpReply::error(400, "name is not a valid DNS hostname");
    }

    let identity = Identity { name, ip };
    let (path, provisioned) = match state.lock() {
        Ok(b) => (identity_path(&b.agents_dir), super::is_provisioned(&b.provision_flag_path)),
        Err(_) => return HttpReply::error(500, "backend lock poisoned"),
    };

    if let Err(e) = save_identity(&path, &identity) {
        eprintln!("identity: could not persist: {e}");
        return HttpReply::error(500, "could not persist identity");
    }

    // Re-render + reload Caddy so the leaf is re-issued for the new subjects.
    match super::caddy::regenerate(provisioned, Some(&identity)) {
        Ok(_reloaded) => HttpReply::ok(&serde_json::json!({ "identity": identity, "reloaded": _reloaded })),
        Err(e) => {
            eprintln!("identity: caddy reload failed: {e}");
            HttpReply::error(502, "identity saved but the TLS reload failed")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_validation_accepts_hostnames_rejects_junk() {
        assert!(validate_name("pilot.acme.corp"));
        assert!(validate_name("box1"));
        assert!(validate_name("a-b.example"));
        // Rejections.
        assert!(!validate_name(""));
        assert!(!validate_name("-bad.example"), "label starts with hyphen");
        assert!(!validate_name("bad-.example"), "label ends with hyphen");
        assert!(!validate_name("a..b"), "empty label");
        assert!(!validate_name("192.168.1.1"), "a bare IP is not a name");
        assert!(!validate_name("under_score.example"), "underscore not allowed");
        assert!(!validate_name(&"x".repeat(254)), "too long");
    }

    #[test]
    fn ip_validation_accepts_v4_and_v6() {
        assert!(validate_ip("192.168.1.116"));
        assert!(validate_ip("10.0.0.1"));
        assert!(validate_ip("::1"));
        assert!(validate_ip("fd00::1"));
        assert!(!validate_ip("not.an.ip"));
        assert!(!validate_ip("999.1.1.1"));
        assert!(!validate_ip(""));
    }

    #[test]
    fn identity_persists_and_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = identity_path(dir.path());
        assert!(load_identity(&path).is_none(), "no identity initially");

        let id = Identity { name: "pilot.acme.corp".to_owned(), ip: "192.168.1.116".to_owned() };
        save_identity(&path, &id).expect("save");
        assert_eq!(load_identity(&path).as_ref(), Some(&id), "identity round-trips");
    }
}
