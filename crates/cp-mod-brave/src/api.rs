use reqwest::blocking::Client;
use std::time::Duration;

use crate::types::{BraveSearchResponse, LLMContextResponse, RichCallbackResponse};

const BRAVE_BASE_URL: &str = "https://api.search.brave.com/res/v1";
const TIMEOUT_SECS: u64 = 10;

/// Parameters for a Brave web search request.
#[derive(Debug)]
pub struct SearchParams<'a> {
    /// Search query string.
    pub query: &'a str,
    /// Number of results to return (1-20).
    pub count: u32,
    /// Recency filter (e.g., "pd", "pw", "pm", "py", or date range).
    pub freshness: Option<&'a str>,
    /// Two-letter ISO country code.
    pub country: &'a str,
    /// Result language ISO 639-1 code.
    pub search_lang: &'a str,
    /// Safe search level: "off", "moderate", or "strict".
    pub safe_search: &'a str,
    /// Brave Goggle URL for domain re-ranking.
    pub goggles_id: Option<&'a str>,
}

/// Parameters for a Brave LLM context request.
#[derive(Debug)]
pub struct LLMContextParams<'a> {
    /// Search query string.
    pub query: &'a str,
    /// Approximate max tokens in response (1024-32768).
    pub max_tokens: u32,
    /// Max search results to consider (1-50).
    pub count: u32,
    /// Relevance threshold: "strict", "balanced", "lenient", or "disabled".
    pub threshold_mode: &'a str,
    /// Recency filter.
    pub freshness: Option<&'a str>,
    /// Two-letter ISO country code.
    pub country: &'a str,
    /// Brave Goggle URL or inline definition.
    pub goggles: Option<&'a str>,
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
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    #[must_use]
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .build()
            .expect("failed to build reqwest client");
        Self { client, api_key }
    }

    /// Search the web via Brave Search API.
    /// Always sends `extra_snippets=true` and `enable_rich_callback=1`.
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failure, non-2xx HTTP status, or JSON parse error.
    pub fn search(&self, p: &SearchParams<'_>) -> Result<(BraveSearchResponse, Option<serde_json::Value>), String> {
        let mut url = format!("{}/web/search?q={}", BRAVE_BASE_URL, urlenc(p.query));
        url.push_str(&format!("&count={}", p.count));
        url.push_str("&extra_snippets=true");
        url.push_str("&enable_rich_callback=1");
        url.push_str(&format!("&country={}", urlenc(p.country)));
        url.push_str(&format!("&search_lang={}", urlenc(p.search_lang)));
        url.push_str(&format!("&safesearch={}", urlenc(p.safe_search)));

        if let Some(f) = p.freshness {
            url.push_str(&format!("&freshness={}", urlenc(f)));
        }
        if let Some(g) = p.goggles_id {
            url.push_str(&format!("&goggles_id={}", urlenc(g)));
        }

        let response = self.get_with_retry(&url)?;
        let search_resp: BraveSearchResponse =
            serde_json::from_str(&response).map_err(|e| format!("Failed to parse search response: {}", e))?;

        // Auto-fetch rich results if callback_key present
        let rich_data = if let Some(ref rich) = search_resp.rich {
            if let Some(ref hint) = rich.hint {
                if let Some(ref key) = hint.callback_key { self.fetch_rich_callback(key).ok() } else { None }
            } else {
                None
            }
        } else {
            None
        };

        Ok((search_resp, rich_data))
    }

    /// Get LLM-optimized context from Brave LLM Context API.
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failure, non-2xx HTTP status, or JSON parse error.
    pub fn llm_context(&self, p: &LLMContextParams<'_>) -> Result<LLMContextResponse, String> {
        let mut url = format!("{}/llm/context?q={}", BRAVE_BASE_URL, urlenc(p.query));
        url.push_str(&format!("&maximum_number_of_tokens={}", p.max_tokens));
        url.push_str(&format!("&count={}", p.count));
        url.push_str(&format!("&context_threshold_mode={}", urlenc(p.threshold_mode)));
        url.push_str(&format!("&country={}", urlenc(p.country)));
        // Hardcoded optimal defaults
        url.push_str("&maximum_number_of_urls=20");
        url.push_str("&maximum_number_of_snippets=50");
        url.push_str("&maximum_number_of_tokens_per_url=4096");

        if let Some(f) = p.freshness {
            url.push_str(&format!("&freshness={}", urlenc(f)));
        }
        if let Some(g) = p.goggles {
            url.push_str(&format!("&goggles={}", urlenc(g)));
        }

        let response = self.get_with_retry(&url)?;
        serde_json::from_str(&response).map_err(|e| format!("Failed to parse LLM context response: {}", e))
    }

    /// Fetch rich results via callback key.
    fn fetch_rich_callback(&self, callback_key: &str) -> Result<serde_json::Value, String> {
        let url = format!("{}/web/rich?callback_key={}", BRAVE_BASE_URL, urlenc(callback_key));
        let response = self.get_with_retry(&url)?;
        let rich: RichCallbackResponse =
            serde_json::from_str(&response).map_err(|e| format!("Failed to parse rich response: {}", e))?;
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
                .map_err(|e| format!("Request failed: {}", e))?;

            let status = resp.status().as_u16();
            let body = resp.text().map_err(|e| format!("Failed to read response: {}", e))?;

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
    let mut result = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..s.floor_char_boundary(max)] }
}
