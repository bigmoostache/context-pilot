use super::{Failure, LLMContextParams, SearchParams, classify_status, llm_context_url, search_url, truncate};

/// Look up a query parameter by name.
fn param(url: &reqwest::Url, key: &str) -> Option<String> {
    url.query_pairs().find(|pair| pair.0 == key).map(|pair| pair.1.into_owned())
}

#[test]
fn search_url_encodes_reserved_and_unicode() {
    let p = SearchParams {
        query: "a&b=c + \u{e9} \"x\"",
        count: 5,
        freshness: Some("2024-01-01to2024-02-01"),
        country: "FR",
        search_lang: "fr",
        safe_search: "off",
        goggles_id: None,
    };
    let url = search_url(&p).expect("valid url");
    assert_eq!(param(&url, "q").as_deref(), Some("a&b=c + \u{e9} \"x\""));
    assert_eq!(param(&url, "count").as_deref(), Some("5"));
    assert_eq!(param(&url, "freshness").as_deref(), Some("2024-01-01to2024-02-01"));
    assert!(param(&url, "goggles_id").is_none());
}

#[test]
fn llm_context_urls_follow_count() {
    let p = LLMContextParams {
        query: "rust",
        max_tokens: 2048,
        count: 50,
        threshold_mode: "strict",
        freshness: None,
        country: "US",
        goggles: Some("https://g.example/x?y=1"),
    };
    let url = llm_context_url(&p).expect("valid url");
    assert_eq!(param(&url, "maximum_number_of_urls").as_deref(), Some("50"));
    assert_eq!(param(&url, "goggles").as_deref(), Some("https://g.example/x?y=1"));
}

#[test]
fn status_classification_retryable() {
    assert_eq!(classify_status(200, ""), Ok(()));
    assert!(matches!(classify_status(503, ""), Err(Failure::Transient(_))));
}

#[test]
fn status_classification_fatal() {
    assert!(matches!(classify_status(429, ""), Err(Failure::Fatal(_))));
    assert!(matches!(classify_status(404, ""), Err(Failure::Fatal(_))));
    let Err(Failure::Fatal(msg)) = classify_status(422, "") else { panic!("422 must be fatal") };
    assert!(msg.contains("Invalid parameters"));
}

#[test]
fn truncate_respects_char_boundary() {
    assert_eq!(truncate("h\u{e9}llo", 2), "h");
    assert_eq!(truncate("abc", 10), "abc");
}

#[test]
fn budget_fits_async_timeout() {
    assert!(super::TOTAL_BUDGET.as_secs() < crate::tools::ASYNC_TIMEOUT_SECS);
}
