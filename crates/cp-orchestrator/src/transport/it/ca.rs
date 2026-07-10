//! Private-CA root distribution (M4, "chemin A").
//!
//! The appliance's TLS is signed by Caddy's internal CA, which clients don't
//! trust by default. So the operator downloads the CA **root** from the cockpit's
//! IT settings (`can_manage_it`) and pushes it to clients (GPO/MDM), verifying its SHA-256
//! fingerprint out-of-band against what the console shows. Two routes:
//!
//! * `GET /api/it/ca.crt` — the root PEM (Admin), served as a file download.
//! * `GET /api/it/ca/fingerprint` — its SHA-256 fingerprint (Admin), matching
//!   `openssl x509 -in root.crt -noout -fingerprint -sha256`.
//!
//! The root lives in Caddy's data dir (`…/pki/authorities/local/root.crt`); its
//! path is given by `CP_CA_ROOT` (set in the systemd unit). Until Caddy has
//! generated it (first TLS handshake), both routes report `404`.

use std::path::PathBuf;

use tiny_http::{Header, Request, Response};

use super::HttpReply;
use super::crypto::{base64_decode, colon_hex_upper, sha256};

/// Filesystem path of the CA root, from `CP_CA_ROOT`. `None` when unconfigured
/// (local dev) — the routes then report `404`, never a stack trace.
fn root_path() -> Option<PathBuf> {
    std::env::var_os("CP_CA_ROOT").map(PathBuf::from)
}

/// `GET /api/it/ca.crt` (Admin) — serve the CA root PEM as a download.
///
/// Owns the [`Request`] so it can set a non-JSON content type. Reports `404`
/// (JSON) when the root isn't configured or hasn't been generated yet.
pub(crate) fn serve_ca_cert(request: Request) {
    let pem = root_path().and_then(|p| std::fs::read(p).ok());
    let Some(pem) = pem else {
        crate::transport::respond_json(request, &HttpReply::error(404, "CA root not available yet"));
        return;
    };
    let mut response = Response::from_data(pem).with_status_code(200);
    for (name, value) in
        [("Content-Type", "application/x-pem-file"), ("Content-Disposition", "attachment; filename=\"root.crt\"")]
    {
        if let Ok(header) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            response = response.with_header(header);
        }
    }
    let _sent = request.respond(response);
}

/// `GET /api/it/ca/fingerprint` (Admin) — the root's SHA-256 fingerprint,
/// colon-hex (matching `openssl … -fingerprint -sha256`). `404` when the root
/// isn't available, `500` when the PEM can't be parsed.
pub(crate) fn ca_fingerprint() -> HttpReply {
    let pem = root_path().and_then(|p| std::fs::read_to_string(p).ok());
    let Some(pem) = pem else {
        return HttpReply::error(404, "CA root not available yet");
    };
    match fingerprint_from_pem(&pem) {
        Some(fp) => HttpReply::ok(&serde_json::json!({ "fingerprint": fp, "algorithm": "sha256" })),
        None => HttpReply::error(500, "could not parse CA root certificate"),
    }
}

/// Compute the SHA-256 fingerprint (colon-hex, uppercase) of the first
/// certificate in a PEM string — i.e. the digest of its DER bytes, exactly what
/// `openssl x509 -fingerprint -sha256` reports.
fn fingerprint_from_pem(pem: &str) -> Option<String> {
    // A present-but-empty armor body would otherwise hash to the empty-string
    // digest and masquerade as a valid fingerprint; reject it so the caller 500s.
    let der = der_from_pem(pem).filter(|d| !d.is_empty())?;
    Some(colon_hex_upper(&sha256(&der)))
}

/// Extract the DER bytes of the first certificate from a PEM string.
fn der_from_pem(pem: &str) -> Option<Vec<u8>> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let start = pem.find(BEGIN)? + BEGIN.len();
    let rest = pem.get(start..)?;
    let end = rest.find(END)?;
    let body = rest.get(..end)?;
    base64_decode(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A tiny self-signed test certificate (DER base64), used only to prove the
    // PEM→DER→SHA-256 pipeline; the fingerprint is computed, not hardcoded.
    const TEST_PEM: &str = "-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n";

    #[test]
    fn der_extraction_strips_armor_and_whitespace() {
        // "TWFu" decodes to "Man".
        assert_eq!(der_from_pem(TEST_PEM).as_deref(), Some(&b"Man"[..]));
        assert!(der_from_pem("no pem here").is_none());
    }

    #[test]
    fn fingerprint_reports_404_when_ca_root_unconfigured() {
        // No CP_CA_ROOT in the test environment → the route reports a clean 404,
        // never a panic.
        assert_eq!(ca_fingerprint().status, 404);
    }

    #[test]
    fn fingerprint_matches_sha256_of_der() {
        // openssl would compute sha256 over the DER ("Man"); verify we agree.
        let expected = colon_hex_upper(&sha256(b"Man"));
        assert_eq!(fingerprint_from_pem(TEST_PEM).as_deref(), Some(expected.as_str()));
        // Real-shape sanity: 32 bytes → 32 hex pairs joined by 31 colons.
        assert_eq!(expected.len(), 32 * 2 + 31);
    }
}
