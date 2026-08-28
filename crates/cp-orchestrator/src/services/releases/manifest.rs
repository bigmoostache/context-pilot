//! Signed update-manifest schema (update-policy §5.3) — the Rust mirror of the
//! `stable.json` that CI generates and signs on every `v*` tag.
//!
//! This is the frozen contract between the publish side (the `manifest` job in
//! `.github/workflows/release.yml`, which builds the JSON with `jq`) and the
//! on-box updater (M3), which deserialises it after verifying the minisign
//! signature. Every field is required — a manifest missing one is rejected at
//! parse time, before any of its content is believed. Unknown fields are
//! tolerated (not lost silently by the publish side — CI emits exactly this
//! shape — but a *future* additive field must not brick a fleet of old boxes).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One channel's signed desired-state: "channel X is on version Y".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Manifest {
    /// Manifest format version — bump on breaking shape changes.
    pub schema: u32,
    /// Channel name this manifest governs (e.g. `"stable"`).
    pub channel: String,
    /// The release tag the fleet should converge on (e.g. `"v0.4.0"`).
    pub version: String,
    /// ISO-8601 publication instant (set by CI at signing time).
    pub released_at: String,
    /// Freshness horizon (§5.6): the box rejects the manifest past this
    /// instant — a stale signed manifest cannot be replayed forever.
    pub expires_at: String,
    /// Anti-rollback floor (§5.6): a box running a version older than this
    /// must not jump directly to `version` (migration/protocol safety).
    pub min_from: String,
    /// Human-readable release notes (the GitHub release page).
    pub notes_url: String,
    /// Per-architecture artifact pins, keyed by arch string
    /// (e.g. `"linux-aarch64"`) — the signature covers these hashes, so trust
    /// extends from the manifest to the tarball bits.
    pub artifacts: BTreeMap<String, ManifestArtifact>,
}

/// One architecture's pinned release artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ManifestArtifact {
    /// Download URL of the release tarball.
    pub url: String,
    /// Hex SHA-256 of the tarball — verified on the box before extraction.
    pub sha256: String,
    /// Tarball size in bytes (progress display + sanity bound).
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte mirror of what the CI `manifest` job emits (same fields,
    /// same shapes) — regenerate by running the `Generate stable.json` step
    /// locally if the workflow's `jq` template ever changes.
    const FIXTURE: &str = r#"{
      "schema": 1,
      "channel": "stable",
      "version": "v0.4.0",
      "released_at": "2026-07-10T03:00:00Z",
      "expires_at": "2026-10-08T03:00:00Z",
      "min_from": "v0.1.0",
      "notes_url": "https://github.com/bigmoostache/context-pilot/releases/tag/v0.4.0",
      "artifacts": {
        "linux-aarch64": {
          "url": "https://github.com/bigmoostache/context-pilot/releases/download/v0.4.0/cpilot-linux-aarch64.tar.gz",
          "sha256": "3f2acf2cbd0d571d029ad9de4b30a38b53e36741e0a5f19b95ec6e51a4bf3a49",
          "size": 12345678
        },
        "linux-x86_64": {
          "url": "https://github.com/bigmoostache/context-pilot/releases/download/v0.4.0/cpilot-linux-x86_64.tar.gz",
          "sha256": "9b74c9897bac770ffc029102a200c5de1a55d43ee0ce9e0fd0fbcbbcdca1cf89",
          "size": 12000000
        }
      }
    }"#;

    /// V1.2a — the CI-shaped fixture deserialises into [`Manifest`] and
    /// serialises back to the exact same JSON value (no field lost in either
    /// direction), and a manifest missing a required field is rejected.
    #[test]
    fn manifest_schema() {
        // Parse the realistic fixture and check its fields + round-trip.
        let manifest: Manifest = serde_json::from_str(FIXTURE).expect("CI-shaped manifest must parse");
        assert_fields_and_roundtrip(&manifest);

        // Every required field — top-level and per-artifact — is mandatory.
        let full: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture is valid JSON");
        assert_required_fields_rejected(&full);
    }

    /// Drop each required key in turn (top-level, then per-artifact) and assert
    /// the pruned value no longer deserialises. Both loops live here so the test
    /// body itself carries no branching and stays under the cognitive-complexity
    /// cap.
    fn assert_required_fields_rejected(full: &serde_json::Value) {
        for key in ["schema", "channel", "version", "released_at", "expires_at", "min_from", "notes_url", "artifacts"] {
            assert_missing_top_level_key_rejected(full, key);
        }
        for key in ["url", "sha256", "size"] {
            assert_missing_artifact_key_rejected(full, key);
        }
    }

    /// Assert the parsed manifest's fields and that it re-serialises to the exact
    /// same JSON value — hoisted out of the test to keep it under the
    /// cognitive-complexity cap.
    fn assert_fields_and_roundtrip(manifest: &Manifest) {
        let arm = manifest.artifacts.get("linux-aarch64").expect("aarch64 artifact present");
        // One grouped tuple comparison rather than six separate assert_eq!
        // expansions, keeping this helper under the cognitive-complexity cap.
        assert_eq!(
            (
                manifest.schema,
                manifest.channel.as_str(),
                manifest.version.as_str(),
                manifest.artifacts.len(),
                arm.size,
                arm.sha256.len()
            ),
            (1, "stable", "v0.4.0", 2, 12_345_678, 64),
            "parsed manifest fields (incl. the aarch64 artifact size + hex sha256 length)"
        );

        // Round-trip: re-serialising loses nothing (value-level equality).
        let original: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture is valid JSON");
        let round_tripped = serde_json::to_value(manifest).expect("manifest serialises");
        assert_eq!(round_tripped, original, "round-trip must not lose or alter any field");
    }

    /// Prune one top-level `key` from a valid manifest value and assert the
    /// result no longer deserialises — hoisted out of the test to keep it under
    /// the cognitive-complexity cap.
    fn assert_missing_top_level_key_rejected(full: &serde_json::Value, key: &str) {
        let mut pruned = full.clone();
        let _removed = pruned.as_object_mut().expect("object").remove(key);
        assert!(serde_json::from_value::<Manifest>(pruned).is_err(), "a manifest missing `{key}` must be rejected");
    }

    /// Prune one per-artifact `key` from a valid manifest value and assert the
    /// result no longer deserialises.
    fn assert_missing_artifact_key_rejected(full: &serde_json::Value, key: &str) {
        let mut pruned = full.clone();
        let _removed = pruned
            .get_mut("artifacts")
            .and_then(|artifacts| artifacts.get_mut("linux-aarch64"))
            .and_then(serde_json::Value::as_object_mut)
            .expect("artifact object")
            .remove(key);
        assert!(serde_json::from_value::<Manifest>(pruned).is_err(), "an artifact missing `{key}` must be rejected");
    }
}
