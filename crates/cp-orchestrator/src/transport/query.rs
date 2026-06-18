//! URL query-string parser.

/// A parsed query string (`k=v&k2=v2`).
pub(super) struct QueryParams {
    /// Decoded key/value pairs.
    pairs: Vec<(String, String)>,
}

impl QueryParams {
    /// Parse a raw query string. Values are taken verbatim (no percent-decode;
    /// ticket tokens and agent ids are hex/identifier-safe).
    pub(super) fn parse(query: &str) -> Self {
        let pairs = query
            .split('&')
            .filter(|s| !s.is_empty())
            .map(|pair| match pair.split_once('=') {
                Some((k, v)) => (k.to_owned(), v.to_owned()),
                None => (pair.to_owned(), String::new()),
            })
            .collect();
        Self { pairs }
    }

    /// Look up the first value for `key`.
    pub(super) fn get(&self, key: &str) -> Option<&str> {
        self.pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_params_parse_and_lookup() {
        let q = QueryParams::parse("agent=a1&ticket=deadbeef&last_rev=5");
        assert_eq!(q.get("agent"), Some("a1"));
        assert_eq!(q.get("ticket"), Some("deadbeef"));
        assert_eq!(q.get("last_rev"), Some("5"));
        assert_eq!(q.get("missing"), None);
    }

    #[test]
    fn query_params_handle_empty() {
        let q = QueryParams::parse("");
        assert_eq!(q.get("agent"), None);
    }
}
