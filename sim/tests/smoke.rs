//! End-to-end smoke tests: the real core delivers content through modelled radios.

use meshcast_core::params::Params;
use meshcast_sim::radio::BulkPreset;
use meshcast_sim::scenario::{build, ScenarioSpec};

fn spec(bulk: BulkPreset, positions: Vec<(f64, f64)>, sources: Vec<usize>, stations: Vec<usize>, hours: f64) -> ScenarioSpec {
    ScenarioSpec {
        nodes: positions.len(),
        area_km2: 1.0,
        stations: stations.len(),
        sources: sources.len(),
        tracks: 2,
        track_kb: 200,
        mix: Vec::new(),
        hours,
        seed: 42,
        bulk,
        control_sf: 7,
        exponent: 3.0,
        shadow_db: 0.0,
        follow_fraction: 1.0,
        positions: Some(positions),
        stations_at: Some(stations),
        sources_at: Some(sources),
    }
}

fn delivered(bulk: BulkPreset, distance_m: f64, hours: f64) -> usize {
    let s = spec(bulk, vec![(0.0, 0.0), (distance_m, 0.0)], vec![0], vec![], hours);
    let mut b = build(&s, Params::default());
    b.engine.run((hours * 3.6e6) as u64, 600_000);
    b.tracks.keys().filter(|id| b.engine.metrics.completions.contains_key(&(1, **id))).count()
}

#[test]
fn band_o_two_nodes_deliver() {
    assert_eq!(delivered(BulkPreset::GfskO, 1500.0, 2.0), 2);
}

#[test]
fn band_o_out_of_range_delivers_nothing() {
    assert_eq!(delivered(BulkPreset::GfskO, 6000.0, 2.0), 0);
}

#[test]
fn band_l_hopping_two_nodes_deliver() {
    assert_eq!(delivered(BulkPreset::GfskL, 600.0, 2.0), 2);
}

#[test]
fn espnow_two_nodes_deliver() {
    assert_eq!(delivered(BulkPreset::EspNow, 200.0, 1.0), 2);
}

#[test]
fn cell_elects_station_and_serves_everyone() {
    let positions: Vec<(f64, f64)> = (0..12).map(|i| ((i % 4) as f64 * 250.0, (i / 4) as f64 * 250.0)).collect();
    let s = spec(BulkPreset::GfskO, positions, vec![0], vec![11], 3.0);
    let mut b = build(&s, Params::default());
    b.engine.run(3 * 3_600_000, 600_000);
    let station = b.engine.nodes[11].node.id();
    assert_eq!(b.engine.nodes[11].node.role(1), meshcast_core::node::Role::Announcer, "station should announce");
    for i in 1..11 {
        assert_eq!(b.engine.nodes[i].node.announcer_of(1), station);
        for id in b.tracks.keys() {
            assert!(b.engine.metrics.completions.contains_key(&(i, *id)), "node {i} missing {id:?}");
        }
    }
}

#[test]
fn band_l_objects_cross_between_two_clusters() {
    // Two clusters of ten, 1.8 km apart in band L: the source is in one, the station in the
    // other, so every object has to cross from one hop sequence to the other. This world (seed 8)
    // stalled at 89.5 % while offers to the other cell's announcer went out on the holder's own
    // hop sequence and a grant that had delivered once never lapsed (FEASIBILITY.md §9).
    let seed = 8u64;
    let (size, radius, distance) = (10, 300.0, 1800.0);
    let mut rng = meshcast_core::rng::Rng::new(seed ^ 0xC1);
    let mut positions = Vec::new();
    for cluster in 0..2 {
        let cx = cluster as f64 * distance;
        for _ in 0..size {
            let r = radius * rng.unit().sqrt();
            let a = rng.unit() * 2.0 * std::f64::consts::PI;
            positions.push((cx + r * a.cos(), r * a.sin()));
        }
    }
    let n = positions.len();
    let mut s = spec(BulkPreset::GfskL, positions, vec![0], vec![n - 1], 12.0);
    s.seed = seed;
    s.tracks = 8;
    s.area_km2 = (distance + 2.0 * radius) * 2.0 * radius / 1e6;
    s.shadow_db = 6.0;
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
    let mut b = build(&s, Params::default());
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    let mut missing = 0;
    for (id, t) in &b.tracks {
        for &f in &t.followers {
            if !b.engine.metrics.completions.contains_key(&(f, *id)) {
                missing += 1;
            }
        }
    }
    assert_eq!(missing, 0, "{missing} follower-object pairs never completed");
}

#[test]
fn hidden_uploaders_take_turns_at_their_announcer() {
    // Six sources in a ring 900 m around a station in band L: neighbours hear each other, sources
    // across the ring do not, and the station hears them all. Carrier sensing cannot keep hidden
    // uploads apart, so the station divides its listening time into phases (PROTOCOL.md §4).
    let mut positions: Vec<(f64, f64)> = (0..6)
        .map(|i| {
            let a = i as f64 * std::f64::consts::PI / 3.0;
            (900.0 * a.cos(), 900.0 * a.sin())
        })
        .collect();
    positions.push((0.0, 0.0));
    for i in 0..6 {
        positions.push((40.0 * (i as f64 - 2.5), 50.0));
    }
    let mut s = spec(BulkPreset::GfskL, positions, (0..6).collect(), vec![6], 4.0);
    s.tracks = 4;
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
    let mut b = build(&s, Params::default());
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    let o = b.engine.metrics.upload_outcome;
    let sent: u64 = o.iter().sum();
    assert!(sent > 0, "no uploads happened");
    // Without phases 63 % of these frames collided when this test was written; with them 0.4 %.
    assert!(o[1] * 100 <= sent * 2, "uploads collided at the station: {o:?}");
    for (id, t) in &b.tracks {
        for &f in &t.followers {
            assert!(b.engine.metrics.completions.contains_key(&(f, *id)), "node {f} missing {id:?}");
        }
    }
}

#[test]
fn announcers_are_never_granted_uploads() {
    // Two clusters in band L, the source among them an announcer. An announcer's HAVE lists what
    // its carousel serves, and announcers do not upload; when that HAVE counted as an offer, the
    // other cluster's announcer granted the source's announcer the manifest three times in a row,
    // each grant waiting out T_grant, and the followers that would have offered fell silent
    // (FEASIBILITY.md §9.6). Positions of seed 1, radio draws of seed 6: the world it was found in.
    let (size, radius, distance) = (10, 300.0, 1800.0);
    let mut rng = meshcast_core::rng::Rng::new(1 ^ 0xC1);
    let mut positions = Vec::new();
    for cluster in 0..2 {
        let cx = cluster as f64 * distance;
        for _ in 0..size {
            let r = radius * rng.unit().sqrt();
            let a = rng.unit() * 2.0 * std::f64::consts::PI;
            positions.push((cx + r * a.cos(), r * a.sin()));
        }
    }
    let n = positions.len();
    let mut s = spec(BulkPreset::GfskL, positions, vec![0], vec![n - 1], 3.0);
    s.seed = 6;
    s.tracks = 8;
    s.area_km2 = (distance + 2.0 * radius) * 2.0 * radius / 1e6;
    s.shadow_db = 6.0;
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
    let mut b = build(&s, Params::default());
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    assert_eq!(b.engine.metrics.grants_to_announcers, 0, "an announcer was granted an upload");
}
