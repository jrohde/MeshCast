# MeshCast Protocol (draft v0)

Status: **draft, expect changes**. Every constant in this document is a starting value to be tuned
in the Phase 0 simulator. Byte layouts are proposals; the reference is the `core` crate once it
exists, and this document must be updated with it.

## 0. Design constraints

- Frames are broadcast. There are no connections, sessions, routes or per-frame acknowledgements.
- Receivers never transmit. Only sources (nodes with new content) and the elected announcer speak.
- All content traffic is delay-tolerant. Control traffic is small and rare.
- A frame must fit one SX126x packet (255 bytes) and one classic ESP-NOW frame (250 bytes).
- Untrusted input: every frame parser is total (no panics), bounds-checked, and fuzzed.
- Region-agnostic: nothing below depends on band, power or duty cycle.

## 1. Objects

An **object** is an immutable byte string with a small header: MIME type, length, optional
title, optional encryption flag. Tracks, bulletins, manifests, firmware images and text pages are
all objects.

- **Object id**: BLAKE3 hash of the object bytes (32 bytes). On the air a **short id** of the first
  8 bytes is used; the manifest carries the full hash. A short-id collision among a few million
  objects is negligible; a node that detects one (two full hashes with the same prefix) keeps both
  and disambiguates by manifest.
- **Symbols**: the object is split into symbols of `T` bytes (draft `T = 200`). The last symbol
  is zero-padded; the true length is in the header. `K = ceil(len / T)` source symbols.
- **Source blocks**: objects larger than `K_max` symbols (draft 1024, so 200 kB) are split into
  consecutive source blocks; each block is independently repairable. A 3-minute Opus track at
  24 kbit/s is 540 kB, so 3 blocks.
- **Integrity**: v0 verifies the full object hash on completion and discards the object on
  mismatch. v1 option: a Merkle root over symbol hashes in the manifest so a poisoned symbol can be
  rejected on arrival (costs 4 bytes per symbol in the manifest).

## 2. Channels and manifests

A **channel** is an Ed25519 public key. Its **channel id** is the first 8 bytes of BLAKE3(pubkey).

A **manifest** is an object of MIME `application/meshcast-manifest` containing, CBOR-encoded:

| Field | Type | Meaning |
|---|---|---|
| `chan` | 32 B | channel public key |
| `seq` | u32 | monotonically increasing; a node keeps only the highest valid seq per channel |
| `title`, `desc` | text | channel metadata |
| `objects` | list of {`id` 32 B, `len` u32, `mime`, `title`, `blocks` u16, `enc` bool} | the channel's catalogue (or a window of it) |
| `schedule` | list of {`id` 8 B, `start` u64 UTC seconds, `repeat` optional} | when to play what |
| `prev` | 32 B optional | id of the previous manifest, for history |
| `sig` | 64 B | Ed25519 signature over everything above |

Rules:
- A manifest is valid only if the signature verifies against `chan`. Invalid manifests are dropped.
- **Subscribing** is storing a channel id in the follow list. The node then wants that channel's
  manifests and the objects they reference.
- **Encrypted channels**: object payloads are encrypted with XChaCha20-Poly1305 under a key derived
  from the channel secret and the object id; titles in the manifest may be encrypted too. The
  channel secret is shared out of band (QR code from the phone app). Non-subscribers can still
  relay the objects, which is intended: relaying costs them nothing and helps subscribers.
- Manifests are ordinary objects: they travel through the same carousel and gossip as tracks.
  The only special-casing is that a `MANIFEST_ANNOUNCE` control frame names the newest manifest id
  per channel so followers know what to want.

## 3. Frames

All frames share a 2-byte prefix and end with a CRC-16/CCITT over everything before it. Byte
order is little-endian.

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | `ver:type` — high nibble protocol version (0), low nibble frame type |
| 1 | 1 | `flags` — type-specific |

Frame types:

| Type | Name | Who sends | Carrier | Purpose |
|---|---|---|---|---|
| 0x1 | `BEACON` | announcer only | the bulk carrier it announces on | heartbeat, election, time, spectrum weather |
| 0x2 | `BULK` | announcer, or a source uploading to the announcer | bulk carriers | one symbol of one object |
| 0x3 | `GOSSIP` | sources, the announcer, and followers with unserved wants | bulk carrier | HAVE / WANT summaries |
| 0x4 | `MANIFEST_ANNOUNCE` | sources and the announcer | bulk carrier **and** the long-range control carrier | newest manifest id per channel; the control-carrier copy is discovery for other cells |
| 0x5 | `NACK` | any node that is nearly complete on an object and sees no progress | bulk carrier, rare | compact repair request, answered by the carousel or by the uploading source |

**A cell is what hears each other on the bulk carrier.** The Phase 0 simulator showed that
running the election over a long-range control carrier elects announcers that most of their
"followers" cannot receive content from. So everything cell-local (beacons, election, gossip,
NACK) travels on the bulk carrier, where a control frame costs milliseconds; the LoRa control
carrier only carries `MANIFEST_ANNOUNCE`, so that neighbouring cells learn which channels exist
and fetch them through bridge nodes.

### 3.1 `BEACON`

| Offset | Size | Field |
|---|---|---|
| 2 | 4 | `announcer_id` (first 4 bytes of the node's public key hash) |
| 6 | 2 | `score` (see §5) |
| 8 | 2 | `next_ms` — milliseconds until the next beacon from this announcer |
| 10 | 2 | `epoch` — carousel epoch, increments when the carousel set changes |
| 12 | 8 | `utc` — UTC seconds if known, else 0 |
| 20 | 1 | `time_quality` — 0 none, 1 mesh-derived, 2 phone/NTP, 3 GPS |
| 21 | 1 | `channel_map` — which bulk channels of the profile this announcer uses |
| 22 | 4 | `occupancy` — measured channel occupancy per bulk channel, 4 × u8 percent ("spectrum weather") |
| 26 | 2 | CRC |

28 bytes. Sent on the control carrier at `T_beacon` (draft 60 s) and also embedded as the first
frame of every carousel round on the bulk carrier.

### 3.2 `BULK`

| Offset | Size | Field |
|---|---|---|
| 2 | 8 | `object_short_id` |
| 10 | 2 | `block` — source block index |
| 12 | 2 | `esi` — encoding symbol id; `< K` is a source symbol, `>= K` a repair symbol (v1) |
| 14 | 2 | `k` — K for this block, so a receiver can allocate without the manifest |
| 16 | T | payload |
| 16+T | 2 | CRC |

With `T = 200`: 218 bytes. Fits SX126x and ESP-NOW.

### 3.3 `GOSSIP`

| Offset | Size | Field |
|---|---|---|
| 2 | 4 | `node_id` |
| 6 | 4 | `announcer_id` — the announcer this node currently follows (0 if none heard) |
| 10 | 1 | `n_have`, complete objects listed |
| 11 | 1 | `n_want` |
| 12 | 8 × n_have | short ids the node has completely |
| … | 8 × n_want | short ids the node wants (from followed manifests) |
| … | 2 | CRC |

Draft cap: 12 have + 12 want per frame, 206 bytes. For larger libraries a node rotates through
its list across gossip rounds, most recently completed and most wanted first. Followers that are
not sources send GOSSIP only when they have something new to offer that the announcer lacks (the
"upload" case) or, rarely, a WANT for an object the announcer has never included; the default is
silence.

### 3.4 `MANIFEST_ANNOUNCE`

`channel_id` (8) + `manifest_short_id` (8) + `seq` (4), repeated up to 12 times, + CRC.

### 3.5 `NACK` (v0 repair)

`object_short_id` (8) + `block` (2) + a bitmap of missing source symbols, run-length encoded,
max 200 bytes, + CRC. Sent by a follower only when the object is at least 80 % complete and the
carousel has not offered the missing symbols for two rounds. The announcer merges NACKs into its
next round. This is the whole repair mechanism in v0; it costs one small control frame per object
per follower at most, which is negligible next to the object itself.

v1 replaces most NACKs with RaptorQ repair symbols (`esi >= K`) generated by the announcer at a
configurable overhead (draft 10 %), so that receivers that missed any `≤ 10 %` of a block recover
without transmitting anything.

## 4. Carousel

The announcer maintains a **carousel set**: every object (including manifests) that a follower in
the cell wants, as learned from GOSSIP and MANIFEST_ANNOUNCE, that the announcer has. A round is:

1. `BEACON` on the bulk carrier.
2. For each object in the set, ordered rarest-first (fewest HAVEs among heard nodes) with new
   manifests first: emit its symbols, one `BULK` frame each, subject to EtherFatsoen and
   EtherDiscipline gating between frames.
3. Merge NACKs received during the round; symbols named in NACKs are queued at the front of the
   next round.
4. Objects that every heard follower reports complete leave the set.

The round never waits for anyone. A follower that joins mid-round starts collecting and completes
the object next round. Symbols are idempotent, so a slow announcer simply takes more rounds.

**Passes and idleness.** Followers never report HAVE, so the announcer cannot know when everyone
is done. Instead each object gets `max_passes` (draft 3) full passes and then leaves the carousel
unless a new WANT arrives after its last pass. Manifests are repeated at most every `T_always`
(draft 5 min) when nothing else is wanted. A carousel with nothing to send is silent; the
announcer then only beacons. (The first simulator runs looped manifests forever at the full duty
cycle, which wasted the budget and caused half-duplex losses during uploads.)

**Repair.** Any node, follower or announcer, that holds at least 80 % of an object and has seen no
new symbol for `T_nack_stall` (draft 60 s) sends one NACK listing the missing symbols. The
announcer's carousel answers from its front queue; a holder whose upload the announcer is missing
answers with exactly those symbols. Stall detection is time-based, not round-based, so it also
works when the carousel is idle.

**A repair is an ask.** A node can reach 95 % of an object by overhearing a neighbouring cell's
carousel and then want the rest, although nobody was ever granted to it. So a NACK is answered by
the granted uploader at once, and by any other holder after a wait: the wait grows with how many
of its own neighbours the holder hears better than the asker, so **whoever hears the asker best
answers first**, and hearing anyone send those symbols cancels an answer that has not begun.
No absolute signal level enters into it; the rank is relative to the holder's own neighbourhood.

**Who uploads: ask, offer, grant, send.** An announcer's WANT entry is either an *open ask*
(`grant = NONE`) or a *grant* naming one uploader. Any node that holds the object, in the
announcer's own cell or a neighbouring one, answers an open ask with an *offer*: one small GOSSIP
carrying HAVE, after a random delay of up to `T_offer`, and not at all if it hears another
holder's offer first. The announcer grants the first offer it hears and names that holder in its
next WANT; only the named holder uploads, one object at a time (further grants queue), on the
announcer's channel and in its slot. A grant lapses after `T_grant` without a symbol arriving,
and the ask becomes open again. NACKs from an announcer are answered only by its granted
uploader. This is the DHCP pattern, and it replaced "any holder answers after a random wait",
which the simulator showed producing fourteen uploads per object per cell among holders that
could not hear each other's suppression.

**Ask only for what is not coming.** An announcer's WANT lists objects that have received no
symbol for `T_nack_stall`; an object whose symbols are arriving is not asked for again, and a
holder whose granted upload is flowing is not asked for a second object until it is done.
Several holders may upload different objects to one announcer at the same time: each spends its
own regulatory budget, and serialising them (tried in Phase 0) halves the cell's inbound rate.
Holders on opposite sides of a cell that cannot hear each other's CCA are the known residual
source of upload collisions (FEASIBILITY.md §7.5.0, rounds 9–10). An
object that is at least 80 % complete is never re-asked in full: it is repaired by NACK, which
the granted uploader answers for as long as the announcer keeps asking. On frequency-agile
carriers an announcer's NACKs, like its gossip, go out in the meeting dwell, because its
uploader may live in another cell on another sequence.

**You carry what you listen to.** A node registers, collects and keeps the objects of the
channels it follows (and, as announcer, of every channel it serves). Objects that no manifest of
interest references any more, because the channel was unfollowed or the object left the
channel's window, are evicted; own objects are kept. Content crosses cells through nodes that
follow the channel, never through bystanders.

**Fresh before repeated.** The first copy of an object into a cell (an upload, or the carousel's
first pass) is worth more than its second and third pass. Fresh content is paced at the full
budget and admitted like metadata; repeated passes take the throttled rate and yield first. (The
simulator found five carousels on one channel throttling every transmitter to the floor,
including the sources uploading new tracks, so that a channel's last tracks never entered the
mesh.)

**Upload**: a source that has an object the announcer lacks sends GOSSIP with HAVE. The announcer
replies with GOSSIP WANT. The source then transmits the object's symbols as `BULK` frames under
the same gating; everyone in range collects them, not just the announcer. When the announcer
reports HAVE, the source stops. A source that hears no announcer for `T_silence` (see §5) may
become the announcer itself.

## 5. Announcer election and healing

Every node runs this state machine on every carrier independently (a node may be announcer on
ESP-NOW and follower on sub-GHz).

### 5.1 Score

`score` is a u16 computed locally, draft weights:

| Term | Weight | Source |
|---|---|---|
| distinct `node_id`s heard in the last hour | 4 per node, capped at 64 nodes | GOSSIP / BEACON reception |
| mains powered | +64 | hardware |
| fraction of regulatory airtime budget unused | 0–64 | EtherDiscipline accounting |
| objects in the carousel set the node can serve | 1 per object, capped at 32 | library |
| has IP uplink | +16 | discovery |

A station on a roof with an SX1302, mains and internet scores near the maximum; a battery dongle
in a drawer scores low.

### 5.2 States

```
FOLLOWER  ── no BEACON for N_miss expected intervals, another announcer audible ──▶ FOLLOWER of that one
FOLLOWER  ── no BEACON for N_miss expected intervals, nobody audible ─────────────▶ CANDIDATE
FOLLOWER  ── own score clearly better than the announcer's for `challenge_beacons` beacons ──▶ CANDIDATE (short wait)
CANDIDATE ── timer expires, still no BEACON ──────────▶ ANNOUNCER
CANDIDATE ── hears BEACON ────────────────────────────▶ FOLLOWER
ANNOUNCER ── hears BEACON with clearly higher score ───▶ FOLLOWER
ANNOUNCER ── hears BEACON with similar score and lower id, strong signal or nobody else heard ──▶ FOLLOWER
```

**Following is by signal, stepping up is by score.** A follower needs to *receive* its
announcer's carousel, so it follows the announcer it hears best (RSSI, averaged) and switches only
for one at least `rssi_hysteresis` (draft 6 dB) stronger. Scores decide who steps up when nobody
is heard, who yields when two announcers meet, and when a much better node challenges the
incumbent.

**Two announcers may coexist.** Overlapping cells in one band are normal: two announcers that
hear each other weakly usually serve different followers, and if the far one yielded, its
followers would be orphaned and re-elect, which the simulator showed as thousands of role changes
per day. So on a near-tie an announcer yields to the lower id only if the other is *in its own
cell*, judged relatively: it hears the other announcer at least as well as its typical (median)
neighbour. No absolute signal threshold, so the rule holds on every carrier; a node that has
heard nobody else treats any peer as near, since there is nobody to orphan. Otherwise both
persist and EtherFatsoen shares the channel between them.

- **N_miss** (draft 3): consecutive expected beacons missed (using the announcer's own `next_ms`).
  With `T_beacon = 60 s` that is about three minutes of silence before anyone acts. Nothing is
  urgent, so this is deliberately slow; the simulator will tune it.
- **Candidate timer**: `T_wait = T_base × (1 − score / 65535) + jitter(0, T_jitter)`, draft
  `T_base = 120 s`, `T_jitter = 20 s`. The best-scoring node waits the shortest time. While waiting
  the node listens; any BEACON returns it to FOLLOWER.
- **Tie-break**: if an ANNOUNCER hears another BEACON, it compares scores. It yields if the other
  score exceeds its own by more than `H` (draft 10 % of the max) or if scores are within `H` and
  the other `announcer_id` is numerically lower. Two announcers that cannot hear each other but
  are both heard by a node in between are detected by that node's GOSSIP (`announcer_id` field
  differs from the announcer's own id); an announcer that sees GOSSIP naming a different announcer
  with a higher score yields. Convergence to one announcer per connected cell takes at most a few
  beacon intervals.
- **Hysteresis and challenge**: a returning former announcer (or any newcomer) whose score
  exceeds the incumbent's by more than `H` for `challenge_beacons` consecutive beacons steps up
  after a short random wait; the incumbent hears the better beacon and yields. Near-equal nodes
  never challenge, so there is no flapping. A rebooted node always starts as a follower.
- **Partition**: if the cell splits, the far side elects its own announcer after
  `N_miss × T_beacon + T_wait`. Two cells exist. Nodes hearing both follow the stronger beacon and
  report the other's objects in HAVE; each announcer can WANT them, so content crosses the boundary.
  When the partition heals the tie-break merges the cells.
- **No state is lost.** The announcer holds no unique state: the carousel set is rebuilt from
  GOSSIP within a round, and every node's library persists locally. Content the old announcer had
  not yet delivered is exactly as undelivered as before.

### 5.3 Frequency agility (polite-access bands)

On carriers with more than one channel (EU band L: 15 × 200 kHz) there are two hop sequences,
both pseudo-random (`channel = splitmix64(id, dwell_index) mod n`, dwell `T_dwell`, draft 20 s):

- the **content plane**: each announcer's own sequence, keyed by its id, on which its carousel
  runs and which its followers and uploaders compute for it;
- the **control plane**: one common sequence for everyone, keyed by a fixed id, active during
  every `meet_every`-th dwell (draft 1 in 5). Announcers beacon there, cell-wide gossip (WANT,
  HAVE, manifest announcements) is timed to it, and so every cell hears every other cell's needs
  and offers.

The announcer sends a beacon at every dwell start (2 ms, with a small random offset so that
announcers hidden from each other do not collide at the meeting dwell) on whichever sequence is
active, so a node with no announcer finds one by staying on one channel: any announcer visits it
about once per cycle, and the meeting dwell brings all of them to the same channel regardless.
The meeting dwell carries control frames only; carousels pause for it.

**Conflict colouring.** Two announcers whose carousels are both heard by some follower are *in
conflict*; they usually cannot hear each other. The follower is the only node that knows, so it
reports the announcers it hears besides its own, with the colour each announced, in its gossip:
once when it first hears a new one or sees one change colour (within `T_report_min`), and every
half `conflict_ttl` while the situation lasts. Each announcer keeps its conflict set and colours
itself greedily: the lowest colour not announced by any conflicting announcer with a lower id
(lower ids keep their colour, higher ids move). On a multi-channel carrier the content plane is
one shared base sequence shifted by the colour, so conflicting announcers are never on the same
channel; when there are more colours than channels, colour `c` also selects time slot
`c div n` of `T_slot`, and single-channel carriers are simply `n = 1`: every colour is a slot.
One mechanism, in frequency where possible and in time where necessary.

EtherDiscipline's per-200 kHz accounting is unchanged: a random sequence spends about `1/n` of
the airtime in each slice. (Sequences derived by a fixed offset per announcer never coincide and
made nodes unable to find each other; the simulator caught this.)

### 5.4 Timing jitter

Every control or metadata frame on a bulk carrier waits a random `0..T_jitter` (draft 500 ms)
before transmission, beacons excepted. CCA cannot see a transmitter at the edge of range (a few
dB above sensitivity), and without jitter two nodes whose timers are both aligned to dwell
boundaries collide every single time.

### 5.5 What a listener experiences

During the outage, playback of already-collected objects continues from the schedule. New content
arrives a few minutes later than it otherwise would. That is the entire user-visible effect.

## 6. Time and playback synchronisation

- Manifests schedule objects at UTC times. Nodes play an object at its scheduled time if they have
  it complete; otherwise they skip it (or play the previous complete object in the channel, a
  channel-level option).
- Time sources in order of trust: GPS, phone/NTP over BLE or IP, announcer BEACON `utc` with
  `time_quality`, and finally nothing (the node plays on demand only). A node adopts a BEACON's time
  only if its own quality is lower.
- Accuracy needed: seconds, not milliseconds. Two neighbours playing the same track one second
  apart is acceptable; one minute apart is not.

## 7. Security model (summary)

- Authenticity: manifests are signed by the channel key; a node never plays or schedules an
  object that is not referenced by a valid manifest of a followed channel.
- Integrity: full object hash; v1 per-symbol Merkle proof option.
- Confidentiality: per-channel symmetric encryption, opt-in.
- Availability: any node can jam any radio; the protocol offers no defence beyond frequency
  agility and store-and-forward. Denial by flooding bogus BULK frames wastes the attacker's
  airtime more than ours; EtherDiscipline caps our own, not theirs.
- Sybil announcers: a malicious node claiming a maximal score becomes announcer and could
  broadcast garbage. Mitigation: garbage never verifies against a manifest, so followers ignore it;
  a v1 option lets followers prefer announcers whose id appears in a followed manifest ("trusted
  stations").
- Requests are not authenticated, and answering them costs far more than sending them. The full
  inventory, with amplification factors and the rule it asks for, is in [ABUSE.md](ABUSE.md).

## 8. Parameters (draft, to be tuned in simulation)

| Name | Draft | Meaning |
|---|---|---|
| `T` | 200 B | symbol size |
| `K_max` | 1024 | symbols per source block |
| `T_beacon` | 60 s | beacon interval on the bulk carrier |
| `N_miss` | 3 | missed beacons before switching or election |
| `T_base`, election jitter | 120 s, 20 s | candidate wait, scaled down by score |
| `H` | 10 % of the maximum score | yield / challenge hysteresis |
| `challenge_beacons` | 3 | beacons with a clearly lower score before a follower steps up |
| `rssi_hysteresis` | 6 dB | a follower switches announcer only for a clearly stronger one |
| `near_rssi` | sensitivity + 17 dB | beacon strength that means "same cell" for the tie-break |
| `max_passes` | 3 | carousel passes per object unless re-wanted |
| `T_always` | 5 min | manifest repetition when idle |
| `T_nack_stall` | 60 s | no progress on an ≥ 80 % object before a NACK |
| `T_want_min` | 10 min | minimum interval between a follower's WANT frames |
| `T_gossip`, `T_gossip_min` | 5 min, 30 s | announcer/source gossip cadence and its floor |
| `control_reserve` | 10 % | share of the band budget kept free for control frames |
| own share | `min(regulatory, occ_high_own / (announcers heard + 1))` | content pacing ceiling; derived, not configured |
| `T_dwell` | 20 s | hop dwell on frequency-agile carriers |
| `meet_every` | 5 | every fifth dwell is on the common control-plane sequence |
| `T_offer` | 0–3 s | random delay before a holder offers on an open ask |
| repair wait | `T_suppress × (neighbours heard better than the asker) / (all neighbours)` + jitter | ungranted NACK answer |
| `T_grant` | 10 min | a grant without any symbol arriving lapses |
| `T_slot` | 10 s | time slot when announcers in conflict share a channel |
| `conflict_ttl`, `T_report_min` | 30 min, 60 s | conflict report lifetime and follower report rate limit |
| `T_jitter` (tx) | 0–500 ms | random delay before control/metadata frames |
| repair overhead (v1) | 10 % | RaptorQ repair symbols per block |
| gossip cap | 12 have + 12 want | per frame |

## 9. Open questions

1. Symbol size versus LoRa airtime: 200 B is right for GFSK; should LoRa-only cells use 64 B?
2. Should the carousel prioritise by schedule proximity (what plays soonest) over rarest-first?
3. Per-symbol authentication in v0 rather than v1, given that anyone can inject BULK frames?
4. Multi-announcer cells on purpose (two bulk channels, two announcers) in dense areas?
5. How does a node learn a channel id in the first place without internet? (QR code, spoken
   over the mesh in a "directory" channel that every node follows by default, or both.)
6. Time without GPS or phone in a fully offline mesh: does mesh-derived time drift acceptably?
7. *(resolved in Phase 0: any holder answers any announcer's WANT, with suppression; see §4.)*
8. Nodes with two bulk carriers (GFSK and ESP-NOW) run two elections; the simulator models one
   bulk carrier per node so far.
9. zsync-style delta transfer for updated objects (web bundles, firmware): the receiver compares
   the block lists of the old and new object and wants only the changed symbols.
10. Fountain coding would remove the 62 % of received symbols that are duplicates, but the
    experiment (FEASIBILITY.md §7.7.1) broke cells with several announcers. The first guess, that
    announcers fill up by overhearing and so can never complete, was measured and is wrong: on
    main 57 to 100 % of what an announcer receives arrives on request, and it holds every object.
    The branch mixes two changes, the fountain carousel and a partial-relay attempt that flooded
    the channel with duplicates; they are being measured apart before anything is concluded.
11. Cross-cell fetching among cells that cannot hear each other is the weakest part of the
    protocol, and the Phase 0 answer is mixed. Conflict colouring with granted uploads took band O
    at town scale from a 10.5-hour to a 7.3-hour median at the same complete delivery, and the
    ESP-NOW neighbourhood from 78 % to 86–91 %; but it took band L at town scale from 100 % in
    11.4 hours to 66 % in 24 hours, because random hopping over fifteen channels was already good
    and the grant round-trips at meeting-dwell cadence are not (FEASIBILITY.md §7.5). Serialised
    and turn-taking uploads were tried and rejected. Next: measure where the band L town path
    stalls (grant latency, coinciding upload channels, or one-object-per-holder), and find a rule
    that is cheap where channels are plentiful without being unsafe where they are scarce.
