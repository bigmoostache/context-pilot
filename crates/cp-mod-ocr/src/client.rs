//! Datalab OCR API client.
//!
//! Converts documents (PDFs, images) to text or structured JSON via the
//! [Datalab](https://www.datalab.to) cloud API (Surya engine).
//!
//! ## API flow
//!
//! 1. **Submit** — `POST /api/v1/convert` with multipart file upload.
//! 2. **Poll** — `GET /api/v1/convert/{request_id}` until `status == "complete"`.
//! 3. **Extract** — read `markdown` (markdown mode) or `json` (text\_boxes mode).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Base URL for the Datalab cloud API.
const BASE_URL: &str = "https://www.datalab.to/api/v1";

/// Maximum file size for OCR processing (25 MB).
const MAX_FILE_SIZE: u64 = 25 * 1024 * 1024;

/// Maximum time to wait for a single conversion to complete.
const MAX_POLL_DURATION: Duration = Duration::from_mins(5);

/// Initial delay between poll requests.
const POLL_INITIAL_DELAY: Duration = Duration::from_secs(2);

/// Maximum delay between poll requests.
const POLL_MAX_DELAY: Duration = Duration::from_secs(30);

/// Maximum number of retries for a failed submit request.
const MAX_SUBMIT_RETRIES: u32 = 3;

/// Output mode for OCR conversion.
#[derive(Debug, Clone, Copy)]
pub(crate) enum OcrMode {
    /// Clean markdown text extraction (`output_format=markdown`).
    Markdown,
    /// JSON with bounding boxes and block types (`output_format=json`).
    TextBoxes,
}

impl OcrMode {
    /// Parse a mode string from tool input.
    pub(crate) fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "markdown" => Ok(Self::Markdown),
            "text_boxes" => Ok(Self::TextBoxes),
            _ => Err(format!("Unknown mode '{s}'. Use 'markdown' or 'text_boxes'.")),
        }
    }

    /// Datalab API `output_format` parameter value.
    const fn api_format(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::TextBoxes => "json",
        }
    }

    /// Response field containing the result.
    const fn response_field(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::TextBoxes => "json",
        }
    }
}

/// HTTP client for the Datalab document conversion API.
///
/// Created per-request on the async worker thread.
#[derive(Debug)]
pub(crate) struct DatalabClient {
    /// API key for authentication.
    api_key: String,
    /// Reusable HTTP client.
    http: reqwest::blocking::Client,
}

/// Result of an OCR conversion.
#[derive(Debug)]
pub(crate) struct OcrResult {
    /// Extracted text (markdown or JSON string).
    pub text: String,
    /// Whether the result was served from disk cache.
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
            .timeout(Duration::from_mins(2))
            .build()
            .map_err(|e| format!("Cannot create Datalab HTTP client: {e}"))?;

        Ok(Self { api_key: api_key.to_owned(), http })
    }

    /// Convert a file via the Datalab API.
    ///
    /// Checks the global disk cache first. On a cache miss, calls the API
    /// and stores the result. Blocks the calling thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, is too large,
    /// submission fails, polling times out, or the API returns an error.
    pub(crate) fn convert(&self, path: &Path, mode: OcrMode) -> Result<OcrResult, String> {
        let _fg = cp_base::flame!("ocr_convert");
        // Validate file exists and check size.
        let meta = std::fs::metadata(path).map_err(|e| format!("Cannot read file '{}': {e}", path.display()))?;
        if meta.len() > MAX_FILE_SIZE {
            return Err(format!("File '{}' is {} MB — exceeds 25 MB limit", path.display(), meta.len() >> 20));
        }

        let file_bytes = std::fs::read(path).map_err(|e| format!("Cannot read file '{}': {e}", path.display()))?;

        // Check cache by content hash + mode.
        let content_hash = content_hash_hex(&file_bytes);
        let cache_key = format!("{content_hash}_{}", mode.api_format());
        if let Some(cached) = read_cache(&cache_key) {
            log::debug!("OCR cache hit for {} ({cache_key})", path.display());
            return Ok(OcrResult { text: cached, cached: true });
        }

        // Cache miss — call the Datalab API.
        let file_name = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("document").to_owned();

        let request_id = self.submit(&file_bytes, &file_name, mode)?;

        std::thread::sleep(POLL_INITIAL_DELAY);

        let text = self.poll_until_complete(&request_id, mode)?;

        // Cache the result.
        write_cache(&cache_key, &text);

        Ok(OcrResult { text, cached: false })
    }

    /// Submit file bytes for conversion with retries.
    fn submit(&self, file_bytes: &[u8], file_name: &str, mode: OcrMode) -> Result<String, String> {
        let mut last_err = String::from("no attempts made");
        let mut delay = Duration::from_secs(1);

        for _attempt in 0..MAX_SUBMIT_RETRIES {
            match self.submit_once(file_bytes, file_name, mode) {
                Ok(id) => return Ok(id),
                Err(e) => {
                    last_err = e;
                    if last_err.contains("HTTP 4") && !last_err.contains("HTTP 429") {
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
    fn submit_once(&self, file_bytes: &[u8], file_name: &str, mode: OcrMode) -> Result<String, String> {
        let url = format!("{BASE_URL}/convert");

        let ext = Path::new(file_name).extension().and_then(std::ffi::OsStr::to_str).unwrap_or("");
        let mime = mime_for_extension(ext).unwrap_or("application/octet-stream");

        let file_part = reqwest::blocking::multipart::Part::bytes(file_bytes.to_vec())
            .file_name(file_name.to_owned())
            .mime_str(mime)
            .map_err(|e| format!("Cannot set MIME type: {e}"))?;

        let form = reqwest::blocking::multipart::Form::new()
            .part("file", file_part)
            .text("output_format", mode.api_format().to_owned())
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
            if let Some(id) = body.get("request_id").and_then(serde_json::Value::as_str) {
                return Ok(id.to_owned());
            }
            if let Some(url_str) = body.get("request_check_url").and_then(serde_json::Value::as_str)
                && let Some(id) = url_str.rsplit('/').next()
            {
                return Ok(id.to_owned());
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
    fn poll_until_complete(&self, request_id: &str, mode: OcrMode) -> Result<String, String> {
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
                    let field = mode.response_field();
                    let text = match body.get(field) {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(other) => other.to_string(),
                        None => String::new(),
                    };
                    return Ok(text);
                }
                "error" | "failed" => {
                    let err_msg =
                        body.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown conversion error");
                    return Err(format!("OCR conversion failed: {err_msg}"));
                }
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

// -- Disk cache ---------------------------------------------------------------

/// Global cache directory for OCR results: `~/.context-pilot/ocr-cache/`.
fn cache_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".context-pilot/ocr-cache"))
}

/// Compute the hex digest of a byte slice (FNV-1a 128-bit).
fn content_hash_hex(data: &[u8]) -> String {
    cp_mod_utilities::hash::compute(data)
}

/// Try to read a cached OCR result.
fn read_cache(key: &str) -> Option<String> {
    let path = cache_dir()?.join(format!("{key}.txt"));
    std::fs::read_to_string(path).ok()
}

/// Write an OCR result to the disk cache.
fn write_cache(key: &str, text: &str) {
    let Some(dir) = cache_dir() else {
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("Cannot create OCR cache dir {}: {e}", dir.display());
        return;
    }
    let path = dir.join(format!("{key}.txt"));
    if let Err(e) = std::fs::write(&path, text) {
        log::warn!("Cannot write OCR cache {}: {e}", path.display());
    }
}

/// Read the Datalab API key from the credential vault.
pub(crate) fn api_key_from_env() -> Option<String> {
    cp_vault::vault().get("datalab").map(|s| s.expose().to_owned())
}

/// Check whether a file extension is a supported OCR format.
pub(crate) fn is_ocr_extension(ext: &str) -> bool {
    mime_for_extension(ext).is_some()
}

/// Map a file extension to its MIME type for the Datalab upload.
fn mime_for_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "pdf" => Some("application/pdf"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "tiff" | "tif" => Some("image/tiff"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "heic" => Some("image/heic"),
        _ => None,
    }
}
