//! Search request-body construction, split out of `api.rs` for the 500-line cap.
//!
//! Builds the JSON body shared by single-index `search` and `multi_search`,
//! folding in optional filter / sort / hybrid-semantic clauses.

use super::api::SearchParams;

/// Build the JSON body for a single search query.
///
/// Shared by [`super::api::MeiliClient::search`] and `multi_search`. The
/// `hybrid` block is only added when `semantic_ratio` is `Some`, which flips
/// the query from pure-keyword to blended keyword+vector ranking.
pub(super) fn build_search_body(params: &SearchParams<'_>) -> serde_json::Value {
    let mut body = serde_json::json!({
        "q": params.query,
        "limit": params.limit,
        "showRankingScore": true,
        "showMatchesPosition": false,
    });

    if let Some(f) = params.filter
        && let Some(obj) = body.as_object_mut()
    {
        let _prev = obj.insert("filter".to_owned(), serde_json::Value::String(f.to_owned()));
    }
    if let Some(s) = params.sort
        && let Some(obj) = body.as_object_mut()
    {
        let _prev =
            obj.insert("sort".to_owned(), serde_json::Value::Array(vec![serde_json::Value::String(s.to_owned())]));
    }
    if let Some(ratio) = params.semantic_ratio
        && let Some(obj) = body.as_object_mut()
    {
        let _prev = obj.insert(
            "hybrid".to_owned(),
            serde_json::json!({
                "semanticRatio": ratio,
                "embedder": "default"
            }),
        );
    }

    body
}
