//! Scenario construction: node placement, channels, publishing, following.

use std::collections::BTreeMap;

use meshcast_core::ed25519_dalek::SigningKey;
use meshcast_core::ids::{NodeId, ObjectId, ShortId};
use meshcast_core::manifest::{Manifest, ManifestObject, ObjectRef, ScheduleEntry};
use meshcast_core::rendition::{Rendition, RenditionTable};
use meshcast_core::node::NodeConfig;
use meshcast_core::object::ContentType;
use meshcast_core::params::Params;
use meshcast_core::rng::Rng;
use serde::Serialize;

use crate::engine::Engine;
use crate::radio::{bulk_phy, control_phy, profile_for, region_for, BulkPreset, Phy, Propagation};

/// One kind of object in a publishing mix: a label for reports and a size. The label also names
/// the content type: anything with "opus" is Opus, anything with "speech" is speech, the rest music.
#[derive(Clone, Debug, Serialize)]
pub struct MixItem {
    pub label: String,
    pub kb: u32,
}

impl MixItem {
    pub fn kind(&self) -> ContentType {
        if self.label.contains("opus") {
            ContentType::Opus
        } else if self.label.contains("speech") {
            ContentType::Speech
        } else {
            ContentType::Music
        }
    }
}

/// Parse "snac-music:42,snac-speech:22,opus-music:540" into a mix.
pub fn parse_mix(s: &str) -> Result<Vec<MixItem>, String> {
    s.split(',')
        .map(|part| {
            let (label, kb) = part.trim().split_once(':').ok_or_else(|| format!("`{part}`: expected label:kB"))?;
            let kb = kb.parse().map_err(|_| format!("`{part}`: size is not a number"))?;
            Ok(MixItem { label: label.to_string(), kb })
        })
        .collect()
}

/// Renditions for devices that cannot decode (PROTOCOL.md §1.2): sources name one per audio
/// object, stations make them, and `small` followers want them instead of the codes.
#[derive(Clone, Debug, Default, Serialize)]
pub struct RenditionSpec {
    /// Opus bit rate of the rendition for music and for speech, kbit/s.
    pub music_kbps: f64,
    pub speech_kbps: f64,
    /// Followers that cannot decode and want renditions instead.
    pub small: usize,
    /// Whether every node that decodes (a dongle with its phone) can make renditions, or only
    /// stations.
    pub players_render: bool,
}

/// Nodes that flood their announcer with WANTs (docs/ABUSE.md).
#[derive(Clone, Debug, Default, Serialize)]
pub struct AttackSpec {
    pub attackers: usize,
    pub period_s: f64,
    /// A fresh made-up node id on every WANT.
    pub spoof: bool,
    /// Ask for renditions as well as codes.
    pub renditions: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScenarioSpec {
    pub nodes: usize,
    pub area_km2: f64,
    pub stations: usize,
    pub sources: usize,
    pub tracks: usize,
    pub track_kb: u32,
    /// Each source's objects cycle through this mix; empty means music of `track_kb`.
    pub mix: Vec<MixItem>,
    pub hours: f64,
    pub seed: u64,
    pub bulk: BulkPreset,
    pub control_sf: u8,
    pub exponent: f64,
    pub shadow_db: f64,
    pub follow_fraction: f64,
    /// Explicit positions (metres) override random placement.
    pub positions: Option<Vec<(f64, f64)>>,
    pub stations_at: Option<Vec<usize>>,
    pub sources_at: Option<Vec<usize>>,
    pub renditions: Option<RenditionSpec>,
    pub attack: Option<AttackSpec>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrackInfo {
    pub label: String,
    pub source: usize,
    pub index: usize,
    pub bytes: u32,
    pub followers: Vec<usize>,
    /// The rendition of this object, and the followers that want it instead.
    #[serde(skip)]
    pub rendition: Option<(ShortId, u32)>,
    pub small: Vec<usize>,
    /// When the schedule plays it (ms from the start of the simulation).
    pub slot_ms: Option<u64>,
}

/// A publishing node and what it needs to publish again later.
pub struct SourceInfo {
    pub node: usize,
    pub key: SigningKey,
    pub channel: meshcast_core::ids::ChannelId,
    pub objects: Vec<ManifestObject>,
    pub seq: u32,
}

pub struct Built {
    pub small: Vec<usize>,
    pub engine: Engine,
    pub tracks: BTreeMap<ShortId, TrackInfo>,
    pub phys: Vec<Phy>,
    pub sources: Vec<SourceInfo>,
}

pub fn track_object(seed: u64, source: usize, index: usize, len: u32, kind: ContentType) -> ManifestObject {
    let id = ObjectId::of(format!("track {source} {index} seed {seed}").as_bytes());
    ManifestObject { id, len, kind, title: format!("Track {index}") }
}

/// The Opus rendition of an audio object at `kbps`: as long as the codes play, at that rate.
/// Profile 1 is music, 2 speech (draft, PROTOCOL.md §1.2).
pub fn rendition_of(o: &ManifestObject, r: &RenditionSpec) -> Option<Rendition> {
    let codec = o.kind.codec()?;
    let (profile, kbps) = match o.kind {
        ContentType::Music => (1, r.music_kbps),
        ContentType::Speech => (2, r.speech_kbps),
        _ => return None,
    };
    let seconds = o.len as f64 * 8.0 / codec.bits_per_second() as f64;
    let len = (seconds * kbps * 1000.0 / 8.0).ceil() as u32;
    let mut tag = Vec::from(&b"rendition "[..]);
    tag.extend_from_slice(&o.id.0);
    Some(Rendition { parent: o.id.short(), profile, id: ObjectId::of(&tag), len })
}

pub fn build(spec: &ScenarioSpec, params: Params) -> Built {
    let mut rng = Rng::new(spec.seed);
    let n = spec.nodes;
    let side = (spec.area_km2 * 1e6).sqrt();
    let positions: Vec<(f64, f64)> = match &spec.positions {
        Some(p) => p.clone(),
        None => (0..n).map(|_| (rng.unit() * side, rng.unit() * side)).collect(),
    };
    let stations: Vec<usize> = match &spec.stations_at {
        Some(s) => s.clone(),
        None => {
            // Evenly spread: pick the nodes closest to grid points.
            let k = spec.stations.min(n);
            let mut chosen = Vec::new();
            let g = (k as f64).sqrt().ceil().max(1.0) as usize;
            let mut targets = Vec::new();
            for gx in 0..g {
                for gy in 0..g {
                    targets.push(((gx as f64 + 0.5) * side / g as f64, (gy as f64 + 0.5) * side / g as f64));
                }
            }
            for t in targets.into_iter().take(k) {
                let mut best = None;
                let mut bd = f64::MAX;
                for (i, p) in positions.iter().enumerate() {
                    if chosen.contains(&i) {
                        continue;
                    }
                    let d = (p.0 - t.0).powi(2) + (p.1 - t.1).powi(2);
                    if d < bd {
                        bd = d;
                        best = Some(i);
                    }
                }
                if let Some(b) = best {
                    chosen.push(b);
                }
            }
            chosen
        }
    };
    let sources: Vec<usize> = match &spec.sources_at {
        Some(s) => s.clone(),
        None => (0..n).filter(|i| !stations.contains(i)).take(spec.sources).collect(),
    };

    let profile = profile_for(spec.bulk);
    let region = region_for(spec.bulk);
    let phys = if spec.bulk == BulkPreset::LoraBulk { vec![bulk_phy(spec.bulk)] } else { vec![control_phy(region, spec.control_sf), bulk_phy(spec.bulk)] };
    let mut rule_choice = vec![0usize; profile.bands.len()];
    for p in &phys {
        if let Some((b, r)) = p.rule_choice {
            rule_choice[b] = r;
        }
    }

    // Small listeners: followers that are neither station nor source, chosen at random.
    let small: Vec<usize> = match &spec.renditions {
        Some(r) if r.small > 0 => {
            let mut pool: Vec<usize> = (0..n).filter(|i| !stations.contains(i) && !sources.contains(i)).collect();
            let mut pick = Vec::new();
            let mut rs = Rng::new(spec.seed ^ 0x5A11);
            while pick.len() < r.small.min(pool.len()) {
                let k = rs.below(pool.len() as u64) as usize;
                pick.push(pool.swap_remove(k));
            }
            pick.sort();
            pick
        }
        _ => Vec::new(),
    };
    let configs: Vec<NodeConfig> = (0..n)
        .map(|i| NodeConfig {
            id: NodeId(i as u32 + 1),
            mains: stations.contains(&i),
            has_ip: stations.contains(&i),
            profile,
            rule_choice: rule_choice.clone(),
            carriers: phys.iter().map(|p| p.to_core()).collect(),
            params,
            seed: spec.seed.wrapping_add(i as u64 * 7919),
            keep_bytes_below: 4096,
            decodes: !small.contains(&i),
            renders: stations.contains(&i) || (!small.contains(&i) && spec.renditions.as_ref().map(|r| r.players_render).unwrap_or(false)),
        })
        .collect();

    let prop = Propagation { exponent: spec.exponent, shadow_sigma_db: spec.shadow_db };
    let mut engine = Engine::new(configs, positions, phys.clone(), prop, spec.seed);

    let mut tracks = BTreeMap::new();
    let mut source_infos = Vec::new();
    let mut rng2 = Rng::new(spec.seed ^ 0xF00D);
    for &s in &sources {
        let mut kb = [0u8; 32];
        for (k, b) in kb.iter_mut().enumerate() {
            *b = (spec.seed as u8).wrapping_add(k as u8).wrapping_mul(31).wrapping_add(s as u8);
        }
        let key = SigningKey::from_bytes(&kb);
        let mut objects = Vec::new();
        let mut metas = Vec::new();
        let default_mix = [MixItem { label: "music".into(), kb: spec.track_kb }];
        let mix: &[MixItem] = if spec.mix.is_empty() { &default_mix } else { &spec.mix };
        let mut labels = Vec::new();
        let mut table = RenditionTable::default();
        for t in 0..spec.tracks {
            let item = &mix[t % mix.len()];
            let o = track_object(spec.seed, s, t, item.kb * 1024, item.kind());
            if let Some(r) = spec.renditions.as_ref().and_then(|r| rendition_of(&o, r)) {
                table.entries.push(r);
            }
            labels.push(item.label.clone());
            metas.push((o.meta(), None));
            objects.push(o);
        }
        let schedule: Vec<ScheduleEntry> = objects.iter().enumerate().map(|(t, o)| ScheduleEntry { object: o.id.short(), start: 3600 * (t as u64 + 1), repeat: 0 }).collect();
        // The source names its renditions in a table; the manifest names the table.
        let (table_meta, table_bytes) = table.as_object();
        let table_ref = (!table.entries.is_empty()).then_some(ObjectRef { id: table_meta.id, len: table_meta.len });
        let m = Manifest::sign_with_renditions(&key, 1, &format!("Channel of node {s}"), objects.clone(), schedule, None, table_ref);
        let chan = m.channel_id();
        if table_ref.is_some() {
            metas.push((table_meta, Some(&table_bytes[..])));
        }
        engine.nodes[s].node.publish(&m, &metas);
        let mut followers = Vec::new();
        for i in 0..n {
            if i == s {
                continue;
            }
            if spec.follow_fraction >= 1.0 || rng2.unit() < spec.follow_fraction {
                engine.nodes[i].node.follow(chan);
                followers.push(i);
            }
        }
        for (t, o) in objects.iter().enumerate() {
            let rendition = table.entries.iter().find(|r| r.parent == o.id.short()).map(|r| (r.id.short(), r.len));
            let small_here: Vec<usize> = if rendition.is_some() { followers.iter().copied().filter(|f| small.contains(f)).collect() } else { Vec::new() };
            tracks.insert(o.id.short(), TrackInfo { label: labels[t].clone(), source: s, index: t, bytes: o.len, followers: followers.clone(), rendition, small: small_here, slot_ms: Some(3600_000 * (t as u64 + 1)) });
        }
        source_infos.push(SourceInfo { node: s, key, channel: chan, objects, seq: 1 });
    }
    engine.rendition_ids = tracks.values().filter_map(|t| t.rendition.map(|(id, _)| id)).collect();
    if let Some(a) = spec.attack.as_ref().filter(|a| a.attackers > 0) {
        let mut pool: Vec<usize> = (0..n).filter(|i| !stations.contains(i) && !sources.contains(i) && !small.contains(i)).collect();
        let mut ra = Rng::new(spec.seed ^ 0xBAD);
        for _ in 0..a.attackers.min(pool.len()) {
            let k = ra.below(pool.len() as u64) as usize;
            let node = pool.swap_remove(k);
            engine.attackers.push(crate::engine::Attacker::new(node, (a.period_s * 1000.0) as u64, a.spoof));
        }
        let mut ids: Vec<ShortId> = tracks.keys().copied().collect();
        if a.renditions {
            ids.extend(tracks.values().filter_map(|t| t.rendition.map(|(id, _)| id)));
        }
        engine.attack_ids = ids;
        engine.start_attacks();
    }
    Built { small, engine, tracks, phys, sources: source_infos }
}
