# Roadmap

Four phases. Each has a definition of done that is a measurement or a demonstration, not a
feeling. Phases may overlap; the order of the definitions of done may not.

## Phase 0: simulator and calculation model

**Goal:** prove or disprove the design on a laptop before touching hardware, and produce the
starting parameters for EtherFatsoen and the election.

Scope:
- Cargo workspace with `core` (protocol, `no_std`) and `sim` (host).
- `core` v0: objects and symbols, manifests with Ed25519 signatures, the five frame types and
  their parsers (fuzzed), carousel, gossip, NACK repair, announcer election with healing,
  EtherFatsoen gate, EtherDiscipline accounting for the EU868 and US915 profiles.
- `sim`: discrete-event engine; radio models for GFSK (band O and band L polite), LoRa SF7–SF12,
  and ESP-NOW LR, using the sensitivity figures in [FEASIBILITY.md](FEASIBILITY.md); log-distance
  path loss with log-normal shadowing; capture effect; hidden nodes; per-node regulatory
  accounting; scenario files (node placement, power sources, libraries, follow lists, failure
  events); metrics export (CSV/JSON) and a few plots.
- A small analytic notebook-equivalent (a `sim` subcommand) for link budget and airtime per frame.

Definition of done, with numbers written into FEASIBILITY.md:
1. Delivered MB per hour per carrier at node densities of 10, 100 and 1000 per km².
2. Time for a 3-minute Opus track and for a 10-track album to reach 90 % of followers across a
   modelled town (e.g. 30 km², 200 nodes, 3 stations), with and without internet at the stations.
3. Collision and hidden-node loss rate versus density; the occupancy at which delivery time
   doubles; whether the 30 % target holds.
4. Announcer failover: time from announcer loss to a new carousel running, in a 2-node, 20-node
   and 200-node cell; demonstration that two announcers never persist in one connected cell for
   more than a few beacon intervals; partition and merge behaviour.
5. The 5000-source / 100 000-receiver national scenario, at least as an extrapolation from
   per-cell results.
6. Tuned draft parameters written back into PROTOCOL.md §8 and ETHERFATSOEN.md §4.

## Phase 1: two boards on a bench, then a street

**Goal:** the real radios agree with the simulator.

Scope:
- `firmware` for XIAO ESP32S3 + Wio-SX1262 (and one Heltec/LilyGo board): `core` + SX126x GFSK
  and LoRa + ESP-NOW; serial console; no BLE, no audio yet.
- Bench: two boards exchange an object over GFSK band O, over GFSK band L polite, and over
  ESP-NOW; the LoRa control channel carries beacons and gossip.
- Street: the same two boards at 100 m, 500 m, 1 km, 3 km; a third board as bridge.
- Decision point on the Rust toolchain for Xtensa: keep, or fall back to C++ firmware with a
  `core` port.

Definition of done:
1. Measured bit rate, packet error rate and range per carrier, tabulated next to the simulator's
   prediction; simulator models corrected where they differ by more than a factor of two.
2. EtherDiscipline accounting verified against a spectrum analyser or SDR recording: the boards
   never exceed the profile's duty cycle or polite limits over an hour.
3. Announcer election observed on hardware: power off the announcer, watch the other take over.

## Phase 2: a station and a district

**Goal:** the upcycled miner joins as a node and a small real network runs for weeks.

Scope:
- `station` on the miner: `core` + SX1302 via `libloragw` bindings or a second SX1262; object
  store; Opus transcoding; HTTPS seed/fetch between two stations in different towns; metrics.
- Coexistence with Meshpoint decided and implemented (time-share or second radio).
- Phone app v0: BLE to a dongle, follow a channel via QR code, see the library, play a track.
- Signed manifests end to end: publish from a phone, hear it on another dongle.
- Ten to twenty nodes across a district, some offline, one station.

Definition of done:
1. Thirty days of uptime with metrics: occupancy, delivery times, NACK rates, election events.
2. A track published in town A appears in town B via the internet seeders and then radiates
   locally over GFSK/ESP-NOW; the same track published with the internet unplugged still crosses
   the district by radio, only slower, and the measured times are recorded.
3. A newcomer who flashes a dongle and scans one QR code hears the channel within a day with no
   further configuration.

## Phase 3: voice, encryption, repair codes, OTA

**Goal:** the features that make it a radio station rather than a file mover.

Scope:
- Spoken bulletins: record on the phone, Opus 8 kbit/s or Codec2, scheduled like tracks.
- Encrypted channels with out-of-band key sharing.
- RaptorQ repair symbols in the carousel (v1 repair), NACK reduced to the exception path.
- EtherFatsoen congestion control tuned on the real network; spectrum weather in beacons.
- Firmware update as an object on a system channel, verified and applied by the node.
- Additional EtherDiscipline profiles verified (AU915, AS923/JP, IN865, KR920, RU864, CN470).

Definition of done:
1. A daily five-minute bulletin reaches every follower in the district before its scheduled slot.
2. An encrypted channel is unreadable to a non-subscriber node with a full copy of the objects.
3. Repair symbols reduce NACK traffic by an order of magnitude at equal delivery time.
4. A firmware update reaches and is applied by every node in the district over the carousel.

## Later, unordered

- Text pages and offline web bundles as objects (a "web channel").
- Map tiles; emergency information channels.
- nRF52 boards; SX1280-class 2.4 GHz LoRa as a carrier; WiFi HaLow if modules get cheap.
- Trusted-station preference and per-symbol authentication (PROTOCOL.md open questions).
- Amateur-radio profile.
