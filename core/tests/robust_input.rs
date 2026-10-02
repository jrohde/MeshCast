//! Everything a node decodes comes from the air or from an object anyone can announce, so every
//! decoder must survive any input: never panic, and never allocate out of proportion to what it
//! was given (docs/ABUSE.md, "Someone else's firmware", item 5). This is a fuzzer that needs no
//! nightly toolchain: valid encodings of every decoded type, mangled many thousands of times by
//! a seeded generator, so that a failure can be replayed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use meshcast_core::ed25519_dalek::SigningKey;
use meshcast_core::frame::*;
use meshcast_core::ids::{ChannelId, NodeId, ObjectId, ShortId};
use meshcast_core::manifest::{Collection, CollectionKind, CollectionRef, Manifest, ManifestObject, ObjectRef, ScheduleEntry};
use meshcast_core::object::ContentType;
use meshcast_core::rendition::{Rendition, RenditionTable};

/// Counts live heap bytes and their peak, so a test can see what one decode allocated.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = LIVE.fetch_add(l.size(), Relaxed) + l.size();
            PEAK.fetch_max(now, Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Relaxed);
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Bytes allocated at the peak of `f`, above what was live before it.
fn peak_of<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = LIVE.load(Relaxed);
    PEAK.store(before, Relaxed);
    let out = f();
    (out, PEAK.load(Relaxed).saturating_sub(before))
}

/// What a decoder may allocate for an input of `len` bytes: a small multiple of the input, as
/// growing vectors and copied strings need, plus a constant.
fn allowed(len: usize) -> usize {
    8 * len + 8 * 1024
}

struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// One to four edits: a flipped bit, a byte set to anything or to a value that starts a large
/// CBOR length, a truncation, an inserted or a removed byte.
fn mangle(seed: &[u8], g: &mut Gen) -> Vec<u8> {
    let mut v = seed.to_vec();
    for _ in 0..1 + g.below(4) {
        if v.is_empty() {
            v.push(g.next() as u8);
            continue;
        }
        let i = g.below(v.len());
        match g.below(6) {
            0 => v[i] ^= 1 << g.below(8),
            1 => v[i] = g.next() as u8,
            2 => v[i] = [0x19, 0x1a, 0x1b, 0x59, 0x5a, 0x79, 0x7a, 0x99, 0x9a, 0x9b, 0xb9, 0xff][g.below(12)],
            3 => v.truncate(i),
            4 => v.insert(i, g.next() as u8),
            _ => {
                v.remove(i);
            }
        }
    }
    v
}

fn id(b: u8) -> ObjectId {
    ObjectId([b; 32])
}

fn seeds() -> Vec<(&'static str, Vec<u8>)> {
    let frames = vec![
        Frame::Beacon(Beacon {
            carrier: CarrierKind::GfskBulk,
            announcer: NodeId(42),
            score: 300,
            caps: CAP_MAINS,
            next_ms: 60000,
            round: 7,
            utc: 1_700_000_000,
            time_quality: 2,
            colour: 3,
            colours: 5,
            upload_phases: 3,
            occupancy: [10, 20, 30, 40],
        }),
        Frame::Bulk(Bulk { object: ShortId([1; 8]), block: 2, esi: 3, len: 500_000, payload: vec![9u8; SYMBOL_SIZE] }),
        Frame::Gossip(Gossip {
            node: NodeId(1),
            announcer: NodeId(2),
            announcer_colour: 1,
            announcer_colours: 4,
            heard: vec![(NodeId(7), 0, 3), (NodeId(8), 1, 3), (NodeId(9), 2, 3)],
            have: vec![ShortId([3; 8]); 1],
            have_sets: vec![PieceSet { manifest: ShortId([6; 8]), first: 0, bits: u64::MAX }; 4],
            want: vec![(ShortId([4; 8]), NodeId::NONE, ASK_LISTENED | 3); 1],
            sets: vec![WantSet { set: PieceSet { manifest: ShortId([5; 8]), first: 64, bits: 0x8000_0000_0000_0001 }, grant: NodeId(9), phase: 3 }; 3],
        }),
        Frame::ManifestAnnounce(ManifestAnnounce { node: NodeId(5), entries: vec![AnnounceEntry { channel: ChannelId([6; 8]), manifest: ShortId([7; 8]), seq: 9, len: 1234 }; 8], whole: true }),
        Frame::Nack(Nack { node: NodeId(8), object: ShortId([2; 8]), block: 0, answerer: NodeId(3), phase: 5, missing: vec![(0, 3), (10, 1)] }),
    ];
    let collection = Collection {
        cid: 7,
        kind: CollectionKind::Album,
        title: "An album".into(),
        pieces: (0..5u8).map(|k| ManifestObject { id: id(k), len: 42_000, kind: ContentType::Music, title: format!("Track {k}") }).collect(),
        schedule: vec![ScheduleEntry { object: id(0).short(), start: 1_700_000_000, repeat: 0 }],
    };
    let refs = vec![CollectionRef { cid: 7, kind: CollectionKind::Album, title: "An album".into(), manifest: ObjectRef { id: id(9), len: 400 }, cover: Some(ObjectRef { id: id(8), len: 16_384 }), changed: true }];
    let root = Manifest::sign(&SigningKey::from_bytes(&[7; 32]), 3, "A channel", refs, Some(id(1)), Some(ObjectRef { id: id(2), len: 300 }));
    let table = RenditionTable { entries: (0..3u8).map(|k| Rendition { parent: id(k).short(), profile: 1, id: id(k + 10), len: 360_000 }).collect() };
    let mut out: Vec<(&'static str, Vec<u8>)> = frames.into_iter().map(|f| ("frame", f.encode())).collect();
    out.push(("collection", collection.encode()));
    out.push(("root", root.encode()));
    out.push(("renditions", table.encode()));
    out
}

fn decode(kind: &str, bytes: &[u8]) {
    match kind {
        "frame" => drop(Frame::decode(bytes)),
        "collection" => drop(Collection::decode(bytes)),
        "root" => drop(Manifest::decode(bytes)),
        _ => drop(RenditionTable::decode(bytes)),
    }
}

/// CBOR that claims a list of 4096 entries and then ends: a decoder that sizes its list from the
/// claim reserves room for 4096 entries from a dozen bytes.
fn claims() -> Vec<(&'static str, Vec<u8>)> {
    let long_list = [0x99u8, 0x10, 0x00];
    // A collection manifest: [cid 1, kind album, title "", pieces: array(4096)...]
    let mut collection = vec![0x85, 0x01, 0x01, 0x60];
    collection.extend_from_slice(&long_list);
    // A root manifest: [body, signature], the body [chan 32 bytes, seq 0, title "", array(4096)...]
    let mut body = vec![0x86, 0x58, 0x20];
    body.extend_from_slice(&[0u8; 32]);
    body.extend_from_slice(&[0x00, 0x60]);
    body.extend_from_slice(&long_list);
    let mut root = vec![0x82, 0x58, body.len() as u8];
    root.extend_from_slice(&body);
    root.extend_from_slice(&[0x58, 0x40]);
    root.extend_from_slice(&[0u8; 64]);
    vec![("collection", collection), ("root", root), ("renditions", long_list.to_vec())]
}

#[test]
fn decoders_survive_mangled_input_and_allocate_in_proportion() {
    for (kind, bytes) in claims() {
        let ((), peak) = peak_of(|| decode(kind, &bytes));
        assert!(peak <= allowed(bytes.len()), "{kind}: {} bytes claiming 4096 entries made the decoder allocate {peak} bytes", bytes.len());
    }
    let mut g = Gen(0x5eed_cafe_f00d_0001);
    let mut worst = (0usize, "", 0usize);
    for (kind, seed) in seeds() {
        decode(kind, &seed);
        for round in 0..100_000 {
            let input = mangle(&seed, &mut g);
            let ((), peak) = peak_of(|| decode(kind, &input));
            assert!(peak <= allowed(input.len()), "{kind}, round {round}: {} input bytes, {peak} allocated: {input:02x?}", input.len());
            if peak > worst.0 {
                worst = (peak, kind, input.len());
            }
        }
    }
    eprintln!("largest allocation of one decode: {} bytes ({}, {} input bytes)", worst.0, worst.1, worst.2);
}
