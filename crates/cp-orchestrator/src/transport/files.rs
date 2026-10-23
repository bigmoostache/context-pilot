//! Byte-oriented HTTP responses — everything the transport returns as raw bytes
//! rather than JSON: the static SPA (`CP_WEB_ROOT`), file downloads, inline raw
//! previews, and agent avatars.
//!
//! Split out of [`super`] to keep the router/dispatch module focused. These
//! handlers reuse the parent's [`cors_headers`](super::cors_headers) and
//! [`respond_json`](super::respond_json) helpers.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tiny_http::{Header, Request, Response};

use super::rest::Backend;
use super::{cors_headers, inspect, respond_json, rest};

/// The built SPA root (`dist/`), from `CP_WEB_ROOT` (validated at boot to be
/// an existing directory).
///
/// When unset the orchestrator serves the API only — its historical
/// behaviour, with the SPA fronted by a separate web server. When set, the
/// orchestrator also serves the web UI itself, so a single binary on a single
/// port is the whole product (the native-appliance deployment).
pub(super) fn web_root() -> Option<&'static PathBuf> {
    cp_env::env().orch.web_root.as_ref()
}

/// Serve `path` from the SPA [`web_root`] (only called when it is `Some`).
///
/// An existing file is returned with a content-type guessed from its extension;
/// anything else falls back to `index.html` with a `200` so client-side routing
/// resolves deep links. Fingerprinted assets under `assets/` are cached
/// immutably; the HTML shell is never cached.
pub(super) fn serve_static(request: Request, path: &str) {
    let Some(root) = web_root() else { return };
    let rel = path.trim_start_matches('/');

    // Refuse path traversal — never resolve outside the web root.
    if rel.split('/').any(|seg| seg == ".." || seg == ".") {
        respond_json(request, &rest::HttpReply::error(403, "forbidden"));
        return;
    }

    let candidate = if rel.is_empty() { root.join("index.html") } else { root.join(rel) };
    let (file, is_asset) =
        if candidate.is_file() { (candidate, rel.starts_with("assets/")) } else { (root.join("index.html"), false) };

    match std::fs::read(&file) {
        Ok(bytes) => {
            let mut response = Response::from_data(bytes).with_status_code(200u16);
            if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], content_type(&file).as_bytes()) {
                response = response.with_header(h);
            }
            let cache: &[u8] = if is_asset { b"public, max-age=31536000, immutable" } else { b"no-cache" };
            if let Ok(h) = Header::from_bytes(&b"Cache-Control"[..], cache) {
                response = response.with_header(h);
            }
            let _sent = request.respond(response);
        }
        Err(_) => respond_json(request, &rest::HttpReply::error(404, "not found")),
    }
}

/// Guess a response content-type from a file extension. Covers everything a Vite
/// build emits (JS/CSS/HTML, fonts, images, wasm, source maps); unknown types
/// fall back to `application/octet-stream`.
fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("txt") => "text/plain; charset=utf-8",
        Some("webmanifest") => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

/// Serve a raw file download with `Content-Disposition: attachment`.
pub(super) fn handle_download(request: Request, state: &Arc<Mutex<Backend>>, id: &str, query: &str) {
    match inspect::finder::fs_download(state, id, query) {
        Ok((bytes, filename)) => {
            let mut response = Response::from_data(bytes).with_status_code(200u16);
            if let Ok(h) = Header::from_bytes(
                &b"Content-Disposition"[..],
                format!("attachment; filename=\"{filename}\"").as_bytes(),
            ) {
                response = response.with_header(h);
            }
            if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], &b"application/octet-stream"[..]) {
                response = response.with_header(h);
            }
            for header in cors_headers(&request) {
                response = response.with_header(header);
            }
            let _sent = request.respond(response);
        }
        Err(reply) => respond_json(request, &reply),
    }
}

/// Serve a file's raw bytes **inline** (Content-Type inferred, no attachment),
/// so the browser renders it directly — powers the Finder's image (T286) and
/// PDF (T281) in-pane previews.
pub(super) fn handle_raw(request: Request, state: &Arc<Mutex<Backend>>, id: &str, query: &str) {
    match inspect::finder::fs_raw(state, id, query) {
        Ok((bytes, ctype)) => {
            let mut response = Response::from_data(bytes).with_status_code(200u16);
            if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()) {
                response = response.with_header(h);
            }
            if let Ok(h) = Header::from_bytes(&b"Content-Disposition"[..], &b"inline"[..]) {
                response = response.with_header(h);
            }
            for header in cors_headers(&request) {
                response = response.with_header(header);
            }
            let _sent = request.respond(response);
        }
        Err(reply) => respond_json(request, &reply),
    }
}

/// A raw `(name, value)` response header.
type HeaderPair<'bytes> = (&'bytes [u8], &'bytes [u8]);

/// Serve an office document's bytes for the in-browser editors, with its
/// version in `X-Office-Version` (sent back as `base` on save, compared with
/// the polled `fs/office/version` to spot an agent's edit).
pub(super) fn handle_office(request: Request, state: &Arc<Mutex<Backend>>, id: &str, query: &str) {
    match inspect::finder::fs_office_read(state, id, query) {
        Ok(file) => {
            let mut response = Response::from_data(file.bytes).with_status_code(200u16);
            let headers: [HeaderPair<'_>; 4] = [
                (b"Content-Type", b"application/octet-stream"),
                (b"Cache-Control", b"no-store"),
                (b"X-Office-Version", file.version.as_bytes()),
                (b"Access-Control-Expose-Headers", b"X-Office-Version"),
            ];
            for (name, value) in headers {
                if let Ok(h) = Header::from_bytes(name, value) {
                    response = response.with_header(h);
                }
            }
            for header in cors_headers(&request) {
                response = response.with_header(header);
            }
            let _sent = request.respond(response);
        }
        Err(reply) => respond_json(request, &reply),
    }
}

/// Serve an agent's avatar image inline (Content-Type from the avatar store).
pub(super) fn handle_avatar(request: Request, state: &Arc<Mutex<Backend>>, id: &str) {
    let avatar = state.lock().ok().and_then(|b| b.avatars.get(id));
    respond_image(request, avatar, "no avatar");
}

/// `GET /api/branding/favicon` (public) — the uploaded cockpit favicon, or 404
/// when the built-in one applies. Cache-busted by the client (`?v=`).
pub(super) fn handle_favicon(request: Request, state: &Arc<Mutex<Backend>>) {
    let favicon = state.lock().ok().and_then(|b| b.branding.get(rest::FAVICON_KEY));
    respond_image(request, favicon, "no favicon");
}

/// Answer with stored image bytes + content type, or a JSON 404 `missing`.
fn respond_image(request: Request, image: Option<(Vec<u8>, String)>, missing: &str) {
    match image {
        Some((bytes, ctype)) => {
            let mut response = Response::from_data(bytes).with_status_code(200u16);
            if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()) {
                response = response.with_header(h);
            }
            if let Ok(h) = Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=3600"[..]) {
                response = response.with_header(h);
            }
            for header in cors_headers(&request) {
                response = response.with_header(header);
            }
            let _sent = request.respond(response);
        }
        None => respond_json(request, &rest::HttpReply::error(404, missing)),
    }
}
