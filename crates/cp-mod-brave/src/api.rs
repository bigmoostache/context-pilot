use reqwest::blocking::Client;
use std::time::Duration;

use crate::types::{BraveSearchResponse, LLMContextResponse, RichCallbackResponse};
use std::fmt::Write as _;

/// Base URL for the Brave Search v1 REST API.
const BRAVE_BASE_URL: &str = "https://api.search.brave.com/res/v1";
/// HTTP request timeout in seconds.
const TIMEOUT_SECS: u64 = 10;

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
    /// Max search results to consider (1-50).
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

/// HTTP client for the Brave Search API.
#[derive(Debug)]
pub struct BraveClient {
    /// Reusable reqwest HTTP client with timeout.
    client: Client,
    /// Brave API subscription token.
    api_key: String,
}

impl BraveClient {
    /// Create a new client with the given API key (10s request timeout).
    ///
    /// # Errors
    ///
    /// Returns `Err` if the HTTP client fails to build.
    pub fn new(api_key: String) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;
        Ok(Self { client, api_key })
    }

    /// Search the web via Brave Search API.
    /// Always sends `extra_snippets=true` and `enable_rich_callback=1`.
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failure, non-2xx HTTP status, or JSON parse error.
    pub fn search(&self, p: &SearchParams<'_>) -> Result<(BraveSearchResponse, Option<serde_json::Value>), String> {
        let mut url = format!("{}/web/search?q={}", BRAVE_BASE_URL, urlenc(p.query));
        let _r1 = write!(url, "&count={}", p.count);
        url.push_str("&extra_snippets=true");
        url.push_str("&enable_rich_callback=1");
        let _r2 = write!(url, "&country={}", urlenc(p.country));
        let _r3 = write!(url, "&search_lang={}", urlenc(p.search_lang));
        let _r4 = write!(url, "&safesearch={}", urlenc(p.safe_search));

        if let Some(f) = p.freshness {
            let _r5 = write!(url, "&freshness={}", urlenc(f));
        }
        if let Some(g) = p.goggles_id {
            let _r6 = write!(url, "&goggles_id={}", urlenc(g));
        }

        let response = self.get_with_retry(&url)?;
        let search_resp: BraveSearchResponse =
            serde_json::from_str(&response).map_err(|e| format!("Failed to parse search response: {e}"))?;

        let rich_data = search_resp
            .rich
            .as_ref()
            .and_then(|r| r.hint.as_ref())
            .and_then(|h| h.callback_key.as_ref())
            .and_then(|key| self.fetch_rich_callback(key).ok());

        Ok((search_resp, rich_data))
    }

    /// Get LLM-optimized context from Brave LLM Context API.
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failure, non-2xx HTTP status, or JSON parse error.
    pub fn llm_context(&self, p: &LLMContextParams<'_>) -> Result<LLMContextResponse, String> {
        let mut url = format!("{}/llm/context?q={}", BRAVE_BASE_URL, urlenc(p.query));
        let _r1 = write!(url, "&maximum_number_of_tokens={}", p.max_tokens);
        let _r2 = write!(url, "&count={}", p.count);
        let _r3 = write!(url, "&context_threshold_mode={}", urlenc(p.threshold_mode));
        let _r4 = write!(url, "&country={}", urlenc(p.country));
        // Hardcoded optimal defaults
        url.push_str("&maximum_number_of_urls=20");
        url.push_str("&maximum_number_of_snippets=50");
        url.push_str("&maximum_number_of_tokens_per_url=4096");

        if let Some(f) = p.freshness {
            let _r5 = write!(url, "&freshness={}", urlenc(f));
        }
        if let Some(g) = p.goggles {
            let _r6 = write!(url, "&goggles={}", urlenc(g));
        }

        let response = self.get_with_retry(&url)?;
        serde_json::from_str(&response).map_err(|e| format!("Failed to parse LLM context response: {e}"))
    }

    /// Fetch rich results via callback key.
    fn fetch_rich_callback(&self, callback_key: &str) -> Result<serde_json::Value, String> {
        let url = format!("{}/web/rich?callback_key={}", BRAVE_BASE_URL, urlenc(callback_key));
        let response = self.get_with_retry(&url)?;
        let rich: RichCallbackResponse =
            serde_json::from_str(&response).map_err(|e| format!("Failed to parse rich response: {e}"))?;
        Ok(rich.data)
    }

    /// GET with 5xx retry (2 attempts, 1s delay).
    fn get_with_retry(&self, url: &str) -> Result<String, String> {
        for attempt in 0..3 {
            let resp = self
                .client
                .get(url)
                .header("Accept", "application/json")
                .header("X-Subscription-Token", &self.api_key)
                .send()
                .map_err(|e| format!("Request failed: {e}"))?;

            let status = resp.status().as_u16();
            let body = resp.text().map_err(|e| format!("Failed to read response: {e}"))?;

            match status {
                200..=299 => return Ok(body),
                429 => {
                    return Err(format!("Rate limited (429). Try again later. Response: {}", truncate(&body, 200)));
                }
                403 => {
                    return Err(format!("Forbidden (403). Check API key. Response: {}", truncate(&body, 200)));
                }
                500..=599 if attempt < 2 => {
                    std::thread::sleep(Duration::from_secs(1));
                }
                _ => {
                    return Err(format!("HTTP {} error: {}", status, truncate(&body, 200)));
                }
            }
        }
        Err("Max retries exceeded".to_string())
    }
}

/// Simple URL encoding for query parameters.
fn urlenc(s: &str) -> String {
    let mut result = String::with_capacity(s.len().saturating_mul(2));
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                let _r = write!(result, "%{b:02X}");
            }
        }
    }
    result
}

/// Truncate a string to at most `max` bytes on a char boundary.
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { s.get(..s.floor_char_boundary(max)).unwrap_or("") }
}
