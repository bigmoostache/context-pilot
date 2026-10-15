//! `TZif` zone files (RFC 8536, versions 1–4).

use super::{Zone, posix};

/// Big-endian byte cursor over a `TZif` file.
struct Reader<'src> {
    /// Whole file.
    data: &'src [u8],
    /// Read position.
    pos: usize,
}

impl<'src> Reader<'src> {
    /// Next `len` bytes.
    fn take(&mut self, len: usize) -> Option<&'src [u8]> {
        let end = self.pos.checked_add(len)?;
        let out = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    /// Advance past `len` bytes.
    fn skip(&mut self, len: usize) -> Option<()> {
        self.take(len).map(drop)
    }

    /// Unsigned big-endian integer of `len` bytes.
    fn uint(&mut self, len: usize) -> Option<u64> {
        Some(self.take(len)?.iter().fold(0u64, |acc, byte| acc.checked_shl(8).unwrap_or(0) | u64::from(*byte)))
    }

    /// Two's-complement big-endian integer of 4 or 8 bytes.
    fn signed(&mut self, len: usize) -> Option<i64> {
        let raw = self.uint(len)?;
        if len == 4 {
            Some(i64::from(0i32.wrapping_add_unsigned(u32::try_from(raw).ok()?)))
        } else {
            Some(0i64.wrapping_add_unsigned(raw))
        }
    }

    /// Version byte and header counts `[isut, isstd, leap, time, type, char]`.
    fn header(&mut self) -> Option<(u8, [usize; 6])> {
        if self.take(4)? != b"TZif" {
            return None;
        }
        let version = self.take(1)?.first().copied()?;
        self.skip(15)?;
        let mut counts = [0usize; 6];
        for count in &mut counts {
            *count = usize::try_from(self.uint(4)?).ok()?;
        }
        Some((version, counts))
    }

    /// `count` UT offsets of the local time type records (6 bytes each).
    fn offsets(&mut self, count: usize) -> Option<Vec<i32>> {
        std::iter::repeat_with(|| {
            let offset = i32::try_from(self.signed(4)?).ok();
            self.skip(2)?;
            offset
        })
        .take(count)
        .collect()
    }

    /// One data block; `width` is 4 (v1) or 8 (v2+) bytes per time value.
    fn block(&mut self, counts: [usize; 6], width: usize) -> Option<Zone> {
        let [isut, isstd, leap, time, types, chars] = counts;
        let times: Vec<i64> = std::iter::repeat_with(|| self.signed(width)).take(time).collect::<Option<_>>()?;
        let indices = self.take(time)?;
        let offsets = self.offsets(types)?;
        let leap_len = leap.saturating_mul(width.saturating_add(4));
        self.skip(chars.saturating_add(leap_len).saturating_add(isstd).saturating_add(isut))?;
        let transitions = times
            .into_iter()
            .zip(indices)
            .map(|(at, idx)| offsets.get(usize::from(*idx)).map(|offset| (at, *offset)))
            .collect::<Option<Vec<_>>>()?;
        Some(Zone { transitions, initial: *offsets.first()?, rule: None })
    }
}

/// Parse a `TZif` file; `None` if malformed.
pub(super) fn parse(data: &[u8]) -> Option<Zone> {
    let mut reader = Reader { data, pos: 0 };
    let (version, v1) = reader.header()?;
    if version == 0 {
        return reader.block(v1, 4);
    }
    // Skip the legacy 32-bit block; the 64-bit one follows with its own header.
    reader.block(v1, 4).map(drop)?;
    let (_, v2) = reader.header()?;
    let mut zone = reader.block(v2, 8)?;
    let footer = reader.data.get(reader.pos..).unwrap_or_default();
    let spec = footer.strip_prefix(b"\n").and_then(|rest| rest.split(|byte| *byte == b'\n').next());
    zone.rule = spec.and_then(|raw| std::str::from_utf8(raw).ok()).and_then(posix::parse);
    Some(zone)
}
