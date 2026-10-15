//! Firecrawl HTTP transport: one request path, a retry policy per call, and an
//! optional wall-clock deadline shared by every request of a long operation.
//!
//! Every Firecrawl `POST` is billed, and a server error or timeout does not prove
//! the request was rejected, so billed calls are only retried when the server
//! certainly did not act: the connection never opened, or it answered 429.

use reqwest::Method;
use reqwest::blocking::Client;
use reqwest::header::RETRY_AFTER;
use std::time::{Duration, Instant};

/// Total attempts per request, first try included.
pub(crate) const MAX_ATTEMPTS: u32 = 3;
/// Per-attempt timeout ceiling (lowered further by a deadline).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Wait between attempts when the server gives no `Retry-After`.
const BACKOFF: Duration = Duration::from_secs(1);
/// Cap on a server-requested `Retry-After`, so one 429 cannot stall a call.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(10);
/// Bytes of an error body quoted back to the model.
const ERROR_BODY_CHARS: usize = 200;

/// Which failures a request may retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Policy {
    /// Read-only (`GET` poll): any transient failure is safe to repeat.
    Idempotent,
    /// Costs credits or starts a job: repeat only if the server surely ignored it.
    Billable,
}

/// How one attempt failed, as far as retrying is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The server answered with this non-2xx status.
    Status(u16),
    /// The connection never opened: the request never reached the server.
    Connect,
    /// Timeout or broken transfer after sending: the server may have acted.
    Transfer,
}

/// Whether `failure` may be retried under `policy`.
pub(crate) const fn should_retry(policy: Policy, failure: Failure) -> bool {
    match failure {
        Failure::Status(429) | Failure::Connect => true,
        Failure::Status(code) => code >= 500 && matches!(policy, Policy::Idempotent),
        Failure::Transfer => matches!(policy, Policy::Idempotent),
    }
}

/// Wait before the next attempt: `Retry-After` seconds (capped), else [`BACKOFF`].
/// HTTP-date values are not parsed and fall back to [`BACKOFF`].
pub(crate) fn retry_delay(retry_after: Option<&str>) -> Duration {
    retry_after
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map_or(BACKOFF, |secs| Duration::from_secs(secs).min(MAX_RETRY_AFTER))
}

/// Model-facing text for a non-2xx answer.
pub(crate) fn status_error(status: u16, body: &str) -> String {
    let quoted = truncate(body, ERROR_BODY_CHARS);
    match status {
        401 | 403 => format!("Unauthorized ({status}). Check FIRECRAWL_API_KEY. Response: {quoted}"),
        402 => format!("Payment required (402): Firecrawl credits exhausted. Response: {quoted}"),
        429 => format!("Rate limited (429) after {MAX_ATTEMPTS} attempts. Response: {quoted}"),
        _ => format!("HTTP {status} error: {quoted}"),
    }
}

/// Truncate a string to at most `max` bytes on a char boundary.
pub(crate) fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { s.get(..s.floor_char_boundary(max)).unwrap_or("") }
}

/// One request to send: method, absolute URL, optional JSON body.
#[derive(Debug)]
pub(crate) struct Call<'req> {
    /// HTTP method.
    pub method: Method,
    /// Absolute URL.
    pub url: &'req str,
    /// JSON body, if any.
    pub body: Option<&'req serde_json::Value>,
}

/// Result of a single attempt.
enum Step {
    /// 2xx body.
    Body(String),
    /// Failed attempt, with what to report and how long to wait before retrying.
    Failed {
        /// Retry classification.
        failure: Failure,
        /// Error text if this turns out to be the last attempt.
        error: String,
        /// Delay before the next attempt.
        wait: Duration,
    },
}

/// Authenticated Firecrawl HTTP endpoint.
#[derive(Debug)]
pub(crate) struct Transport {
    /// Shared connection pool; timeouts are set per request.
    client: Client,
    /// Bearer token.
    api_key: String,
    /// API root, without trailing slash (e.g. `https://api.firecrawl.dev/v2`).
    base_url: String,
    /// No attempt starts, and no attempt outlives, this instant.
    deadline: Option<Instant>,
}

impl Transport {
    /// Build a transport against `base_url`.
    ///
    /// # Errors
    /// The HTTP client failed to build (TLS backend init).
    pub(crate) fn new(api_key: String, base_url: &str) -> Result<Self, String> {
        // Test builds skip system proxies so the loopback mock server is reached directly.
        let builder = if cfg!(test) { Client::builder().no_proxy() } else { Client::builder() };
        let client = builder.build().map_err(|e| format!("failed to build HTTP client: {e}"))?;
        Ok(Self { client, api_key, base_url: base_url.trim_end_matches('/').to_owned(), deadline: None })
    }

    /// Bound every later request by `deadline`.
    pub(crate) const fn set_deadline(&mut self, deadline: Instant) {
        self.deadline = Some(deadline);
    }

    /// Absolute URL for an API `path` (leading slash).
    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// Whether `url` points inside this API root, so the bearer token may be sent to it.
    pub(crate) fn owns(&self, url: &str) -> bool {
        url.strip_prefix(self.base_url.as_str()).is_some_and(|rest| rest.starts_with('/'))
    }

    /// Time left for one attempt; `None` once the deadline has passed.
    fn attempt_timeout(&self) -> Option<Duration> {
        let Some(deadline) = self.deadline else { return Some(REQUEST_TIMEOUT) };
        let left = deadline.saturating_duration_since(Instant::now());
        (!left.is_zero()).then(|| left.min(REQUEST_TIMEOUT))
    }

    /// Whether sleeping `wait` would end past the deadline.
    fn wait_overruns(&self, wait: Duration) -> bool {
        self.deadline.is_some_and(|d| Instant::now().checked_add(wait).is_none_or(|end| end >= d))
    }

    /// Send a request and decode its JSON answer, retrying per `policy`.
    ///
    /// # Errors
    /// Last attempt's failure, a deadline overrun, or a JSON decode error.
    pub(crate) fn send<T>(&self, call: &Call<'_>, policy: Policy) -> Result<T, String>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut last_error = "Firecrawl deadline reached before the request was sent".to_owned();
        for attempt in 1..=MAX_ATTEMPTS {
            let Some(timeout) = self.attempt_timeout() else { return Err(last_error) };
            let (failure, error, wait) = match self.attempt(call, timeout) {
                Step::Body(text) => {
                    return serde_json::from_str(&text).map_err(|e| format!("Failed to parse response: {e}"));
                }
                Step::Failed { failure, error, wait } => (failure, error, wait),
            };
            if attempt == MAX_ATTEMPTS || !should_retry(policy, failure) || self.wait_overruns(wait) {
                return Err(error);
            }
            last_error = error;
            std::thread::sleep(wait);
        }
        Err(last_error)
    }

    /// One fire-and-forget attempt with its own `timeout`, ignoring the deadline
    /// (used for cleanup once the deadline is gone). The answer body is dropped.
    ///
    /// # Errors
    /// Network failure or non-2xx status.
    pub(crate) fn send_once(&self, call: &Call<'_>, timeout: Duration) -> Result<(), String> {
        match self.attempt(call, timeout) {
            Step::Body(_) => Ok(()),
            Step::Failed { error, .. } => Err(error),
        }
    }

    /// Perform one HTTP exchange.
    fn attempt(&self, call: &Call<'_>, timeout: Duration) -> Step {
        let mut req = self.client.request(call.method.clone(), call.url).bearer_auth(&self.api_key).timeout(timeout);
        if let Some(json) = call.body {
            req = req.json(json);
        }
        let resp = match req.send() {
            Ok(r) => r,
            Err(e) => {
                let failure = if e.is_connect() { Failure::Connect } else { Failure::Transfer };
                return Step::Failed { failure, error: format!("Request failed: {e}"), wait: BACKOFF };
            }
        };
        let status = resp.status().as_u16();
        let wait = retry_delay(resp.headers().get(RETRY_AFTER).and_then(|v| v.to_str().ok()));
        let text = match resp.text() {
            Ok(t) => t,
            Err(e) => {
                return Step::Failed {
                    failure: Failure::Transfer,
                    error: format!("Failed to read response: {e}"),
                    wait,
                };
            }
        };
        if (200..300).contains(&status) {
            Step::Body(text)
        } else {
            Step::Failed { failure: Failure::Status(status), error: status_error(status, &text), wait }
        }
    }
}
