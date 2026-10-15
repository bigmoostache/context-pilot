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
//!
//! The whole job runs under one wall-clock budget ([`WORKER_BUDGET`]): every
//! request timeout and every backoff sleep is clamped to what is left, so the
//! worker always reports before the watcher deadline ([`WATCHER_TIMEOUT`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::cache;

/// Base URL for the Datalab cloud API.
const BASE_URL: &str = "https://www.datalab.to/api/v1";

/// Maximum file size accepted by the Datalab API (25 MB).
const MAX_FILE_SIZE: u64 = 25 * 1024 * 1024;

/// Maximum number of submit attempts (network errors, 429, 5xx).
const MAX_SUBMIT_RETRIES: u32 = 3;

/// Upper bound on the backoff between submit attempts.
const SUBMIT_MAX_DELAY: Duration = Duration::from_secs(16);

/// Wall-clock budget for one job, submit + poll included.
pub(crate) const WORKER_BUDGET: Duration = Duration::from_mins(8);

/// Watcher deadline: the worker budget plus a margin, so a "timed out"
/// notification only fires when the worker itself is wedged.
pub(crate) const WATCHER_TIMEOUT: Duration = Duration::from_mins(9);

/// Output format for OCR conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OcrMode {
    /// Clean markdown text extraction (`output_format=markdown`).
    Markdown,
    /// JSON with bounding boxes and block types (`output_format=json`).
    TextBoxes,
}

impl OcrMode {
    /// Parse the `mode` tool parameter.
    pub(crate) fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "markdown" => Ok(Self::Markdown),
            "text_boxes" => Ok(Self::TextBoxes),
            _ => Err(format!("Unknown mode '{s}'. Use 'markdown' or 'text_boxes'.")),
        }
    }

    /// Value of the `output_format` form field.
    pub(crate) const fn api_format(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::TextBoxes => "json",
        }
    }

    /// Field of the completed poll response holding the result.
    const fn response_field(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::TextBoxes => "json",
        }
    }
}

/// Delays and limits of one job. Production values in [`Timing::PRODUCTION`];
/// tests shrink them to milliseconds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    /// Wall-clock budget for the whole job.
    pub budget: Duration,
    /// Cap on a single HTTP request.
    pub request: Duration,
    /// First poll delay (also the wait between submit and first poll).
    pub poll_initial: Duration,
    /// Cap on the poll backoff.
    pub poll_max: Duration,
    /// First delay between submit attempts.
    pub submit_initial: Duration,
}

impl Timing {
    /// Values used against the real API.
    pub(crate) const PRODUCTION: Self = Self {
        budget: WORKER_BUDGET,
        request: Duration::from_mins(2),
        poll_initial: Duration::from_secs(2),
        poll_max: Duration::from_secs(30),
        submit_initial: Duration::from_secs(1),
    };
}

/// Outcome of one failed HTTP exchange: worth retrying or not.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Attempt {
    /// Transient (network error, 429, 5xx, unreadable body): retry.
    Retry(String),
    /// Permanent (4xx, conversion failed, malformed result): give up.
    Fatal(String),
}

/// State of a conversion as reported by one poll.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Poll {
    /// Finished; carries the extracted text.
    Done(String),
    /// Still running; carries the reported status.
    Pending(String),
}

/// One document to upload.
#[derive(Debug, Clone, Copy)]
struct Upload<'doc> {
    /// File content.
    bytes: &'doc [u8],
    /// File name sent with the multipart part (drives the MIME type).
    name: &'doc str,
    /// Requested output format.
    mode: OcrMode,
}

/// HTTP client for the Datalab document conversion API.
///
/// Created per-job on the worker thread.
#[derive(Debug)]
pub(crate) struct DatalabClient {
    /// API key for authentication.
    api_key: String,
    /// Reusable HTTP client.
    http: reqwest::blocking::Client,
    /// API root, without trailing slash.
    base_url: String,
    /// Directory of the result cache.
    cache_dir: PathBuf,
    /// Delays and limits.
    timing: Timing,
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
    /// Client for the real Datalab API with the global cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub(crate) fn new(api_key: &str) -> Result<Self, String> {
        Self::with_endpoint(api_key, BASE_URL, cache::default_dir(), Timing::PRODUCTION)
    }

    /// Client against an arbitrary endpoint and cache (tests use a local mock).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub(crate) fn with_endpoint(
        api_key: &str,
        base_url: &str,
        cache_dir: PathBuf,
        timing: Timing,
    ) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .timeout(timing.request)
            .build()
            .map_err(|e| format!("Cannot create Datalab HTTP client: {e}"))?;
        Ok(Self { api_key: api_key.to_owned(), http, base_url: base_url.to_owned(), cache_dir, timing })
    }

    /// Convert a file, serving the disk cache first. Blocks the calling thread
    /// for at most the job budget.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, is too large, submission
    /// fails, the budget runs out, or the API reports a failure.
    pub(crate) fn convert(&self, path: &Path, mode: OcrMode) -> Result<OcrResult, String> {
        let _fg = cp_base::flame!("ocr_convert");
        let deadline = Instant::now().checked_add(self.timing.budget).unwrap_or_else(Instant::now);
        let meta = std::fs::metadata(path).map_err(|e| format!("Cannot read file '{}': {e}", path.display()))?;
        if meta.len() > MAX_FILE_SIZE {
            return Err(format!("File '{}' is {} MB \u{2014} exceeds 25 MB limit", path.display(), meta.len() >> 20));
        }
        let file_bytes = std::fs::read(path).map_err(|e| format!("Cannot read file '{}': {e}", path.display()))?;

        let cache_key = cache::key(&file_bytes, mode.api_format());
        if let Some(cached) = cache::read(&self.cache_dir, &cache_key) {
            log::debug!("OCR cache hit for {} ({cache_key})", path.display());
            return Ok(OcrResult { text: cached, cached: true });
        }

        let file_name = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("document").to_owned();
        let upload = Upload { bytes: &file_bytes, name: &file_name, mode };
        let request_id = self.submit(&upload, deadline)?;
        let text = self.poll_until_complete(&request_id, mode, deadline)?;
        // An empty result is never cached: a later run may get real text.
        if !text.is_empty() {
            cache::write(&self.cache_dir, &cache_key, &text);
        }
        Ok(OcrResult { text, cached: false })
    }

    /// Submit file bytes, retrying transient failures within the budget.
    fn submit(&self, upload: &Upload<'_>, deadline: Instant) -> Result<String, String> {
        let mut last_err = String::from("no attempts made");
        let mut delay = self.timing.submit_initial;
        for _attempt in 0..MAX_SUBMIT_RETRIES {
            match self.submit_once(upload, deadline) {
                Ok(id) => return Ok(id),
                Err(Attempt::Fatal(e)) => return Err(e),
                Err(Attempt::Retry(e)) => last_err = e,
            }
            if !sleep_within(deadline, delay) {
                break;
            }
            delay = delay.saturating_mul(2).min(SUBMIT_MAX_DELAY);
        }
        Err(format!("OCR submit failed after {MAX_SUBMIT_RETRIES} attempts: {last_err}"))
    }

    /// Single attempt to submit a file for conversion.
    fn submit_once(&self, upload: &Upload<'_>, deadline: Instant) -> Result<String, Attempt> {
        let Upload { bytes: file_bytes, name: file_name, mode } = *upload;
        let timeout = self.request_timeout(deadline)?;
        let ext = Path::new(file_name).extension().and_then(std::ffi::OsStr::to_str).unwrap_or("");
        let mime = mime_for_extension(ext).unwrap_or("application/octet-stream");
        let file_part = reqwest::blocking::multipart::Part::bytes(file_bytes.to_vec())
            .file_name(file_name.to_owned())
            .mime_str(mime)
            .map_err(|e| Attempt::Fatal(format!("Cannot set MIME type: {e}")))?;
        let form = reqwest::blocking::multipart::Form::new()
            .part("file", file_part)
            .text("output_format", mode.api_format().to_owned())
            .text("mode", "fast")
            .text("disable_image_extraction", "true");

        let resp = self
            .http
            .post(format!("{}/convert", self.base_url))
            .header("X-Api-Key", &self.api_key)
            .timeout(timeout)
            .multipart(form)
            .send()
            .map_err(|e| Attempt::Retry(format!("OCR submit request failed: {e}")))?;
        let status = resp.status().as_u16();
        let body = read_json(resp);
        parse_submit_response(status, &body)
    }

    /// Poll until the conversion finishes, the API reports a failure, or the
    /// budget runs out. Transient errors are retried; every error names the
    /// `request_id` so the paid job can be traced.
    fn poll_until_complete(&self, request_id: &str, mode: OcrMode, deadline: Instant) -> Result<String, String> {
        let url = format!("{}/convert/{request_id}", self.base_url);
        let mut delay = self.timing.poll_initial;
        let mut last = String::from("not polled yet");
        loop {
            if !sleep_within(deadline, delay) {
                let budget = self.timing.budget;
                return Err(format!(
                    "OCR conversion timed out after {budget:?} (request_id {request_id}, last: {last})"
                ));
            }
            match self.poll_once(&url, mode, deadline) {
                Ok(Poll::Done(text)) => return Ok(text),
                Ok(Poll::Pending(status)) => last = format!("status {status}"),
                Err(Attempt::Retry(e)) => last = e,
                Err(Attempt::Fatal(e)) => return Err(format!("{e} (request_id {request_id})")),
            }
            delay = delay.saturating_mul(2).min(self.timing.poll_max);
        }
    }

    /// One poll request.
    fn poll_once(&self, url: &str, mode: OcrMode, deadline: Instant) -> Result<Poll, Attempt> {
        let timeout = self.request_timeout(deadline)?;
        let resp = self
            .http
            .get(url)
            .header("X-Api-Key", &self.api_key)
            .timeout(timeout)
            .send()
            .map_err(|e| Attempt::Retry(format!("OCR poll request failed: {e}")))?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(classify_status(status, format!("OCR poll returned HTTP {status}")));
        }
        let body = read_json(resp);
        if body.is_null() {
            return Err(Attempt::Retry("OCR poll: unreadable response body".to_owned()));
        }
        parse_poll_body(&body, mode)
    }

    /// Per-request timeout: the request cap clamped to the time left.
    fn request_timeout(&self, deadline: Instant) -> Result<Duration, Attempt> {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Attempt::Retry("job budget exhausted".to_owned()));
        }
        Ok(left.min(self.timing.request))
    }
}

/// Sleep `delay`, clamped to the time left before `deadline`.
/// Returns `false` (without sleeping) once the deadline has passed.
fn sleep_within(deadline: Instant, delay: Duration) -> bool {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return false;
    }
    std::thread::sleep(delay.min(left));
    true
}

/// Body as JSON; `Null` when unreadable (HTML error pages, truncated reads).
fn read_json(resp: reqwest::blocking::Response) -> serde_json::Value {
    resp.text().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(serde_json::Value::Null)
}

/// 429 and 5xx are transient; every other failure status is permanent.
pub(crate) const fn classify_status(status: u16, msg: String) -> Attempt {
    if status == 429 || status >= 500 { Attempt::Retry(msg) } else { Attempt::Fatal(msg) }
}

/// Extract the `request_id` from a submit response.
pub(crate) fn parse_submit_response(status: u16, body: &serde_json::Value) -> Result<String, Attempt> {
    if status == 200 || status == 202 {
        if let Some(id) = body.get("request_id").and_then(serde_json::Value::as_str) {
            return Ok(id.to_owned());
        }
        if let Some(url_str) = body.get("request_check_url").and_then(serde_json::Value::as_str)
            && let Some(id) = url_str.rsplit('/').next().filter(|id| !id.is_empty())
        {
            return Ok(id.to_owned());
        }
        return Err(Attempt::Fatal(format!("OCR submit: response missing request_id: {body}")));
    }
    let msg = body
        .get("detail")
        .or_else(|| body.get("message"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown error");
    Err(classify_status(status, format!("OCR submit returned HTTP {status}: {msg}")))
}

/// Interpret a poll response body.
pub(crate) fn parse_poll_body(body: &serde_json::Value, mode: OcrMode) -> Result<Poll, Attempt> {
    let status = body.get("status").and_then(serde_json::Value::as_str).unwrap_or("unknown");
    match status {
        "complete" => {
            let field = mode.response_field();
            let Some(v) = body.get(field).filter(|v| !v.is_null()) else {
                return Err(Attempt::Fatal(format!("OCR complete but response has no '{field}' field")));
            };
            Ok(Poll::Done(v.as_str().map_or_else(|| v.to_string(), ToOwned::to_owned)))
        }
        "error" | "failed" => {
            let msg = body.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown conversion error");
            Err(Attempt::Fatal(format!("OCR conversion failed: {msg}")))
        }
        other => Ok(Poll::Pending(other.to_owned())),
    }
}

/// Read the Datalab API key from the credential vault.
pub(crate) fn api_key_from_env() -> Option<String> {
    cp_vault::vault().get("datalab").map(|s| s.expose().to_owned())
}

/// Check whether a file extension is a supported OCR format (any case).
pub(crate) fn is_ocr_extension(ext: &str) -> bool {
    mime_for_extension(ext).is_some()
}

/// Map a file extension (any case) to its MIME type for the Datalab upload.
fn mime_for_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
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
