use std::sync::LazyLock;
use std::time::{Duration, Instant};

use reqwest::Url;
use reqwest::blocking::Client;

use crate::types::{BraveSearchResponse, LLMContextResponse, RichCallbackResponse};

/// Base URL for the Brave Search v1 REST API.
const BRAVE_BASE_URL: &str = "https://api.search.brave.com/res/v1";
/// Upper bound for a single HTTP request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Wall-clock budget for one tool call, retries and rich callback included.
/// Must stay below the async tool timeout so callers get a real error, not a generic timeout.
pub const TOTAL_BUDGET: Duration = Duration::from_secs(30);
/// Below this remaining budget, a new attempt is not worth starting.
const MIN_ATTEMPT: Duration = Duration::from_secs(2);
/// Pause between retries of a transient failure.
const RETRY_DELAY: Duration = Duration::from_secs(1);
/// Maximum attempts per request (first try included).
const MAX_ATTEMPTS: u32 = 3;
/// Max bytes of an error body echoed back to the model.
const ERROR_BODY_BYTES: usize = 200;

/// Process-wide HTTP client: one connection/TLS pool shared by every call.
static HTTP: LazyLock<Result<Client, String>> = LazyLock::new(|| {
    Client::builder().timeout(REQUEST_TIMEOUT).build().map_err(|e| format!("failed to build HTTP client: {e}"))
});

/// Parameters for a Brave web search request.
#[derive(Debug)]
pub struct SearchParams<'req> {
    /// Search query string.
    pub query: &'req str,
    /// Number of results to return (1-20).
    pub count: u32,
    /// Recency filter (e.g., "pd", "pw", "pm", "py", or date range).
    pub freshness: Option<&'req str>,
    /// Two-letter ISO country code.
    pub country: &'req str,
    /// Result language ISO 639-1 code.
    pub search_lang: &'req str,
    /// Safe search level: "off", "moderate", or "strict".
    pub safe_search: &'req str,
    /// Brave Goggle URL for domain re-ranking.
    pub goggles_id: Option<&'req str>,
}

/// Parameters for a Brave LLM context request.
#[derive(Debug)]
pub struct LLMContextParams<'req> {
    /// Search query string.
    pub query: &'req str,
    /// Approximate max tokens in response (1024-32768).
    pub max_tokens: u32,
    /// Max search results to consider (1-50); also caps the number of URLs used.
    pub count: u32,
    /// Relevance threshold: "strict", "balanced", "lenient", or "disabled".
    pub threshold_mode: &'req str,
    /// Recency filter.
    pub freshness: Option<&'req str>,
    /// Two-letter ISO country code.
    pub country: &'req str,
    /// Brave Goggle URL or inline definition.
    pub goggles: Option<&'req str>,
}

/// Web search result plus the optional rich-callback payload.
#[derive(Debug)]
pub struct SearchOutcome {
    /// Organic search response.
    pub response: BraveSearchResponse,
    /// Rich data (stocks, weather...) when the API offered and delivered it.
    pub rich: Option<serde_json::Value>,
    /// Why the offered rich data could not be fetched, so it is reported, not swallowed.
    pub rich_error: Option<String>,
}

/// Outcome of a single failed attempt.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    /// Worth retrying (network error, 5xx).
    Transient(String),
    /// Retrying cannot help (auth, rate limit, bad parameters).
    Fatal(String),
}

/// HTTP client for the Brave Search API.
#[derive(Debug)]
pub struct BraveClient {
    /// Shared reqwest client (cheap `Arc` clone of [`HTTP`]).
    client: Client,
    /// Brave API subscription token.
    api_key: String,
}

impl BraveClient {
    /// Create a client over the shared connection pool.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the shared HTTP client failed to build.
    pub fn new(api_key: String) -> Result<Self, String> {
        let client = HTTP.as_ref().map_err(Clone::clone)?.clone();
        Ok(Self { client, api_key })
    }

    /// Search the web, then follow the rich-callback key if the response carries one.
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failure, non-2xx HTTP status, or JSON parse error of the main search.
    /// A rich-callback failure is reported in [`SearchOutcome::rich_error`] instead.
    pub fn search(&self, p: &SearchParams<'_>) -> Result<SearchOutcome, String> {
        let start = Instant::now();
        let body = self.get(&search_url(p)?, start)?;
        let response: BraveSearchResponse =
            serde_json::from_str(&body).map_err(|e| format!("Failed to parse search response: {e}"))?;

        let key = response.rich.as_ref().and_then(|r| r.hint.as_ref()).and_then(|h| h.callback_key.clone());
        let (rich, rich_error) = match key.map(|k| self.fetch_rich_callback(&k, start)) {
            None => (None, None),
            Some(Ok(data)) => (Some(data), None),
            Some(Err(e)) => (None, Some(e)),
        };
        Ok(SearchOutcome { response, rich, rich_error })
    }

    /// Get LLM-optimized context from Brave LLM Context API.
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failure, non-2xx HTTP status, or JSON parse error.
    pub fn llm_context(&self, p: &LLMContextParams<'_>) -> Result<LLMContextResponse, String> {
        let body = self.get(&llm_context_url(p)?, Instant::now())?;
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse LLM context response: {e}"))
    }

    /// Fetch rich results via callback key, within what is left of the call budget.
    fn fetch_rich_callback(&self, callback_key: &str, start: Instant) -> Result<serde_json::Value, String> {
        let url = Url::parse_with_params(&format!("{BRAVE_BASE_URL}/web/rich"), [("callback_key", callback_key)])
            .map_err(|e| format!("Invalid rich URL: {e}"))?;
        let body = self.get(&url, start)?;
        let rich: RichCallbackResponse =
            serde_json::from_str(&body).map_err(|e| format!("Failed to parse rich response: {e}"))?;
        Ok(rich.data)
    }

    /// GET with retries on transient failures, bounded by [`TOTAL_BUDGET`] since `start`.
    fn get(&self, url: &Url, start: Instant) -> Result<String, String> {
        let mut last = "time budget exhausted before the request could start".to_owned();
        for attempt in 1..=MAX_ATTEMPTS {
            let remaining = TOTAL_BUDGET.saturating_sub(start.elapsed());
            if remaining < MIN_ATTEMPT {
                break;
            }
            match self.attempt(url, remaining.min(REQUEST_TIMEOUT)) {
                Ok(body) => return Ok(body),
                Err(Failure::Fatal(e)) => return Err(e),
                Err(Failure::Transient(e)) => last = format!("{e} (attempt {attempt}/{MAX_ATTEMPTS})"),
            }
            if attempt < MAX_ATTEMPTS {
                std::thread::sleep(RETRY_DELAY);
            }
        }
        Err(last)
    }

    /// One HTTP attempt, classified for the retry loop.
    fn attempt(&self, url: &Url, timeout: Duration) -> Result<String, Failure> {
        let resp = self
            .client
            .get(url.clone())
            .timeout(timeout)
            .header("Accept", "application/json")
            .header("X-Subscription-Token", &self.api_key)
            .send()
            .map_err(|e| Failure::Transient(format!("Request failed: {e}")))?;
        let status = resp.status().as_u16();
        let body = resp.text().map_err(|e| Failure::Transient(format!("Failed to read response: {e}")))?;
        classify_status(status, &body).map(|()| body)
    }
}

/// Map an HTTP status to success, a retryable failure, or a final one.
fn classify_status(status: u16, body: &str) -> Result<(), Failure> {
    let excerpt = truncate(body, ERROR_BODY_BYTES);
    match status {
        200..=299 => Ok(()),
        429 => Err(Failure::Fatal(format!("Rate limited (429). Try again later. Response: {excerpt}"))),
        403 => Err(Failure::Fatal(format!("Forbidden (403). Check API key. Response: {excerpt}"))),
        422 => Err(Failure::Fatal(format!("Invalid parameters (422). Response: {excerpt}"))),
        500..=599 => Err(Failure::Transient(format!("HTTP {status} error: {excerpt}"))),
        _ => Err(Failure::Fatal(format!("HTTP {status} error: {excerpt}"))),
    }
}

/// Build the `/web/search` URL. Always requests extra snippets and the rich callback.
fn search_url(p: &SearchParams<'_>) -> Result<Url, String> {
    let count = p.count.to_string();
    let mut pairs = vec![
        ("q", p.query),
        ("count", count.as_str()),
        ("extra_snippets", "true"),
        ("enable_rich_callback", "1"),
        ("country", p.country),
        ("search_lang", p.search_lang),
        ("safesearch", p.safe_search),
    ];
    pairs.extend(p.freshness.map(|f| ("freshness", f)));
    pairs.extend(p.goggles_id.map(|g| ("goggles_id", g)));
    Url::parse_with_params(&format!("{BRAVE_BASE_URL}/web/search"), pairs).map_err(|e| format!("Invalid URL: {e}"))
}

/// Build the `/llm/context` URL. `maximum_number_of_urls` follows `count` so the model's choice is honoured.
fn llm_context_url(p: &LLMContextParams<'_>) -> Result<Url, String> {
    let max_tokens = p.max_tokens.to_string();
    let count = p.count.to_string();
    let mut pairs = vec![
        ("q", p.query),
        ("maximum_number_of_tokens", max_tokens.as_str()),
        ("count", count.as_str()),
        ("maximum_number_of_urls", count.as_str()),
        ("context_threshold_mode", p.threshold_mode),
        ("country", p.country),
        ("maximum_number_of_snippets", "50"),
        ("maximum_number_of_tokens_per_url", "4096"),
    ];
    pairs.extend(p.freshness.map(|f| ("freshness", f)));
    pairs.extend(p.goggles.map(|g| ("goggles", g)));
    Url::parse_with_params(&format!("{BRAVE_BASE_URL}/llm/context"), pairs).map_err(|e| format!("Invalid URL: {e}"))
}

/// Truncate a string to at most `max` bytes on a char boundary.
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { s.get(..s.floor_char_boundary(max)).unwrap_or("") }
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
