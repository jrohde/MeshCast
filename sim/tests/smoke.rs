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
            s.attack = Some(meshcast_sim::scenario::AttackSpec { attackers: 1, period_s: 60.0, spoof: true, renditions: false, lure: false, claim_max: false });
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

fn cell(bulk: BulkPreset, nodes: usize, hours: f64, seed: u64) -> ScenarioSpec {
    let mut s = spec(bulk, Vec::new(), Vec::new(), Vec::new(), hours);
    s.nodes = nodes;
    s.stations = 1;
    s.sources = 1;
    s.seed = seed;
    s.shadow_db = 6.0;
    s.positions = None;
    s.stations_at = None;
    s.sources_at = None;
    s
}

#[test]
fn band_l_cold_start_elects_without_a_storm() {
    // Fifty nodes switched on at once in band L. A new announcer's first beacon goes out on its own
    // hop sequence while the other candidates scan, so before candidates stepped up in the meeting
    // dwell nobody heard it in time and every node became announcer once: 243 role changes in this
    // world before it settled on the same two announcers it has now, and 109 after (PROTOCOL.md
    // §5.2).
    let s = cell(BulkPreset::GfskL, 50, 1.0, 1);
    let mut b = build(&s, Params::default());
    b.engine.run(3_600_000, 600_000);
    let roles = b.engine.metrics.role_events.len();
    assert!(roles <= 130, "{roles} role changes in the first hour");
}

#[test]
fn a_station_back_from_a_power_cut_takes_over_once() {
    // The station goes off for an hour; a battery node takes over. When the station returns it is
    // more capable than that node, so it challenges, once, and the battery node yields. Before
    // challenges were on capability, a candidate also stood down on hearing its own announcer's
    // beacon, so on a hopping carrier the station took 107 to 1797 s, and on ESP-NOW up to an hour.
    let s = cell(BulkPreset::GfskL, 20, 3.0, 1);
    let mut b = build(&s, Params::default());
    let station = (0..s.nodes).find(|&i| b.engine.nodes[i].mains).unwrap();
    b.engine.schedule_kill(station, 3_600_000);
    b.engine.schedule_revive(station, 7_200_000);
    b.engine.run(3 * 3_600_000, 600_000);
    let id = b.engine.nodes[station].node.id().0;
    let back = b.engine.metrics.role_events.iter().find(|e| e.t_ms >= 7_200_000 && e.node == id && e.role == "Announcer").map(|e| e.t_ms - 7_200_000);
    assert!(back.map(|t| t <= 300_000).unwrap_or(false), "station took over after {back:?} ms");
    let challenges: u32 = b.engine.nodes.iter().map(|n| n.node.challenges()).sum();
    assert_eq!(challenges, 1);
    assert_eq!(b.engine.nodes[station].node.role(1), meshcast_core::node::Role::Announcer);
}

fn island(params: Params, lure: bool) -> (usize, u64) {
    // A source and one neighbour around station A, and station B with three followers 1.35 km
    // away in band L (range about 985 m). Only one follower of B, at 900 m from A, hears A at
    // all; nobody in A's cell hears B's cell. The stations do not hear each other. With `lure`, a
    // node 800 m from that follower poses as an announcer that has everything: 6 dB weaker for it than
    // B, so it does not follow it by signal, but stronger than A.
    let mut positions = vec![(-400.0, 0.0), (-300.0, 50.0), (0.0, 0.0), (900.0, 0.0), (1350.0, 0.0), (1500.0, 100.0), (1550.0, -100.0)];
    if lure {
        positions.push((900.0, 800.0));
    }
    let mut s = spec(BulkPreset::GfskL, positions, vec![0], vec![2, 4], 6.0);
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
    let mut b = build(&s, params);
    if lure {
        b.engine.attackers.push(meshcast_sim::engine::Attacker::new(7, 5_000, false, true, false));
        b.engine.nodes[7].mute = true;
        b.engine.attack_ids = b.tracks.keys().copied().collect();
        b.engine.start_attacks();
    }
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    let mut missing = 0;
    for (id, t) in &b.tracks {
        for &f in t.followers.iter().filter(|&&f| f != 7) {
            if !b.engine.metrics.completions.contains_key(&(f, *id)) {
                missing += 1;
            }
        }
    }
    (missing, b.engine.nodes.iter().map(|n| n.node.stats.excursions).sum())
}

#[test]
fn a_follower_fetches_what_its_cell_cannot_get() {
    // B asks and nobody it hears holds anything; its follower at the edge hears A list it all.
    // That follower goes on an excursion, fetches, comes back and uploads to B (PROTOCOL.md §4).
    let (missing, excursions) = island(Params::default(), false);
    assert!(excursions > 0, "no excursion");
    assert_eq!(missing, 0, "{missing} follower-object pairs never completed");
    // Without excursions B's cell gets nothing at all.
    let (missing, _) = island(Params { t_excursion_ms: u64::MAX / 4, ..Params::default() }, false);
    assert!(missing > 0, "B's cell was served without an excursion");
}

#[test]
fn a_lure_is_visited_once() {
    // The same island, and a node that lists every object and serves none, heard better than A by
    // the only follower that could fetch. It goes there first, gets nothing, does not go back,
    // and fetches from A (ABUSE.md). Without that memory it chose the lure every time.
    let (missing, excursions) = island(Params::default(), true);
    assert!(excursions >= 2, "{excursions} excursions");
    assert_eq!(missing, 0, "{missing} follower-object pairs never completed");
}

#[test]
fn followers_that_overheard_a_neighbouring_cell_repair_from_it() {
    // Two clusters 1.8 km apart in band L; positions of seed 1, radio draws of seed 2. Only two
    // followers of the far cluster hear the near one, and only its source, whose uploads they
    // overhear; neither announcer hears the other cluster. The two collected 112 of 113 symbols
    // of three objects and had nobody to ask for the last one: their own announcer was asking
    // for the object itself. Now they name the holder they heard (PROTOCOL.md §3.5, §4).
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
    let mut s = spec(BulkPreset::GfskL, positions, vec![0], vec![n - 1], 12.0);
    s.seed = 2;
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
fn a_false_announcer_is_left() {
    // A band L cell switched on at once, and a node beside it that beacons as an announcer with
    // the maximum score and full capability, lists every object and serves nothing. The station
    // yields to it and every follower that hears it best follows it. Each of them stops once it
    // has had nothing of what the false announcer lists for T_excursion, and the cell recovers
    // (PROTOCOL.md §5.2). Before, nobody could outscore such a beacon, so nobody left it.
    let positions: Vec<(f64, f64)> = (0..12).map(|i| ((i % 4) as f64 * 200.0, (i / 4) as f64 * 200.0)).collect();
    let mut s = spec(BulkPreset::GfskL, positions, vec![0], vec![11], 8.0);
    s.positions.as_mut().unwrap().push((300.0, 500.0));
    s.nodes = 13;
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
    let mut b = build(&s, Params::default());
    b.engine.attackers.push(meshcast_sim::engine::Attacker::new(12, 5_000, false, true, true));
    b.engine.nodes[12].mute = true;
    b.engine.attack_ids = b.tracks.keys().copied().collect();
    b.engine.start_attacks();
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    let mut missing = 0;
    for (id, t) in &b.tracks {
        for &f in t.followers.iter().filter(|&&f| f != 12) {
            if !b.engine.metrics.completions.contains_key(&(f, *id)) {
                missing += 1;
            }
        }
    }
    assert_eq!(missing, 0, "{missing} follower-object pairs never completed");
}

#[test]
fn a_channel_followed_again_is_fetched_again() {
    // A follower that stops following a channel forgets its objects and its manifest's bytes but
    // remembers the manifest's seq, so announcers that list that seq tell it nothing new. When it
    // follows the channel again it must ask for the manifest itself: nobody repeats manifests
    // unasked (PROTOCOL.md §4). Before, it waited for a seq that would not come.
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 4.0);
    let mut b = build(&s, Params::default());
    let chan = b.sources[0].channel;
    b.engine.run(3_600_000, 600_000);
    let holds_all = |b: &meshcast_sim::scenario::Built| b.tracks.keys().all(|id| b.engine.nodes[1].node.holds(id));
    assert!(holds_all(&b), "the follower should hold everything after an hour");
    b.engine.nodes[1].node.unfollow(chan);
    b.engine.run(3_600_000 + 600_000, 600_000);
    let m = b.engine.nodes[1].node.manifest_state(&chan).expect("the seq is remembered");
    assert!(!m.3 && b.tracks.keys().all(|id| !b.engine.nodes[1].node.holds(id)), "unfollowing should evict the channel");
    b.engine.nodes[1].node.follow(chan);
    b.engine.poke(1);
    b.engine.run(2 * 3_600_000, 600_000);
    assert!(b.engine.nodes[1].node.manifest_state(&chan).map(|m| m.3).unwrap_or(false), "the manifest should be fetched again");
    assert!(holds_all(&b), "the channel's objects should be fetched again");
}

#[test]
fn a_false_announcement_blocks_nothing() {
    // MANIFEST_ANNOUNCE is not signed. A node that believed an announced seq kept it as the
    // channel's newest, so one frame claiming the highest seq made it ignore every real
    // announcement and refuse the real, signed manifest; and while it waited for the announced
    // one it evicted the window it held. Now an announced manifest is only something to fetch
    // (PROTOCOL.md §2, ABUSE.md).
    use meshcast_core::frame::{AnnounceEntry, Frame, ManifestAnnounce};
    use meshcast_core::ids::{NodeId, ShortId};
    use meshcast_core::manifest::Manifest;
    use meshcast_core::object::ContentType;
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 4.0);
    let mut b = build(&s, Params::default());
    b.engine.run(3_600_000, 600_000);
    let window: Vec<ShortId> = b.tracks.keys().copied().collect();
    assert!(window.iter().all(|id| b.engine.nodes[1].node.holds(id)), "the follower should hold the window after an hour");
    let chan = b.sources[0].channel;
    let lie = Frame::ManifestAnnounce(ManifestAnnounce {
        node: NodeId(99),
        entries: vec![AnnounceEntry { channel: chan, manifest: ShortId([0xEE; 8]), seq: u32::MAX, len: 5_000 }],
        whole: false,
    });
    for i in [1, 2] {
        let now = b.engine.now;
        b.engine.nodes[i].node.handle_frame(now, 1, &lie, -60);
    }
    b.engine.run(3_600_000 + 600_000, 600_000);
    assert!(window.iter().all(|id| b.engine.nodes[1].node.holds(id)), "an announcement alone should not evict the window");
    // The source publishes for real: the window plus one object.
    let src = &mut b.sources[0];
    let o = meshcast_sim::scenario::track_object(s.seed, src.node, src.objects.len(), 20_000, ContentType::Speech);
    src.objects.push(o.clone());
    src.seq += 1;
    let m = Manifest::sign(&src.key, src.seq, "Channel", src.objects.clone(), Vec::new(), None);
    let node = src.node;
    b.engine.nodes[node].node.publish(&m, &[(o.meta(), None)]);
    b.engine.poke(node);
    b.engine.run(3 * 3_600_000, 600_000);
    let seq = b.sources[0].seq;
    let held = b.engine.nodes[1].node.manifest_state(&chan);
    assert!(held.map(|m| m.0 == seq || m.3).unwrap_or(false) && b.engine.nodes[1].node.holds(&o.id.short()), "the real manifest should arrive: {held:?}");
}

#[test]
fn a_station_back_from_a_power_cut_keeps_its_library() {
    // A station serves every channel but follows none of them itself. Switched back on, it is
    // not yet announcing, so it evicted at once every object it does not follow, and fetched it
    // all again when it took over a minute later. A node that restarts keeps what it carried for
    // `want_ttl` (PROTOCOL.md §4).
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 3.0);
    let mut b = build(&s, Params::default());
    let chan = b.sources[0].channel;
    b.engine.nodes[2].node.unfollow(chan);
    b.engine.run(3_600_000, 600_000);
    let objects: Vec<_> = b.tracks.keys().copied().collect();
    assert!(objects.iter().all(|id| b.engine.nodes[2].node.holds(id)), "the station should serve everything after an hour");
    b.engine.set_alive(2, false);
    b.engine.run(3_600_000 + 600_000, 600_000);
    b.engine.set_alive(2, true);
    b.engine.run(3_600_000 + 600_000 + 120_000, 600_000);
    assert!(objects.iter().all(|id| b.engine.nodes[2].node.holds(id)), "a restart should not cost the station its library");
}
