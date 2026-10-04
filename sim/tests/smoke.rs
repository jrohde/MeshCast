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
        clocks: Default::default(),
        collections: Default::default(),
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

fn island(params: Params, lure: bool) -> (usize, u64, u64) {
    // A source and one neighbour around station A, and station B with three followers 1.35 km
    // away in band L (range about 985 m). Only one follower of B, at 900 m from A, hears A at
    // all; nobody in A's cell hears B's cell. The stations do not hear each other. With `lure`, a
    // node 800 m from that follower poses as an announcer that has everything: 6 dB weaker for it than
    // B, so it does not follow it by signal, but stronger than A. A's neighbour does not follow the
    // channel, so nothing in A's cell asks and A passes nothing that the edge follower could
    // overhear: an excursion is the only way in. (When it followed, its prompt ask after an asked-
    // for manifest made A pass the pieces, and B's cell overheard them; PROTOCOL.md §4.)
    let mut positions = vec![(-400.0, 0.0), (-300.0, 50.0), (0.0, 0.0), (900.0, 0.0), (1350.0, 0.0), (1500.0, 100.0), (1550.0, -100.0)];
    if lure {
        positions.push((900.0, 800.0));
    }
    let mut s = spec(BulkPreset::GfskL, positions, vec![0], vec![2, 4], 6.0);
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
    let mut b = build(&s, params);
    let chan = b.sources[0].channel;
    b.engine.nodes[1].node.unfollow(chan);
    if lure {
        b.engine.attackers.push(meshcast_sim::engine::Attacker::new(7, 5_000, false, true, false));
        b.engine.nodes[7].mute = true;
        b.engine.attack_ids = b.tracks.keys().copied().collect();
        b.engine.start_attacks();
    }
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    let mut missing = 0;
    for (id, t) in &b.tracks {
        for &f in t.followers.iter().filter(|&&f| f != 7 && f != 1) {
            if !b.engine.metrics.completions.contains_key(&(f, *id)) {
                missing += 1;
            }
        }
    }
    // When the last of it arrived.
    let last = b.tracks.iter().flat_map(|(id, t)| t.followers.iter().filter(|&&f| f != 7 && f != 1).filter_map(|&f| b.engine.metrics.completions.get(&(f, *id)).copied()).collect::<Vec<_>>()).max().unwrap_or(0);
    (missing, b.engine.nodes.iter().map(|n| n.node.stats.excursions).sum(), last)
}

#[test]
fn a_follower_fetches_what_its_cell_cannot_get() {
    // B asks and nobody it hears holds anything; its follower at the edge hears A list it all.
    // That follower goes on an excursion, fetches, comes back and uploads to B (PROTOCOL.md §4).
    let (missing, excursions, with) = island(Params::default(), false);
    assert!(excursions > 0, "no excursion");
    assert_eq!(missing, 0, "{missing} follower-object pairs never completed");
    // Without excursions B's cell gets it only once that follower gives B up, having had not one
    // symbol while B named no uploader, for the ceiling (PROTOCOL.md §5.2): after 107 minutes
    // instead of 62 in this world.
    let (missing, _, without) = island(Params { t_excursion_ms: u64::MAX / 4, ..Params::default() }, false);
    assert!(missing > 0 || without >= with + 30 * 60_000, "B's cell was served as soon without an excursion ({without} ms, with {with} ms)");
}

#[test]
fn a_lure_is_visited_once() {
    // The same island, and a node that lists every object and serves none, heard better than A by
    // the only follower that could fetch. It goes there first, gets nothing, does not go back,
    // and fetches from A (ABUSE.md). Without that memory it chose the lure every time.
    let (missing, excursions, _) = island(Params::default(), true);
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
    // A follower that stops following a channel forgets its objects. A node that keeps no menu of
    // channels it does not follow (one whose menu gave way to its carry budget, or with relaying
    // for other channels off) forgets its manifest's bytes too but remembers the manifest's seq, so
    // announcers that list that seq tell it nothing new. When it follows the channel again it must
    // ask for the manifest itself: nobody repeats manifests unasked (PROTOCOL.md §4). Before, it
    // waited for a seq that would not come. A node that keeps the menu holds the manifest still.
    for keeps_menu in [false, true] {
        let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 4.0);
        let mut b = build(&s, Params { relay_unfollowed: keeps_menu, ..Params::default() });
        let chan = b.sources[0].channel;
        b.engine.run(3_600_000, 600_000);
        let holds_all = |b: &meshcast_sim::scenario::Built| b.tracks.keys().all(|id| b.engine.nodes[1].node.holds(id));
        assert!(holds_all(&b), "the follower should hold everything after an hour");
        b.engine.nodes[1].node.unfollow(chan);
        b.engine.run(3_600_000 + 600_000, 600_000);
        let m = b.engine.nodes[1].node.manifest_state(&chan).expect("the seq is remembered");
        assert!(b.tracks.keys().all(|id| !b.engine.nodes[1].node.holds(id)), "unfollowing should evict the channel's objects");
        assert_eq!(m.3, keeps_menu, "the manifest's bytes should stay exactly when the node keeps the menu");
        b.engine.nodes[1].node.follow(chan);
        b.engine.poke(1);
        b.engine.run(2 * 3_600_000, 600_000);
        assert!(b.engine.nodes[1].node.manifest_state(&chan).map(|m| m.3).unwrap_or(false), "the manifest should be held again");
        assert!(holds_all(&b), "the channel's objects should be fetched again");
    }
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
    use meshcast_core::manifest::{Collection, CollectionKind, Manifest};
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
    let series = Collection { cid: 1, kind: CollectionKind::Series, title: "Series".into(), pieces: src.objects.clone(), schedule: Vec::new() };
    let m = Manifest::sign(&src.key, src.seq, "Channel", vec![series.reference(None, true)], None, None);
    let node = src.node;
    b.engine.nodes[node].node.publish(&m, &[series], &[(o.meta(), None)]);
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

#[test]
fn hidden_uploaders_take_turns_on_espnow_too() {
    // The ring of `hidden_uploaders_take_turns_at_their_announcer`, at ESP-NOW's range (about
    // 460 m at exponent 3). No regulator caps an ESP-NOW sender, so the announcer's receiver is
    // the only limit, and uploads to it wait for the end of the meeting dwell and start together.
    // Before phases were used where nothing caps the sender, 45 % of ESP-NOW's upload frames
    // collided at their announcer (FEASIBILITY.md §15.1).
    let mut positions: Vec<(f64, f64)> = (0..6)
        .map(|i| {
            let a = i as f64 * std::f64::consts::PI / 3.0;
            (420.0 * a.cos(), 420.0 * a.sin())
        })
        .collect();
    positions.push((0.0, 0.0));
    for i in 0..6 {
        positions.push((20.0 * (i as f64 - 2.5), 25.0));
    }
    let mut s = spec(BulkPreset::EspNow, positions, (0..6).collect(), vec![6], 3.0);
    s.tracks = 4;
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
    let mut b = build(&s, Params::default());
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    let o = b.engine.metrics.upload_outcome;
    let sent: u64 = o.iter().sum();
    assert!(sent > 0, "no uploads happened");
    assert!(o[1] * 100 <= sent * 2, "uploads collided at the station: {o:?}");
    for (id, t) in &b.tracks {
        for &f in &t.followers {
            assert!(b.engine.metrics.completions.contains_key(&(f, *id)), "node {f} missing {id:?}");
        }
    }
}

#[test]
fn a_source_keeps_its_own_uploads() {
    // A source need not follow its own channel, so its own objects are not of interest to it.
    // It dropped its queued uploads of them whenever it dropped what no manifest of interest
    // names, as on adopting another channel's manifest (FEASIBILITY.md §15.2).
    use meshcast_core::frame::{Frame, Gossip};
    use meshcast_core::ids::NodeId;
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 1.0);
    let mut b = build(&s, Params::default());
    b.engine.run(3_600_000, 600_000);
    let own: Vec<_> = b.tracks.keys().copied().collect();
    let src = &mut b.engine.nodes[0].node;
    let me = src.id();
    let ann = NodeId(99);
    let grant = Frame::Gossip(Gossip { node: ann, announcer: ann, announcer_colour: 0, announcer_colours: 1, heard: Vec::new(), have: Vec::new(), have_sets: Vec::new(), want: own.iter().map(|id| (*id, me, 0)).collect(), sets: Vec::new() });
    let now = b.engine.now;
    src.handle_frame(now, 1, &grant, -60);
    let queued = |n: &meshcast_core::node::Node| own.iter().filter(|id| n.uploads(1).contains(&(**id, ann))).count();
    assert_eq!(queued(src), own.len(), "both objects should be lined up for the announcer that granted them");
    let chan = b.sources[0].channel;
    src.unfollow(chan);
    assert_eq!(queued(src), own.len(), "a source should keep its uploads of its own objects");
}

#[test]
fn an_excursion_asks_for_what_it_fetched_names() {
    // The two-cluster world of the `clusters` scenario (first seed's positions, world 2) in which
    // a follower of the far cluster goes on an excursion. It asked the visited announcer for the
    // manifest, had it seconds later, and waited `T_want_min` to ask for what the manifest named:
    // the far cluster had everything after 55 minutes. On a visit only the visitor asks, so it
    // now asks for that soon (FEASIBILITY.md §15.3).
    let (size, radius_m, distance_m) = (10, 300.0, 1800.0);
    let mut rng = meshcast_core::rng::Rng::new(1 ^ 0xC1);
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
    let mut s = spec(BulkPreset::GfskL, positions, vec![0], vec![n - 1], 2.0);
    s.tracks = 8;
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
    s.shadow_db = 6.0;
    s.seed = 2;
    let mut b = build(&s, Params::default());
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    // The scenario measure: each object's median over its followers, averaged over the objects.
    let m = &b.engine.metrics;
    let mut medians = Vec::new();
    for (id, t) in &b.tracks {
        let mut c: Vec<u64> = t.followers.iter().map(|f| m.completions.get(&(*f, *id)).copied().unwrap_or(u64::MAX)).collect();
        c.sort_unstable();
        let k = c.len();
        medians.push(if k % 2 == 1 { c[k / 2] as f64 } else { (c[k / 2 - 1] as f64 + c[k / 2] as f64) / 2.0 });
    }
    let mean = medians.iter().sum::<f64>() / medians.len() as f64 / 60_000.0;
    assert!(mean < 50.0, "the median follower had an object after {mean:.1} min");
}

#[test]
fn a_holder_serves_its_own_announcer_first() {
    // A source granted objects by another cell's announcer and then by its own uploads its own
    // announcer's next: its own cell is where it is heard best, and every follower there that gets
    // an object becomes a holder for the neighbouring cells. In arrival order a source served
    // another cell for ten minutes while its own cell waited (FEASIBILITY.md §15.2).
    use meshcast_core::frame::{Frame, Gossip};
    use meshcast_core::ids::NodeId;
    let mut s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 1.0);
    s.tracks = 3;
    let mut b = build(&s, Params::default());
    b.engine.run(3_600_000, 600_000);
    let own: Vec<_> = b.tracks.keys().copied().collect();
    let station = b.engine.nodes[2].node.id();
    let src = &mut b.engine.nodes[0].node;
    assert_eq!(src.announcer_of(1), station, "the source should follow the station");
    let me = src.id();
    let grant = |ann: NodeId, ids: &[meshcast_core::ids::ShortId]| Frame::Gossip(Gossip { node: ann, announcer: ann, announcer_colour: 0, announcer_colours: 1, heard: Vec::new(), have: Vec::new(), have_sets: Vec::new(), want: ids.iter().map(|id| (*id, me, 0)).collect(), sets: Vec::new() });
    let now = b.engine.now;
    let other = NodeId(99);
    src.handle_frame(now, 1, &grant(other, &own[..2]), -60);
    src.handle_frame(now, 1, &grant(station, &own[2..]), -60);
    let order: Vec<NodeId> = src.uploads(1).iter().map(|(_, to)| *to).collect();
    assert_eq!(order, vec![other, station, other], "the own announcer's grant should come right after the running upload");
}

#[test]
fn a_follower_of_one_collection_carries_only_that_collection() {
    // A provider publishes two collections, each with a cover; the follower follows one of them.
    // The follower fetches, collects and keeps the pieces and cover of its collection, and nothing
    // of the other (PROTOCOL.md §2). The station knows both from their manifests, and fetches what
    // its follower asks for: the pieces of the other collection nobody asked for (§4, FEASIBILITY.md
    // §27).
    let mut s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 3.0);
    s.tracks = 4;
    s.collections = meshcast_sim::scenario::CollectionSpec { per_source: 2, follow: 1, cover_kb: 4, singles: false };
    let mut b = build(&s, Params::default());
    b.engine.nodes[2].node.unfollow(b.sources[0].channel);
    b.engine.run(3 * 3_600_000, 600_000);
    let (mine, other): (Vec<_>, Vec<_>) = b.tracks.iter().partition(|(_, t)| t.followers.contains(&1));
    assert_eq!((mine.len(), other.len()), (3, 3), "two pieces and a cover each");
    assert!(mine.iter().all(|(id, _)| b.engine.nodes[1].node.holds(id)), "the follower should hold its collection");
    assert!(other.iter().all(|(id, _)| !b.engine.nodes[1].node.holds(id)), "the follower should hold nothing of the other collection");
    assert!(mine.iter().all(|(id, _)| b.engine.nodes[2].node.holds(id)), "the station should serve what its follower asked for");
    assert!(other.iter().all(|(id, _)| !b.engine.nodes[2].node.holds(id)), "the station should not fetch what nobody asked for");
}

#[test]
fn a_new_root_keeps_a_collection_until_its_manifest_is_held() {
    // A new root manifest names a new collection manifest. Until that is held, the follower keeps
    // the pieces of the collection manifest it holds: a new root alone must not cost it its
    // window, as an announcement alone must not (PROTOCOL.md §2). Here the new collection
    // manifest never comes, and the window must stay.
    use meshcast_core::frame::{AnnounceEntry, Frame, ManifestAnnounce};
    use meshcast_core::ids::NodeId;
    use meshcast_core::manifest::{Collection, CollectionKind, Manifest};
    use meshcast_core::object::ContentType;
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 4.0);
    let mut b = build(&s, Params::default());
    b.engine.run(3_600_000, 600_000);
    let window: Vec<_> = b.tracks.keys().copied().collect();
    assert!(window.iter().all(|id| b.engine.nodes[1].node.holds(id)), "the follower should hold the window after an hour");
    let src = &b.sources[0];
    let mut pieces = src.objects.clone();
    pieces.push(meshcast_sim::scenario::track_object(s.seed, src.node, pieces.len(), 20_000, ContentType::Speech));
    let series = Collection { cid: 1, kind: CollectionKind::Series, title: "Series".into(), pieces, schedule: Vec::new() };
    let root = Manifest::sign(&src.key, src.seq + 1, "Channel", vec![series.reference(None, true)], None, None);
    let (meta, bytes) = root.as_object();
    let chan = src.channel;
    let f = &mut b.engine.nodes[1].node;
    f.store.insert_complete(meta, Some(&bytes));
    let now = b.engine.now;
    let hint = Frame::ManifestAnnounce(ManifestAnnounce { node: NodeId(99), entries: vec![AnnounceEntry { channel: chan, manifest: meta.id.short(), seq: root.seq, len: meta.len }], whole: false });
    f.handle_frame(now, 1, &hint, -60);
    assert_eq!(f.manifest_state(&chan).map(|m| (m.0, m.2)), Some((root.seq, true)), "the new root should be adopted");
    b.engine.run(3 * 3_600_000, 600_000);
    assert!(window.iter().all(|id| b.engine.nodes[1].node.holds(id)), "a new root alone should not evict the window");
}

#[test]
fn a_root_brings_the_collection_manifests_new_in_it() {
    // A source publishes a new episode: a new collection manifest and a new root naming it. The
    // root's grant covers the collection manifests new in it that its holder listed with it, so
    // the station has both in one round of asking; asked for separately, the collection manifest
    // cost a second round, on a hopping carrier a meeting dwell or two (PROTOCOL.md §4).
    use meshcast_core::manifest::{Collection, CollectionKind, Manifest};
    use meshcast_core::object::ContentType;
    let s = spec(BulkPreset::GfskL, vec![(0.0, 0.0), (600.0, 0.0), (300.0, 0.0)], vec![0], vec![2], 3.0);
    let mut b = build(&s, Params::default());
    b.engine.run(3_600_000, 600_000);
    let src = &mut b.sources[0];
    let o = meshcast_sim::scenario::track_object(s.seed, src.node, src.objects.len(), 20_000, ContentType::Speech);
    src.objects.push(o.clone());
    src.seq += 1;
    let series = Collection { cid: 1, kind: CollectionKind::Series, title: "Series".into(), pieces: src.objects.clone(), schedule: Vec::new() };
    let root = Manifest::sign(&src.key, src.seq, "Channel", vec![series.reference(None, true)], None, None);
    let node = src.node;
    b.engine.nodes[node].node.publish(&root, std::slice::from_ref(&series), &[(o.meta(), None)]);
    b.engine.poke(node);
    b.engine.run(3 * 3_600_000, 600_000);
    let at = |id: meshcast_core::ids::ShortId| b.engine.metrics.completions.get(&(2, id)).copied();
    let (r, c) = (at(root.as_object().0.id.short()), at(series.as_object().0.id.short()));
    let (Some(r), Some(c)) = (r, c) else { panic!("the station should have the root and the collection manifest: {r:?} {c:?}") };
    assert!(c <= r + 10_000, "the collection manifest came {:.1} s after its root", (c as f64 - r as f64) / 1000.0);
    assert!(b.engine.nodes[2].node.holds(&o.id.short()), "the new episode should arrive");
}

#[test]
fn a_holder_uploads_manifests_first() {
    // Nothing of a collection can be read without its manifest, and a manifest is a few symbols:
    // a holder uploads a manifest it is granted before the pieces it already lined up, as the
    // carousel passes manifests before anything else (PROTOCOL.md §4).
    use meshcast_core::frame::{Frame, Gossip};
    let mut s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 1.0);
    s.tracks = 3;
    let mut b = build(&s, Params::default());
    b.engine.run(3_600_000, 600_000);
    let pieces: Vec<_> = b.tracks.keys().copied().collect();
    let manifest = b.sources[0].collections[0].as_object().0.id.short();
    let station = b.engine.nodes[2].node.id();
    let src = &mut b.engine.nodes[0].node;
    let me = src.id();
    let grant = |ids: &[meshcast_core::ids::ShortId]| Frame::Gossip(Gossip { node: station, announcer: station, announcer_colour: 0, announcer_colours: 1, heard: Vec::new(), have: Vec::new(), have_sets: Vec::new(), want: ids.iter().map(|id| (*id, me, 0)).collect(), sets: Vec::new() });
    let now = b.engine.now;
    src.handle_frame(now, 1, &grant(&pieces), -60);
    src.handle_frame(now, 1, &grant(&[manifest]), -60);
    let order: Vec<_> = src.uploads(1).iter().map(|(id, _)| *id).collect();
    assert_eq!(order.len(), 4, "three pieces and the manifest should be lined up: {order:?}");
    assert_eq!(order[1], manifest, "the manifest should come right after the running upload: {order:?}");
}

#[test]
fn a_follower_asks_for_the_rest_of_its_ask() {
    // A follower that has to ask for a manifest asks soon for what it names: that is the rest of
    // the same ask. A channel followed again, after its objects were evicted, is the root, then
    // its collection manifest, then the pieces, and each ask waited `T_want_min` (10 minutes):
    // newcomers in a living network took 21 minutes instead of 14 (FEASIBILITY.md §16.4).
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 3.0);
    let mut b = build(&s, Params::default());
    let chan = b.sources[0].channel;
    b.engine.run(3_600_000, 600_000);
    let holds_all = |b: &meshcast_sim::scenario::Built| b.tracks.keys().all(|id| b.engine.nodes[1].node.holds(id));
    assert!(holds_all(&b), "the follower should hold everything after an hour");
    b.engine.nodes[1].node.unfollow(chan);
    b.engine.run(3_600_000 + 600_000, 600_000);
    assert!(b.tracks.keys().all(|id| !b.engine.nodes[1].node.holds(id)), "unfollowing should evict the channel");
    let t = b.engine.now;
    b.engine.nodes[1].node.follow(chan);
    b.engine.poke(1);
    b.engine.run(t + 12 * 60_000, 600_000);
    assert!(holds_all(&b), "the follower should have the channel back within 12 minutes");
}

#[test]
fn what_an_announcer_asked_for_is_forgotten_with_it() {
    // A follower does not correct an announcer that is asking for a manifest itself (PROTOCOL.md
    // §2). What its old announcer had asked for says nothing about a new one: remembered for
    // `want_ttl`, it kept a follower from telling its new announcer of a root for an hour, and a
    // newcomer waited 65 minutes for one piece (FEASIBILITY.md §16).
    use meshcast_core::frame::{Frame, Gossip};
    use meshcast_core::ids::ShortId;
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (400.0, 0.0), (800.0, 0.0), (1200.0, 0.0)], vec![0], vec![1, 3], 2.0);
    let mut b = build(&s, Params::default());
    b.engine.run(3_600_000, 600_000);
    let old = b.engine.nodes[2].node.announcer_of(1);
    let gone = (old.0 - 1) as usize;
    assert!(gone == 1 || gone == 3, "the follower should follow a station");
    // An object the follower knows: what an announcer asks for is noted only for those.
    let x: ShortId = *b.tracks.keys().next().unwrap();
    let ask = Frame::Gossip(Gossip { node: old, announcer: old, announcer_colour: 0, announcer_colours: 1, heard: Vec::new(), have: Vec::new(), have_sets: Vec::new(), want: vec![(x, meshcast_core::ids::NodeId::NONE, 0)], sets: Vec::new() });
    let now = b.engine.now;
    b.engine.nodes[2].node.handle_frame(now, 1, &ask, -60);
    assert!(b.engine.nodes[2].node.announcer_asked_for(&x), "the follower should note its announcer's ask");
    b.engine.set_alive(gone, false);
    b.engine.run(now + 30 * 60_000, 600_000);
    let new = b.engine.nodes[2].node.announcer_of(1);
    assert!(new != old && !new.is_none(), "the follower should follow the other station");
    assert!(!b.engine.nodes[2].node.announcer_asked_for(&x), "what the old announcer asked for should be forgotten");
}

#[test]
fn listeners_of_one_album_are_relayed_to() {
    // 100 nodes on 15 km² in band L, three sources of four albums each, every listener following
    // one album. Content crosses cells only through nodes that carry it, and with one carrier in
    // four the chain of an album broke: 14 % of its listeners never had it. A follower now fetches,
    // for another cell's listeners, what that cell's announcer has asked for in vain for
    // `T_want_min` of a channel it follows, and keeps it to hand on (PROTOCOL.md §4).
    let mut s = cell(BulkPreset::GfskL, 100, 12.0, 7);
    s.area_km2 = 15.0;
    s.stations = 2;
    s.sources = 3;
    s.tracks = 20;
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
    s.collections = meshcast_sim::scenario::CollectionSpec { per_source: 4, follow: 1, cover_kb: 0, singles: false };
    let mut b = build(&s, Params::default());
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    let (mut pairs, mut missing) = (0, 0);
    for (id, t) in &b.tracks {
        for &f in &t.followers {
            pairs += 1;
            if !b.engine.metrics.completions.contains_key(&(f, *id)) {
                missing += 1;
            }
        }
    }
    assert_eq!(missing, 0, "{missing} of {pairs} listener-piece pairs never completed");
}

#[test]
fn two_uploaders_under_a_duty_cycle_take_turns() {
    // Band O, 50 nodes on 1 km², two sources of an hour of music in 70 kB pieces. A holder that
    // finds the channel busy backs off over a window that doubles each time, and one that sends
    // back to back never finds it busy: in this world one source uploaded at a third of its rate
    // for 25 minutes, deferring 4,576 times, and the hour took 37 minutes instead of 20. The
    // announcer now divides its listening time in phases under a duty cycle too (PROTOCOL.md §4).
    let mut s = cell(BulkPreset::GfskO, 50, 1.0, 5);
    s.sources = 2;
    s.tracks = 12;
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:70").unwrap();
    let mut b = build(&s, Params::default());
    b.engine.run(3_600_000, 600_000);
    let m = &b.engine.metrics;
    let mut last = 0;
    for (id, t) in &b.tracks {
        for &f in &t.followers {
            last = last.max(m.completions.get(&(f, *id)).copied().unwrap_or(u64::MAX));
        }
    }
    assert!(last < 25 * 60_000, "the last listener had the hour after {:.1} min", last as f64 / 60_000.0);
}

/// A source with one station between it and a follower, and `grant` frames from that station.
fn granted_world(tracks: usize, mix: &str, singles: bool) -> (meshcast_sim::scenario::Built, Vec<meshcast_core::ids::ShortId>) {
    let mut s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 1.0);
    s.tracks = tracks;
    s.mix = meshcast_sim::scenario::parse_mix(mix).unwrap();
    s.collections.singles = singles;
    let mut b = build(&s, Params::default());
    b.engine.run(3_600_000, 600_000);
    let mut pieces: Vec<(usize, meshcast_core::ids::ShortId)> = b.tracks.iter().map(|(id, t)| (t.index, *id)).collect();
    pieces.sort();
    (b, pieces.into_iter().map(|(_, id)| id).collect())
}

fn grant_frame(station: meshcast_core::ids::NodeId, to: meshcast_core::ids::NodeId, ids: &[meshcast_core::ids::ShortId]) -> meshcast_core::frame::Frame {
    use meshcast_core::frame::{Frame, Gossip};
    Frame::Gossip(Gossip { node: station, announcer: station, announcer_colour: 0, announcer_colours: 1, heard: Vec::new(), have: Vec::new(), have_sets: Vec::new(), want: ids.iter().map(|id| (*id, to, 0)).collect(), sets: Vec::new() })
}

#[test]
fn a_holder_uploads_earlier_pieces_first() {
    // A listener plays a collection from its first piece, so the earlier piece is needed first,
    // whichever grant brought it. A holder lined up what each grant brought behind what it already
    // had, and the first piece of a programme went after 33 later ones (FEASIBILITY.md §19).
    let (mut b, p) = granted_world(4, "snac-music:42", false);
    let station = b.engine.nodes[2].node.id();
    let src = &mut b.engine.nodes[0].node;
    let me = src.id();
    let now = b.engine.now;
    src.handle_frame(now, 1, &grant_frame(station, me, &[p[2], p[3]]), -60);
    src.handle_frame(now, 1, &grant_frame(station, me, &[p[1]]), -60);
    let order: Vec<_> = src.uploads(1).iter().map(|(id, _)| *id).collect();
    assert_eq!(order, vec![p[2], p[1], p[3]], "piece 1 should go right after the running upload of piece 2");
}

#[test]
fn a_holder_uploads_singles_smallest_first() {
    // The pieces of singles have no order: every one is first, and among them the smallest goes
    // first, as the carousel serves the most listeners per byte first (PROTOCOL.md §4). An album
    // of the same pieces goes in its order.
    for singles in [false, true] {
        let (mut b, p) = granted_world(4, "snac-music:42,snac-speech:22", singles);
        let station = b.engine.nodes[2].node.id();
        let src = &mut b.engine.nodes[0].node;
        let me = src.id();
        let now = b.engine.now;
        src.handle_frame(now, 1, &grant_frame(station, me, &p), -60);
        let order: Vec<_> = src.uploads(1).iter().map(|(id, _)| *id).collect();
        let want = if singles { vec![p[1], p[3], p[0], p[2]] } else { p.clone() };
        assert_eq!(order, want, "singles {singles}: the upload order is wrong");
    }
}

#[test]
fn an_announcer_asks_for_every_collection_s_first_pieces() {
    // Band L: an announcer asks in sets, four to a frame (PROTOCOL.md §3.3). In the order of
    // manifest ids, the sets of the lowest ids filled every frame while they were wanted, and the
    // pieces of a collection whose id sorted last were asked for three times in 26 minutes. Now
    // every collection gets a set before any gets a second, collections in progress first, and
    // once nothing has arrived for `T_want_min` none is in progress: two rounds name the first
    // missing piece of every collection (FEASIBILITY.md §19).
    let mut s = spec(BulkPreset::GfskL, vec![(0.0, 0.0), (300.0, 0.0), (150.0, 0.0)], vec![0], vec![2], 1.0);
    s.tracks = 16;
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:14").unwrap();
    s.collections.per_source = 8;
    let mut b = build(&s, Params::default());
    let manifests: Vec<_> = b.sources[0].collections.iter().map(|c| c.as_object().0.id.short()).collect();
    let mut t = 0;
    while !manifests.iter().all(|m| b.engine.nodes[2].node.holds(m)) {
        t += 5_000;
        assert!(t < 3_600_000, "the station should hold every collection manifest within an hour");
        b.engine.run(t, 600_000);
    }
    // Nobody else holds the pieces now: the station keeps asking, and after `T_want_min` no
    // collection is in progress any more.
    b.engine.set_alive(0, false);
    b.engine.run(t + 15 * 60_000, 600_000);
    let mut lacking: Vec<Vec<meshcast_core::ids::ShortId>> = vec![Vec::new(); 8];
    let mut by_index: Vec<(usize, meshcast_core::ids::ShortId)> = b.tracks.iter().map(|(id, tr)| (tr.index, *id)).collect();
    by_index.sort();
    for (k, id) in by_index {
        if !b.engine.nodes[2].node.holds(&id) {
            lacking[k / 2].push(id);
        }
    }
    let open = lacking.iter().filter(|l| !l.is_empty()).count();
    assert!(open > 4, "more collections than fit one frame should be wanted, {open} are");
    let station = &mut b.engine.nodes[2].node;
    let mut asked = station.ask_now();
    asked.extend(station.ask_now());
    for (c, l) in lacking.iter().enumerate() {
        if let Some(first) = l.first() {
            assert!(asked.contains(first), "collection {c}: its first missing piece was not asked for in two rounds");
        }
    }
}

#[test]
fn what_nobody_holds_does_not_keep_the_rest_from_being_asked_for() {
    // Band O: an announcer asks by name, eight to a frame (PROTOCOL.md §3.3). What nobody in
    // reach holds is wanted for good, and asked for in the order of place alone the same eight
    // took every frame: in a sparse band L network an announcer had never asked, after 50
    // minutes, for what its follower wanted, in 351 of 389 cases (FEASIBILITY.md §25). Now what
    // has been asked for and neither granted nor arriving for `T_excursion` goes last, the longest
    // asked ago first, and a few rounds name everything that is wanted.
    let mut s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (300.0, 0.0), (150.0, 0.0)], vec![0], vec![2], 1.0);
    s.tracks = 40;
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:14").unwrap();
    let mut b = build(&s, Params::default());
    let manifest = b.sources[0].collections[0].as_object().0.id.short();
    let mut t = 0;
    while !b.engine.nodes[2].node.holds(&manifest) {
        t += 1_000;
        assert!(t < 3_600_000, "the station should hold the collection manifest within an hour");
        b.engine.run(t, 600_000);
    }
    // Nobody else holds the pieces now: what the station lacks stays wanted, and after
    // `T_excursion` what it asked for is stuck.
    b.engine.set_alive(0, false);
    b.engine.run(t + Params::default().t_excursion_ms + 5 * 60_000, 600_000);
    let lacking: Vec<meshcast_core::ids::ShortId> = b.tracks.keys().copied().filter(|id| !b.engine.nodes[2].node.holds(id)).collect();
    assert!(lacking.len() > 16, "more than two frames of pieces should be wanted, {} are", lacking.len());
    let station = &mut b.engine.nodes[2].node;
    let mut asked = Vec::new();
    for _ in 0..lacking.len().div_ceil(8) {
        asked.extend(station.ask_now());
    }
    let never: Vec<_> = lacking.iter().filter(|id| !asked.contains(id)).collect();
    assert!(never.is_empty(), "{} of {} wanted pieces were not asked for in {} rounds", never.len(), lacking.len(), lacking.len().div_ceil(8));
}

#[test]
fn a_small_object_short_of_one_symbol_is_repaired() {
    // A node repairs by NACK what it holds at least 80 % of, or all of but one symbol. Under the
    // fraction alone an object of two to four symbols, a collection manifest, could never be
    // repaired, and one that had lost one of its two symbols waited for the next round of asking
    // (FEASIBILITY.md §20). Here a follower fetching its channel again loses the second symbol of
    // the collection manifest, and repairs it from its announcer. It keeps no menu of channels it
    // does not follow, so unfollowing evicts the collection manifest (PROTOCOL.md §4).
    let mut s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 3.0);
    s.tracks = 4;
    s.track_kb = 20;
    let mut b = build(&s, Params { relay_unfollowed: false, ..Params::default() });
    let chan = b.sources[0].channel;
    let manifest = b.sources[0].collections[0].as_object().0;
    let id = manifest.id.short();
    let k = manifest.len.div_ceil(meshcast_core::frame::SYMBOL_SIZE as u32);
    assert!((2..=4).contains(&k), "the collection manifest should be two to four symbols, is {k}");
    b.engine.run(3_600_000, 600_000);
    b.engine.nodes[1].node.unfollow(chan);
    b.engine.run(3_600_000 + 600_000, 600_000);
    assert!(!b.engine.nodes[1].node.holds(&id), "unfollowing should evict the collection manifest");
    b.engine.lose_symbol(1, id, (k - 1) as u16);
    let t = b.engine.now;
    b.engine.nodes[1].node.follow(chan);
    b.engine.poke(1);
    let mut at = None;
    let mut u = t;
    while u < t + 20 * 60_000 {
        u += 5_000;
        b.engine.run(u, 600_000);
        if b.engine.nodes[1].node.holds(&id) {
            at = Some(u - t);
            break;
        }
    }
    let at = at.expect("the follower should hold the collection manifest within 20 minutes");
    assert!(at <= 4 * 60_000, "the follower held the collection manifest after {:.1} min", at as f64 / 60_000.0);
}

#[test]
fn made_up_names_are_kept_within_bounds() {
    // Names are not checked, and every made-up one was kept for an hour: as a neighbour, and as an
    // asker in the announcer's carousel. A follower on its own firmware asks once a second, each
    // time under a new name, for half an hour. With the caps a node keeps at most `max_neighbours`
    // and at most `max_askers_per_object` per object, and everyone still gets everything; without
    // them the same flood fills both tables far beyond (docs/ABUSE.md, item 5).
    let positions: Vec<(f64, f64)> = (0..12).map(|i| (150.0 * (i % 4) as f64, 150.0 * (i / 4) as f64)).collect();
    let mut s = spec(BulkPreset::GfskO, positions, vec![0], vec![5], 0.5);
    s.tracks = 6;
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:42").unwrap();
    s.attack = Some(meshcast_sim::scenario::AttackSpec { attackers: 1, period_s: 1.0, spoof: true, renditions: false, lure: false, claim_max: false });
    let p = Params::default();
    let unbounded = Params { max_neighbours: usize::MAX, max_askers_per_object: usize::MAX, max_offered_ids: usize::MAX, max_conflicts: usize::MAX, ..Params::default() };
    for (params, bounded) in [(p, true), (unbounded, false)] {
        let mut b = build(&s, params);
        b.engine.run((s.hours * 3.6e6) as u64, 600_000);
        let peaks = &b.engine.metrics.table_peaks;
        let (neighbours, askers, objects) = (peaks["neighbours"], peaks["carousel askers"], peaks["carousel objects"]);
        if bounded {
            assert!(neighbours <= p.max_neighbours, "{neighbours} neighbours kept");
            assert!(askers <= objects * p.max_askers_per_object, "{askers} askers kept for {objects} objects");
            for (id, t) in &b.tracks {
                for &f in &t.followers {
                    assert!(b.engine.metrics.completions.contains_key(&(f, *id)), "node {f} missing {id:?}");
                }
            }
        } else {
            assert!(neighbours > p.max_neighbours && askers > objects * p.max_askers_per_object, "the flood should exceed both caps without them: {neighbours} neighbours, {askers} askers for {objects} objects");
        }
    }
}

fn have_frame(announcer: meshcast_core::ids::NodeId, ids: &[meshcast_core::ids::ShortId]) -> meshcast_core::frame::Frame {
    use meshcast_core::frame::{Frame, Gossip};
    Frame::Gossip(Gossip { node: announcer, announcer, announcer_colour: 0, announcer_colours: 1, heard: Vec::new(), have: ids.to_vec(), have_sets: Vec::new(), want: Vec::new(), sets: Vec::new() })
}

#[test]
fn a_holder_stops_uploading_what_its_announcer_holds() {
    // An announcer often completes an object before an upload of it ends, from symbols it
    // overheard or from another holder, and lists it in its next HAVE. The holder ends that
    // upload, running or lined up; before, a quarter of the upload frames of a band O town went to
    // announcers that already held the object (FEASIBILITY.md §22). Another announcer's HAVE
    // ends nothing of ours to this one.
    let (mut b, p) = granted_world(4, "snac-music:42", false);
    let station = b.engine.nodes[2].node.id();
    let other = meshcast_core::ids::NodeId(0x6666);
    let src = &mut b.engine.nodes[0].node;
    let me = src.id();
    let now = b.engine.now;
    src.handle_frame(now, 1, &grant_frame(station, me, &[p[0], p[1], p[2]]), -60);
    assert_eq!(src.uploads(1).len(), 3, "three grants should line up three uploads");
    src.handle_frame(now + 10, 1, &have_frame(other, &[p[0], p[1]]), -60);
    assert_eq!(src.uploads(1).len(), 3, "another announcer's HAVE should end nothing");
    src.handle_frame(now + 20, 1, &have_frame(station, &[p[0], p[1]]), -60);
    let left: Vec<_> = src.uploads(1).iter().map(|(id, _)| *id).collect();
    assert_eq!(left, vec![p[2]], "only the object the announcer does not hold should be left");
    assert_eq!(src.stats.uploads_ended_held, 2);
}

#[test]
fn a_repair_answer_leaves_out_what_an_upload_just_sent() {
    // A granted uploader answers a NACK behind the upload it came during. That upload sends the
    // symbols the asker lacked, and the asker, which we heard, hears them: the answer leaves them
    // out. In a band O town thirteen followers were sent 150 symbols each that the upload ahead of
    // the answers had already brought them (FEASIBILITY.md §22).
    use meshcast_core::frame::{Frame, Nack};
    let (mut b, p) = granted_world(4, "snac-music:42", false);
    // Another cell's announcer that never speaks again: nothing ends the upload to it early.
    let other = meshcast_core::ids::NodeId(0x6666);
    let asker = meshcast_core::ids::NodeId(0x7777);
    let now = b.engine.now;
    {
        let src = &mut b.engine.nodes[0].node;
        let me = src.id();
        src.handle_frame(now, 1, &grant_frame(other, me, &[p[0]]), -60);
        src.handle_frame(now + 10, 1, &Frame::Nack(Nack { node: asker, object: p[0], block: 0, answerer: me, phase: 0, missing: vec![(0, 40)] }), -60);
        assert_eq!(src.uploads(1), vec![(p[0], other), (p[0], asker)], "the answer should wait behind the upload");
    }
    b.engine.poke(0);
    let mut t = now;
    while b.engine.nodes[0].node.uploads(1).contains(&(p[0], other)) {
        t += 5_000;
        assert!(t < now + 20 * 60_000, "the upload should end within 20 minutes");
        b.engine.run(t, 600_000);
    }
    let up = b.engine.nodes[0].node.stats.uploads_started;
    assert!(!b.engine.nodes[0].node.uploads(1).contains(&(p[0], asker)), "the answer should be left with nothing to send");
    b.engine.run(t + 60_000, 600_000);
    assert_eq!(b.engine.nodes[0].node.stats.uploads_started, up, "no answer should have gone out");
}

#[test]
fn an_upload_to_another_cell_outlives_a_change_of_announcer() {
    // What another cell's announcer granted a holder is still that announcer's when the holder
    // follows someone new. Dropped then, the upload ended unfinished and unsaid, its grant ran idle
    // for `T_grant`, and in a band O town the first piece of a programme came last
    // (FEASIBILITY.md §22).
    use meshcast_core::frame::{Beacon, CarrierKind, Frame};
    let (mut b, p) = granted_world(4, "snac-music:42", false);
    let other = meshcast_core::ids::NodeId(0x6666);
    let louder = meshcast_core::ids::NodeId(0x5555);
    let now = b.engine.now;
    let src = &mut b.engine.nodes[0].node;
    let me = src.id();
    let before = src.announcer_of(1);
    src.handle_frame(now, 1, &grant_frame(other, me, &[p[0]]), -60);
    assert_eq!(src.uploads(1), vec![(p[0], other)]);
    let beacon = Beacon { carrier: CarrierKind::GfskBulk, announcer: louder, score: 1000, caps: 0, next_ms: 30_000, round: 0, time: 0, time_quality: 0, colour: 0, colours: 1, upload_phases: 1, occupancy: [0; 4] };
    src.handle_frame(now + 10, 1, &Frame::Beacon(beacon), -20);
    assert!(src.announcer_of(1) == louder && before != louder, "the source should now follow the louder announcer");
    assert_eq!(src.uploads(1), vec![(p[0], other)], "the upload to another cell should go on");
}

#[test]
fn the_control_carrier_is_heard_in_its_window() {
    // A dongle's one SX1262 receives LoRa or GFSK, never both at once, so it listens on the LoRa
    // control carrier only in the control window, and every control-carrier frame goes in it
    // (PROTOCOL.md §3). A node 5 km from a source, beyond the reach of band O GFSK, still gets
    // the source's root and its collection manifest on LoRa, inside a window, and no frame
    // reaches a node tuned to its other carrier.
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (5000.0, 0.0)], vec![0], vec![], 1.0);
    let p = Params::default();
    let (period, window) = (p.t_ctrl_period_ms, p.t_ctrl_window_ms);
    assert!(window > 0 && period > window, "the control window should be on by default");
    let mut b = build(&s, p);
    let manifest = b.sources[0].collections[0].as_object().0.id.short();
    b.engine.run(3_600_000, 600_000);
    let t = *b.engine.metrics.completions.get(&(1, manifest)).expect("the far node should get the collection manifest on LoRa");
    // The window that contains the arrival, or else the next one: the arrival is in it.
    let far = &b.engine.nodes[1];
    let (start, end) = far.node.ctrl_window_at(far.clock.local(t)).unwrap();
    let local = far.clock.local(t);
    assert!(start <= local && local <= end, "it arrived at {local}, outside the window {start}..{end}");
    assert_eq!(b.engine.metrics.frames_not_listening, 0, "a frame went out while its receivers listened elsewhere");
}

#[test]
fn nodes_whose_clocks_start_apart_agree_on_the_time() {
    // Band L hops, so a node that knows no time finds nobody on it (PROTOCOL.md §6). Fifty nodes
    // whose clocks start up to an hour apart and run up to 20 ppm fast or slow: without a shared
    // time 4 to 20 % of a neighbourhood's programme arrived; with it, every node keeps one time
    // within a second after a quarter of an hour, and within a few milliseconds after that.
    let mut s = cell(BulkPreset::GfskL, 50, 2.0, 1);
    s.clocks = meshcast_sim::scenario::ClockSpec { epoch_s: 3600, ppm: 20, same: false };
    let mut b = build(&s, Params::default());
    b.engine.run(15 * 60_000, 600_000);
    let (spread, near, n) = b.engine.shared_time_spread();
    assert_eq!(near, n, "{near} of {n} nodes within a second of the median, spread {spread} ms");
    b.engine.run(2 * 3_600_000, 600_000);
    let (spread, _, _) = b.engine.shared_time_spread();
    assert!(spread < 100, "the shared time drifted {spread} ms apart");
}

#[test]
fn a_follower_that_missed_the_first_pass_asks_again() {
    // Band L, the neighbourhood of the matrix, world 5. Two followers still listened for the time
    // when the station first passed four objects (PROTOCOL.md §6). Afterwards they took one or two
    // symbols from each repetition: every symbol counted as progress, so the want never stalled,
    // they never asked again, and after six hours they held 64 of 113. What has not completed
    // `T_excursion` after the last ask is asked for again (PROTOCOL.md §4).
    let mut s = cell(BulkPreset::GfskL, 50, 6.0, 5);
    s.sources = 2;
    s.tracks = 10;
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

fn bystander_bridge(params: Params) -> (usize, usize, u64) {
    // Station A with the source, station B with two listeners 1.35 km away in band L (range about
    // 985 m), and one node between them that hears both and follows A by signal. Only B's two
    // listeners follow the channel: nobody in A's cell asks for it, and the node between follows
    // nothing.
    let positions = vec![(-400.0, 0.0), (0.0, 0.0), (450.0, 0.0), (1350.0, 0.0), (1500.0, 100.0), (1550.0, -100.0)];
    let mut s = spec(BulkPreset::GfskL, positions, vec![0], vec![1, 3], 6.0);
    s.mix = meshcast_sim::scenario::parse_mix("snac-music:42,snac-speech:22").unwrap();
    let mut b = build(&s, params);
    let chan = b.sources[0].channel;
    for i in [1, 2, 3] {
        b.engine.nodes[i].node.unfollow(chan);
    }
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    let mut missing = 0;
    for id in b.tracks.keys() {
        for f in [4, 5] {
            if !b.engine.metrics.completions.contains_key(&(f, *id)) {
                missing += 1;
            }
        }
    }
    let bridge = &b.engine.nodes[2].node;
    (missing, b.tracks.len() * 2, bridge.stats.relays_declined)
}

#[test]
fn a_bystander_relays_for_another_cell() {
    // Content crosses a cell nobody in which follows its channel: the node between hears B ask for
    // its listeners, fetches the collection manifest the ask names from its own announcer, which
    // holds the manifests of every channel it hears of, then the pieces, which A fetches from the
    // source only because the node asked, and hands them on to B (PROTOCOL.md §2, §4).
    let (missing, pairs, _) = bystander_bridge(Params::default());
    assert_eq!(missing, 0, "{missing} of {pairs} listener-piece pairs never completed");
    // Relaying only for channels it follows, the node between does nothing, and B's listeners hear
    // nobody else.
    let (missing, pairs, _) = bystander_bridge(Params { relay_unfollowed: false, ..Params::default() });
    assert_eq!(missing, pairs, "B's listeners should get nothing without a relay");
    // A node with no carry budget relays nothing for others.
    let (missing, pairs, declined) = bystander_bridge(Params { carry_budget_bytes: 0, ..Params::default() });
    assert!(missing == pairs && declined > 0, "a node without a carry budget should decline: {missing} of {pairs} missing, {declined} declined");
}

#[test]
fn made_up_relay_asks_are_kept_within_bounds() {
    // An ask names its object by an id nobody checks, and relaying is for any channel. A follower
    // relays only what a manifest it holds names, or fetches first the collection manifest that a
    // set names, from its own announcer; and what it keeps of such asks is capped
    // (PROTOCOL.md §4, docs/ABUSE.md item 5).
    use meshcast_core::frame::{Frame, Gossip, PieceSet, WantSet, ASK_LISTENED};
    use meshcast_core::ids::{NodeId, ShortId};
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (300.0, 0.0), (600.0, 0.0)], vec![0], vec![1], 2.0);
    let cap = 8;
    let mut b = build(&s, Params { max_relay_asks: cap, ..Params::default() });
    b.engine.run(3_600_000, 600_000);
    let made_up = |i: u32| {
        let mut x = [0xAB; 8];
        x[..4].copy_from_slice(&i.to_le_bytes());
        ShortId(x)
    };
    // Another cell's announcer asks, for listeners, for the same twelve made-up pieces every minute
    // for twenty minutes, past `T_relay_wait`, and for twelve sets of new made-up collections.
    let foreign = NodeId(9_999);
    for k in 0..20u32 {
        let ask = Frame::Gossip(Gossip {
            node: foreign,
            announcer: foreign,
            announcer_colour: 0,
            announcer_colours: 1,
            heard: Vec::new(),
            have: Vec::new(),
            have_sets: Vec::new(),
            want: (0..12).map(|i| (made_up(i), NodeId::NONE, ASK_LISTENED)).collect(),
            sets: (0..12).map(|i| WantSet { set: PieceSet { manifest: made_up(1_000 + 12 * k + i), first: 0, bits: 0b111 }, grant: NodeId::NONE, phase: ASK_LISTENED }).collect(),
        });
        let now = b.engine.now;
        b.engine.nodes[2].node.handle_frame(now, 1, &ask, -60);
        b.engine.run(now + 60_000, 600_000);
    }
    let node = &b.engine.nodes[2].node;
    assert!((0..12).all(|i| !node.wants_object(&made_up(i))), "a made-up piece should not be relayed");
    let kept = node.table_sizes().into_iter().find(|(n, _)| *n == "relay asks").map(|(_, v)| v).unwrap_or(0);
    assert!(kept <= cap, "{kept} relay asks kept, cap {cap}");
    for id in b.tracks.keys() {
        assert!(node.holds(id), "the follower should still hold {id:?}");
    }
}

#[test]
fn a_station_that_fetches_on_request_hears_of_a_new_episode_soon() {
    // A station fetches what its followers ask for (PROTOCOL.md §2), and a follower asks at most
    // every `T_want_min`. A new episode whose manifest comes just after the follower asked for the
    // one before waited until its next ask; now, once the station's next GOSSIP shows it neither
    // has the episode nor asks for it, the follower asks for it (PROTOCOL.md §4).
    use meshcast_core::manifest::{Collection, CollectionKind, Manifest};
    use meshcast_core::object::ContentType;
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (1000.0, 0.0), (500.0, 0.0)], vec![0], vec![2], 2.0);
    let mut b = build(&s, Params::default());
    let chan = b.sources[0].channel;
    b.engine.nodes[2].node.unfollow(chan);
    b.engine.run(3_600_000, 600_000);
    let publish = |b: &mut meshcast_sim::scenario::Built| {
        let src = &mut b.sources[0];
        let o = meshcast_sim::scenario::track_object(s.seed, src.node, src.objects.len(), 20_000, ContentType::Speech);
        src.objects.push(o.clone());
        src.seq += 1;
        let series = Collection { cid: 1, kind: CollectionKind::Series, title: "Series".into(), pieces: src.objects.clone(), schedule: Vec::new() };
        let root = Manifest::sign(&src.key, src.seq, "Channel", vec![series.reference(None, true)], None, None);
        let node = src.node;
        b.engine.nodes[node].node.publish(&root, std::slice::from_ref(&series), &[(o.meta(), None)]);
        b.engine.poke(node);
        (o.id.short(), b.engine.now)
    };
    let (first, _) = publish(&mut b);
    b.engine.run(3_600_000 + 2 * 60_000, 600_000);
    let (second, at) = publish(&mut b);
    b.engine.run(3_600_000 + 30 * 60_000, 600_000);
    let done = |id| b.engine.metrics.completions.get(&(1, id)).copied();
    assert!(done(first).is_some(), "the follower should have the first episode");
    let got = done(second).expect("the follower should have the second episode");
    assert!(got <= at + 5 * 60_000, "the second episode arrived {:.1} min after it was published", (got - at) as f64 / 60_000.0);
}

#[test]
fn a_relay_ends_when_the_cell_that_asked_holds_it() {
    // A relay ends when the cell that asked for it holds the object, and only then: another cell's
    // announcer listing the object says nothing about the cell that asked. Withdrawing on any
    // announcer's HAVE, relays for a cell that still lacked the object were dropped, taken on again
    // and dropped, and its followers left their announcer (FEASIBILITY.md §28).
    use meshcast_core::frame::{Frame, Gossip, ASK_LISTENED};
    use meshcast_core::ids::NodeId;
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (300.0, 0.0), (600.0, 0.0)], vec![0], vec![1], 2.0);
    let mut b = build(&s, Params::default());
    let chan = b.sources[0].channel;
    // Node 2 follows nothing and keeps the menu: it can name the pieces, and holds none.
    b.engine.nodes[2].node.unfollow(chan);
    b.engine.run(3_600_000, 600_000);
    let w = *b.tracks.keys().next().unwrap();
    assert!(!b.engine.nodes[2].node.holds(&w), "node 2 should not hold the piece");
    let gossip = |from: NodeId, want: bool, have: bool| {
        Frame::Gossip(Gossip { node: from, announcer: from, announcer_colour: 0, announcer_colours: 1, heard: Vec::new(), have: if have { vec![w] } else { Vec::new() }, have_sets: Vec::new(), want: if want { vec![(w, NodeId::NONE, ASK_LISTENED)] } else { Vec::new() }, sets: Vec::new() })
    };
    let (asking, other) = (NodeId(9_001), NodeId(9_002));
    // Another cell's announcer asks for its listeners until node 2 takes the relay on.
    let mut taken = false;
    for _ in 0..30 {
        let now = b.engine.now;
        b.engine.nodes[2].node.handle_frame(now, 1, &gossip(asking, true, false), -60);
        if b.engine.nodes[2].node.wants_object(&w) {
            taken = true;
            break;
        }
        b.engine.run(now + 60_000, 600_000);
    }
    assert!(taken, "node 2 should take the relay on");
    let now = b.engine.now;
    b.engine.nodes[2].node.handle_frame(now, 1, &gossip(other, false, true), -60);
    assert!(b.engine.nodes[2].node.wants_object(&w), "another cell holding it should not end the relay");
    b.engine.nodes[2].node.handle_frame(now, 1, &gossip(asking, false, true), -60);
    assert!(!b.engine.nodes[2].node.wants_object(&w), "the asking cell holding it should end the relay");
}

#[test]
fn an_announcer_reads_a_root_it_overheard_unannounced() {
    // Symbols carry no kind. An announcer that overheard a neighbouring cell's upload of its
    // channel's new root, and heard no announcement of it, held the root as content and never read
    // it, and its cell stayed a seq behind. A root's bytes name its channel and carry the channel's
    // signature: they say what it is (PROTOCOL.md §1, FEASIBILITY.md §28).
    use meshcast_core::frame::{Bulk, Frame, SYMBOL_SIZE};
    use meshcast_core::manifest::{Collection, CollectionKind, Manifest};
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (300.0, 0.0), (600.0, 0.0)], vec![0], vec![1], 2.0);
    let mut b = build(&s, Params::default());
    b.engine.run(3_600_000, 600_000);
    let chan = b.sources[0].channel;
    let station = 1;
    assert!(b.engine.nodes[station].node.is_announcing(), "the station should announce");
    let seq = b.engine.nodes[station].node.manifest_state(&chan).expect("the station should know the channel's root").0;
    // The source signs a new root; the station hears only its symbols, as if uploaded next door.
    let src = &b.sources[0];
    let series = Collection { cid: 1, kind: CollectionKind::Series, title: "Series".into(), pieces: src.objects.clone(), schedule: Vec::new() };
    let m = Manifest::sign(&src.key, seq + 1, "Channel", vec![series.reference(None, true)], None, None);
    let (meta, bytes) = m.as_object();
    let now = b.engine.now;
    for (esi, chunk) in bytes.chunks(SYMBOL_SIZE).enumerate() {
        let mut payload = vec![0u8; SYMBOL_SIZE];
        payload[..chunk.len()].copy_from_slice(chunk);
        let f = Frame::Bulk(Bulk { object: meta.id.short(), block: 0, esi: esi as u16, len: meta.len, payload });
        b.engine.nodes[station].node.handle_frame(now, 1, &f, -60);
    }
    let held = b.engine.nodes[station].node.manifest_state(&chan);
    assert_eq!(held.map(|h| (h.0, h.2)), Some((seq + 1, true)), "the station should have read the root it overheard: {held:?}");
}

#[test]
fn an_announcer_that_listens_alone_is_relayed_to() {
    // An announcer is a listener too. Marking only what its followers asked for, an announcer that
    // followed a channel nobody else in its cell followed asked for it unmarked, and the node
    // between it and the source, which follows nothing, never relayed it (PROTOCOL.md §4,
    // FEASIBILITY.md §28).
    let s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (100.0, 0.0), (2500.0, 0.0), (5000.0, 0.0)], vec![0], vec![1, 3], 4.0);
    let mut b = build(&s, Params::default());
    let chan = b.sources[0].channel;
    b.engine.nodes[2].node.unfollow(chan);
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    assert!(b.engine.nodes[3].node.is_announcing(), "the far station should announce");
    let missing = b.tracks.keys().filter(|id| !b.engine.nodes[3].node.holds(id)).count();
    assert_eq!(missing, 0, "the far station should hold every piece, {missing} missing");
}

#[test]
fn announcers_that_meet_only_on_the_control_carrier_share_its_window() {
    // Band O does not hop, so a node that knows no time hears its cell's announcer and the time with
    // it. But two cells whose bulk carriers do not reach each other meet only on the control
    // carrier, in the windows they share, and the shared time places the windows. Telling and
    // watching the time there only where the carrier hops, four in five such pairs of announcers in
    // band O never shared a window (PROTOCOL.md §6, FEASIBILITY.md §29).
    use meshcast_core::frame::CarrierKind;
    let mut s = spec(BulkPreset::GfskO, vec![(0.0, 0.0), (100.0, 0.0), (6000.0, 0.0), (6100.0, 0.0)], vec![1], vec![0, 2], 3.0);
    s.clocks = meshcast_sim::scenario::ClockSpec { epoch_s: 3600, ppm: 20, same: false };
    let mut b = build(&s, Params::default());
    b.engine.run((s.hours * 3.6e6) as u64, 600_000);
    let ctrl = b.engine.phys.iter().position(|p| p.kind == CarrierKind::LoraControl).unwrap();
    let bulk = b.engine.phys.iter().position(|p| p.kind != CarrierKind::LoraControl).unwrap();
    let (pairs, apart, partly) = b.engine.ctrl_only_pairs(ctrl, bulk);
    assert!(pairs >= 1, "the two cells should hear each other only on the control carrier");
    assert_eq!((apart, partly), (0, 0), "their control windows should meet");
}
