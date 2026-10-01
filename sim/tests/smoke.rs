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
        renditions: None,
        attack: None,
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
    // And the station keeps quiet while they speak: 26 % arrived while it was transmitting before
    // it did, 1.7 % after.
    assert!(o[2] * 100 <= sent * 5, "the station talked over its uploaders: {o:?}");
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

#[test]
fn small_listeners_get_renditions_before_their_slot() {
    // One band O cell: a station that can make renditions, a source, and followers of which two
    // cannot decode. Those two want each programme as Opus shortly before its slot, get it in
    // time, and never fetch the codes; with nobody who needs them, no rendition is sent at all.
    let positions: Vec<(f64, f64)> = (0..12).map(|i| (150.0 * (i % 4) as f64, 150.0 * (i / 4) as f64)).collect();
    for small in [2usize, 0] {
        let mut s = spec(BulkPreset::GfskO, positions.clone(), vec![0], vec![5], 5.0);
        s.tracks = 4;
        s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
        s.renditions = Some(meshcast_sim::scenario::RenditionSpec { music_kbps: 16.0, speech_kbps: 8.0, small, players_render: false });
        let mut b = build(&s, Params::default());
        b.engine.run((s.hours * 3.6e6) as u64, 600_000);
        assert_eq!(b.small.len(), small);
        for (id, t) in &b.tracks {
            let (r, _) = t.rendition.expect("every audio object has a rendition");
            for &f in &t.followers {
                if t.small.contains(&f) {
                    let done = b.engine.metrics.completions.get(&(f, r)).copied();
                    assert!(done.map(|d| d <= t.slot_ms.unwrap()).unwrap_or(false), "node {f}: rendition of {id:?} not there before its slot ({done:?})");
                    assert!(!b.engine.metrics.completions.contains_key(&(f, *id)), "node {f} fetched codes it cannot play");
                } else {
                    assert!(b.engine.metrics.completions.contains_key(&(f, *id)), "node {f} missing {id:?}");
                }
            }
        }
        if small == 0 {
            assert_eq!(b.engine.metrics.bulk_sent_rendition, 0, "renditions sent where nobody asked");
        }
    }
}

#[test]
fn a_want_flood_is_bounded() {
    // One band O cell and a follower that asks for every object once a minute, each time under a
    // made-up node id. Fresh content still reaches everyone, and repetition backs off per object
    // whoever asks, so the cell carries a bounded multiple of its normal traffic instead of
    // running at the duty-cycle limit (25 times the normal traffic before the backoff).
    let positions: Vec<(f64, f64)> = (0..12).map(|i| (150.0 * (i % 4) as f64, 150.0 * (i / 4) as f64)).collect();
    let mut frames = Vec::new();
    for attack in [false, true] {
        let mut s = spec(BulkPreset::GfskO, positions.clone(), vec![0], vec![5], 6.0);
        s.tracks = 6;
        s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
        if attack {
            s.attack = Some(meshcast_sim::scenario::AttackSpec { attackers: 1, period_s: 60.0, spoof: true, renditions: false });
        }
        let mut b = build(&s, Params::default());
        b.engine.run((s.hours * 3.6e6) as u64, 600_000);
        for (id, t) in &b.tracks {
            for &f in &t.followers {
                assert!(b.engine.metrics.completions.contains_key(&(f, *id)), "node {f} missing {id:?} (attack {attack})");
            }
        }
        frames.push(b.engine.metrics.bulk_sent);
    }
    assert!(frames[1] <= 8 * frames[0], "a WANT flood made the cell carry {} frames against {} without it", frames[1], frames[0]);
}
