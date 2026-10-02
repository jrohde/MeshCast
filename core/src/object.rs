//! Objects and their symbol layout (PROTOCOL.md §1).

use crate::frame::SYMBOL_SIZE;
use crate::audio::{Codec, SNAC_24KHZ, SNAC_32KHZ};
use crate::ids::ObjectId;

/// Maximum source symbols per block.
pub const K_MAX: u16 = 1024;

/// What an object is: one byte in its manifest entry (PROTOCOL.md §1.1). Frames never carry it;
/// a node that wants an object already has the manifest that lists it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum ContentType {
    Manifest = 1,
    Text = 2,
    Firmware = 3,
    /// A channel's rendition table (PROTOCOL.md §1.2).
    Renditions = 4,
    /// A collection manifest: one collection's pieces, named by a root manifest (PROTOCOL.md §2).
    Collection = 5,
    /// A collection's cover, as JPEG.
    Image = 6,
    /// Spoken programmes as SNAC 24 kHz codes.
    Speech = 16,
    /// Music as SNAC 32 kHz codes.
    Music = 17,
    /// Opus: a rendition of a SNAC object for a device that cannot decode it, made on demand and
    /// sent only where someone asks for it (PROTOCOL.md §1.2).
    Opus = 18,
    Other = 255,
}

impl ContentType {
    /// Objects a node reads itself to know what to want and how to play: a channel's root
    /// manifest, its collection manifests and its rendition table. Their bytes are always kept,
    /// whatever their size.
    pub fn is_read_by_nodes(self) -> bool {
        matches!(self, ContentType::Manifest | ContentType::Collection | ContentType::Renditions)
    }

    /// A manifest of either level: passed first, kept whatever its size (PROTOCOL.md §2, §4).
    pub fn is_manifest(self) -> bool {
        matches!(self, ContentType::Manifest | ContentType::Collection)
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => ContentType::Manifest,
            2 => ContentType::Text,
            3 => ContentType::Firmware,
            4 => ContentType::Renditions,
            5 => ContentType::Collection,
            6 => ContentType::Image,
            16 => ContentType::Speech,
            17 => ContentType::Music,
            18 => ContentType::Opus,
            _ => ContentType::Other,
        }
    }

    /// The pinned model that decodes this content, if it is codec audio.
    pub fn codec(self) -> Option<&'static Codec> {
        match self {
            ContentType::Speech => Some(&SNAC_24KHZ),
            ContentType::Music => Some(&SNAC_32KHZ),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObjectMeta {
    pub id: ObjectId,
    pub len: u32,
    pub kind: ContentType,
}

/// Number of symbols for an object of `len` bytes (at least one).
pub fn total_symbols(len: u32) -> u32 {
    ((len as u64 + SYMBOL_SIZE as u64 - 1) / SYMBOL_SIZE as u64).max(1) as u32
}

/// Number of source blocks for an object of `len` bytes.
pub fn blocks(len: u32) -> u16 {
    ((total_symbols(len) + K_MAX as u32 - 1) / K_MAX as u32).max(1) as u16
}

/// Source symbols in block `block` of an object of `len` bytes.
pub fn block_k(len: u32, block: u16) -> u16 {
    let total = total_symbols(len);
    let start = block as u32 * K_MAX as u32;
    if start >= total {
        0
    } else {
        (total - start).min(K_MAX as u32) as u16
    }
}

impl ObjectMeta {
    pub fn total_symbols(&self) -> u32 {
        total_symbols(self.len)
    }
    pub fn blocks(&self) -> u16 {
        blocks(self.len)
    }
    pub fn block_k(&self, block: u16) -> u16 {
        block_k(self.len, block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout() {
        assert_eq!(total_symbols(0), 1);
        assert_eq!(total_symbols(200), 1);
        assert_eq!(total_symbols(201), 2);
        assert_eq!(blocks(200 * 1024), 1);
        assert_eq!(blocks(200 * 1024 + 1), 2);
        assert_eq!(block_k(540_000, 0), 1024);
        assert_eq!(block_k(540_000, 2), 2700 - 2048);
        assert_eq!(block_k(540_000, 3), 0);
    }
}
