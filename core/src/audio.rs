//! Audio as neural-codec codes (PROTOCOL.md §1.1): the two pinned models and the payload layout.
//!
//! Audio objects carry SNAC codes, not a waveform. Only the device that plays an object decodes
//! it; this module is what a source needs to pack codes and what a player needs to read them back.

use alloc::vec::Vec;

/// Bits per code: every SNAC codebook has 4096 entries.
pub const CODE_BITS: u32 = 12;

/// A neural audio codec pinned to exact weights. A content type's codec never changes; a
/// different or retrained model gets a new content type.
#[derive(Debug, PartialEq, Eq)]
pub struct Codec {
    /// Hugging Face repository of the upstream model.
    pub model: &'static str,
    /// Commit of that repository.
    pub revision: &'static str,
    /// SHA-256 of its `pytorch_model.bin`; a player that cannot match it does not play.
    pub weights_sha256: &'static str,
    pub sample_rate: u32,
    /// Codes per group, coarse level first, one entry per quantizer level.
    pub codes_per_level: &'static [u32],
    /// Samples one group decodes to.
    pub group_samples: u32,
}

/// Speech: SNAC 24 kHz, three levels, 0.98 kbit/s.
pub const SNAC_24KHZ: Codec = Codec {
    model: "hubertsiuzdak/snac_24khz",
    revision: "d73ad176a12188fcf4f360ba3bf2c2fbbe8f58ec",
    weights_sha256: "4b8164cc6606bfa627f1a784734c1e539891518f1191ed9194fe1e3b9b4bff40",
    sample_rate: 24_000,
    codes_per_level: &[1, 2, 4],
    group_samples: 4 * 512,
};

/// Music: SNAC 32 kHz, four levels, 1.88 kbit/s.
pub const SNAC_32KHZ: Codec = Codec {
    model: "hubertsiuzdak/snac_32khz",
    revision: "c84c6ac842dc7a44a6fb0f3b576ce94b48a7780f",
    weights_sha256: "bfee2f057c1e287443786bedab377b5176b430e911417683977b7af71ea3ba65",
    sample_rate: 32_000,
    codes_per_level: &[1, 2, 4, 8],
    group_samples: 8 * 384,
};

impl Codec {
    pub fn codes_per_group(&self) -> u32 {
        self.codes_per_level.iter().sum()
    }

    pub fn group_bits(&self) -> u32 {
        self.codes_per_group() * CODE_BITS
    }

    /// Whole groups in a payload of `len` bytes; leftover bits are padding.
    pub fn groups(&self, len: u32) -> u32 {
        (len as u64 * 8 / self.group_bits() as u64) as u32
    }

    /// Playing time of a payload of `len` bytes.
    pub fn duration_ms(&self, len: u32) -> u64 {
        self.groups(len) as u64 * self.group_samples as u64 * 1000 / self.sample_rate as u64
    }

    /// Payload bytes for `groups` groups.
    pub fn payload_len(&self, groups: u32) -> u32 {
        (groups as u64 * self.group_bits() as u64).div_ceil(8) as u32
    }

    pub fn bits_per_second(&self) -> u32 {
        (self.group_bits() as u64 * self.sample_rate as u64 / self.group_samples as u64) as u32
    }
}

/// Pack 12-bit codes into the payload bitstream: code `i` occupies bits `12 i` to `12 i + 11`,
/// and bit `b` of the stream is bit `b % 8` of byte `b / 8`. Codes must already be in payload
/// order (group by group in time, coarse level first within a group).
pub fn pack(codes: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity((codes.len() * CODE_BITS as usize).div_ceil(8));
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in codes {
        debug_assert!(c < 1 << CODE_BITS);
        acc |= ((c as u32) & 0xfff) << bits;
        bits += CODE_BITS;
        while bits >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            bits -= 8;
        }
    }
    if bits > 0 {
        out.push(acc as u8);
    }
    out
}

/// Read code `i` back from a payload, or `None` past its end.
pub fn code_at(payload: &[u8], i: usize) -> Option<u16> {
    let bit = i * CODE_BITS as usize;
    let byte = bit / 8;
    if bit + CODE_BITS as usize > payload.len() * 8 {
        return None;
    }
    let lo = payload[byte] as u32;
    let hi = *payload.get(byte + 1).unwrap_or(&0) as u32;
    let v = (lo | hi << 8) >> (bit % 8);
    Some((v & 0xfff) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ContentType;

    #[test]
    fn content_types_round_trip_and_name_their_codec() {
        for t in [ContentType::Manifest, ContentType::Text, ContentType::Firmware, ContentType::Speech,
                  ContentType::Music, ContentType::Opus, ContentType::Other] {
            assert_eq!(ContentType::from_u8(t as u8), t);
        }
        assert_eq!(ContentType::Speech.codec(), Some(&SNAC_24KHZ));
        assert_eq!(ContentType::Music.codec(), Some(&SNAC_32KHZ));
        assert_eq!(ContentType::Opus.codec(), None);
    }

    #[test]
    fn rates_and_sizes_match_protocol_1_1() {
        assert_eq!(SNAC_24KHZ.group_bits(), 84);
        assert_eq!(SNAC_32KHZ.group_bits(), 180);
        assert_eq!(SNAC_24KHZ.bits_per_second(), 984);
        assert_eq!(SNAC_32KHZ.bits_per_second(), 1875);
        // A 3-minute track is 1875 groups of 96 ms: 42 188 bytes.
        let len = SNAC_32KHZ.payload_len(1875);
        assert_eq!(len, 42_188);
        assert_eq!(SNAC_32KHZ.groups(len), 1875);
        assert_eq!(SNAC_32KHZ.duration_ms(len), 180_000);
        // Five minutes of speech is about 37 kB.
        let groups = (300_000u64 * 24_000 / 1000 / 2048) as u32;
        assert!((36_000..38_000).contains(&SNAC_24KHZ.payload_len(groups)));
    }

    #[test]
    fn packing_is_lsb_first_and_reads_back() {
        let codes: Vec<u16> = (0..31u16).map(|i| (i * 1234 + 7) % 4096).collect();
        let bytes = pack(&codes);
        assert_eq!(bytes.len(), (31 * 12usize).div_ceil(8));
        for (i, &c) in codes.iter().enumerate() {
            assert_eq!(code_at(&bytes, i), Some(c));
        }
        assert_eq!(code_at(&bytes, 31), None);
        // First code in the low bits of byte 0, its top nibble in the low half of byte 1.
        assert_eq!(pack(&[0xabc, 0x123]), [0xbc, 0x3a, 0x12]);
    }
}
