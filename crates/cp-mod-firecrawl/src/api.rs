use reqwest::blocking::Client;
use std::time::Duration;

use crate::types::{MapResponse, ScrapeResponse, SearchResponse};

const FIRECRAWL_BASE_URL: &str = "https://api.firecrawl.dev/v2";
const TIMEOUT_SECS: u64 = 30;

/// Parameters for firecrawl_scrape.
#[derive(Debug)]
pub struct ScrapeParams<'a> {
    pub url: &'a str,
    pub formats: Vec<&'a str>,
    pub country: Option<&'a str>,
    pub languages: Option<Vec<&'a str>>,
}

/// Parameters for firecrawl_search.
#[derive(Debug)]
pub struct SearchParams<'a> {
    pub query: &'a str,
    pub limit: u32,
    pub sources: Vec<&'a str>,
    pub categories: Option<Vec<&'a str>>,
    pub tbs: Option<&'a str>,
    pub location: Option<&'a str>,
}

/// Parameters for firecrawl_map.
#[derive(Debug)]
pub struct MapParams<'a> {
    pub url: &'a str,
    pub limit: u32,
    pub search: Option<&'a str>,
    pub include_subdomains: bool,
    pub country: Option<&'a str>,
    pub languages: Option<Vec<&'a str>>,
}

#[derive(Debug)]
pub struct FirecrawlClient {
    client: Client,
    api_key: String,
}

impl FirecrawlClient {
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .build()
            .expect("failed to build reqwest client");
        Self { client, api_key }
    }

    /// Scrape a single URL for full content extraction.
    pub fn scrape(&self, p: &ScrapeParams<'_>) -> Result<ScrapeResponse, String> {
        let mut body = serde_json::json!({
            "url": p.url,
            "formats": p.formats,
        });

        if let (Some(country), Some(langs)) = (&p.country, &p.languages) {
            body["location"] = serde_json::json!({
                "country": country,
                "languages": langs,
            });
        }

        self.post_json("/scrape", &body)
    }

    /// Search and scrape in one API call.
    pub fn search(&self, p: &SearchParams<'_>) -> Result<SearchResponse, String> {
        let mut body = serde_json::json!({
            "query": p.query,
            "limit": p.limit,
            "scrapeOptions": {
                "formats": ["markdown", "links"],
            },
        });

        if !p.sources.is_empty() {
            body["sources"] = serde_json::json!(p.sources);
        }
        if let Some(ref cats) = p.categories {
            body["categories"] = serde_json::json!(cats);
        }
        if let Some(tbs) = p.tbs {
            body["tbs"] = serde_json::json!(tbs);
        }
        if let Some(loc) = p.location {
            body["location"] = serde_json::json!(loc);
        }

        self.post_json("/search", &body)
    }

    /// Map a domain to discover all URLs.
    pub fn map(&self, p: &MapParams<'_>) -> Result<MapResponse, String> {
        let mut body = serde_json::json!({
            "url": p.url,
            "limit": p.limit,
            "includeSubdomains": p.include_subdomains,
        });

        if let Some(search) = p.search {
            body["search"] = serde_json::json!(search);
        }
        if let (Some(country), Some(langs)) = (&p.country, &p.languages) {
            body["location"] = serde_json::json!({
                "country": country,
                "languages": langs,
            });
        }

        self.post_json("/map", &body)
    }

    /// POST JSON with 5xx retry (2 attempts, 1s delay).
    fn post_json<T: serde::de::DeserializeOwned>(&self, path: &str, body: &serde_json::Value) -> Result<T, String> {
        let url = format!("{}{}", FIRECRAWL_BASE_URL, path);

        for attempt in 0..3 {
            let resp = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(body)
                .send()
                .map_err(|e| format!("Request failed: {}", e))?;

            let status = resp.status().as_u16();
            let resp_body = resp.text().map_err(|e| format!("Failed to read response: {}", e))?;

            match status {
                200..=299 => {
                    return serde_json::from_str(&resp_body).map_err(|e| format!("Failed to parse response: {}", e));
                }
                429 => {
                    return Err(format!("Rate limited (429). Response: {}", truncate(&resp_body, 200)));
                }
                403 => {
                    return Err(format!(
                        "Forbidden (403). Check FIRECRAWL_API_KEY. Response: {}",
                        truncate(&resp_body, 200)
                    ));
                }
                500..=599 if attempt < 2 => {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
                _ => {
                    return Err(format!("HTTP {} error: {}", status, truncate(&resp_body, 200)));
                }
            }
        }
        Err("Max retries exceeded".to_string())
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..s.floor_char_boundary(max)] }
}
