//! Datalab OCR API client.
//!
//! Converts PDFs, images, and other binary documents into markdown text
//! via the [Datalab](https://www.datalab.to) cloud API.  The extracted
//! text is then chunked and indexed by the background indexer.
//!
//! ## API flow
//!
//! 1. **Submit** — `POST /api/v1/convert` with multipart file upload.
//! 2. **Poll** — `GET /api/v1/convert/{request_id}` until `status == "complete"`.
//! 3. **Extract** — read the `markdown` field from the response.
//!
//! Authentication is via the `X-Api-Key` header using `DATALAB_API_KEY`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Base URL for the Datalab cloud API.
const BASE_URL: &str = "https://www.datalab.to/api/v1";

/// Maximum file size for OCR processing (25 MB).
///
/// Larger files are skipped to avoid excessive API costs and timeouts.
pub(crate) const MAX_OCR_FILE_SIZE: u64 = 25 * 1024 * 1024;

/// Maximum time to wait for a single conversion to complete.
const MAX_POLL_DURATION: Duration = Duration::from_secs(300);

/// Initial delay between poll requests.
const POLL_INITIAL_DELAY: Duration = Duration::from_millis(2000);

/// Maximum delay between poll requests.
const POLL_MAX_DELAY: Duration = Duration::from_secs(30);

/// Maximum number of retries for a failed submit request.
const MAX_SUBMIT_RETRIES: u32 = 3;

/// Delay between consecutive OCR submissions (rate-limit guard).
const INTER_REQUEST_DELAY: Duration = Duration::from_millis(500);

/// HTTP client for the Datalab document conversion API.
///
/// Created once per indexer thread lifetime.  Reuses the inner
/// `reqwest::blocking::Client` for connection pooling.
#[derive(Debug)]
pub(crate) struct DatalabClient {
    /// API key for authentication.
    api_key: String,
    /// Reusable HTTP client.
    http: reqwest::blocking::Client,
}

/// Result of an OCR conversion, including cache status.
#[derive(Debug)]
pub(crate) struct OcrResult {
    /// Extracted markdown text.
    pub text: String,
    /// Whether the result was served from the disk cache.
    pub cached: bool,
}

impl DatalabClient {
    /// Create a client from an API key string.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub(crate) fn new(api_key: &str) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("Cannot create Datalab HTTP client: {e}"))?;

        Ok(Self { api_key: api_key.to_string(), http })
    }

    /// Convert a file to markdown text via the Datalab API.
    ///
    /// Checks the global disk cache (`~/.context-pilot/ocr-cache/`) first.
    /// On a cache miss, calls the Datalab API and stores the result.
    /// Blocks the calling thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, submission fails,
    /// polling times out, or the API returns an error status.
    pub(crate) fn convert_to_text(&self, path: &Path) -> Result<OcrResult, String> {
        // Read file bytes (needed for both hash check and upload).
        let file_bytes = std::fs::read(path).map_err(|e| format!("Cannot read file for OCR: {e}"))?;

        // Check cache by content hash.
        let hash = sha256_hex(&file_bytes);
        if let Some(cached) = read_cache(&hash) {
            log::debug!("OCR cache hit for {} ({hash})", path.display());
            return Ok(OcrResult { text: cached, cached: true });
        }

        // Cache miss — call the Datalab API.
        let file_name = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("document").to_string();

        let request_id = self.submit(&file_bytes, &file_name)?;

        // Small delay before starting to poll (conversion takes time).
        std::thread::sleep(POLL_INITIAL_DELAY);

        let markdown = self.poll_until_complete(&request_id)?;

        // Rate-limit guard: pause before the next submission.
        std::thread::sleep(INTER_REQUEST_DELAY);

        // Cache the result (including empty — avoids re-calling for no-text files).
        write_cache(&hash, &markdown);

        Ok(OcrResult { text: markdown, cached: false })
    }

    /// Submit file bytes for conversion.
    ///
    /// Retries up to [`MAX_SUBMIT_RETRIES`] times with exponential backoff
    /// on transient failures (5xx, 429, network errors).
    ///
    /// Returns the `request_id` for polling.
    fn submit(&self, file_bytes: &[u8], file_name: &str) -> Result<String, String> {
        let mut last_err = String::from("no attempts made");
        let mut delay = Duration::from_secs(1);

        for _attempt in 0..MAX_SUBMIT_RETRIES {
            match self.submit_once(file_bytes, file_name) {
                Ok(id) => return Ok(id),
                Err(e) => {
                    last_err = e;
                    // Only retry on transient errors
                    if last_err.contains("HTTP 4") && !last_err.contains("HTTP 429") {
                        // Client error (not rate-limited) — don't retry.
                        return Err(last_err);
                    }
                    std::thread::sleep(delay);
                    delay = delay.saturating_mul(2).min(Duration::from_secs(16));
                }
            }
        }

        Err(format!("OCR submit failed after {MAX_SUBMIT_RETRIES} retries: {last_err}"))
    }

    /// Single attempt to submit a file for conversion.
    fn submit_once(&self, file_bytes: &[u8], file_name: &str) -> Result<String, String> {
        let url = format!("{BASE_URL}/convert");

        let file_part = reqwest::blocking::multipart::Part::bytes(file_bytes.to_vec())
            .file_name(file_name.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| format!("Cannot set MIME type: {e}"))?;

        let form = reqwest::blocking::multipart::Form::new()
            .part("file", file_part)
            .text("output_format", "markdown")
            .text("mode", "fast")
            .text("disable_image_extraction", "true");

        let resp = self
            .http
            .post(&url)
            .header("X-Api-Key", &self.api_key)
            .multipart(form)
            .send()
            .map_err(|e| format!("OCR submit request failed: {e}"))?;

        let status = resp.status().as_u16();
        let body: serde_json::Value = resp.json().map_err(|e| format!("OCR submit: cannot parse response: {e}"))?;

        if status == 200 || status == 202 {
            // Extract request_id from response.
            if let Some(id) = body.get("request_id").and_then(serde_json::Value::as_str) {
                return Ok(id.to_string());
            }
            // Some responses use request_check_url instead.
            if let Some(url_str) = body.get("request_check_url").and_then(serde_json::Value::as_str) {
                // Extract request_id from the URL (last path segment).
                if let Some(id) = url_str.rsplit('/').next() {
                    return Ok(id.to_string());
                }
            }
            return Err(format!("OCR submit: response missing request_id: {body}"));
        }

        let msg = body
            .get("detail")
            .or_else(|| body.get("message"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown error");
        Err(format!("OCR submit returned HTTP {status}: {msg}"))
    }

    /// Poll the conversion endpoint until the result is ready.
    ///
    /// Uses exponential backoff between polls, starting at
    /// [`POLL_INITIAL_DELAY`] and capping at [`POLL_MAX_DELAY`].
    /// Gives up after [`MAX_POLL_DURATION`].
    fn poll_until_complete(&self, request_id: &str) -> Result<String, String> {
        let url = format!("{BASE_URL}/convert/{request_id}");
        let deadline = Instant::now().checked_add(MAX_POLL_DURATION).unwrap_or_else(Instant::now);
        let mut delay = POLL_INITIAL_DELAY;

        loop {
            let resp = self
                .http
                .get(&url)
                .header("X-Api-Key", &self.api_key)
                .send()
                .map_err(|e| format!("OCR poll request failed: {e}"))?;

            let status_code = resp.status().as_u16();

            if status_code == 429 {
                // Rate-limited — wait longer.
                delay = delay.saturating_mul(2).min(POLL_MAX_DELAY);
                if Instant::now() >= deadline {
                    return Err(format!("OCR conversion timed out after {MAX_POLL_DURATION:?}"));
                }
                std::thread::sleep(delay);
                continue;
            }

            if status_code != 200 {
                return Err(format!("OCR poll returned HTTP {status_code}"));
            }

            let body: serde_json::Value = resp.json().map_err(|e| format!("OCR poll: cannot parse response: {e}"))?;

            let poll_status = body.get("status").and_then(serde_json::Value::as_str).unwrap_or("unknown");

            match poll_status {
                "complete" => {
                    let markdown = body.get("markdown").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
                    return Ok(markdown);
                }
                "error" | "failed" => {
                    let err_msg =
                        body.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown conversion error");
                    return Err(format!("OCR conversion failed: {err_msg}"));
                }
                // "pending" | "processing" | etc. → keep polling.
                _ => {}
            }

            if Instant::now() >= deadline {
                return Err(format!("OCR conversion timed out after {MAX_POLL_DURATION:?} (status: {poll_status})"));
            }

            std::thread::sleep(delay);
            delay = delay.saturating_mul(2).min(POLL_MAX_DELAY);
        }
    }
}

// -- Disk cache --------------------------------------------------------------

/// Global cache directory for OCR results: `~/.context-pilot/ocr-cache/`.
fn cache_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".context-pilot/ocr-cache"))
}

/// Compute the SHA-256 hex digest of a byte slice (used as cache key).
fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest as _;
    let hash = sha2::Sha256::digest(data);
    let mut hex = String::with_capacity(64);
    for &b in hash.as_slice() {
        use std::fmt::Write as _;
        let _r = write!(hex, "{b:02x}");
    }
    hex
}

/// Try to read a cached OCR result by content hash.
///
/// Returns `Some(markdown)` on hit, `None` on miss or any I/O error.
fn read_cache(hash: &str) -> Option<String> {
    let path = cache_dir()?.join(format!("{hash}.md"));
    std::fs::read_to_string(path).ok()
}

/// Write an OCR result to the disk cache.
///
/// Creates the cache directory if it doesn't exist.
/// Failures are logged but never fatal — the cache is best-effort.
fn write_cache(hash: &str, markdown: &str) {
    let Some(dir) = cache_dir() else {
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("Cannot create OCR cache dir {}: {e}", dir.display());
        return;
    }
    let path = dir.join(format!("{hash}.md"));
    if let Err(e) = std::fs::write(&path, markdown) {
        log::warn!("Cannot write OCR cache {}: {e}", path.display());
    }
}

/// Read `DATALAB_API_KEY` from the environment.
///
/// Returns `None` if the variable is not set or is empty.
/// Used during indexer startup to decide whether OCR is available.
#[must_use]
pub(crate) fn api_key_from_env() -> Option<String> {
    std::env::var("DATALAB_API_KEY").ok().filter(|k| !k.is_empty())
}

/// Check whether a file extension requires OCR processing.
///
/// These are binary document/image formats that cannot be read as
/// plain text and need the Datalab API for text extraction.
#[must_use]
pub(crate) fn is_ocr_extension(ext: &str) -> bool {
    matches!(ext, "pdf" | "png" | "jpg" | "jpeg" | "tiff" | "tif" | "bmp" | "webp" | "heic")
}
