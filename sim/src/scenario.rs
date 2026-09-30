//! Scenario construction: node placement, channels, publishing, following.

use std::collections::BTreeMap;

use meshcast_core::ed25519_dalek::SigningKey;
use meshcast_core::ids::{NodeId, ObjectId, ShortId};
use meshcast_core::manifest::{Manifest, ManifestObject, ScheduleEntry};
use meshcast_core::node::NodeConfig;
use meshcast_core::object::ContentType;
use meshcast_core::params::Params;
use meshcast_core::rng::Rng;
use serde::Serialize;

use crate::engine::Engine;
use crate::radio::{bulk_phy, control_phy, profile_for, region_for, BulkPreset, Phy, Propagation};

#[derive(Clone, Debug, Serialize)]
pub struct ScenarioSpec {
    pub nodes: usize,
    pub area_km2: f64,
    pub stations: usize,
    pub sources: usize,
    pub tracks: usize,
    pub track_kb: u32,
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
}

#[derive(Clone, Debug, Serialize)]
pub struct TrackInfo {
    pub source: usize,
    pub index: usize,
    pub bytes: u32,
    pub followers: Vec<usize>,
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
    pub engine: Engine,
    pub tracks: BTreeMap<ShortId, TrackInfo>,
    pub phys: Vec<Phy>,
    pub sources: Vec<SourceInfo>,
}

pub fn track_object(seed: u64, source: usize, index: usize, len: u32, kind: ContentType) -> ManifestObject {
    let id = ObjectId::of(format!("track {source} {index} seed {seed}").as_bytes());
    ManifestObject { id, len, kind, title: format!("Track {index}") }
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
        for t in 0..spec.tracks {
            let o = track_object(spec.seed, s, t, spec.track_kb * 1024, ContentType::Music);
            metas.push((o.meta(), None));
            objects.push(o);
        }
        let schedule: Vec<ScheduleEntry> = objects.iter().enumerate().map(|(t, o)| ScheduleEntry { object: o.id.short(), start: 3600 * (t as u64 + 1), repeat: 0 }).collect();
        let m = Manifest::sign(&key, 1, &format!("Channel of node {s}"), objects.clone(), schedule, None);
        let chan = m.channel_id();
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
            tracks.insert(o.id.short(), TrackInfo { source: s, index: t, bytes: o.len, followers: followers.clone() });
        }
        source_infos.push(SourceInfo { node: s, key, channel: chan, objects, seq: 1 });
    }
    Built { engine, tracks, phys, sources: source_infos }
}
