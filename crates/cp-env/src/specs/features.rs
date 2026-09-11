//! Behaviour flags - one spec per [`Feature`], derived from the enum so the
//! table, the model and the JSON exposed to the cockpit cannot disagree.

use crate::model::features::Feature;
use crate::spec::{Fallback, Group, Kind, Scope, Spec};

/// The spec of one flag.
const fn flag(feature: Feature) -> Spec {
    Spec {
        name: feature.env_name(),
        kind: Kind::Bool,
        fallback: Fallback::Literal(feature.default_literal()),
        scope: Scope::Orchestrator,
        group: Group::Features,
        doc: feature.doc(),
        ..Spec::BASE
    }
}

/// Feature flags.
pub(super) static SPECS: &[Spec] = &[
    flag(Feature::ClaudeOauth),
    flag(Feature::Day0Setup),
    flag(Feature::ItPane),
    flag(Feature::Updater),
    flag(Feature::KeysEditable),
    flag(Feature::Onboarding),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The static table must cover every enum variant, or a flag would exist
    /// in the model without being documented or validated.
    #[test]
    fn table_covers_every_feature() {
        assert_eq!(SPECS.len(), Feature::ALL.len());
        for feature in Feature::ALL {
            assert!(SPECS.iter().any(|spec| spec.name == feature.env_name()), "{} missing", feature.env_name());
        }
    }
}
