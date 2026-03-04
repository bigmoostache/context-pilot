use serde::{Deserialize, Serialize};

// ─── Brave Search API Response Types ───

/// Top-level search response from Brave Web Search API.
#[derive(Debug, Deserialize, Serialize)]
pub struct BraveSearchResponse {
    /// Response type identifier.
    #[serde(rename = "type")]
    pub response_type: Option<String>,
    /// Query info (original query text, etc.).
    pub query: Option<QueryInfo>,
    /// Organic web results.
    pub web: Option<WebResults>,
    /// Rich results (knowledge graph, calculator, etc.).
    pub rich: Option<RichResults>,
}

/// Query metadata returned by the API.
#[derive(Debug, Deserialize, Serialize)]
pub struct QueryInfo {
    /// Original query string as submitted.
    pub original: Option<String>,
}

/// Container for organic web search results.
#[derive(Debug, Deserialize, Serialize)]
pub struct WebResults {
    /// List of individual web results.
    pub results: Vec<WebResult>,
}

/// A single organic web search result.
#[derive(Debug, Deserialize, Serialize)]
pub struct WebResult {
    /// Page title.
    pub title: Option<String>,
    /// Full URL of the result.
    pub url: Option<String>,
    /// Snippet/description text.
    pub description: Option<String>,
    /// Additional contextual snippets (if `extra_snippets=true`).
    pub extra_snippets: Option<Vec<String>>,
    /// Human-readable age string (e.g., "2 hours ago").
    pub age: Option<String>,
}

/// Rich results section (knowledge panels, calculators, etc.).
#[derive(Debug, Deserialize, Serialize)]
pub struct RichResults {
    /// Hint for fetching rich callback data.
    pub hint: Option<RichHint>,
}

/// Callback key for fetching rich result details.
#[derive(Debug, Deserialize, Serialize)]
pub struct RichHint {
    /// Opaque key passed to the `/web/rich` endpoint.
    pub callback_key: Option<String>,
}

/// Rich callback response — flexible JSON since it varies by type
#[derive(Debug, Deserialize, Serialize)]
pub struct RichCallbackResponse {
    /// Unstructured rich data (stocks, weather, calculator, crypto, sports, etc.).
    #[serde(flatten)]
    pub data: serde_json::Value,
}

// ─── Brave LLM Context API Response Types ───

/// Response from Brave's LLM Context API (pre-extracted web content for LLMs).
#[derive(Debug, Deserialize, Serialize)]
pub struct LLMContextResponse {
    /// Grounding data: relevance-scored text chunks from top results.
    pub grounding: Option<Grounding>,
    /// Source metadata (URLs, titles).
    pub sources: Option<serde_json::Value>,
}

/// Grounding section containing extracted content items.
#[derive(Debug, Deserialize, Serialize)]
pub struct Grounding {
    /// Generic grounding items (paragraphs, tables, code blocks).
    pub generic: Option<Vec<GroundingItem>>,
}

/// A single grounding item: extracted content from one source page.
#[derive(Debug, Deserialize, Serialize)]
pub struct GroundingItem {
    /// Source page URL.
    pub url: Option<String>,
    /// Source page title.
    pub title: Option<String>,
    /// Extracted text snippets (may be strings or structured objects).
    pub snippets: Option<Vec<serde_json::Value>>,
}
