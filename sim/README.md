# sim

Phase 0: a discrete-event simulator that runs the real `meshcast-core` protocol against modelled
radios, so the design is tested before any hardware is soldered.

## Build and run

```
cargo build --release
./target/release/meshcast-sim budget                      # analytic link budget table, no simulation
./target/release/meshcast-sim two-nodes --distance-m 1000 --hours 6 --tracks 3
./target/release/meshcast-sim cell --nodes 50 --area-km2 4 --stations 1 --sources 1 --hours 12
./target/release/meshcast-sim failover --nodes 20 --kill-at-h 2 --revive-at-h 6 --hours 10
./target/release/meshcast-sim cell --nodes 200 --area-km2 30 --stations 3 --sources 5 --hours 24 --bulk gfsk-l --out report.json
```

`--bulk` selects the content carrier: `gfsk-o` (EU band O, 500 mW, 10 % duty cycle), `gfsk-l`
(EU band L, 25 mW, polite access hopping over 15 channels), `esp-now` (2.4 GHz long-range mode),
`gfsk-us` (FCC 15.247, 1 W, no duty cycle), `lora-bulk` (LoRa SF7 as the only carrier, for sparse
rural cells). `--verbose` prints role changes; `--out` writes the full report as JSON.

## What is modelled

- **Nodes**: every node is a `meshcast_core::node::Node` driven by `Tick` and `Rx` events; the
  engine never touches protocol state. Stations differ only by `mains` and `has_ip`, which feed
  the election score.
- **Propagation**: log-distance path loss (exponent `--exponent`, default 3) plus per-link
  log-normal shadowing (`--shadow-db`, default 6), free-space loss at 1 m per carrier frequency.
- **Reception**: sensitivity per carrier from the SX1262 datasheet and the ESP-NOW field test
  (see `docs/FEASIBILITY.md`); capture effect (6 dB); half-duplex (a node transmitting cannot
  receive); collisions between overlapping frames on the same carrier and channel.
- **CCA and occupancy**: energy from others above the CCA threshold marks the channel busy and
  accumulates into a 10 s occupancy window that EtherFatsoen reads.
- **Frequency agility**: on multi-channel carriers each announcer hops a pseudo-random sequence
  (20 s dwell); followers and uploaders follow it; nodes without an announcer scan.
- **EtherDiscipline**: the core's own accounting enforces duty cycle or polite access per band;
  the engine only reports airtime.

Not modelled yet: terrain, buildings, antenna patterns, GFSK versus LoRa co-channel interference
across carriers, clock drift, packet error rate below the sensitivity cliff, multiple bulk
carriers per node.

## Output

The report prints per-object completion times over followers (p50, p90, max), collisions,
announcers at the end, occupancy percentiles on the bulk channel, airtime share per transmitting
node, and per-node core counters (frames by class, CCA deferrals, discipline waits, NACKs, WANTs,
uploads). Results and their interpretation are in `docs/FEASIBILITY.md` §7.
