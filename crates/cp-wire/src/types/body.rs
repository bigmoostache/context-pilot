//! Content-addressed body — the immutable payload behind a [`ContentHash`].
//!
//! A [`Body`] pairs a hash with its raw bytes.  Small bodies are inlined
//! directly into oplog entries (zero double-write); large bodies spill to
//! `oplog/bodies/{hash}` under the I13 body-before-reference barrier
//! (design doc §3.1).

use serde::{Deserialize, Serialize};

use super::ContentHash;

/// A content-addressed body: hash + data.
///
/// The hash is the SHA-256 of `data` — the receiver **must** verify this
/// on ingest (the hash is the identity, not a convenience).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Body {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// SHA-256 of `data` (the content address).
    pub hash: ContentHash,

    /// Raw payload bytes, base64-encoded for JSON transport.
    ///
    /// On the binary framed transport this will be raw bytes; for the
    /// JSON/test path serde's default byte-vec encoding suffices.
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_round_trip() {
        let body = Body { schema_version: 1, hash: ContentHash::new([0xff; 32]), data: b"hello world".to_vec() };
        let json = serde_json::to_string(&body).expect("serialize");
        let back: Body = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(body, back);
    }

    #[test]
    fn empty_body_round_trip() {
        let body = Body { schema_version: 1, hash: ContentHash::new([0x00; 32]), data: vec![] };
        let json = serde_json::to_string(&body).expect("serialize");
        let back: Body = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(body, back);
    }
}
