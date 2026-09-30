use meshcast_sim::{metrics, radio, scenario};

use std::collections::BTreeMap;
use std::fs;

use clap::{Parser, Subcommand};
use meshcast_core::frame::{CarrierKind, SYMBOL_SIZE};
use meshcast_core::object::ContentType;
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
    /// Publishing mix that overrides --track-kb, e.g.
    /// "snac-music:42,snac-speech:22,opus-music:540,opus-speech:180".
    #[arg(long)]
    mix: Option<String>,
    /// Run this many seeds (seed, seed+1, ...) in parallel and report the spread. A single run
    /// is one throw of the dice; compare designs on ensembles.
    #[arg(long, default_value_t = 1)]
    seeds: u64,
    /// Write full metrics as JSON here.
    #[arg(long)]
    out: Option<String>,
    /// Print role changes as they happen.
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

impl Common {
    fn mix_items(&self) -> Vec<scenario::MixItem> {
        self.mix.as_deref().map(|m| scenario::parse_mix(m).unwrap_or_else(|e| panic!("--mix: {e}"))).unwrap_or_default()
    }
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
        /// Hours between node comings and goings; each time 10 % of nodes switch off or on.
        #[arg(long, default_value_t = 0.0)]
        node_churn_h: f64,
        /// Hours between newcomers: a node is replaced by one that has learned nothing.
        #[arg(long, default_value_t = 0.0)]
        newcomer_h: f64,
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
    by_kind: Vec<KindSummary>,
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

/// Delivery per kind of object in the publishing mix.
#[derive(Serialize, Clone)]
struct KindSummary {
    label: String,
    objects: usize,
    kb: u32,
    /// Follower completions over follower-object pairs.
    complete: f64,
    /// Mean over objects of each object's median completion time.
    p50_mean_h: Option<f64>,
    /// Worst object's 90th-percentile completion time.
    p90_max_h: Option<f64>,
}

fn by_kind(objects: &[ObjectSummary]) -> Vec<KindSummary> {
    let mut labels: Vec<String> = Vec::new();
    for o in objects {
        if !labels.contains(&o.label) {
            labels.push(o.label.clone());
        }
    }
    labels
        .into_iter()
        .map(|label| {
            let os: Vec<&ObjectSummary> = objects.iter().filter(|o| o.label == label).collect();
            let pairs: usize = os.iter().map(|o| o.followers).sum();
            let done: usize = os.iter().map(|o| o.complete).sum();
            let p50: Vec<f64> = os.iter().filter_map(|o| o.p50_h).collect();
            KindSummary {
                objects: os.len(),
                kb: os[0].bytes / 1024,
                complete: if pairs > 0 { done as f64 / pairs as f64 } else { 0.0 },
                p50_mean_h: if p50.len() == os.len() && !p50.is_empty() { Some(p50.iter().sum::<f64>() / p50.len() as f64) } else { None },
                p90_max_h: if os.iter().all(|o| o.p90_h.is_some()) { os.iter().filter_map(|o| o.p90_h).fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.max(x)))) } else { None },
                label,
            }
        })
        .collect()
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
                mix: common.mix_items(),
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
                mix: common.mix_items(),
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
                mix: common.mix_items(),
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
        Cmd::Dynamics { nodes, area_km2, stations, channels, follows, publish_h, bulletin_kb, window, churn_h, node_churn_h, newcomer_h, common } => {
            run_dynamics(nodes, area_km2, stations, channels, follows, publish_h, bulletin_kb, window, churn_h, node_churn_h, newcomer_h, &common);
        }
        Cmd::Failover { nodes, area_km2, kill_at_h, revive_at_h, common } => {
            let spec = ScenarioSpec {
                nodes,
                area_km2,
                stations: 1,
                sources: 1,
                tracks: common.tracks,
                track_kb: common.track_kb,
                mix: common.mix_items(),
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
    if common.seeds <= 1 {
        let (report, wall) = simulate(spec, common.verbose, failover);
        print_report(&report, wall);
        if let Some(path) = &common.out {
            fs::write(path, serde_json::to_string_pretty(&report).unwrap()).expect("write report");
            println!("\nfull report written to {path}");
        }
        return;
    }
    let seeds: Vec<u64> = (0..common.seeds).map(|i| spec.seed + i).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(seeds.len());
    let t0 = std::time::Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&seed) = seeds.get(i) else { break };
                let mut s = spec.clone();
                s.seed = seed;
                let (report, _) = simulate(s, false, failover);
                results.lock().unwrap().push(report);
            });
        }
    });
    let mut reports = results.into_inner().unwrap();
    reports.sort_by_key(|r| r.spec.seed);
    let ensemble = Ensemble::of(&reports);
    ensemble.print(t0.elapsed());
    if let Some(path) = &common.out {
        fs::write(path, serde_json::to_string_pretty(&ensemble).unwrap()).expect("write report");
        println!("\nensemble written to {path}");
    }
}

fn simulate(spec: ScenarioSpec, verbose: bool, failover: Option<(f64, f64)>) -> (Report, std::time::Duration) {
    let params = Params::default();
    let mut built = build(&spec, params);
    built.engine.verbose = verbose;
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
            label: info.label.clone(),
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
    let kinds = by_kind(&objects);

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
            let (known, done, short) = n.node.inventory();
            let mut s2 = short.clone();
            s2.sort_unstable();
            let med = s2.get(s2.len() / 2).copied().unwrap_or(0);
            let served = s.symbols_served + s.symbols_overheard;
            let line = line + &format!(" holds={}/{} short_median={:.1}% served={:.0}%", done, known, med as f64 / 10.0,
                if served > 0 { 100.0 * s.symbols_served as f64 / served as f64 } else { 0.0 });
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
        by_kind: kinds,
        occupancy_p50_bulk: occ_p50,
        occupancy_max_bulk: occ_max,
        delivered_bytes_per_hour_per_announcer: dbph,
        failover: fo,
        core_stats,
    };
    if std::env::var("MESHCAST_DEBUG_WANTS").is_ok() {
        for (i, n) in eng.nodes.iter().enumerate() {
            if n.node.role(bulk_c) == meshcast_core::node::Role::Announcer || i < 3 {
                println!("WANTS node {i} (id {}): {}", n.node.id().0, n.node.want_report());
            }
            let missing: Vec<String> = built.tracks.iter().filter(|(id, t)| t.followers.contains(&i) && !m.completions.contains_key(&(i, **id))).map(|(id, _)| format!("{:?}:{}", id, if n.node.holds(id) { "held" } else { "absent" })).collect();
            if !missing.is_empty() {
                println!("UNRECORDED node {i} (id {}): {}", n.node.id().0, missing.join(" "));
            }
        }
    }
    (report, wall)
}

/// One seed's headline numbers, kept in an ensemble.
#[derive(Serialize)]
struct SeedLine {
    seed: u64,
    delivered: f64,
    bulk_sent: u64,
    by_kind: Vec<KindSummary>,
}

/// Several seeds of one scenario: the spread is the result, not any single run.
#[derive(Serialize)]
struct Ensemble {
    spec: ScenarioSpec,
    seeds: Vec<SeedLine>,
}

fn delivered(r: &Report) -> f64 {
    let pairs: usize = r.objects.iter().map(|o| o.followers).sum();
    let done: usize = r.objects.iter().map(|o| o.complete).sum();
    if pairs > 0 { done as f64 / pairs as f64 } else { 0.0 }
}

impl Ensemble {
    fn of(reports: &[Report]) -> Ensemble {
        Ensemble {
            spec: reports[0].spec.clone(),
            seeds: reports
                .iter()
                .map(|r| SeedLine { seed: r.spec.seed, delivered: delivered(r), bulk_sent: r.bulk_sent, by_kind: by_kind(&r.objects) })
                .collect(),
        }
    }

    fn print(&self, wall: std::time::Duration) {
        let n = self.seeds.len();
        let d: Vec<f64> = self.seeds.iter().map(|s| s.delivered * 100.0).collect();
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let min = |v: &[f64]| v.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = |v: &[f64]| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let s = &self.spec;
        println!("ensemble of {n} seeds ({}..{}), {} nodes on {} km², {} h, {:.0} s wall",
            self.seeds[0].seed, self.seeds[n - 1].seed, s.nodes, s.area_km2, s.hours, wall.as_secs_f64());
        println!("  delivered      mean {:5.1} %  min {:5.1} %  max {:5.1} %   per seed: {}", mean(&d), min(&d), max(&d),
            d.iter().map(|x| format!("{x:.1}")).collect::<Vec<_>>().join(" "));
        let frames: Vec<f64> = self.seeds.iter().map(|s| s.bulk_sent as f64).collect();
        println!("  bulk frames    mean {:.0}  min {:.0}  max {:.0}", mean(&frames), min(&frames), max(&frames));
        for (i, k) in self.seeds[0].by_kind.iter().enumerate() {
            let c: Vec<f64> = self.seeds.iter().map(|s| s.by_kind[i].complete * 100.0).collect();
            let p50: Vec<f64> = self.seeds.iter().filter_map(|s| s.by_kind[i].p50_mean_h).map(|h| h * 60.0).collect();
            let p90: Vec<f64> = self.seeds.iter().filter_map(|s| s.by_kind[i].p90_max_h).map(|h| h * 60.0).collect();
            let m = |v: &[f64]| if v.len() == n { format!("{:6.1} min (min {:5.1}, max {:5.1})", mean(v), min(v), max(v)) } else { format!("{} of {n} seeds complete", v.len()) };
            println!("  {:<12} {:>4} kB  complete mean {:5.1} % min {:5.1} %  median {}  worst p90 {}", k.label, k.kb, mean(&c), min(&c), m(&p50), m(&p90));
        }
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
    println!("\nby kind (follower completions; mean of per-object median; worst per-object p90):");
    for k in &r.by_kind {
        let f = |x: Option<f64>| x.map(|v| format!("{v:.2} h")).unwrap_or_else(|| "-".into());
        println!("  {:<12} {:>3} objects of {:>4} kB: {:>6.1} % complete, p50 {:>8}, p90 {:>8}", k.label, k.objects, k.kb, k.complete * 100.0, f(k.p50_mean_h), f(k.p90_max_h));
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
fn run_dynamics(nodes: usize, area_km2: f64, stations: usize, channels: usize, follows: usize, publish_h: f64, bulletin_kb: u32, window: usize, churn_h: f64, node_churn_h: f64, newcomer_h: f64, common: &Common) {
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
        mix: vec![scenario::MixItem { label: "speech".into(), kb: bulletin_kb }],
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
    if node_churn_h > 0.0 {
        let step = (node_churn_h * 3.6e6) as Millis;
        let mut t = step;
        while t < until {
            events.push((t, 2, 0));
            t += step;
        }
    }
    if newcomer_h > 0.0 {
        let step = (newcomer_h * 3.6e6) as Millis;
        let mut t = step;
        while t < until {
            events.push((t, 3, 0));
            t += step;
        }
    }
    events.sort();
    let mut offline: Vec<usize> = Vec::new();
    // Newcomer, when it joined, and the catalogue that existed at that moment: a cold start is
    // measured against what was there to fetch, not against bulletins published later.
    let mut newcomers: Vec<(usize, Millis, Vec<ShortId>)> = Vec::new();
    let t0 = std::time::Instant::now();
    let mut next_index: Vec<usize> = built.sources.iter().map(|s| s.objects.len()).collect();
    for (t, kind, c) in events {
        built.engine.run(t, 600_000);
        match kind {
            0 => {
                let src = &mut built.sources[c];
                let o = track_object(common.seed, src.node, next_index[c], bulletin_kb * 1024, ContentType::Speech);
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
                built.tracks.insert(o.id.short(), TrackInfo { label: "speech".into(), source: node, index: next_index[c] - 1, bytes: o.len, followers: followers.clone() });
                pubs.push((o.id.short(), t, followers));
            }
            2 => {
                // A tenth of the nodes go away or come back: batteries, pockets, switches.
                let n_toggle = (nodes / 10).max(1);
                for _ in 0..n_toggle {
                    let i = rng.below(nodes as u64) as usize;
                    if built.sources.iter().any(|s| s.node == i) {
                        continue; // a publisher that vanishes has nothing to measure against
                    }
                    if let Some(pos) = offline.iter().position(|&x| x == i) {
                        offline.remove(pos);
                        built.engine.set_alive(i, true);
                    } else {
                        offline.push(i);
                        built.engine.set_alive(i, false);
                    }
                }
            }
            3 => {
                // Someone new joins: same place, nothing learned, follows a few channels.
                let mut i = rng.below(nodes as u64) as usize;
                let mut guard = 0;
                while (built.sources.iter().any(|s| s.node == i) || offline.contains(&i)) && guard < 20 {
                    i = rng.below(nodes as u64) as usize;
                    guard += 1;
                }
                if built.sources.iter().any(|s| s.node == i) {
                    continue;
                }
                built.engine.replace_with_newcomer(i);
                subs[i].clear();
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
                built.engine.poke(i);
                let at_join: Vec<ShortId> = subs[i].iter().flat_map(|&c| built.sources[c].objects.iter().map(|o| o.id.short()).collect::<Vec<_>>()).collect();
                newcomers.push((i, t, at_join));
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
    // A newcomer is caught up when it holds every object of every channel it follows.
    let mut caught = 0usize;
    let mut catch_h: Vec<f64> = Vec::new();
    for (i, t_join, at_join) in &newcomers {
        if at_join.is_empty() {
            continue;
        }
        let times: Vec<Millis> = at_join.iter().filter_map(|id| eng.metrics.completions.get(&(*i, *id)).copied()).collect();
        if times.len() == at_join.len() {
            caught += 1;
            if let Some(mx) = times.iter().max() {
                catch_h.push(mx.saturating_sub(*t_join) as f64 / 3.6e6);
            }
        }
    }
    // A node that was switched off when something was published cannot have received it, and
    // counting that as a failure measures the batteries, not the protocol. So also ask the
    // steady-state question: of the nodes that are on at the end, how many hold the whole
    // current window of every channel they follow?
    let mut up_to_date = 0usize;
    let mut online = 0usize;
    for i in 0..nodes {
        if !eng.nodes[i].alive || subs[i].is_empty() || built.sources.iter().any(|s| s.node == i) {
            continue;
        }
        online += 1;
        let ok = subs[i].iter().all(|&c| built.sources[c].objects.iter().all(|o| eng.nodes[i].node.store.has_complete(&o.id.short())));
        if ok {
            up_to_date += 1;
        }
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
    let sum = |f: fn(&meshcast_core::node::Stats) -> u64| eng.nodes.iter().map(|n| f(&n.node.stats)).sum::<u64>();
    println!("  of the uploads, repair answers {}; grants given {}, lapsed {}", sum(|s| s.repairs_started), sum(|s| s.grants_given), sum(|s| s.grants_lapsed));
    let (gc, gl) = (sum(|s| s.grants_completed).max(1) as f64, sum(|s| s.grants_lapsed).max(1) as f64);
    let rc: i64 = eng.nodes.iter().map(|n| n.node.stats.grant_rssi_completed).sum();
    let rl: i64 = eng.nodes.iter().map(|n| n.node.stats.grant_rssi_lapsed).sum();
    let uo = eng.metrics.upload_outcome;
    let ut = uo.iter().sum::<u64>().max(1) as f64;
    println!("  upload frames at their announcer: delivered {:.0} %, collided {:.0} %, announcer transmitting {:.0} %, announcer on another channel {:.0} %, too weak {:.0} %, announcer off {:.0} % (of {})",
        uo[0] as f64 * 100.0 / ut, uo[1] as f64 * 100.0 / ut, uo[2] as f64 * 100.0 / ut, uo[3] as f64 * 100.0 / ut, uo[4] as f64 * 100.0 / ut, uo[5] as f64 * 100.0 / ut, uo.iter().sum::<u64>());
    let ui = eng.metrics.upload_interferer;
    println!("  collided uploads broken by: an uploader to the same announcer {}, an uploader to another {}, an announcer {}, other {}", ui[0], ui[1], ui[2], ui[3]);
    println!("  holder heard by the announcer: completed grants {:.1} dBm mean, lapsed grants {:.1} dBm mean", rc as f64 / gc, rl as f64 / gl);
    if !newcomers.is_empty() {
        let mean = if catch_h.is_empty() { 0.0 } else { catch_h.iter().sum::<f64>() / catch_h.len() as f64 };
        let worst = catch_h.iter().cloned().fold(0.0f64, f64::max);
        println!("newcomers: {} joined, {} fetched the whole catalogue that existed when they joined, mean {:.2} h, worst {:.2} h", newcomers.len(), caught, mean, worst);
    }
    println!("of the {} follower nodes on at the end, {} hold the current window of every channel they follow ({:.1} %)", online, up_to_date, if online > 0 { 100.0 * up_to_date as f64 / online as f64 } else { 0.0 });
    if !offline.is_empty() {
        println!("nodes offline at the end: {}", offline.len());
    }
    println!("bulk airtime share, busiest nodes: {}", airtime.iter().map(|(i, a)| format!("{i}:{:.1}%", a * 100.0)).collect::<Vec<_>>().join(" "));
    if let Some(path) = &common.out {
        fs::write(path, serde_json::to_string_pretty(&report).unwrap()).expect("write report");
        println!("full report written to {path}");
    }
}
