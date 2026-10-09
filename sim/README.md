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

`--quiet-channels N --quiet-publish-h H` adds N channels that nobody follows at the start and that
publish every H hours, spread over that span (0: never): the rest of a large menu
(`docs/FEASIBILITY.md` §30). With more channels than nodes, a node runs several.

`MESHCAST_IP_STATIONS=1` gives every station what every source publishes as it is published, as
if it had fetched it over the internet: the catalogue at the start and, in `dynamics`, each later
bulletin to the stations that are on (`docs/FEASIBILITY.md` §36.3); the radio does the rest. Unset
or `0`: no internet.
`MESHCAST_BLOCK_NACK=0` makes a node NACK only what is nearly complete, as before
`docs/FEASIBILITY.md` §36.4. `MESHCAST_ORDER_BY_LOST=0` lets candidates step up by chance instead
of by how loud they heard the announcer they lost, and `MESHCAST_FRESH_TIE=0` keeps two announcers
that stepped up together from settling a near-tie by id when they judge each other far, as before
`docs/FEASIBILITY.md` §37.
`MESHCAST_SLOTS_UNDER_CAP=0` keeps announcers in conflict from taking turns under a duty cycle or
polite access; `MESHCAST_UPLOAD_SLOTS=1` makes uploads there wait for their announcer's slot as
well and spend a slot cycle's budget at once, and with the first switch only the latter;
`MESHCAST_TURN_GUARD=0` lets a turn take any frame that starts in it. The three together behave
as before `docs/FEASIBILITY.md` §38. `MESHCAST_T_SLOT_MS` sets `T_slot`.
`MESHCAST_BUSY_FOR_US=0` makes a follower leave an announcer that has named no uploader in
`T_excursion` for an object it wants, however busy it is getting the rest, as before
`docs/FEASIBILITY.md` §39.
`meshcast-sim partition` is `failover` with a wall instead of a switched-off station: from
`--wall-at-h` to `--merge-at-h` every link between the west and the east half of the area loses
`--wall-db` (100) more; its report says how soon each half had an announcer, whether two that hear
each other announced on one side (from ten minutes after the wall went up) or after the wall came
down, and when the followers in the half without the source held everything
(`docs/FEASIBILITY.md` §41).
`MESHCAST_BURST_PER_TURN=0` keeps a token-bucket burst raised for a turn or an upload phase
for good, and `MESHCAST_CHALLENGE_IN_ORDER=0` lets a challenger on a carrier that does not hop
step up within the election jitter by chance, both as before `docs/FEASIBILITY.md` §40.

A `cell` report also gives the busiest 10 s of the bulk carrier at each node, measured at every
frame a node hears (the occupancy line above it samples every ten minutes); how many parts the
bulk carrier joins the nodes into, and how many others each node hears both ways; and the
repairs: NACKs sent, full passes a NACK moved on, repair phases shared with a holder's, and
checks that found a NACK due but no phase left. `failover` also gives the longest stretch with
more than one announcer after the loss. A `cell` report also counts the bulk receptions of a
symbol the receiver lacks, of an object it wants ("wanted bulk"), by sender (its own announcer or
another node) and outcome, and for those from its own announcer what broke them: another
announcer or a node that is not announcing, heard by the sender or not (`docs/FEASIBILITY.md`
§38), and for those from others whether what broke them was heard by their sender. It counts
the windows in which each node's EtherFatsoen gate closed with its smoothed occupancy above
30 % or 50 % of all energy or above 30 % foreign, and the highest it reached, and reports them
for the typical and the worst node (`docs/FEASIBILITY.md` §39).

`MESHCAST_CARDS=small` gives every channel's root a card (`docs/PROTOCOL.md` §2) of 10 bytes: a
medium, one genre and one language; `MESHCAST_CARDS=full` one of three genres, three languages, an
area and a line of 60 bytes, which takes a root to a second symbol (`docs/FEASIBILITY.md` §34).

`--strip-m W` places the nodes over a strip W metres wide instead of a square of the same area,
with the stations spread along it: villages along a road or a valley (`docs/FEASIBILITY.md` §32).
`dynamics` then also reports, with `MESHCAST_TRACE_SPREAD=1`, each hour for each publication in
its period how far it has spread: its source, the span its holders cover, how many nodes want it
and which followers hold it, all in kilometres along the strip; and, to tell how cells are joined,
every node's position (`POS`), every pair of nodes that hear each other on the bulk carrier
(`LINK`) at the start, and each hour every node's role and announcer (`ROLES`).

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
- **Clocks**: by default every node reads the simulator's time. `--clock-epoch-s 3600 --clock-ppm 20`
  gives each node a clock that starts at a random time up to an hour and runs up to 20 ppm fast or
  slow, and restarts from zero when the node is switched on again; `--clock-same` starts them all
  at one random time. Each node then keeps its own shared time (PROTOCOL.md §6). A receiver loses
  a frame that runs past the end of its hop dwell ("retuned" in the report).

Not modelled yet: terrain, buildings, antenna patterns, GFSK versus LoRa co-channel interference
across carriers, packet error rate below the sensitivity cliff, multiple bulk
carriers per node, the few hundred microseconds a radio takes to change between LoRa and GFSK.
A node whose bulk carrier is GFSK or LoRa hears its control carrier only in the control window
and its bulk carrier only outside it, as one SX1262 would (PROTOCOL.md §3); an ESP-NOW or IP
bulk carrier is another radio. Frames that reach a node tuned to its other carrier count as
"not listening" in the report.

## Output

The report prints per-object completion times over followers (p50, p90, max), collisions,
announcers at the end, occupancy percentiles on the bulk channel, airtime share per transmitting
node, and per-node core counters (frames by class, CCA deferrals, discipline waits, NACKs, WANTs,
uploads, uploads ended because their announcer listed the object as held, and the time uploads
waited out phases other announcers gave away). Results and their interpretation are in
`docs/FEASIBILITY.md` §7.

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
prints how many manifest corrections followers sent to their announcer (`docs/PROTOCOL.md` §2),
how often followers asked their announcer for proof, and how often they left one: never one
symbol, no answer for what it listed, or no uploader named for what it lacks (§5.2).

A source publishes its pieces as collections (`docs/PROTOCOL.md` §2): one series by default.
`--collections N` splits them in order over N albums, `--follow-collections K` makes each follower
follow K of a source's collections, chosen at random, instead of the whole channel, and
`--cover-kb N` gives every collection a cover of N kB. Covers are reported as their own kind and
left out of whole content. An ensemble also reports what a follower holds at the end, in kB, as
`held_kb_per_follower`. `dynamics` keeps one series per channel and publishes each bulletin as a
new episode of it.

Ensembles and `dynamics` also report `table peaks`: the largest size each table a node fills from
what it hears (neighbours, ids they offered, asks recorded, store entries and so on) reached in
any honest node, sampled with the occupancy. They are what the caps of `docs/PROTOCOL.md` §8 were
chosen against (`docs/ABUSE.md`, item 5).

`dynamics` also reports a `bulk carrier` line: the parts of the network the bulk carrier joins
(nodes that hear each other both ways), and the share of follower-publication pairs whose follower
is in its source's part, the most any protocol could deliver without moving a node; and a `menu of
followers` line: how often, hourly, a follower held the newest root of a channel it follows.

`dynamics` also reports a `carrying` line: frames sent in all, bulk frames, what a node holds at
the end (mean and most, in kB), relay wants taken on, objects that gave way to the carry budget,
relays declined because the budget was full, relays handed on to the cell that asked, and relays
withdrawn because that cell held the object (`docs/PROTOCOL.md` §4). And, to see where frames and
relays go (`docs/FEASIBILITY.md` §28):

- `bulk frames`: sent by announcers (their carousels' first passes and repeats) and by others
  (uploads to their announcer).
- `relayed objects`: how many objects were relayed, by how many distinct nodes each, and relays
  per object.
- `relay life`: the share of other cells' asks of each age, doubling from one minute, that someone
  else met before they were twice as old, as the nodes learned it, averaged over nodes by how
  many asks reached each age.
- `listened asks`: every ask an announcer made for its listeners, with how many came from a cell
  where nobody listened to the object when it was first asked, how soon they were met, after how
  many open asks they were granted, and how many were granted to another cell's node.
- `evicted relayed`, `evicted announcer's`, `evicted menu` and `evictions`: what gave way to the
  carry budget, by kind and by what the node knew of it then (neighbours holding it, an announcer
  listing it, in or out of its collection's window, idle time), each with the share the node held
  again later.
- `control carrier`: every hour, the pairs of announcers that hear each other on the control
  carrier but not on the bulk carrier, how many of them share none of their control windows in the
  coming hour, and how many fewer than nine in ten (`docs/PROTOCOL.md` §3, §6).
- `menu`: every hour, the share of channels whose newest root a node holds, over nodes and over
  announcers, and how many newest roots a node holds of how many channels exist.
- `bulk frames by what they carry`: roots, collection manifests and pieces, sent by announcers and
  by others.

## Looking inside one world

`--seeds N` runs N worlds and reports their spread; compare designs on that, not on one run
(`docs/FEASIBILITY.md` §9.1). To look inside one of them:

- `MESHCAST_ONLY_SEED=6` with `--seed 1 --seeds 8` runs only the sixth world of that ensemble
  and prints its full report. For scenarios that generate their own positions (`clusters`) this
  is not the same world as `--seed 6`: the ensemble keeps the positions of its first seed.
- `MESHCAST_TRACE_GRANTS=1` writes one line per event to stderr: `GA` open ask, `GT` grant, `GH`
  grant heard by its holder, `OF` offer, `UP` upload frame sent, `UO` upload frame received by
  its announcer, `UC` upload frame collided there (with the other frame and its sender, and the
  phase count each uploader believed against the announcer's; the line does not name the
  object), `UH` upload frame lost because its announcer was sending, `BO` other symbol of an
  object an announcer wants, `MA` manifest announcement, `OC` object completed, `RW` another cell's
  ask taken on as a relay, `OX` an offer
  reaching (`ok`) or not reaching (`col` collided, with the interferer; `hd` the announcer was
  sending) an announcer that wants what it offers, and at the end
  `MISSING` for every object a follower lacks (with the announcers it hears and who listed the
  object, to tell whether an excursion or a named repair was possible) and `NOLEN` for every
  want whose length the wanter does not know.
- `MESHCAST_DEBUG_WANTS=1` traces each announcer's want list; `MESHCAST_DEBUG_NODE=4` adds node
  index 4's.
- `MESHCAST_TRACE_ROLES=1` prints every role change as `ROLE t node carrier role announcer`, and
  every node switched off or on (`ALIVE t node bool`) or replaced by a newcomer (`NEWCOMER t node`).
- `MESHCAST_TRACE_PAIRS=1` prints, every ten minutes, every pair of announcers on a bulk carrier:
  `PAIR t carrier a b score_a score_b rx_ab rx_ba a_heard_b a_median b_heard_a b_median
  followers_a share_of_a_hearing_b followers_b share_of_b_hearing_a hear= shun=`, the same-cell
  judgement of the tie-break and the share of each one's followers that hear the other
  (`docs/FEASIBILITY.md` §37.1); an announcer without followers shares nothing (0).
  `MESHCAST_TRACE_NB=1` prints every node's neighbour count then.
- `MESHCAST_TRACE_BEACONS=1` prints every beacon with its score, channel and colour, and every
  node that receives it above sensitivity with its own score; an `x` after a node means it was
  tuned to another channel and missed it, a `d` that it was listening on its other carrier.
- `MESHCAST_TRACE_ANNOUNCE=1` prints every manifest announcement and who heard it.
- `MESHCAST_TRACE_TIME=1` prints, every five minutes of a `cell` run (`MESHCAST_TRACE_TIME_STEP_S`
  to change that), how far apart the nodes' shared times are, how many keep within a second of the
  median, the steps taken, role changes, announcers, nodes without a time and excursions, and at
  the end the stations' roles; `=2` also lists the nodes more than 10 ms off.
- `MESHCAST_T_GUARD_MS` and `MESHCAST_T_ACQUIRE_S` override `T_guard` and `T_acquire` (0 turns
  acquisition off). `MESHCAST_TELL=0` keeps announcers from telling their time on the control
  carrier, and `MESHCAST_WINDOW_WANDERS=0` puts the control window in the middle of every period,
  as before `docs/FEASIBILITY.md` §29.
- `MESHCAST_T_EXCURSION_MIN=40` overrides `T_excursion` for an experiment, and
  `MESHCAST_BACKOFF_MAX_ATTEMPT=4` the number of times the random backoff after a busy channel
  doubles. `MESHCAST_CTRL_WINDOW_MS=4000 MESHCAST_CTRL_PERIOD_MS=60000` set the
  control window (a window of 0 turns it off: a node then hears both carriers at once), and
  `MESHCAST_NO_CTRL_RX=1` makes nobody receive on a separate control carrier at all.
- `MESHCAST_CELL_MENU=0` makes an announcer serve every channel it hears of, and
  `MESHCAST_ANNOUNCE_RECENT=0` makes it announce in channel order only, as before
  (`docs/FEASIBILITY.md` §30). `MESHCAST_TRACE_LATE=1` prints, in `dynamics`, each follower that
  did not hold a publication within its period: when it was published, what it was, whether the
  follower was on, and its announcer at the end.
- `MESHCAST_LEAVE_BACKOFF=0` makes a follower wait as long for an object under every announcer, as
  before `docs/FEASIBILITY.md` §31; `MESHCAST_LEAVE_LACKING=0` makes it leave only an announcer that lists
  what it wants and does not send it. `MESHCAST_TRACE_LEAVE=1` prints a `LEAVE` line each time a
  follower leaves its announcer for what it does not get: the object, whether the announcer holds
  or wants it, whether the follower only relays it, how many nodes know it, how many bridges there
  are (nodes that hear, and are heard by, both the announcer and a holder) and how many of them
  know it, and every holder with its role, its announcer, whether the announcer hears it, and how
  strongly the follower does.
- `MESHCAST_MARK_RELAYED=0` makes an announcer leave what it relays out of its asks for listeners,
  as before `docs/FEASIBILITY.md` §33.
- `MESHCAST_CARRY_BUDGET_KB=256` gives every node a carry budget, in kB (0: no relays);
  `MESHCAST_T_RELAY_WAIT_S` overrides `T_relay_wait`. `MESHCAST_PROACTIVE=1` makes announcers fetch
  every piece of every channel they hear of, and `MESHCAST_RELAY_ANY=0` makes nodes relay only for
  channels they follow and keep no menu of the others, as before `docs/FEASIBILITY.md` §27.
  `MESHCAST_RELAY_LEARN=100` sets `relay_risk` in permille, and `=0` makes nodes relay after a fixed
  `T_relay_wait` instead; `MESHCAST_RELAY_WITHDRAW=0` keeps relays until `want_ttl` after the last
  ask; `MESHCAST_EVICT_EVIDENCE=0` lets the least recently used give way and counts a menu manifest
  as used when it arrives, as before §28. `MESHCAST_STATIONS_UNBUDGETED=1` exempts the stations
  from the carry budget, as a device with room to spare would be. With `MESHCAST_TRACE_GRANTS=1`,
  `dynamics` also prints `EV` for every object that gives way, with whether it was a relay or the
  menu and the node's role.
- `MESHCAST_TRACE_BUSY=0` counts, for node index 0, which transmitter kept its channel busy each
  time it wanted to send, with the first and last minute; the cell report prints it as
  `busy blame`. It found an announcer whose carousel chained frames without a gap
  (`docs/FEASIBILITY.md` §14).
- `MESHCAST_TRACE_RX=38` prints every symbol that reaches node index 38 above sensitivity (`RX`),
  with what it held of the object before, and `lost=half-duplex` or `lost=collision with N` when
  the frame did not arrive intact. Silence in it means the node heard nothing at all, not that it
  was off.
- `MESHCAST_TRACE_WANTS_AT_ROLE=38` prints node index 38's want list (progress, grant, timers)
  at each of its role changes, what it waited for when it left an announcer, and with
  `MESHCAST_TRACE_GRANTS=1` at each ask it sends as an announcer (`WANTS-AT-ASK`).
- `MESHCAST_TRACE_NEWCOMERS=1` prints, in `dynamics`, each newcomer's node, when it joined, how
  long it took to hold everything that existed then, and the object that came last. The report
  gives that time twice: as it was, and counting only the time the newcomer was switched on.
- In tests, `Engine::lose_symbol(node, object, esi)` loses one chosen symbol at one node once: a
  fault on purpose, to test a repair.
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
