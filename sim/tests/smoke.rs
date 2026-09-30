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
