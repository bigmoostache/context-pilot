//! Feature flags on the wire — `GET /api/features` and the route gate.
//!
//! The flags themselves come from the validated environment
//! (`CP_FEATURE_*`, see `docs/ENV.md`); this module is where the backend
//! **enforces** them. A disabled surface answers `404` from [`gate`] before any
//! handler runs, so the cockpit hiding a pane is cosmetic and the server is
//! authoritative — the same contract as the RBAC gates.

use cp_env::model::features::{Feature, Features};
use serde_json::{Map, Value};

use super::super::HttpReply;

/// The effective flags of this process.
#[must_use]
pub(crate) fn features() -> Features {
    cp_env::env().features
}

/// `GET /api/features` (public, pre-login): every flag by its JSON key.
pub(crate) fn features_route() -> HttpReply {
    HttpReply::ok(&features_json(features()))
}

/// The JSON object the cockpit reads: `{ "claude_oauth": true, … }`.
#[must_use]
pub(crate) fn features_json(flags: Features) -> Value {
    let mut object = Map::new();
    for (feature, on) in flags.entries() {
        drop(object.insert(feature.key().to_owned(), Value::Bool(on)));
    }
    Value::Object(object)
}

/// The reply a disabled flag forces for `segments`, or `None` when the route
/// is unaffected. Runs first in the router, so a switched-off surface is
/// indistinguishable from a route that does not exist.
#[must_use]
pub(crate) fn gate(segments: &[&str], flags: Features) -> Option<HttpReply> {
    let off = match *segments {
        ["api", "claude-usage" | "claude-login" | "claude-accounts", ..] => !flags.is_on(Feature::ClaudeOauth),
        ["api", "it", ..] => !flags.is_on(Feature::ItPane),
        ["api", "releases" | "update", ..] => !flags.is_on(Feature::Updater),
        _ => false,
    };
    off.then(|| HttpReply::error(404, "not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every gated family answers 404 when its flag is off and passes when on.
    #[test]
    fn gate_follows_each_flag() {
        let cases: [(&[&str], Feature); 5] = [
            (&["api", "claude-login", "start"], Feature::ClaudeOauth),
            (&["api", "claude-accounts"], Feature::ClaudeOauth),
            (&["api", "it", "identity"], Feature::ItPane),
            (&["api", "update", "status"], Feature::Updater),
            (&["api", "releases"], Feature::Updater),
        ];
        for (segments, feature) in cases {
            let off = Features::default().with(feature, false);
            let on = Features::default().with(feature, true);
            assert_eq!(gate(segments, off).map(|reply| reply.status), Some(404), "{segments:?} off");
            assert!(gate(segments, on).is_none(), "{segments:?} on");
        }
    }

    /// Routes outside the gated families are never touched, whatever the flags.
    #[test]
    fn gate_ignores_unrelated_routes() {
        let all_off = Feature::ALL.iter().fold(Features::default(), |acc, feature| acc.with(*feature, false));
        for segments in [&["api", "health"][..], &["api", "features"], &["api", "fleet", "meta"], &["api", "env-keys"]]
        {
            assert!(gate(segments, all_off).is_none(), "{segments:?}");
        }
    }

    /// The JSON keys are exactly the enum's keys, one boolean each.
    #[test]
    fn json_lists_every_flag_by_key() {
        let json = features_json(Features::default().with(Feature::Updater, true));
        let object = json.as_object().expect("object");
        assert_eq!(object.len(), Feature::ALL.len());
        assert_eq!(object.get("updater"), Some(&Value::Bool(true)));
        assert_eq!(object.get("day0_setup"), Some(&Value::Bool(false)));
    }
}
