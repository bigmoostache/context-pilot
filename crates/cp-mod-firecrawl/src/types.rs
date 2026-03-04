use serde::{Deserialize, Serialize};

/// Firecrawl scrape API response.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScrapeResponse {
    /// Whether the scrape request succeeded.
    pub success: bool,
    /// Extracted page data (markdown, HTML, links, metadata).
    pub data: Option<ScrapeData>,
    /// Error message on failure.
    pub error: Option<String>,
}

/// Extracted content from a scraped page.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScrapeData {
    /// Page content as clean Markdown.
    pub markdown: Option<String>,
    /// Raw HTML content.
    pub html: Option<String>,
    /// All links found on the page.
    pub links: Option<Vec<String>>,
    /// Page metadata (title, description, source URL).
    pub metadata: Option<ScrapeMetadata>,
}

/// Metadata extracted from a scraped page.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScrapeMetadata {
    /// HTML `<title>` content.
    pub title: Option<String>,
    /// Meta description tag content.
    pub description: Option<String>,
    /// Detected page language.
    pub language: Option<String>,
    /// Original URL that was scraped.
    #[serde(rename = "sourceURL")]
    pub source_url: Option<String>,
    /// HTTP status code returned by the page.
    #[serde(rename = "statusCode")]
    pub status_code: Option<u16>,
}

/// Firecrawl search API response.
///
/// The `data` field can be either:
/// - A list of `SearchResult` (when scrapeOptions produce full results)
/// - An object like `{"web": [...], "images": [...]}` (when results aren't scraped)
///
/// We use `serde_json::Value` and parse manually.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    /// Whether the search request succeeded.
    pub success: bool,
    /// Search results (polymorphic — see type docs).
    pub data: Option<serde_json::Value>,
    /// Error message on failure.
    pub error: Option<String>,
}

/// A single search result with full scraped content.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    /// Page URL.
    pub url: Option<String>,
    /// Page title.
    pub title: Option<String>,
    /// Snippet/description.
    pub description: Option<String>,
    /// Full page content as Markdown.
    pub markdown: Option<String>,
    /// All links found on the page.
    pub links: Option<Vec<String>>,
    /// Page metadata.
    pub metadata: Option<ScrapeMetadata>,
}

/// Firecrawl map API response.
#[derive(Debug, Serialize, Deserialize)]
pub struct MapResponse {
    /// Whether the map request succeeded.
    pub success: bool,
    /// Discovered URLs on the domain.
    pub links: Option<Vec<MapLink>>,
    /// Error message on failure.
    pub error: Option<String>,
}

/// A link discovered during domain mapping.
/// Title and description may not always be present.
#[derive(Debug, Serialize, Deserialize)]
pub struct MapLink {
    /// Full URL.
    pub url: Option<String>,
    /// Page title (from sitemap or crawl).
    pub title: Option<String>,
    /// Page description (from sitemap metadata).
    pub description: Option<String>,
}
