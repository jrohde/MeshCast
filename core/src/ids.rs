use alloc::string::String;
use core::fmt;

/// Full object identifier: BLAKE3 of the object bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub [u8; 32]);

impl ObjectId {
    pub fn of(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }
    pub fn short(&self) -> ShortId {
        let mut s = [0u8; 8];
        s.copy_from_slice(&self.0[..8]);
        ShortId(s)
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Obj({})", hex(&self.0[..4]))
    }
}

/// On-air object identifier: first 8 bytes of the [`ObjectId`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShortId(pub [u8; 8]);

impl ShortId {
    pub fn as_u64(&self) -> u64 {
        u64::from_le_bytes(self.0)
    }
}

impl fmt::Debug for ShortId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex(&self.0[..4]))
    }
}

/// Node identifier (first 4 bytes of the node key hash). 0 means "none".
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct NodeId(pub u32);

impl NodeId {
    pub const NONE: NodeId = NodeId(0);
    pub fn is_none(&self) -> bool {
        self.0 == 0
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "N{}", self.0)
    }
}

/// Channel identifier: first 8 bytes of BLAKE3(channel public key).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelId(pub [u8; 8]);

impl ChannelId {
    pub fn of_pubkey(pk: &[u8; 32]) -> Self {
        let h = blake3::hash(pk);
        let mut s = [0u8; 8];
        s.copy_from_slice(&h.as_bytes()[..8]);
        ChannelId(s)
    }
}

impl fmt::Debug for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ch({})", hex(&self.0[..4]))
    }
}

pub fn hex(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(H[(x >> 4) as usize] as char);
        s.push(H[(x & 15) as usize] as char);
    }
    s
}
