//! Objects and their symbol layout (PROTOCOL.md §1).

use crate::frame::SYMBOL_SIZE;
use crate::ids::ObjectId;

/// Maximum source symbols per block.
pub const K_MAX: u16 = 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum Mime {
    Audio = 0,
    Manifest = 1,
    Text = 2,
    Firmware = 3,
    Other = 255,
}

impl Mime {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Mime::Audio,
            1 => Mime::Manifest,
            2 => Mime::Text,
            3 => Mime::Firmware,
            _ => Mime::Other,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObjectMeta {
    pub id: ObjectId,
    pub len: u32,
    pub mime: Mime,
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
