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
            (i, format!("tx[ctl/meta/content]={:?} rx={} bad={} cca_defer={} disc_wait={} sym_new={} sym_dup={} nacks={} wants={} uploads={} manifests={} occ={}‰ rate={}‰ role={:?}",
                s.tx_frames, s.rx_frames, s.rx_bad, s.cca_deferrals, s.discipline_waits, s.symbols_new, s.symbols_dup, s.nacks_sent, s.wants_sent, s.uploads_started, s.manifests_adopted,
                n.node.occupancy(bulk_c), n.node.rate(bulk_c), n.node.role(bulk_c)))
        })
        .collect();

    let report = Report {
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
