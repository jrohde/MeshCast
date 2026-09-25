use std::collections::BTreeMap;

use meshcast_core::ids::{NodeId, ShortId};
use meshcast_core::node::Role;
use meshcast_core::Millis;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct RoleEvent {
    pub t_ms: Millis,
    pub node: u32,
    pub carrier: usize,
    pub role: String,
    pub announcer: u32,
}

#[derive(Default, Debug)]
pub struct Metrics {
    /// (node index, object) -> completion time.
    pub completions: BTreeMap<(usize, ShortId), Millis>,
    pub role_events: Vec<RoleEvent>,
    pub frames_sent: u64,
    pub frames_delivered: u64,
    pub frames_collided: u64,
    pub frames_half_duplex: u64,
    pub frames_below_sensitivity: u64,
    pub bulk_sent: u64,
    pub bulk_delivered: u64,
    pub occupancy_samples: Vec<(Millis, usize, usize, u16)>,
}

impl Metrics {
    pub fn role(&mut self, t: Millis, node: NodeId, carrier: usize, role: Role, announcer: NodeId) {
        self.role_events.push(RoleEvent { t_ms: t, node: node.0, carrier, role: format!("{role:?}"), announcer: announcer.0 });
    }
}

#[derive(Debug, Serialize)]
pub struct ObjectSummary {
    pub object: String,
    pub source: usize,
    pub index: usize,
    pub bytes: u32,
    pub followers: usize,
    pub complete: usize,
    pub p50_h: Option<f64>,
    pub p90_h: Option<f64>,
    pub max_h: Option<f64>,
}

pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    Some(sorted[idx.min(sorted.len() - 1)])
}
