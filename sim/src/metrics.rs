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
    /// The largest size every table a node fills from what it hears reached in any honest node,
    /// sampled with the occupancy (docs/ABUSE.md, item 5).
    pub table_peaks: BTreeMap<&'static str, usize>,
    /// Upload frames by what happened at the announcer they were meant for:
    /// [delivered, collided, receiver transmitting, receiver on another channel, too weak, receiver off].
    pub upload_outcome: [u64; 6],
    /// Who broke collided upload frames: [uploader to the same announcer, uploader to another,
    /// an announcer, anything else].
    pub upload_interferer: [u64; 4],
    /// Upload-upload collisions at one announcer by cause: [a sender's phase count was stale,
    /// a sender had no grant, a sender never heard a phase count, phases were current].
    pub upload_collision_cause: [u64; 4],
    /// Uploads sent while the announcer was on another channel, by cause: [it is no longer an
    /// announcer, the sender's idea of its colour was stale, the sender did not know it, other].
    pub upload_wrong_channel: [u64; 4],
    /// Grants (WANT entries naming a holder) sent to a node that was announcing at the time:
    /// announcers do not upload, so each of these waits out `T_grant` for nothing.
    pub grants_to_announcers: u64,
    /// Bulk frames of renditions (PROTOCOL.md §1.2).
    pub bulk_sent_rendition: u64,
    /// WANT frames sent by attackers.
    pub attack_frames: u64,
    /// (node index, object) -> completion time.
    pub completions: BTreeMap<(usize, ShortId), Millis>,
    pub role_events: Vec<RoleEvent>,
    pub frames_sent: u64,
    pub frames_delivered: u64,
    pub frames_collided: u64,
    pub frames_half_duplex: u64,
    /// Frames that reached a node while its one radio listened on its other carrier.
    pub frames_not_listening: u64,
    /// Frames a receiver lost because it retuned before they ended: a hop dwell ended (PROTOCOL.md §6).
    pub frames_retuned: u64,
    pub frames_below_sensitivity: u64,
    pub bulk_sent: u64,
    pub bulk_delivered: u64,
    pub occupancy_samples: Vec<(Millis, usize, usize, u16)>,
    /// Per receiver: bulk frames delivered and lost to collisions.
    pub per_node_bulk: Vec<(u64, u64)>,
    /// Collisions during meeting dwells versus outside them, by frame type index.
    pub collided_meeting: [u64; 6],
    pub collided_other: [u64; 6],
    /// Bulk collisions by (sender is announcer, strongest interferer is announcer).
    pub bulk_collision_kinds: [[u64; 2]; 2],
    /// Bulk frames sent by announcers / by others.
    pub bulk_sent_by: [u64; 2],
    /// The same by what the frame carries: [root manifest, collection manifest, piece or other, unknown].
    pub bulk_sent_kind: [[u64; 4]; 2],
    /// Upload-upload collisions: same object (duplicate uploaders) vs different objects.
    pub upload_collision_same_object: u64,
    pub upload_collision_other_object: u64,
    /// Asks an announcer made for its listeners (ASK_LISTENED), by (announcer index, object): how
    /// long they stay open and how often they are repeated before a holder meets them
    /// (FEASIBILITY.md §28).
    pub listened_asks: BTreeMap<(usize, ShortId), ListenedAsk>,
    /// Objects that gave way to a node's carry budget, with what the node knew of them then, and
    /// how long after it the node held each again, if it did (FEASIBILITY.md §28).
    pub evictions: Vec<(meshcast_core::node::EvictionInfo, Option<Millis>)>,
    pub evicted_pending: BTreeMap<(usize, ShortId), (usize, Millis)>,
    /// How many nodes took on a relay of each object.
    pub relayers: BTreeMap<ShortId, (u32, std::collections::BTreeSet<usize>)>,
}

/// One announcer's ask for its listeners for one object.
#[derive(Clone, Copy, Debug, Default)]
pub struct ListenedAsk {
    /// When it was first sent, and how many GOSSIP frames carried it open (no grant yet).
    pub first: Millis,
    pub open_frames: u32,
    /// When it was first granted, and whether to a node that followed another announcer then.
    pub granted: Option<(Millis, bool)>,
    /// Whether a node that followed this announcer when it first asked listens to the object.
    pub real_listener: bool,
}

impl Metrics {
    pub fn role(&mut self, t: Millis, node: NodeId, carrier: usize, role: Role, announcer: NodeId) {
        self.role_events.push(RoleEvent { t_ms: t, node: node.0, carrier, role: format!("{role:?}"), announcer: announcer.0 });
    }
}

#[derive(Debug, Serialize)]
pub struct ObjectSummary {
    pub object: String,
    pub label: String,
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

/// Replay role events on carrier `c` after `victim` is switched off at `kill_ms`: the most
/// announcers at once and the longest stretch, until `until`, with more than one. The victim stops
/// announcing at the kill, which is counted then, and counts again from its own first role event
/// after it comes back: a dead node and a reboot emit none.
pub fn several_announcers(events: &[RoleEvent], c: usize, victim: u32, kill_ms: Millis, until: Millis) -> (usize, Millis) {
    fn tally(current: &BTreeMap<u32, bool>, t: Millis, max_sim: &mut usize, since: &mut Option<Millis>, longest: &mut Millis) {
        let n = current.values().filter(|&&a| a).count();
        *max_sim = (*max_sim).max(n);
        if n > 1 {
            since.get_or_insert(t);
        } else if let Some(s) = since.take() {
            *longest = (*longest).max(t - s);
        }
    }
    let mut current: BTreeMap<u32, bool> = BTreeMap::new();
    let (mut max_sim, mut since, mut longest) = (0usize, None, 0);
    let mut killed = false;
    for e in events.iter().filter(|e| e.carrier == c) {
        if !killed && e.t_ms >= kill_ms {
            current.insert(victim, false);
            killed = true;
            tally(&current, kill_ms, &mut max_sim, &mut since, &mut longest);
        }
        current.insert(e.node, e.role == "Announcer");
        if killed {
            tally(&current, e.t_ms, &mut max_sim, &mut since, &mut longest);
        }
    }
    if !killed {
        current.insert(victim, false);
        tally(&current, kill_ms, &mut max_sim, &mut since, &mut longest);
    }
    if let Some(s) = since {
        longest = longest.max(until.saturating_sub(s));
    }
    (max_sim, longest)
}
