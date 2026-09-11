//! The table - every environment variable either binary knows about.
//!
//! One file per documentation group. Add a variable here and nowhere else:
//! validation, the typed model and `docs/ENV.md` follow.

use std::sync::LazyLock;

use crate::spec::Spec;

mod appliance;
mod auth_seed;
mod features;
mod gateway_bridge;
mod orchestrator;
pub mod secrets;
mod shared;

/// Every spec, in docs order.
static ALL: LazyLock<Vec<Spec>> = LazyLock::new(|| {
    [
        shared::CORE,
        orchestrator::SPECS,
        auth_seed::SPECS,
        gateway_bridge::SPECS,
        appliance::SPECS,
        features::SPECS,
        shared::DEV,
        secrets::SECRETS.as_slice(),
        secrets::EXTERNAL,
    ]
    .concat()
});

/// Every spec, in docs order.
#[must_use]
pub fn all() -> &'static [Spec] {
    &ALL
}

/// The spec named `name`, if any.
#[must_use]
pub fn find(name: &str) -> Option<&'static Spec> {
    ALL.iter().find(|spec| spec.name == name)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// Two entries with the same name would make `find` ambiguous.
    #[test]
    fn names_are_unique() {
        let mut seen = BTreeSet::new();
        for spec in all() {
            assert!(seen.insert(spec.name), "duplicate spec: {}", spec.name);
        }
    }

    /// Every entry carries a description - the docs table is generated from it.
    #[test]
    fn every_spec_is_documented() {
        for spec in all() {
            assert!(!spec.name.is_empty(), "unnamed spec");
            assert!(!spec.doc.is_empty(), "{} has no doc", spec.name);
        }
    }

    /// A required variable with a literal default could never be missing -
    /// the two flags contradict each other.
    #[test]
    fn required_never_has_literal_default() {
        for spec in all() {
            let contradictory = spec.required && matches!(spec.fallback, crate::spec::Fallback::Literal(_));
            assert!(!contradictory, "{} is required and defaulted", spec.name);
        }
    }
}
