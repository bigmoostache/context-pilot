//! Bearer comparison shared by the command and query paths (design doc I9).
//!
//! The `cap_token` is a fixed-length 256-bit hex secret. A plain `==` on
//! strings returns at the first differing byte, so its running time leaks how
//! long the matching prefix is. [`bearer_matches`] instead visits every byte of
//! the expected token whatever the input, so the time taken depends only on the
//! lengths, never on where the first mismatch is.

/// Whether `presented` is the agent's bearer `expected`, compared in constant
/// time over `expected`'s length.
///
/// An empty `presented` is always rejected, even against an empty `expected`,
/// so a misconfigured agent with no token never accepts an anonymous caller.
/// A length mismatch is rejected too, but only after the full scan, so the
/// length check adds no early exit of its own.
#[must_use]
pub fn bearer_matches(presented: &str, expected: &str) -> bool {
    let given = presented.as_bytes();
    let wanted = expected.as_bytes();
    let mut diff = u8::from(given.len() != wanted.len() || given.is_empty());
    for (idx, want) in wanted.iter().enumerate() {
        // Past the end of a short input, compare against 0 so the loop still
        // runs over every byte of `expected`.
        let got = given.get(idx).copied().unwrap_or(0);
        diff |= got ^ want;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::bearer_matches;

    const TOKEN: &str = "cap-token-256bit";

    #[test]
    fn exact_token_matches() {
        assert!(bearer_matches(TOKEN, TOKEN));
    }

    #[test]
    fn differing_token_is_rejected() {
        assert!(!bearer_matches("cap-token-256biT", TOKEN), "last byte differs");
        assert!(!bearer_matches("Xap-token-256bit", TOKEN), "first byte differs");
    }

    #[test]
    fn prefix_and_extension_are_rejected() {
        assert!(!bearer_matches("cap-token", TOKEN), "a strict prefix must not match");
        assert!(!bearer_matches("cap-token-256bit-extra", TOKEN), "a longer token must not match");
    }

    #[test]
    fn empty_bearer_is_rejected_even_against_an_empty_token() {
        assert!(!bearer_matches("", TOKEN));
        assert!(!bearer_matches("", ""));
    }
}
