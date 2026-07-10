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
pub struct Manifest {
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
pub struct ManifestArtifact {
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
        // Parse the realistic fixture.
        let manifest: Manifest = serde_json::from_str(FIXTURE).expect("CI-shaped manifest must parse");
        assert_eq!(manifest.schema, 1);
        assert_eq!(manifest.channel, "stable");
        assert_eq!(manifest.version, "v0.4.0");
        assert_eq!(manifest.artifacts.len(), 2);
        let arm = &manifest.artifacts["linux-aarch64"];
        assert_eq!(arm.size, 12_345_678);
        assert_eq!(arm.sha256.len(), 64, "hex sha256");

        // Round-trip: re-serialising loses nothing (value-level equality).
        let original: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture is valid JSON");
        let round_tripped = serde_json::to_value(&manifest).expect("manifest serialises");
        assert_eq!(round_tripped, original, "round-trip must not lose or alter any field");

        // Every required field is mandatory: dropping any one key must fail.
        let full: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture is valid JSON");
        for key in ["schema", "channel", "version", "released_at", "expires_at", "min_from", "notes_url", "artifacts"]
        {
            let mut pruned = full.clone();
            let _removed = pruned.as_object_mut().expect("object").remove(key);
            let outcome = serde_json::from_value::<Manifest>(pruned);
            assert!(outcome.is_err(), "a manifest missing `{key}` must be rejected");
        }
        // Same for the per-artifact fields.
        for key in ["url", "sha256", "size"] {
            let mut pruned = full.clone();
            let _removed = pruned["artifacts"]["linux-aarch64"].as_object_mut().expect("artifact").remove(key);
            let outcome = serde_json::from_value::<Manifest>(pruned);
            assert!(outcome.is_err(), "an artifact missing `{key}` must be rejected");
        }
    }
}
