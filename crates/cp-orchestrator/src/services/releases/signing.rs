//! Release-manifest signing material (update-policy §5.4).
//!
//! The box trusts an update manifest only after verifying its detached
//! minisign signature against this embedded public key. The matching secret
//! key never touches the repo or the box: it lives in the GitHub Actions
//! secret `MINISIGN_SECRET_KEY` (CI signs the manifest at publish time) plus
//! an offline copy on the signing host. Rotation: update-policy §5.4.1.

/// Minisign public key verifying every release manifest (`stable.json.minisig`).
///
/// Generated 2026-07-10 on the signing host — key id `5C445DA7034A99A4`.
/// This is the base64 body of `minisign.pub` (second line of the file); the
/// `minisign-verify` crate parses it with `PublicKey::from_base64`.
pub const UPDATE_PUBKEY: &str = "RWSkmUoDp11EXC/O98y3UWueIh+QohxCLKj5oMmqxRO6EdwegqfjWdnM";

#[cfg(test)]
mod tests {
    use super::UPDATE_PUBKEY;

    /// V0.1c — the embedded public key is well-formed: `minisign-verify`
    /// parses it without error (a corrupt or truncated paste would fail here,
    /// not at update time on a box in the field).
    #[test]
    fn pubkey_parses() {
        let _key = minisign_verify::PublicKey::from_base64(UPDATE_PUBKEY).expect("UPDATE_PUBKEY must parse");
    }
}
