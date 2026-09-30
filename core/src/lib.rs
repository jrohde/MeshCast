//! MeshCast protocol core.
//!
//! `no_std` + `alloc`. No I/O: a [`node::Node`] consumes [`node::Event`]s and emits
//! [`node::Action`]s. The same crate runs in firmware, on a station and inside the simulator.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod audio;
pub mod carousel;
pub mod crc;
pub mod discipline;
pub mod election;
pub mod fatsoen;
pub mod frame;
pub mod ids;
pub mod manifest;
pub mod node;
pub mod object;
pub mod params;
pub mod profile;
pub mod rng;
pub mod store;

/// Milliseconds since an arbitrary epoch (boot, or simulation start).
pub type Millis = u64;

/// Re-exported so hosts can create channel keys without depending on the crate directly.
pub use ed25519_dalek;
