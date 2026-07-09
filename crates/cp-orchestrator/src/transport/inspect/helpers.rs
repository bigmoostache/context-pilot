//! Shared helpers for inspection-panel endpoints.

use std::sync::Mutex;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

/// Resolve the agent's working directory from the registry record.
pub(super) fn agent_folder(state: &Mutex<Backend>, agent_id: &str) -> Result<String, HttpReply> {
    let entry = crate::transport::rest::resolve_entry(state, agent_id)?;
    Ok(entry.folder)
}

/// Extract the `worker` query parameter from a raw query string.
pub(super) fn extract_worker_param(query: &str) -> Option<String> {
    query.split('&').filter(|s| !s.is_empty()).find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k == "worker" { Some(v.to_owned()) } else { None }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_worker_param_finds_value() {
        assert_eq!(extract_worker_param("worker=abc123"), Some("abc123".to_owned()));
        assert_eq!(extract_worker_param("agent=x&worker=def"), Some("def".to_owned()));
        assert_eq!(extract_worker_param("agent=x"), None);
        assert_eq!(extract_worker_param(""), None);
    }
}
