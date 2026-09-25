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
| 0x1 | `BEACON` | announcer only | control (LoRa) and bulk carriers | heartbeat, election, time, spectrum weather |
| 0x2 | `BULK` | announcer, or a source uploading to the announcer | bulk carriers | one symbol of one object |
| 0x3 | `GOSSIP` | sources and the announcer | control | HAVE / WANT summaries |
| 0x4 | `MANIFEST_ANNOUNCE` | sources and the announcer | control | newest manifest id per channel |
| 0x5 | `NACK` | a follower that is missing few symbols of an object | control, rare | compact repair request (v0 repair mechanism) |

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
FOLLOWER  ── no BEACON for N_miss expected intervals ──▶ CANDIDATE
CANDIDATE ── timer expires, still no BEACON ──────────▶ ANNOUNCER
CANDIDATE ── hears BEACON ────────────────────────────▶ FOLLOWER
ANNOUNCER ── hears BEACON with higher score + hysteresis, or lower score wins tie-break ──▶ FOLLOWER
```

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
- **Hysteresis**: a returning former announcer (or any newcomer) with a better score takes over
  only if it exceeds the incumbent by `H`; this prevents flapping between near-equal nodes.
- **Partition**: if the cell splits, the far side elects its own announcer after
  `N_miss × T_beacon + T_wait`. Two cells exist. Nodes hearing both follow the stronger beacon and
  report the other's objects in HAVE; each announcer can WANT them, so content crosses the boundary.
  When the partition heals the tie-break merges the cells.
- **No state is lost.** The announcer holds no unique state: the carousel set is rebuilt from
  GOSSIP within a round, and every node's library persists locally. Content the old announcer had
  not yet delivered is exactly as undelivered as before.

### 5.3 What a listener experiences

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

## 8. Parameters (draft, to be tuned in simulation)

| Name | Draft | Meaning |
|---|---|---|
| `T` | 200 B | symbol size |
| `K_max` | 1024 | symbols per source block |
| `T_beacon` | 60 s | beacon interval on the control carrier |
| `N_miss` | 3 | missed beacons before election |
| `T_base`, `T_jitter` | 120 s, 20 s | candidate wait |
| `H` | 6553 (10 %) | takeover hysteresis |
| repair overhead (v1) | 10 % | RaptorQ repair symbols per block |
| NACK threshold | 80 % complete, 2 rounds | when a follower may ask |
| gossip cap | 12 have + 12 want | per frame |

## 9. Open questions

1. Symbol size versus LoRa airtime: 200 B is right for GFSK; should LoRa-only cells use 64 B?
2. Should the carousel prioritise by schedule proximity (what plays soonest) over rarest-first?
3. Per-symbol authentication in v0 rather than v1, given that anyone can inject BULK frames?
4. Multi-announcer cells on purpose (two bulk channels, two announcers) in dense areas?
5. How does a node learn a channel id in the first place without internet? (QR code, spoken
   over the mesh in a "directory" channel that every node follows by default, or both.)
6. Time without GPS or phone in a fully offline mesh: does mesh-derived time drift acceptably?
