use meshcast_sim::{metrics, radio, scenario};

use std::collections::BTreeMap;
use std::fs;

use clap::{Parser, Subcommand};
use meshcast_core::frame::{CarrierKind, SYMBOL_SIZE};
use meshcast_core::params::Params;
use meshcast_core::Millis;
use serde::Serialize;

use crate::metrics::{percentile, ObjectSummary};
use crate::radio::{bulk_phy, control_phy, range_m, BulkPreset, Propagation, Region};
use crate::scenario::{build, ScenarioSpec};

#[derive(Parser)]
#[command(name = "meshcast-sim", about = "MeshCast Phase 0: discrete-event simulator running the real protocol core")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args, Clone)]
struct Common {
    /// Simulated duration in hours.
    #[arg(long, default_value_t = 24.0)]
    hours: f64,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(long, value_enum, default_value_t = BulkPreset::GfskO)]
    bulk: BulkPreset,
    /// LoRa spreading factor of the control carrier.
    #[arg(long, default_value_t = 7)]
    control_sf: u8,
    /// Path-loss exponent (2 = free space, 3 = suburban, 3.5–4 = urban).
    #[arg(long, default_value_t = 3.0)]
    exponent: f64,
    /// Log-normal shadowing sigma per link, dB.
    #[arg(long, default_value_t = 6.0)]
    shadow_db: f64,
    #[arg(long, default_value_t = 10)]
    tracks: usize,
    /// Size of one track in kB (540 kB ≈ 3 min of Opus at 24 kbit/s).
    #[arg(long, default_value_t = 540)]
    track_kb: u32,
    /// Write full metrics as JSON here.
    #[arg(long)]
    out: Option<String>,
    /// Print role changes as they happen.
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Analytic link budget and throughput per carrier preset (no simulation).
    Budget {
        #[arg(long, default_value_t = 3.0)]
        exponent: f64,
    },
    /// Two nodes at a given distance: node 0 publishes, node 1 follows.
    TwoNodes {
        #[arg(long, default_value_t = 1000.0)]
        distance_m: f64,
        #[command(flatten)]
        common: Common,
    },
    /// N nodes in a square area with S stations and K sources; everyone follows every channel.
    Cell {
        #[arg(long, default_value_t = 50)]
        nodes: usize,
        #[arg(long, default_value_t = 4.0)]
        area_km2: f64,
        #[arg(long, default_value_t = 1)]
        stations: usize,
        #[arg(long, default_value_t = 1)]
        sources: usize,
        #[arg(long, default_value_t = 1.0)]
        follow_fraction: f64,
        #[command(flatten)]
        common: Common,
    },
    /// Two clusters of nodes with a gap between them: the source is in cluster A, the station in
    /// cluster B. Tests whether content crosses when only the cluster edges hear each other.
    Clusters {
        /// Nodes per cluster.
        #[arg(long, default_value_t = 10)]
        size: usize,
        /// Cluster radius in metres.
        #[arg(long, default_value_t = 300.0)]
        radius_m: f64,
        /// Distance between cluster centres in metres.
        #[arg(long, default_value_t = 1500.0)]
        distance_m: f64,
        #[command(flatten)]
        common: Common,
    },
    /// A living network: K channels, each node follows a few of them, subscriptions change over
    /// time, and every channel publishes a new bulletin periodically while dropping its oldest.
    Dynamics {
        #[arg(long, default_value_t = 50)]
        nodes: usize,
        #[arg(long, default_value_t = 1.0)]
        area_km2: f64,
        #[arg(long, default_value_t = 1)]
        stations: usize,
        /// Number of channels (each has its own source node).
        #[arg(long, default_value_t = 8)]
        channels: usize,
        /// Channels each node follows at the start.
        #[arg(long, default_value_t = 3)]
        follows: usize,
        /// Hours between two publications of the same channel.
        #[arg(long, default_value_t = 24.0)]
        publish_h: f64,
        /// Size of one bulletin in kB (300 kB is five minutes of Opus speech at 8 kbit/s).
        #[arg(long, default_value_t = 300)]
        bulletin_kb: u32,
        /// Objects a channel keeps in its manifest.
        #[arg(long, default_value_t = 3)]
        window: usize,
        /// Hours between subscription changes; each time 10 % of nodes swap one channel.
        #[arg(long, default_value_t = 6.0)]
        churn_h: f64,
        #[command(flatten)]
        common: Common,
    },
    /// Like `cell`, but the announcer is switched off at a given hour and back on later.
    Failover {
        #[arg(long, default_value_t = 20)]
        nodes: usize,
        #[arg(long, default_value_t = 1.0)]
        area_km2: f64,
        #[arg(long, default_value_t = 2.0)]
        kill_at_h: f64,
        #[arg(long, default_value_t = 6.0)]
        revive_at_h: f64,
        #[command(flatten)]
        common: Common,
    },
}

#[derive(Serialize)]
struct Report {
    spec: ScenarioSpec,
    phys: Vec<radio::Phy>,
    objects: Vec<ObjectSummary>,
    frames_sent: u64,
    frames_delivered: u64,
    frames_collided: u64,
    frames_half_duplex: u64,
    bulk_sent: u64,
    bulk_delivered: u64,
    announcers_final: Vec<u32>,
    role_events: usize,
    airtime_share: Vec<(usize, Vec<f64>)>,
    occupancy_p50_bulk: f64,
    occupancy_max_bulk: f64,
    delivered_bytes_per_hour_per_announcer: f64,
    failover: Option<FailoverReport>,
    core_stats: Vec<(usize, String)>,
    /// Per node: announcer followed, bulk frames received, bulk frames lost to collisions,
    /// tracks completed.
    per_node: Vec<(usize, u32, u64, u64, usize)>,
    collided_meeting: [u64; 6],
    collided_other: [u64; 6],
    bulk_collision_kinds: [[u64; 2]; 2],
    bulk_sent_by: [u64; 2],
    upload_same: u64,
    upload_other: u64,
}

#[derive(Serialize, Debug)]
struct FailoverReport {
    killed_node: usize,
    kill_at_h: f64,
    new_announcer: Option<u32>,
    recovered_after_s: Option<f64>,
    max_simultaneous_announcers_after: usize,
    announcer_after_revive: Option<u32>,
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Budget { exponent } => budget(exponent),
        Cmd::TwoNodes { distance_m, common } => {
            let spec = ScenarioSpec {
                nodes: 2,
                area_km2: 0.0,
                stations: 0,
                sources: 1,
                tracks: common.tracks,
                track_kb: common.track_kb,
                hours: common.hours,
                seed: common.seed,
                bulk: common.bulk,
                control_sf: common.control_sf,
                exponent: common.exponent,
                shadow_db: 0.0,
                follow_fraction: 1.0,
                positions: Some(vec![(0.0, 0.0), (distance_m, 0.0)]),
                stations_at: Some(vec![]),
                sources_at: Some(vec![0]),
            };
            run(spec, &common, None);
        }
        Cmd::Cell { nodes, area_km2, stations, sources, follow_fraction, common } => {
            let spec = ScenarioSpec {
                nodes,
                area_km2,
                stations,
                sources,
                tracks: common.tracks,
                track_kb: common.track_kb,
                hours: common.hours,
                seed: common.seed,
                bulk: common.bulk,
                control_sf: common.control_sf,
                exponent: common.exponent,
                shadow_db: common.shadow_db,
                follow_fraction,
                positions: None,
                stations_at: None,
                sources_at: None,
            };
            run(spec, &common, None);
        }
        Cmd::Clusters { size, radius_m, distance_m, common } => {
            let mut rng = meshcast_core::rng::Rng::new(common.seed ^ 0xC1);
            let mut positions = Vec::new();
            for cluster in 0..2 {
                let cx = cluster as f64 * distance_m;
                for _ in 0..size {
                    let r = radius_m * rng.unit().sqrt();
                    let a = rng.unit() * 2.0 * std::f64::consts::PI;
                    positions.push((cx + r * a.cos(), r * a.sin()));
                }
            }
            let n = positions.len();
            let spec = ScenarioSpec {
                nodes: n,
                area_km2: (distance_m + 2.0 * radius_m) * 2.0 * radius_m / 1e6,
                stations: 1,
                sources: 1,
                tracks: common.tracks,
                track_kb: common.track_kb,
                hours: common.hours,
                seed: common.seed,
                bulk: common.bulk,
                control_sf: common.control_sf,
                exponent: common.exponent,
                shadow_db: common.shadow_db,
                follow_fraction: 1.0,
                positions: Some(positions),
                stations_at: Some(vec![n - 1]),
                sources_at: Some(vec![0]),
            };
            run(spec, &common, None);
        }
        Cmd::Dynamics { nodes, area_km2, stations, channels, follows, publish_h, bulletin_kb, window, churn_h, common } => {
            run_dynamics(nodes, area_km2, stations, channels, follows, publish_h, bulletin_kb, window, churn_h, &common);
        }
        Cmd::Failover { nodes, area_km2, kill_at_h, revive_at_h, common } => {
            let spec = ScenarioSpec {
                nodes,
                area_km2,
                stations: 1,
                sources: 1,
                tracks: common.tracks,
                track_kb: common.track_kb,
                hours: common.hours,
                seed: common.seed,
                bulk: common.bulk,
                control_sf: common.control_sf,
                exponent: common.exponent,
                shadow_db: common.shadow_db,
                follow_fraction: 1.0,
                positions: None,
                stations_at: None,
                sources_at: None,
            };
            run(spec, &common, Some((kill_at_h, revive_at_h)));
        }
    }
}

fn budget(exponent: f64) {
    let prop = Propagation { exponent, shadow_sigma_db: 0.0 };
    println!("Link budget and throughput (path-loss exponent {exponent}, no shadowing, no fade margin)\n");
    println!("{:<42} {:>7} {:>9} {:>8} {:>10} {:>12} {:>10}", "carrier", "tx dBm", "sens dBm", "range", "raw kbit/s", "avg kbit/s", "MB/hour");
    let mut rows: Vec<(String, f64, f64, f64, f64, f64)> = Vec::new();
    for sf in [7u8, 9, 12] {
        let p = control_phy(Region::Eu868, sf);
        let raw = p.bitrate_bps as f64 / 1000.0;
        rows.push((p.name.clone(), p.tx_dbm, p.sensitivity_dbm, range_m(&p, &prop), raw, raw * 0.10));
    }
    for preset in [BulkPreset::GfskO, BulkPreset::GfskL, BulkPreset::EspNow, BulkPreset::GfskUs, BulkPreset::LoraBulk] {
        let p = bulk_phy(preset);
        let raw = p.bitrate_bps as f64 / 1000.0;
        let share = match preset {
            BulkPreset::GfskO => 0.10,
            BulkPreset::GfskL => 100.0 * 15.0 / 3600.0,
            BulkPreset::EspNow => 0.5,
            BulkPreset::GfskUs => 0.5,
            BulkPreset::LoraBulk => 0.10,
        };
        // Effective payload rate: 200 B payload per 218 B frame plus overhead.
        let frame_ms = p.to_core().airtime_ms(218) as f64;
        let payload_kbps = SYMBOL_SIZE as f64 * 8.0 / frame_ms;
        rows.push((p.name.clone(), p.tx_dbm, p.sensitivity_dbm, range_m(&p, &prop), raw, payload_kbps * share));
    }
    for (name, tx, sens, range, raw, avg) in rows {
        let r = if range >= 1000.0 { format!("{:.1} km", range / 1000.0) } else { format!("{:.0} m", range) };
        println!("{:<42} {:>7.0} {:>9.1} {:>8} {:>10.1} {:>12.1} {:>10.1}", name, tx, sens, r, raw, avg, avg * 3600.0 / 8.0 / 1000.0);
    }
    println!("\navg = after the regulatory share (10 % band O, 15×100 s/h band L, 50 % self-imposed where no duty cycle).");
    println!("An hour of Opus at 24 kbit/s is 10.8 MB; at 16 kbit/s mono 7.2 MB. Halve the ranges in a city.");
}

fn run(spec: ScenarioSpec, common: &Common, failover: Option<(f64, f64)>) {
    let params = Params::default();
    let mut built = build(&spec, params);
    built.engine.verbose = common.verbose;
    let until: Millis = (spec.hours * 3.6e6) as Millis;
    let mut fo_killed = None;
    if let Some((kill_h, revive_h)) = failover {
        // The station (highest score) is expected to be announcer; kill node index of station 0.
        let victim = (0..spec.nodes).find(|&i| built.engine.nodes[i].mains).unwrap_or(0);
        built.engine.schedule_kill(victim, (kill_h * 3.6e6) as Millis);
        built.engine.schedule_revive(victim, (revive_h * 3.6e6) as Millis);
        fo_killed = Some((victim, kill_h, revive_h));
    }
    let t0 = std::time::Instant::now();
    built.engine.run(until, 600_000);
    let wall = t0.elapsed();
    let eng = &built.engine;
    let m = &eng.metrics;

    // Objects.
    let mut objects = Vec::new();
    for (id, info) in &built.tracks {
        let mut times: Vec<f64> = info
            .followers
            .iter()
            .filter_map(|&f| m.completions.get(&(f, *id)).map(|&t| t as f64 / 3.6e6))
            .collect();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        objects.push(ObjectSummary {
            object: format!("{:?}", id),
            source: info.source,
            index: info.index,
            bytes: info.bytes,
            followers: info.followers.len(),
            complete: times.len(),
            p50_h: percentile(&times, 0.5),
            p90_h: percentile(&times, 0.9),
            max_h: times.last().copied(),
        });
    }
    objects.sort_by_key(|o| (o.source, o.index));

    // Announcers at the end.
    let bulk_c = eng.phys.iter().position(|p| p.kind != CarrierKind::LoraControl).unwrap_or(1);
    let announcers_final: Vec<u32> = eng.nodes.iter().filter(|n| n.alive && n.node.role(bulk_c) == meshcast_core::node::Role::Announcer).map(|n| n.node.id().0).collect();

    // Airtime share per node per carrier.
    let hours = spec.hours.max(1e-9);
    let mut airtime_share: Vec<(usize, Vec<f64>)> = eng
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.airtime_ms.iter().sum::<u64>() as f64 / (hours * 3.6e6) > 0.0001)
        .map(|(i, n)| (i, n.airtime_ms.iter().map(|&a| a as f64 / (hours * 3.6e6)).collect()))
        .collect();
    airtime_share.sort_by(|a, b| b.1.iter().sum::<f64>().partial_cmp(&a.1.iter().sum::<f64>()).unwrap());

    // Occupancy on the bulk carrier.
    let mut occ: Vec<f64> = m.occupancy_samples.iter().filter(|s| s.2 == bulk_c).map(|s| s.3 as f64 / 10.0).collect();
    occ.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let occ_p50 = percentile(&occ, 0.5).unwrap_or(0.0);
    let occ_max = occ.last().copied().unwrap_or(0.0);

    // Delivered bytes per announcer-hour: unique (node, object) completions × bytes / announcer hours.
    let delivered_bytes: f64 = m.completions.keys().filter_map(|(_, id)| built.tracks.get(id).map(|t| t.bytes as f64)).sum();
    let announcer_hours: f64 = airtime_share.iter().map(|(_, s)| s[bulk_c]).sum::<f64>().max(1e-9) * hours;
    let _ = announcer_hours;
    let n_ann = announcers_final.len().max(1) as f64;
    let dbph = delivered_bytes / hours / n_ann;

    // Failover analysis.
    let fo = fo_killed.map(|(victim, kill_h, revive_h)| {
        let kill_ms = (kill_h * 3.6e6) as Millis;
        let revive_ms = (revive_h * 3.6e6) as Millis;
        let victim_id = eng.nodes[victim].node.id().0;
        let first_new = m.role_events.iter().find(|e| e.t_ms >= kill_ms && e.role == "Announcer" && e.node != victim_id);
        // Count simultaneous announcers after the kill by replaying role events.
        let mut current: BTreeMap<u32, bool> = BTreeMap::new();
        let mut max_sim = 0usize;
        for e in &m.role_events {
            if e.carrier != bulk_c {
                continue;
            }
            current.insert(e.node, e.role == "Announcer");
            if e.t_ms >= kill_ms {
                current.insert(victim_id, e.node == victim_id && e.role == "Announcer" && e.t_ms >= revive_ms);
                let n = current.values().filter(|&&a| a).count();
                max_sim = max_sim.max(n);
            }
        }
        let after_revive = m.role_events.iter().filter(|e| e.t_ms >= revive_ms && e.role == "Announcer").last().map(|e| e.node);
        FailoverReport {
            killed_node: victim,
            kill_at_h: kill_h,
            new_announcer: first_new.map(|e| e.node),
            recovered_after_s: first_new.map(|e| (e.t_ms - kill_ms) as f64 / 1000.0),
            max_simultaneous_announcers_after: max_sim,
            announcer_after_revive: after_revive,
        }
    });

    let core_stats: Vec<(usize, String)> = eng
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, n)| n.airtime_ms.iter().any(|&a| a > 0) || *i < 3)
        .map(|(i, n)| {
            let s = &n.node.stats;
            let line = format!("tx[ctl/meta/content]={:?} rx={} bad={} cca_defer={} disc_wait={} sym_new={} sym_dup={} nacks={} wants={} uploads={} repairs={} occ={}‰ rate={}‰ role={:?} colour={:?} reports={} noted={} conflicts={:?}",
                s.tx_frames, s.rx_frames, s.rx_bad, s.cca_deferrals, s.discipline_waits, s.symbols_new, s.symbols_dup, s.nacks_sent, s.wants_sent, s.uploads_started, s.repairs_queued,
                n.node.occupancy(bulk_c), n.node.rate(bulk_c), n.node.role(bulk_c), n.node.colouring(), s.conflict_reports_sent, s.conflicts_noted, n.node.conflict_set())
                + &format!(" defer[meet/slot/reg/bucket/class/cca]={:?}", s.defer_ms);
            (i, line)
        })
        .collect();

    let per_node: Vec<(usize, u32, u64, u64, usize)> = eng
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let done = built.tracks.keys().filter(|id| m.completions.contains_key(&(i, **id))).count();
            let (ok, lost) = m.per_node_bulk.get(i).copied().unwrap_or((0, 0));
            (i, n.node.announcer_of(bulk_c).0, ok, lost, done)
        })
        .collect();
    let report = Report {
        collided_meeting: m.collided_meeting,
        collided_other: m.collided_other,
        bulk_collision_kinds: m.bulk_collision_kinds,
        bulk_sent_by: m.bulk_sent_by,
        upload_same: m.upload_collision_same_object,
        upload_other: m.upload_collision_other_object,
        per_node,
        spec: spec.clone(),
        phys: built.phys.clone(),
        objects,
        frames_sent: m.frames_sent,
        frames_delivered: m.frames_delivered,
        frames_collided: m.frames_collided,
        frames_half_duplex: m.frames_half_duplex,
        bulk_sent: m.bulk_sent,
        bulk_delivered: m.bulk_delivered,
        announcers_final,
        role_events: m.role_events.len(),
        airtime_share,
        occupancy_p50_bulk: occ_p50,
        occupancy_max_bulk: occ_max,
        delivered_bytes_per_hour_per_announcer: dbph,
        failover: fo,
        core_stats,
    };
    print_report(&report, wall);
    if let Some(path) = &common.out {
        fs::write(path, serde_json::to_string_pretty(&report).unwrap()).expect("write report");
        println!("\nfull report written to {path}");
    }
}

fn print_report(r: &Report, wall: std::time::Duration) {
    println!("MeshCast sim: {} nodes, {:.1} km², {} station(s), {} source(s) × {} tracks of {} kB, {} h simulated in {:.1?}",
        r.spec.nodes, r.spec.area_km2, r.spec.stations, r.spec.sources, r.spec.tracks, r.spec.track_kb, r.spec.hours, wall);
    for p in &r.phys {
        println!("  carrier: {}", p.name);
    }
    println!("\nframes: sent {} delivered {} collided {} half-duplex {} | bulk sent {} delivered {}",
        r.frames_sent, r.frames_delivered, r.frames_collided, r.frames_half_duplex, r.bulk_sent, r.bulk_delivered);
    println!("announcers at end: {:?} ({} role events)", r.announcers_final, r.role_events);
    println!("collisions by frame type [beacon,bulk,gossip,announce,nack]: meeting dwell {:?}, other {:?}", &r.collided_meeting[1..], &r.collided_other[1..]);
    println!("bulk sent by [others, announcers]: {:?}; bulk collisions [sender other/announcer][interferer other/announcer]: {:?}; upload-upload same object {} / other object {}", r.bulk_sent_by, r.bulk_collision_kinds, r.upload_same, r.upload_other);
    println!("bulk-channel occupancy at nodes: p50 {:.1} %, max {:.1} %", r.occupancy_p50_bulk, r.occupancy_max_bulk);
    println!("delivered to followers: {:.2} MB per hour per announcer", r.delivered_bytes_per_hour_per_announcer / 1e6);
    println!("\nairtime share per transmitting node, busiest first (control, bulk):");
    for (i, s) in r.airtime_share.iter().take(12) {
        println!("  node {:>4}: {}", i, s.iter().map(|x| format!("{:.2} %", x * 100.0)).collect::<Vec<_>>().join(", "));
    }
    if r.airtime_share.len() > 12 {
        println!("  ... {} more", r.airtime_share.len() - 12);
    }
    println!("\nobjects (completion time over followers, hours):");
    println!("  {:<10} {:>4} {:>7} {:>9} {:>9} {:>8} {:>8} {:>8}", "object", "src", "kB", "follow", "complete", "p50 h", "p90 h", "max h");
    for o in &r.objects {
        let f = |x: Option<f64>| x.map(|v| format!("{v:.2}")).unwrap_or_else(|| "-".into());
        println!("  {:<10} {:>4} {:>7} {:>9} {:>9} {:>8} {:>8} {:>8}", o.object, o.source, o.bytes / 1024, o.followers, o.complete, f(o.p50_h), f(o.p90_h), f(o.max_h));
    }
    if let Some(fo) = &r.failover {
        println!("\nfailover: {:?}", fo);
    }
    println!("\ncore stats (transmitting nodes and the first three):");
    for (i, s) in &r.core_stats {
        println!("  node {:>4}: {}", i, s);
    }
}


#[derive(Serialize)]
struct DynamicsReport {
    nodes: usize,
    channels: usize,
    follows: usize,
    publish_h: f64,
    churn_h: f64,
    hours: f64,
    publications: usize,
    /// Per publication: hours after publication at which p50 / p90 of the followers had it.
    latency_p50_h: Vec<Option<f64>>,
    latency_p90_h: Vec<Option<f64>>,
    delivered_within_period: f64,
    wasted_bulk_fraction: f64,
    orphaned_objects_mean: f64,
    uploads: u64,
    announcers_final: usize,
    role_events: usize,
    airtime_top: Vec<(usize, f64)>,
}

#[allow(clippy::too_many_arguments)]
fn run_dynamics(nodes: usize, area_km2: f64, stations: usize, channels: usize, follows: usize, publish_h: f64, bulletin_kb: u32, window: usize, churn_h: f64, common: &Common) {
    use meshcast_core::manifest::{Manifest, ScheduleEntry};
    use meshcast_core::rng::Rng;
    use meshcast_core::ids::ShortId;
    use scenario::{track_object, TrackInfo};
    let spec = ScenarioSpec {
        nodes,
        area_km2,
        stations,
        sources: channels,
        tracks: window,
        track_kb: bulletin_kb,
        hours: common.hours,
        seed: common.seed,
        bulk: common.bulk,
        control_sf: common.control_sf,
        exponent: common.exponent,
        shadow_db: common.shadow_db,
        follow_fraction: 0.0,
        positions: None,
        stations_at: None,
        sources_at: None,
    };
    let params = Params::default();
    let mut built = build(&spec, params);
    built.engine.verbose = common.verbose;
    let mut rng = Rng::new(common.seed ^ 0xD1);
    // Initial subscriptions: each node follows `follows` distinct channels.
    let mut subs: Vec<Vec<usize>> = vec![Vec::new(); nodes];
    for i in 0..nodes {
        let mut choice: Vec<usize> = (0..channels).collect();
        for k in (1..choice.len()).rev() {
            let j = rng.below(k as u64 + 1) as usize;
            choice.swap(k, j);
        }
        for &c in choice.iter().take(follows.min(channels)) {
            if built.sources[c].node != i {
                subs[i].push(c);
                built.engine.nodes[i].node.follow(built.sources[c].channel);
            }
        }
    }
    // The initial catalogue counts as publications at t = 0 with the initial followers.
    let mut pubs: Vec<(ShortId, Millis, Vec<usize>)> = Vec::new();
    for (c, src) in built.sources.iter().enumerate() {
        let followers: Vec<usize> = (0..nodes).filter(|&i| subs[i].contains(&c)).collect();
        for o in &src.objects {
            pubs.push((o.id.short(), 0, followers.clone()));
        }
    }
    let until: Millis = (common.hours * 3.6e6) as Millis;
    let publish_ms = (publish_h * 3.6e6) as Millis;
    let churn_ms = (churn_h * 3.6e6) as Millis;
    // Event schedule: publications staggered over the period, churn on its own cadence.
    let mut events: Vec<(Millis, u8, usize)> = Vec::new();
    for c in 0..channels {
        let mut t = publish_ms * (c as Millis + 1) / channels as Millis;
        while t < until {
            events.push((t, 0, c));
            t += publish_ms;
        }
    }
    let mut t = churn_ms;
    while t < until {
        events.push((t, 1, 0));
        t += churn_ms;
    }
    events.sort();
    let t0 = std::time::Instant::now();
    let mut next_index: Vec<usize> = built.sources.iter().map(|s| s.objects.len()).collect();
    for (t, kind, c) in events {
        built.engine.run(t, 600_000);
        match kind {
            0 => {
                let src = &mut built.sources[c];
                let o = track_object(common.seed, src.node, next_index[c], bulletin_kb * 1024);
                next_index[c] += 1;
                src.objects.push(o.clone());
                while src.objects.len() > window {
                    src.objects.remove(0);
                }
                src.seq += 1;
                let schedule: Vec<ScheduleEntry> = src.objects.iter().enumerate().map(|(k, o)| ScheduleEntry { object: o.id.short(), start: t / 1000 + 3600 * k as u64, repeat: 0 }).collect();
                let m = Manifest::sign(&src.key, src.seq, &format!("Channel {c}"), src.objects.clone(), schedule, None);
                let node = src.node;
                built.engine.nodes[node].node.publish(&m, &[(o.meta(), None)]);
                built.engine.poke(node);
                let followers: Vec<usize> = (0..nodes).filter(|&i| subs[i].contains(&c)).collect();
                built.tracks.insert(o.id.short(), TrackInfo { source: node, index: next_index[c] - 1, bytes: o.len, followers: followers.clone() });
                pubs.push((o.id.short(), t, followers));
            }
            _ => {
                let n_swap = (nodes / 10).max(1);
                for _ in 0..n_swap {
                    let i = rng.below(nodes as u64) as usize;
                    if subs[i].is_empty() || channels < 2 {
                        continue;
                    }
                    let drop_idx = rng.below(subs[i].len() as u64) as usize;
                    let dropped = subs[i].remove(drop_idx);
                    built.engine.nodes[i].node.unfollow(built.sources[dropped].channel);
                    let mut add = rng.below(channels as u64) as usize;
                    let mut guard = 0;
                    while (subs[i].contains(&add) || add == dropped || built.sources[add].node == i) && guard < 20 {
                        add = rng.below(channels as u64) as usize;
                        guard += 1;
                    }
                    if !subs[i].contains(&add) && built.sources[add].node != i {
                        subs[i].push(add);
                        built.engine.nodes[i].node.follow(built.sources[add].channel);
                    }
                    built.engine.poke(i);
                }
            }
        }
    }
    built.engine.run(until, 600_000);
    let wall = t0.elapsed();
    let eng = &built.engine;
    let m = &eng.metrics;
    let mut p50 = Vec::new();
    let mut p90 = Vec::new();
    let mut within = 0usize;
    let mut total = 0usize;
    for (id, t_pub, followers) in &pubs {
        let mut lat: Vec<f64> = followers.iter().filter_map(|&f| m.completions.get(&(f, *id)).map(|&t| (t.saturating_sub(*t_pub)) as f64 / 3.6e6)).collect();
        lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
        total += followers.len();
        within += lat.iter().filter(|&&l| l <= publish_h).count();
        p50.push(percentile(&lat, 0.5));
        p90.push(percentile(&lat, 0.9));
    }
    let wasted: u64 = eng.nodes.iter().map(|n| n.node.stats.bulk_uninterested).sum();
    let useful: u64 = eng.nodes.iter().map(|n| n.node.stats.symbols_new + n.node.stats.symbols_dup).sum();
    let orphans: f64 = eng.nodes.iter().map(|n| n.node.orphaned_objects().len() as f64).sum::<f64>() / nodes as f64;
    let uploads: u64 = eng.nodes.iter().map(|n| n.node.stats.uploads_started).sum();
    let bulk_c = eng.phys.iter().position(|p| p.kind != CarrierKind::LoraControl).unwrap_or(1);
    let announcers_final = eng.nodes.iter().filter(|n| n.alive && n.node.role(bulk_c) == meshcast_core::node::Role::Announcer).count();
    let hours = common.hours.max(1e-9);
    let mut airtime: Vec<(usize, f64)> = eng.nodes.iter().enumerate().map(|(i, n)| (i, n.airtime_ms[bulk_c] as f64 / (hours * 3.6e6))).filter(|(_, a)| *a > 0.0005).collect();
    airtime.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    airtime.truncate(8);
    let report = DynamicsReport {
        nodes,
        channels,
        follows,
        publish_h,
        churn_h,
        hours: common.hours,
        publications: pubs.len(),
        latency_p50_h: p50.clone(),
        latency_p90_h: p90.clone(),
        delivered_within_period: if total > 0 { within as f64 / total as f64 } else { 0.0 },
        wasted_bulk_fraction: if wasted + useful > 0 { wasted as f64 / (wasted + useful) as f64 } else { 0.0 },
        orphaned_objects_mean: orphans,
        uploads,
        announcers_final,
        role_events: m.role_events.len(),
        airtime_top: airtime.clone(),
    };
    println!("MeshCast dynamics: {} nodes, {} channels, {} follows each, publish every {} h, churn every {} h, {} h simulated in {:.1?}", nodes, channels, follows, publish_h, churn_h, common.hours, wall);
    println!("  carrier: {}", eng.phys[bulk_c].name);
    println!("publications: {} (initial catalogue + {} later)", pubs.len(), pubs.len().saturating_sub(channels * window));
    println!("delivered to followers within one publication period: {:.1} %", report.delivered_within_period * 100.0);
    let f = |x: &Option<f64>| x.map(|v| format!("{v:.2}")).unwrap_or_else(|| "-".into());
    let later: Vec<String> = pubs.iter().zip(p50.iter().zip(p90.iter())).filter(|((_, t, _), _)| *t > 0).map(|((_, t, fl), (a, b))| format!("t={:.0}h n={} p50={} p90={}", *t as f64 / 3.6e6, fl.len(), f(a), f(b))).collect();
    println!("latency of later publications (hours after publishing):");
    for l in later.iter().take(24) {
        println!("  {l}");
    }
    if later.len() > 24 {
        println!("  ... {} more", later.len() - 24);
    }
    println!("bulk frames received by uninterested nodes: {:.1} % of all bulk receptions", report.wasted_bulk_fraction * 100.0);
    println!("orphaned objects per node at the end (no manifest references them): {:.1}", orphans);
    println!("uploads {}, announcers {}, role events {}", uploads, announcers_final, m.role_events.len());
    println!("bulk airtime share, busiest nodes: {}", airtime.iter().map(|(i, a)| format!("{i}:{:.1}%", a * 100.0)).collect::<Vec<_>>().join(" "));
    if let Some(path) = &common.out {
        fs::write(path, serde_json::to_string_pretty(&report).unwrap()).expect("write report");
        println!("full report written to {path}");
    }
}
