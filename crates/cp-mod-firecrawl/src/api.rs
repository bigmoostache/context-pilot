use reqwest::Method;
use std::time::{Duration, Instant};

use crate::http::{Call, Policy, Transport};

use crate::types::{CrawlStartResponse, CrawlStatusResponse, MapResponse, ScrapeResponse, SearchResponse};

/// Base URL for the Firecrawl v2 REST API.
const FIRECRAWL_BASE_URL: &str = "https://api.firecrawl.dev/v2";

/// Parameters for `firecrawl_scrape`.
#[derive(Debug)]
pub struct ScrapeParams<'req> {
    /// Target URL to scrape.
    pub url: &'req str,
    /// Output formats: "markdown", "html", "links", "summary", "images".
    pub formats: Vec<&'req str>,
    /// ISO country code for geo-targeted scraping.
    pub country: Option<&'req str>,
    /// Preferred response languages.
    pub languages: Option<Vec<&'req str>>,
}

/// Parameters for `firecrawl_search`.
#[derive(Debug)]
pub struct SearchParams<'req> {
    /// Search query string.
    pub query: &'req str,
    /// Max pages to scrape (1-10).
    pub limit: u32,
    /// Source types: "web", "news", "images".
    pub sources: Vec<&'req str>,
    /// Target categories: "github", "research", "pdf".
    pub categories: Option<Vec<&'req str>>,
    /// Time filter: "qdr:h", "qdr:d", "qdr:w", "qdr:m", "qdr:y".
    pub tbs: Option<&'req str>,
    /// Location string (e.g., "Germany", "San Francisco,California").
    pub location: Option<&'req str>,
}

/// Parameters for `firecrawl_map`.
#[derive(Debug)]
pub struct MapParams<'req> {
    /// Root domain or subdomain to map.
    pub url: &'req str,
    /// Max URLs to return (1-5000).
    pub limit: u32,
    /// Optional keyword filter on discovered URLs.
    pub search: Option<&'req str>,
    /// Whether to include subdomains.
    pub include_subdomains: bool,
    /// ISO country code for geo-targeted mapping.
    pub country: Option<&'req str>,
    /// Preferred response languages.
    pub languages: Option<Vec<&'req str>>,
}

/// Parameters for `firecrawl_crawl`.
#[derive(Debug)]
pub struct CrawlParams<'req> {
    /// Starting URL to crawl from.
    pub url: &'req str,
    /// Max pages to crawl (1-100).
    pub limit: u32,
    /// Max link-following depth.
    pub max_depth: Option<u32>,
    /// Regex patterns — only crawl matching paths.
    pub include_paths: Option<Vec<&'req str>>,
    /// Regex patterns — skip matching paths.
    pub exclude_paths: Option<Vec<&'req str>>,
    /// Whether to follow links to subdomains.
    pub allow_subdomains: bool,
}

/// HTTP client for the Firecrawl API (v2).
#[derive(Debug)]
pub struct FirecrawlClient {
    /// Retrying, deadline-aware HTTP transport.
    http: Transport,
}

/// Timeout of the best-effort `DELETE` that cancels an abandoned crawl.
const CANCEL_TIMEOUT: Duration = Duration::from_secs(10);

/// `location` body object from whichever of `country` / `languages` is set;
/// `None` when neither is.
pub(crate) fn location_json(country: Option<&str>, languages: Option<&[&str]>) -> Option<serde_json::Value> {
    let mut loc = serde_json::Map::new();
    if let Some(c) = country {
        drop(loc.insert("country".into(), serde_json::json!(c)));
    }
    if let Some(langs) = languages.filter(|l| !l.is_empty()) {
        drop(loc.insert("languages".into(), serde_json::json!(langs)));
    }
    (!loc.is_empty()).then_some(serde_json::Value::Object(loc))
}

/// Insert `key: value` into a JSON object body.
fn put(body: &mut serde_json::Value, key: &str, value: serde_json::Value) {
    if let Some(obj) = body.as_object_mut() {
        drop(obj.insert(key.into(), value));
    }
}

impl FirecrawlClient {
    /// Create a client against the public Firecrawl v2 API.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the HTTP client fails to build.
    pub fn new(api_key: String) -> Result<Self, String> {
        Self::with_base_url(api_key, FIRECRAWL_BASE_URL)
    }

    /// Create a client against another API root (tests, self-hosted Firecrawl).
    ///
    /// # Errors
    ///
    /// Returns `Err` if the HTTP client fails to build.
    pub fn with_base_url(api_key: String, base_url: &str) -> Result<Self, String> {
        Ok(Self { http: Transport::new(api_key, base_url)? })
    }

    /// Bound every later request (retries and waits included) by `deadline`.
    pub const fn set_deadline(&mut self, deadline: Instant) {
        self.http.set_deadline(deadline);
    }

    /// Scrape a single URL for full content extraction. Billed: never retried
    /// once the server may have acted on it.
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failure, non-2xx HTTP status, or JSON parse error.
    pub fn scrape(&self, p: &ScrapeParams<'_>) -> Result<ScrapeResponse, String> {
        let mut body = serde_json::json!({ "url": p.url, "formats": p.formats });
        if let Some(loc) = location_json(p.country, p.languages.as_deref()) {
            put(&mut body, "location", loc);
        }
        self.post("/scrape", &body)
    }

    /// Search and scrape in one API call. Billed: never retried once the server
    /// may have acted on it.
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failure, non-2xx HTTP status, or JSON parse error.
    pub fn search(&self, p: &SearchParams<'_>) -> Result<SearchResponse, String> {
        let mut body = serde_json::json!({
            "query": p.query,
            "limit": p.limit,
            "scrapeOptions": { "formats": ["markdown", "links"] },
        });
        if !p.sources.is_empty() {
            put(&mut body, "sources", serde_json::json!(p.sources));
        }
        if let Some(cats) = p.categories.as_ref() {
            put(&mut body, "categories", serde_json::json!(cats));
        }
        if let Some(tbs) = p.tbs {
            put(&mut body, "tbs", serde_json::json!(tbs));
        }
        if let Some(loc) = p.location {
            put(&mut body, "location", serde_json::json!(loc));
        }
        self.post("/search", &body)
    }

    /// Map a domain to discover all URLs.
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failure, non-2xx HTTP status, or JSON parse error.
    pub fn map(&self, p: &MapParams<'_>) -> Result<MapResponse, String> {
        let mut body = serde_json::json!({
            "url": p.url,
            "limit": p.limit,
            "includeSubdomains": p.include_subdomains,
        });
        if let Some(search) = p.search {
            put(&mut body, "search", serde_json::json!(search));
        }
        if let Some(loc) = location_json(p.country, p.languages.as_deref()) {
            put(&mut body, "location", loc);
        }
        self.post("/map", &body)
    }

    /// Start a recursive crawl job. Returns a job ID for polling. A retry after
    /// the server may have accepted the job would start (and bill) a second one,
    /// so only refused requests are retried.
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failure, non-2xx HTTP status, or JSON parse error.
    pub fn start_crawl(&self, p: &CrawlParams<'_>) -> Result<CrawlStartResponse, String> {
        let mut body = serde_json::json!({
            "url": p.url,
            "limit": p.limit,
            "allowSubdomains": p.allow_subdomains,
            "scrapeOptions": { "formats": ["markdown"] },
        });
        if let Some(depth) = p.max_depth {
            put(&mut body, "maxDepth", serde_json::json!(depth));
        }
        if let Some(paths) = p.include_paths.as_ref() {
            put(&mut body, "includePaths", serde_json::json!(paths));
        }
        if let Some(paths) = p.exclude_paths.as_ref() {
            put(&mut body, "excludePaths", serde_json::json!(paths));
        }
        self.post("/crawl", &body)
    }

    /// Poll crawl job status by its ID (first page of results).
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failure, non-2xx HTTP status, or JSON parse error.
    pub fn poll_crawl(&self, job_id: &str) -> Result<CrawlStatusResponse, String> {
        let url = self.http.url(&format!("/crawl/{job_id}"));
        self.http.send(&Call { method: Method::GET, url: &url, body: None }, Policy::Idempotent)
    }

    /// Fetch a further result page from a status response's `next` URL.
    ///
    /// # Errors
    ///
    /// `next` points outside the API root (the bearer token is never sent
    /// elsewhere), or the request fails.
    pub fn crawl_page(&self, next: &str) -> Result<CrawlStatusResponse, String> {
        if !self.http.owns(next) {
            return Err(format!("refusing to follow pagination URL outside the Firecrawl API: {next}"));
        }
        self.http.send(&Call { method: Method::GET, url: next, body: None }, Policy::Idempotent)
    }

    /// Cancel a running crawl job so it stops consuming credits. One attempt,
    /// with its own timeout: it runs after the operation's deadline is spent.
    ///
    /// # Errors
    ///
    /// Network failure or non-2xx status.
    pub fn cancel_crawl(&self, job_id: &str) -> Result<(), String> {
        let url = self.http.url(&format!("/crawl/{job_id}"));
        self.http.send_once(&Call { method: Method::DELETE, url: &url, body: None }, CANCEL_TIMEOUT)
    }

    /// Billed `POST`: retried only when the server certainly did not act.
    fn post<T>(&self, path: &str, body: &serde_json::Value) -> Result<T, String>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = self.http.url(path);
        self.http.send(&Call { method: Method::POST, url: &url, body: Some(body) }, Policy::Billable)
    }
}
