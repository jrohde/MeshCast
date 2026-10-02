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

`dynamics` models a living network: K channels, each node follows a few, subscriptions change
over time, every channel publishes a new bulletin periodically and drops its oldest. It reports
delivery latency per publication, wasted receptions and orphaned objects.

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

An ensemble (`--seeds N`) also reports **whole content**, what a listener of a programme split into
pieces waits for: per follower and source, how long until the follower held the first of that
source's objects and until it held all of them (the median of each, the 90th percentile of the
second, and the share of pairs that never held all), with the mean number of frames sent of any
kind; and where the last piece waited, when the follower's announcer held it and how much later
the follower did, which tells a supply problem from a delivery problem. It also reports **playback**, per
follower and collection it follows: when the follower held the collection's first piece, and the
earliest time it could have started playing the collection there and played it to the end without
waiting, given how long each piece plays (the latest of each piece's arrival less the playing time
of the pieces before it). The cell report also
breaks down collisions between uploads to one announcer by cause. `--tracks N --mix snac-music:42` splits a source's content into N pieces of 42 kB; the
object-size sweep of `docs/FEASIBILITY.md` §13 is built from those two options. `dynamics` also
prints how many manifest corrections followers sent to their announcer (`docs/PROTOCOL.md` §2).

A source publishes its pieces as collections (`docs/PROTOCOL.md` §2): one series by default.
`--collections N` splits them in order over N albums, `--follow-collections K` makes each follower
follow K of a source's collections, chosen at random, instead of the whole channel, and
`--cover-kb N` gives every collection a cover of N kB. Covers are reported as their own kind and
left out of whole content. An ensemble also reports what a follower holds at the end, in kB, as
`held_kb_per_follower`. `dynamics` keeps one series per channel and publishes each bulletin as a
new episode of it.

## Looking inside one world

`--seeds N` runs N worlds and reports their spread; compare designs on that, not on one run
(`docs/FEASIBILITY.md` §9.1). To look inside one of them:

- `MESHCAST_ONLY_SEED=6` with `--seed 1 --seeds 8` runs only the sixth world of that ensemble
  and prints its full report. For scenarios that generate their own positions (`clusters`) this
  is not the same world as `--seed 6`: the ensemble keeps the positions of its first seed.
- `MESHCAST_TRACE_GRANTS=1` writes one line per event to stderr: `GA` open ask, `GT` grant, `GH`
  grant heard by its holder, `OF` offer, `UP` upload frame sent, `UO` upload frame received by
  its announcer, `UC` upload frame collided there, `BO` other symbol of an object an announcer
  wants, `MA` manifest announcement, `OC` object completed, `OX` an offer reaching (`ok`) or not reaching
  (`col` collided, with the interferer; `hd` the announcer was sending) an announcer that wants
  what it offers, and at the end `MISSING` for every
  object a follower lacks (with the announcers it hears and who listed the object, to tell
  whether an excursion or a named repair was possible) and `NOLEN` for every want whose length
  the wanter does not know.
- `MESHCAST_DEBUG_WANTS=1` traces each announcer's want list.
- `MESHCAST_TRACE_ROLES=1` prints every role change as `ROLE t node carrier role announcer`.
- `MESHCAST_TRACE_BEACONS=1` prints every beacon with its score, channel and colour, and every
  node that receives it above sensitivity with its own score; an `x` after a node means it was
  tuned to another channel and missed it.
- `MESHCAST_TRACE_ANNOUNCE=1` prints every manifest announcement and who heard it.
- `MESHCAST_T_EXCURSION_MIN=40` overrides `T_excursion` for an experiment, and
  `MESHCAST_BACKOFF_MAX_ATTEMPT=4` the number of times the random backoff after a busy channel
  doubles.
- `MESHCAST_TRACE_BUSY=0` counts, for node index 0, which transmitter kept its channel busy each
  time it wanted to send, with the first and last minute; the cell report prints it as
  `busy blame`. It found an announcer whose carousel chained frames without a gap
  (`docs/FEASIBILITY.md` §14).
- The grant trace reads sets as their sender means them, so `GA`, `GT` and `OF` lines name
  pieces, not sets.

The reports count role changes, challenges (a follower stepping up against a less capable
announcer) and excursions (`docs/PROTOCOL.md` §4, §5.2); `dynamics` also prints how many
announcers there were 5, 10, 20, 30 and 60 minutes after everyone switched on at once, and how
many role changes happened in the first hour and after it.

## Attackers

`--attackers N` makes N followers flood the announcer they follow with WANTs for every object of
the scenario, at random times averaging `--attack-period-s` (default 60); `--attack-spoof` gives
every WANT a fresh made-up node id, and `--attack-renditions` asks for renditions too.
`--attack-lure` makes them pose instead as announcers that have everything: where every follower
listens (the rendezvous on a hopping carrier, any time on one that does not hop) they beacon and
list objects in HAVE, alternately, and never serve anything (their own protocol
is muted); with `--attack-claim-max` their beacons claim the maximum score and full capability
instead of nothing. The report
adds the attackers' frames; compare bulk frames and the followers' latency with a run without
them. What the protocol does about it is in `docs/ABUSE.md` and `docs/FEASIBILITY.md` §11. (A
fixed period from time zero turned out to put every WANT on a dwell boundary, where it jammed the
start of each upload; that is a jammer, not a request flood, and the timing is random for that
reason.)
