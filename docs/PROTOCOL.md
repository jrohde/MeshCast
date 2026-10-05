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

An **object** is an immutable byte string described by its manifest entry: content type (§1.1),
length, optional title, optional encryption flag. Tracks, bulletins, manifests, firmware images and text pages are
all objects.

- **Object id**: BLAKE3 hash of the object bytes (32 bytes). On the air a **short id** of the first
  8 bytes is used; the manifest carries the full hash. A short-id collision among a few million
  objects is negligible; a node that detects one (two full hashes with the same prefix) keeps both
  and disambiguates by manifest.
- **Symbols**: the object is split into symbols of `T` bytes (draft `T = 200`). The last symbol
  is zero-padded; the true length is in the header. `K = ceil(len / T)` source symbols.
- **Source blocks**: objects larger than `K_max` symbols (draft 1024, so 200 kB) are split into
  consecutive source blocks; each block is independently repairable. A 3-minute music track
  (§1.1) is 42 kB, one block; the 540 kB objects the Phase 0 simulations used (3 minutes of
  24 kbit/s Opus) are 3 blocks.
- **Symbols before metadata**: a node can collect symbols of an object before it knows what the
  object is (an announcer overhearing a neighbouring carousel, or a manifest's symbols arriving
  before its announcement). It keeps their payloads, not just a count, and the object completes
  when its metadata arrives: once, with the same consequences as completing by a symbol (a
  manifest is adopted, the want is dropped). Only an object too large for the node to keep at
  all is tracked by count alone, as every large object is. An announcement (§2) is metadata
  enough: the kind it names is taken for an object whose kind is not yet known, and what was
  counted without its payload is fetched again if the node must read it. A root manifest needs
  none: its bytes name its channel and carry the channel's signature, so a node that completes
  an object nobody has named reads it as a root and, if it is one, adopts it (§2). An announcer
  that overheard a neighbouring cell's upload of a new root, and heard no announcement of it,
  otherwise held the root without reading it, and its cell stayed a seq behind
  (FEASIBILITY.md §28).
- **Integrity**: v0 verifies the full object hash on completion and discards the object on
  mismatch, so one wrong symbol per pass can keep an object from completing. v1 verifies an object
  in pieces as they arrive against its id, using the BLAKE3 tree the id already is the root of
  (1024-byte chunks); the tree travels as its own object, named in the `integrity` slot of the
  piece's entry (§2). A tag per symbol, as first drafted here at 4 bytes, would need at least
  64 bits. Requirement and open questions: ABUSE.md, "Someone else's firmware".

### 1.1 Content types and the two audio codecs

Every object has a one-byte **content type**, carried in the manifest entry that lists it
(§2). Frames never carry it: a BULK frame names an object by short id, and a node that wants the
object already has the manifest that says what it is.

| Code | Name | MIME | Content |
|---|---|---|---|
| 1 | manifest | `application/meshcast-manifest` | a channel's root manifest (§2) |
| 2 | text | `text/plain; charset=utf-8` | text pages |
| 3 | firmware | `application/octet-stream` | firmware images |
| 4 | renditions | `application/meshcast-renditions` | a channel's rendition table (§1.2) |
| 5 | collection | `application/meshcast-collection` | a collection manifest (§2) |
| 6 | image | `image/jpeg` | a collection's cover; JPEG because every device with a screen can decode it, microcontrollers included (for example ChaN's TJpgDec, elm-chan.org/fsw/tjpgd) |
| 16 | speech | `audio/x-snac; model=snac_24khz` | spoken programmes: news, talk, bulletins |
| 17 | music | `audio/x-snac; model=snac_32khz` | music |
| 18 | opus | `audio/ogg; codecs=opus` | a rendition (§1.2): audio for a device that cannot run the neural decoder, made on demand from a SNAC object and sent only where someone asks for it |
| 255 | other | | anything else; relayed, not interpreted |

Audio objects hold the discrete codes of a neural codec, not a waveform (FEASIBILITY.md §8 has
the measurements behind the choice). Speech and music use different models, each pinned to exact
weights:

| Code | Model | Weights | Bit rate | 3 min | 5 min |
|---|---|---|---|---|---|
| 16 speech | SNAC 24 kHz, 3 levels | `hubertsiuzdak/snac_24khz` at revision `d73ad17`, `pytorch_model.bin` SHA-256 `4b8164cc…9b4bff40` | 0.98 kbit/s | 22 kB | 37 kB |
| 17 music | SNAC 32 kHz, 4 levels | `hubertsiuzdak/snac_32khz` at revision `c84c6ac`, `pytorch_model.bin` SHA-256 `bfee2f05…1ea3ba65` | 1.88 kbit/s | 42 kB | 70 kB |

Rules:
- **A code is a contract and never changes meaning.** A retrained or different model gets a new
  code, so an object decodes the same way on every device for as long as it exists. The full
  hashes live in `core` (`audio.rs`) next to the codes.
- **The publisher chooses the kind**; nodes do not guess it. A spoken programme over a music bed
  is music.
- **Only the device that plays decodes**: the phone, or a station with a speaker. Relays and
  dongles carry the codes as opaque bytes and never transcode. A node that decodes may also
  make renditions for a device that cannot (§1.2). Decoding happens ahead of
  playback, as soon as an object completes, so a player does not need to decode in real time.
- **Encoding happens once, at the source**: on the phone that records a bulletin or on the
  station that ingests a track.
- The model weights (77 MB and 26 MB as fp16) ship with the player software and are not sent
  over the mesh. A player that cannot verify the SHA-256 above does not play the object.

**Payload layout.** No header. The payload is a bitstream of 12-bit codes (each codebook has
4096 entries); code *i* occupies bits 12*i* to 12*i* + 11, and bit *b* of the stream is bit
*b* mod 8 of byte ⌊*b*/8⌋. Codes are grouped by the span of the coarsest level:

| Code | Group | Codes per group, coarse level first | Bits | Duration |
|---|---|---|---|---|
| 16 speech | 4 finest frames | 1 + 2 + 4 = 7 | 84 | 85.3 ms (4 × 512 samples at 24 kHz) |
| 17 music | 8 finest frames | 1 + 2 + 4 + 8 = 15 | 180 | 96 ms (8 × 384 samples at 32 kHz) |

Within a group each level's codes are in time order. The number of groups is
⌊8 × `len` / bits per group⌋; leftover bits are zero. Because groups follow each other in time,
any prefix of an object decodes to a prefix of the audio, which a later live mode can use.

### 1.2 Renditions: sound for the last hop

Codes travel through the mesh; sound travels only the last hop, and only where someone asks for
it.

A device that cannot run the neural decoder can still play a common codec. The LilyGo T-Deck Pro
has an ESP32-S3 with 16 MB flash and 8 MB PSRAM, an SX1262 and, in one variant, a PCM512A audio
module (<https://github.com/Xinyuan-LilyGO/T-Deck-Pro>); the music decoder needs 18.3 G
multiply-adds per audio second (FEASIBILITY.md §8.2). Such a device asks for a *rendition*: the
programme as Opus, made from the codes by a node that can decode them.

- **A rendition is an ordinary object.** It has an id (the hash of its bytes), a length and
  content type 18 (Opus). Its bytes are a pure function of the codes: a *profile* fixes the
  decoder (a reference implementation of the model the content type names, with defined integer
  arithmetic), the resampling and the encoder (a pinned Opus build and its settings).
- **The source signs it.** The source runs every profile when it publishes and lists the
  renditions in a *rendition table*: an object of content type 4 holding, per audio object, its
  short id, the profile and the rendition's full id and length. The manifest names the table by
  id and length (36 bytes, whatever the window holds), so the table is signed through the
  manifest and the renditions through the table. Only the nodes that need renditions fetch the
  table: devices that cannot decode, nodes that make renditions, and announcers that serve them.
- **Nobody sends a rendition until someone asks.** A device that cannot decode wants the
  rendition instead of the codes, and only shortly before it plays it: `T_render_ahead` (draft
  30 min) before its slot in the schedule (§6), or when its user picks it. A radio needs what is
  on next, not the whole window.
- **Any node that holds the codes and can run the profile can offer the rendition.** It answers
  an ask with an offer as if it held it, makes it only when it is granted the upload, so that one
  node and not every capable one spends the work, and sends it only if it matches the id in the
  table. A platform that does not reproduce the profile bit for bit therefore cannot serve
  renditions, but can never serve a wrong one: correctness never depends on determinism, only
  availability does. Stations and phones make renditions (a dongle uploads what its phone made);
  dongles never decode.
- **An announcer serves a rendition like any object**, while a follower wants it, and never
  wants one for itself. It serves a rendition of a scheduled programme only from twice
  `T_render_ahead` before its slot (to allow for clocks) until the programme has played: an ask
  outside that window is not a listener's, and serving it is what a rendition flood asks for. If it can make it, it does; otherwise it asks, in its own cell and in the
  rendezvous, and a node that can make it offers and uploads it. Announcers do not upload, so a
  station that announces makes renditions for its own cell only; a cell whose announcer cannot
  make them relies on a follower that can, or on a neighbouring cell's.
- **Verification is the object hash.** The device checks the rendition against the id in the
  table with BLAKE3, the check it applies to every object. No pairing, no second key, no QR code.
  A rendition of an encrypted channel is encrypted like its objects (§2), so a node can make it
  only if it holds the channel key; the device then needs the key too.
- **A rendition costs what listening costs.** A device plays in real time, so a rendition on
  the sub-GHz cell costs its bit rate for as long as someone listens, once per cell however many
  devices listen there (FEASIBILITY.md §10). Where the device has a carrier with more room, such
  as ESP-NOW on an ESP32-S3 near a station (TRANSPORTS.md), the rendition should take it; the
  object is the same.

Profiles (draft; bit rates to be set by a listening test on the target device):

| Profile | From | Decoder | Rendition |
|---|---|---|---|
| 1 | 17 music | SNAC 32 kHz, reference integer decoder v1 | Opus, mono, 16 kbit/s |
| 2 | 16 speech | SNAC 24 kHz, reference integer decoder v1 | Opus, mono, 8 kbit/s |

Determinism is a Phase 1 deliverable: the same codes must give the same rendition hash on
x86-64 and AArch64. The assumptions to be tested: a decoder in integer arithmetic with defined
rounding is bit-exact by construction, and libopus built in fixed point without platform
intrinsics is a deterministic function of its input and settings. If a platform cannot
reproduce a profile, it does not serve that profile.

## 2. Channels, collections and manifests

A **channel** is an Ed25519 public key: one per provider (a newsroom, a band, a radio maker). Its
**channel id** is the first 8 bytes of BLAKE3(pubkey).

A provider publishes **collections**: an *album* (pieces in a fixed order), a *series* (episodes
that come and go, such as a podcast or a daily bulletin; a series with a schedule is a radio
station), or *singles* (pieces without an order). Two kinds of manifest describe them.

The **root manifest** of a channel (content type 1, `application/meshcast-manifest`) is its signed
index, CBOR-encoded:

| Field | Type | Meaning |
|---|---|---|
| `chan` | 32 B | channel public key |
| `seq` | u32 | monotonically increasing; a node keeps only the highest valid seq per channel |
| `title` | text | channel name |
| `collections` | list of {`cid` u32, `kind` u8, `title`, `manifest` {`id` 32 B, `len` u32}, `cover` {`id` 32 B, `len` u32} or null, `changed` bool} | the channel's collections; `cid` is chosen by the provider and stays the same across versions, `kind` is 1 album, 2 series, 3 singles, and `changed` says the collection manifest is new in this root (a new collection, or a new version of its manifest): a holder brings those with the root (§4) |
| `prev` | 32 B optional | id of the previous root manifest, for history |
| `renditions` | {`id` 32 B, `len` u32} optional | the channel's rendition table (§1.2) |
| `sig` | 64 B | Ed25519 signature over everything above |

A **collection manifest** (content type 5, `application/meshcast-collection`) lists one
collection's pieces, CBOR-encoded:

| Field | Type | Meaning |
|---|---|---|
| `cid`, `kind`, `title` | u32, u8, text | as in the root |
| `pieces` | list of {`id` 32 B, `len` u32, `kind` u8 content type (§1.1), `title`, `integrity` {`id` 32 B, `len` u32} or null} | the collection's pieces in order, or the window of a series; `integrity` is reserved for the integrity data of §1 and is null until that is specified |
| `schedule` | list of {`id` 8 B, `start` u64 UTC seconds, `repeat` u32} | when to play what, for a station; usually empty |

A collection manifest is not signed: the root names it by its full hash, so it is exactly as
authentic as the root, and a node reads one only once it holds a valid root that names it. A
cover is an object of content type 6 (§1.1).

The order of `pieces` is the order a listener plays them in, and the network delivers in it: a
piece's **place** is its position in the list, and the earlier place goes first wherever a node
chooses what to ask for or upload next (§4). A series lists its window in the order it plays, so
a provider that wants its newest episode heard first lists it first. The pieces of singles have
no order: every one of them has place 0, as has anything that is not a piece.

The two levels keep what changes small and what a listener fetches to what it follows. A new
episode changes one collection manifest and the root, not the catalogue; a follower of one
podcast of a provider with fifty collections fetches the root and that one collection manifest.
Playback is on demand by default: a schedule is optional, and following it as radio is one way a
player can play a series, not a different kind of content.

Rules:
- A root manifest is valid only if the signature verifies against `chan`. Invalid manifests are
  dropped.
- **Subscribing** is storing in the follow list either a channel id, meaning every collection of
  that channel now and later, or a channel id and a `cid`, meaning that collection. The node then
  wants the channel's root manifest and all its collection manifests, which are small and tell it
  what another cell asks for (§4), and the pieces and covers of what it follows; of the other
  collections it fetches only what it relays (§4).
- **An announcer serves what its cell listens to and publishes, and fetches on request.** It
  serves a channel while a follower of its cell has asked for anything of it within `cell_keep`
  (draft 24 h) or announced it (its own publication, or a correction, below), and the channels it
  follows itself: it keeps the channel's root and every collection manifest current, wants a newer
  root it hears announced, and passes new manifests once unasked. Of any other channel it keeps,
  like every node, what comes by (below), and answers an ask for what it holds. Of pieces and
  covers it fetches what a follower asks for (§4), and keeps what it holds while it announces,
  within its carry budget (§4). Serving every channel it heard of, every announcer wanted and
  passed the manifests of all of them, and each new announcer passed them all again: with 400
  channels nobody followed beside the 24 that were, a sparse band L network delivered 88.8 % of
  its bulletins over 24 worlds, one of them 8.5 %, instead of 99.8 %, with twice the frames and 44
  times the role changes; serving its cell's, and announcing new roots first (§3.4), 98.9 %
  (FEASIBILITY.md §30). It does not fetch a channel because it heard of it: once
  nodes relay for any channel (§4), content reaches the cells whose listeners ask for it, and
  announcers that fetched everything they heard of sent more frames for the same delivery
  (FEASIBILITY.md §27).
- **Every node keeps the menu it hears.** Of a channel it neither follows nor serves, a node
  collects and keeps the root and collection manifests that come by, announced and pushed on the
  control carrier or passed by its announcer when new, without asking for them, and within its
  carry budget (§4). They let it name what another cell asks for, so that it can relay it (§4).
  Where asks name pieces one by one (§3.3), a node without them could not relay for a channel it
  did not follow (FEASIBILITY.md §27).
- **A new root alone does not cost a collection its pieces.** A node keeps, per collection, the
  collection manifest it adopted, and with it its pieces, until it holds the one a newer root
  names; a collection that a newer root no longer names has left the channel. This is the rule
  for announcements one level down: a follower that has adopted a new root and not yet its
  collection manifest would otherwise evict the window it holds.
- **Encrypted channels**: object payloads are encrypted with XChaCha20-Poly1305 under a key derived
  from the channel secret and the object id; titles in the manifest may be encrypted too. The
  channel secret is shared out of band (QR code from the phone app). Non-subscribers can still
  relay the objects, which is intended: relaying needs no key and helps subscribers.
- Manifests, root and collection, are ordinary objects: they travel through the same carousel
  and gossip as pieces. The special-casing: a `MANIFEST_ANNOUNCE` control frame names the newest
  root manifest id per channel so that nodes know what to want; a carousel passes a manifest once
  unasked when it is new, and passes manifests before anything else (§4); and a node keeps a
  manifest's bytes whatever its size, because it reads them. There is no limit on how many
  collections a channel has, how many pieces a collection has, or how large a piece is
  (FEASIBILITY.md §13). Sets (§3.3) refer to collection manifests.
- **An announcement is a hint, not a fact.** `MANIFEST_ANNOUNCE` is not signed, so a node keeps
  apart, per channel, the manifest it *adopted* (its signature checked) and a newer one
  *announced* and not yet held. The announced one is fetched, never believed: the adopted one
  and its objects stay until the newer one is held, and a real manifest is adopted whatever was
  announced. The latest announcement heard replaces one still pending, unless symbols of that
  one are arriving. Announcers announce only manifests they hold. Before, a node took an
  announced seq for the channel's newest: one frame claiming the highest seq made it ignore every
  real announcement and refuse the real manifest, announcers passed the claim on to their cells,
  and every honest announcement made a follower evict the window it held while it waited for the
  new manifest (FEASIBILITY.md §13).
- **A root travels with its announcement.** The first time a node announces a root manifest it
  holds on the long-range control carrier, the root's symbols follow there, in the control window
  (§3), and those of the collection manifests the root flags as changed as far as they fit in the
  same window, unless someone has sent them there in the last `T_want_min`; a larger collection
  manifest goes the usual way, through its cell. Pushed whole, a collection manifest of 120 pieces
  arrived after twelve minutes instead of four (FEASIBILITY.md §23). Every node in range that
  follows the channel, and every announcer, keeps them; a source that pushed its own counts it as
  delivered, since it sent the true bytes to everyone in range, its announcer included. Symbols
  heard on the control carrier say nothing about a node's own cell: they end no offer of its own
  and prove nothing about its announcer (§5.2). A root is a symbol or two; without this it crossed
  a cell only through a node that followed its channel, and where interests are sparse many cells
  never had it and their listeners never learned of the bulletin it named (FEASIBILITY.md §21,
  §22).
- **Followers keep their announcer current.** An announcer announces its manifests in ascending
  channel order: all of them in one frame, flagged as its whole list, when they fit, otherwise
  from a cursor that steps one entry less than a frame holds, so that any two neighbours on the
  list share a frame; when they do not fit, in the round after it adopted a root, the roots it
  adopted last instead (§3.4), which prove nothing about a channel they leave out. A follower that holds the adopted
  manifest of a channel it follows or
  publishes, and hears its own announcer announce an older seq of it or leave it out (absent from
  a whole list, or between two neighbours), announces its own: after a random wait of up to
  `T_offer`, not if it hears anyone announce that seq or a newer one first, not while its
  announcer is asking for that manifest (it knows of it and is fetching it; what a follower's
  announcer asked for counts only since it followed that announcer, FEASIBILITY.md §16, and only
  for `T_want_min`, since an announcer that fetches asks again every round), and at most once
  per channel every `T_want_min`. An ask that counted for `want_ttl` let an announcer that asked
  once and then took an older announcement for the newest go uncorrected for an hour
  (FEASIBILITY.md §20). The announcer then wants it like any announced
  manifest, and a holder uploads it. Like an offer, it answers something the announcer said.
  Without it, an announcer whose library is older than its cell's (a station back from a power
  cut, a follower that just stepped up) never learned the newer manifests, because a source
  announces its own only until its announcer has them: in a living band O network with nodes
  coming and going, up to 41 % of the followers that were on at the end lacked a current window
  (FEASIBILITY.md §13).

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
| 0x2 | `BULK` | announcer, or a source uploading to the announcer; a node that announces a root on the control carrier for the first time (§2) | bulk carriers; the symbols of a root and of the collection manifests new in it also on the control carrier | one symbol of one object |
| 0x3 | `GOSSIP` | sources, the announcer, and followers with unserved wants | bulk carrier | HAVE / WANT summaries |
| 0x4 | `MANIFEST_ANNOUNCE` | sources, the announcer, and followers whose announcer is behind (§2) | bulk carrier; a source's or announcer's copy **also** on the long-range control carrier | newest manifest id per channel; the control-carrier copy is discovery for other cells |
| 0x5 | `NACK` | any node that is nearly complete on an object and sees no progress; a follower asking a silent announcer for proof (§5.2) | bulk carrier, rare | compact repair request, answered by the carousel or by the uploading source |

**A cell is what hears each other on the bulk carrier.** The Phase 0 simulator showed that
running the election over a long-range control carrier elects announcers that most of their
"followers" cannot receive content from. So everything cell-local (beacons, election, gossip,
NACK) travels on the bulk carrier, where a control frame costs milliseconds; the LoRa control
carrier only carries `MANIFEST_ANNOUNCE`, so that neighbouring cells learn which channels exist
and fetch them through bridge nodes, and the roots those announce (§2).

**One radio, two carriers: the control window.** An SX1262 receives LoRa or (G)FSK, never both at
once: the packet type changes only in standby, and the other mode's settings are lost (SX1261/2
datasheet rev 1.2, §13.4.2); the change itself is quick, standby to receive taking 83 µs
(Table 8-2). A node whose bulk carrier is GFSK on that radio therefore listens on the control
carrier only in a common window, `T_ctrl_window` (draft 4 s) once every `T_ctrl_period` (60 s) of
the shared time the hop sequences already use (§5.3, §6), and on the bulk carrier the rest of the
time. **Where the window falls in a period follows from the period's number**, pseudo-randomly, as
a hop sequence does: with `r = splitmix64(WINDOW_ID, k)` for period `k` (`WINDOW_ID` = 0xFFFFFFFD),
the window begins `r mod (T_ctrl_period − T_ctrl_window + 1)` ms into the period. Where the cell's
carrier hops and a period is a whole number of dwells (the draft: three), it keeps inside one dwell
and clear of the dwell's first tenth, where announcers beacon and candidates step up: it begins in
dwell `r mod (T_ctrl_period / T_dwell)` of the period, `T_dwell / 10 + T_guard + (r div
(T_ctrl_period / T_dwell)) mod (T_dwell − T_ctrl_window − 2 T_guard − T_dwell / 10 + 1)` ms after
the dwell starts. Everyone who shares a time shares every window, and two groups whose times
differ share one now and then, which is how the later time crosses to the other (§6). Placed
halfway through every period, as first specified, the windows of two groups apart stayed apart for
good; placed anywhere in a hopping cell, a window now and then took the start of a meeting dwell,
and a band L neighbourhood started playback a meeting later (FEASIBILITY.md §29). Every node sends
control-carrier frames only in the window, each one whole, and nothing on a bulk carrier that
shares the radio during it; a bulk carrier on another radio (ESP-NOW, IP) is not held. A station
whose concentrator receives LoRa and FSK at once (SX1302) follows the same rule, so that the ones
that cannot hear it miss nothing.
Short windows were tried as well: 1 to 4 s every 30 or 60 s delivered within a few percent of a
node with two receivers, and a node that never listened on the control carrier fell far behind,
so the draft takes the longest window, which leaves the most room for clocks that disagree and for
busy windows (FEASIBILITY.md §23). In band O, where the draft puts the control carrier on the
frequency of the bulk carrier, the window also keeps the two from overlapping.

### 3.1 `BEACON`

| Offset | Size | Field |
|---|---|---|
| 1 | 1 | flags: bulk carrier kind (bits 0–2), capability (bits 3–4: mains power ×2 + IP uplink; §5.1) |
| 2 | 4 | `announcer_id` (first 4 bytes of the node's public key hash) |
| 6 | 2 | `score` (see §5) |
| 8 | 2 | `next_ms` — milliseconds until the next beacon from this announcer |
| 10 | 2 | `round` — carousel round counter |
| 12 | 8 | `time` — the sender's shared time in milliseconds as the frame begins (§6) |
| 20 | 1 | `time_quality` — 1 shared time; 2 phone/NTP and 3 GPS are reserved (§6) |
| 21 | 1 | `colour` — this announcer's rank in its conflict set: its channel offset, or its time slot on a single-channel carrier |
| 22 | 1 | `colours` — colours in use around it: how many slots the cycle has |
| 23 | 1 | `upload_phases` — how many phases its listening time is divided into for uploads (§4); 1 means none |
| 24 | 4 | `occupancy` — measured channel occupancy per bulk channel, 4 × u8 percent ("spectrum weather") |
| 28 | 2 | CRC-16 |

30 bytes. Sent on the bulk carrier every `T_beacon` (draft 60 s), at every dwell start on a
hopping carrier, and as the first frame of every carousel round.

### 3.2 `BULK`

| Offset | Size | Field |
|---|---|---|
| 2 | 8 | `object_short_id` |
| 10 | 2 | `block` — source block index |
| 12 | 2 | `esi` — encoding symbol id; `< K` is a source symbol, `>= K` a repair symbol (v1) |
| 14 | 4 | `len` — the object's length in bytes |
| 18 | T | payload |
| 18+T | 2 | CRC |

With `T = 200`: 220 bytes. Fits SX126x and ESP-NOW.

**A symbol carries what it takes to use it.** The object's length fixes its number of blocks
and each block's K, so a node that hears any symbol can register the object, place the symbol and
complete the object from symbols alone, before or without its manifest. ALC, under FLUTE, can do
the same: its EXT_FTI header extension carries the FEC Object Transmission Information in the
packets themselves (RFC 5775 §4.2), and for RaptorQ that information starts with the transfer
length (RFC 6330 §3.3.2). The manifest, when it arrives, is still the authority: its signed id verifies the
bytes, and if it gives a different length the entry is reset. The field replaces an earlier `k`
(K of this block), which the length determines; the frame grew by 2 bytes, 0.9 %. Without the
length, a node that had heard every symbol of an object but not its metadata could never complete
it, and a want for such an object was granted and answered over and over; FEASIBILITY.md §9.6.

### 3.3 `GOSSIP`

| Offset | Size | Field |
|---|---|---|
| 1 | 1 | flags: `n_heard` (bits 0–1), `n_sets` (bits 2–4), `n_have_sets` (bits 5–7) |
| 2 | 4 | `node_id` |
| 6 | 4 | `announcer_id` — the announcer this node currently follows (its own id if it announces, 0 if none heard) |
| 10 | 1 | `announcer_colour` — that announcer's colour, so holders can reach it without having heard its beacon |
| 11 | 1 | `announcer_colours` — the number of colours in its cycle |
| 12 | 1 | `n_have`, complete objects listed |
| 13 | 1 | `n_want` |
| 14 | 6 × n_heard | other announcers this node hears: id (4), colour (1), colours (1); the conflict report (§5) |
| … | 8 × n_have | short ids the node has completely |
| … | 18 × n_have_sets | have sets: manifest short id (8), first piece (2), bitmap (8) |
| … | 13 × n_want | wants: short id (8), granted holder (4; 0 = open ask), phase byte (1): the upload phase of the grant in its low four bits (§4), and in bit 7 an announcer's mark that it, or one of its own followers, listens to the object |
| … | 23 × n_sets | want sets: manifest short id (8), first piece (2), bitmap (8), granted holder (4; 0 = open ask), phase byte (1) as for a want |
| … | 2 | CRC-16 |

**Sets** name pieces by their place in a collection manifest (§2): bit *i* of the bitmap is piece
*first + i*, the (*first + i*)-th piece the collection manifest lists, so one entry covers up to 64
pieces of one collection; the manifest short id in a set is the collection manifest's.
A want set is to a WANT entry what it is to one object: open, or granted to one holder in one
phase. A node sends the pieces of a manifest it holds as sets and everything else (manifests,
renditions, objects of no manifest it holds) by name, where sets are used at all (§4). A receiver
reads a set by the manifest it holds of that id; a want set of a manifest it does not hold is
ignored and asked again, and a have set of a manifest it does not hold is kept per neighbour and
read when it adopts that manifest: a follower about to fetch from another cell often lacks the
manifest the other cell's sets refer to, and threw away what that cell's announcer had said it
had (FEASIBILITY.md §14).

Draft cap: 3 heard; HAVE ids and have sets within 96 bytes (8 × n_have + 18 × n_have_sets); wants
and want sets within 104 bytes (13 × n_want + 23 × n_sets); at most 234 bytes, as before sets
existed. For larger libraries a node rotates through its list across gossip rounds, most recently
completed and most wanted first; an announcer lists what it completed in the last `T_grant` first
in every HAVE, at most half the list (§4, "An upload ends when its announcer holds the object"). Which want sets go first when not all fit is in §4: one for
every collection before a second for any, collections in progress first. Followers that are not
sources send GOSSIP only when they have something new to offer that the announcer lacks (the
"upload" case) or, rarely, a WANT for an object the announcer has never included; the default is
silence.

### 3.4 `MANIFEST_ANNOUNCE`

Flags bit 0: the entries are the sender's whole list (§2). Flags bit 1: the entries are a
selection, the roots the sender adopted last, newest first, from which a receiver can tell
nothing about a channel missing from them. Then `node_id` (4), a count (1) and up to 8 entries of
`channel_id` (8) + `manifest_short_id` (8) + `seq` (4) + `len` (4), otherwise in ascending channel
order and wrapping around at most once, + CRC: 201 bytes when full. An announcer that knows no
manifest sends an empty whole list, so that its followers tell it theirs. An announcer whose list
does not fit one frame sends, in the round after it adopted a root but never in two rounds
running, the roots it adopted last instead of the next stretch in channel order. In channel order
alone, the new root of one channel among four hundred waited hours for its turn, and the cells
that followed it with it: over eight worlds a sparse band L network delivered 96.8 % of its
bulletins instead of 99.1 %. Every other round regardless, the stretches in channel order by
which followers tell what their announcer lacks (§2) came half as often, and with 24 channels and
real clocks the network delivered 0.04 points less than in channel order alone; in the round after
a new root only, as much (FEASIBILITY.md §30).

### 3.5 `NACK` (v0 repair)

| Offset | Size | Field |
|---|---|---|
| 1 | 1 | flags: the upload phase answers use (low 4 bits; §4), set by an announcer |
| 2 | 4 | `node_id` of the asker |
| 6 | 8 | `object_short_id` |
| 14 | 2 | `block` |
| 16 | 4 | `answerer` — the holder the asker names to answer (0: its announcer's carousel, or for an announcer any holder, after a wait) |
| 20 | 1 | `n_ranges` |
| 21 | 4 × n_ranges | missing source symbols as (first `esi` u16, count u16) runs |
| … | 2 | CRC-16 |

Draft cap 40 ranges, 183 bytes. Sent by a node that is nearly complete on an object (draft 80 %,
or all of it but one symbol, §4) and has seen no progress for `T_nack_stall`. A follower's NACK
goes to its announcer, whose carousel sends the missing symbols first, before the rest of its round;
but if that announcer is itself asking for the object (its WANT lists it) and has granted it to
nobody for `T_grant`, nobody in
the cell can repair it, and the follower names instead the holder it hears best among those that
follow another announcer: a holder in its own cell would answer its announcer's ask anyway, and
naming one as well only doubled the work (FEASIBILITY.md §12). That holder is usually in the next
cell, and its uploads are what the follower overheard (§4, "Content crosses wherever a link
does"). An announcer's NACK names
who answers, and in which phase: its granted uploader if the object has one, otherwise the holder
of the object it hears best (holders say what they have in GOSSIP HAVE). The named holder answers
at once; only if the announcer knows no holder does any holder answer, after a wait. A follower
that suspects its announcer of serving nothing sends a NACK for one symbol of an object it wants,
whatever its progress, naming that announcer (§5.2); other announcers that hear a NACK naming an
announcer leave it to that one, so that a neighbour cannot pass the test on a false announcer's
behalf. This is the whole repair mechanism in v0; it costs one small control frame per object
per asker at most, which is negligible next to the object itself.

v1 replaces most NACKs with RaptorQ repair symbols (`esi >= K`) generated by the announcer at a
configurable overhead (draft 10 %), so that receivers that missed any `≤ 10 %` of a block recover
without transmitting anything.

## 4. Carousel

The announcer maintains a **carousel set**: every object (including manifests) that a follower in
the cell wants, as learned from GOSSIP and MANIFEST_ANNOUNCE, that the announcer has. A round is:

1. `BEACON` on the bulk carrier.
2. For each object in the set, manifests first (root manifests before collection manifests: a
   node reads a collection manifest only once it holds the root that names it, so the other way
   round it would miss it) and then **the most listeners served per byte**: emit its symbols,
   one `BULK` frame each, subject to EtherFatsoen and EtherDiscipline gating between frames. The
   listeners of an object are the followers asking for it; ordering by listeners divided by size
   is Smith's rule, which minimises the total time listeners wait on one shared transmitter.
   Among objects of one size it is simply most-wanted first; a small object no longer waits
   behind a large one; and every wanted object is still sent every round, so nothing starves.
   Between objects that serve as many listeners per byte, the earlier place (§2) goes first,
   because it plays first. Passing the earlier place first whatever its size was tried as well:
   listeners in band L could start up to a minute sooner, and a band O town needed 7 % more
   frames (FEASIBILITY.md §19).
3. Symbols named in NACKs go to a front queue that is sent before anything else, at once, in
   the middle of a round as well.
4. Objects that every heard follower reports complete leave the set.

The round never waits for anyone. A follower that joins mid-round starts collecting and completes
the object next round. Symbols are idempotent, so a slow announcer simply takes more rounds.

**One pass, then repair.** Followers never report HAVE, so the announcer cannot know when
everyone is done. Each object gets one full pass (`max_passes` = 1) and then leaves the carousel
unless a new WANT arrives after that pass: a follower that caught most of it repairs the rest
with a NACK, one that caught little asks again. Three passes per object, the earlier draft, cost
35 to 49 % more airtime in every scenario and bought no speed; the repair does the work of the
repeated passes, aimed (FEASIBILITY.md §9.8). A follower asks again when nothing it wants has
arrived for `T_want_min`, or one thing it wants has not for twice that. Waiting until nothing
arrived let one object that trickled in from a neighbouring cell keep a follower silent for
hours, holding 79.6 % of another, just short of a repair; asking again whenever one want had
waited `T_want_min` was about as fast and cost up to 8 % more frames (FEASIBILITY.md §22).
And a trickle is not coming: what has not completed `T_excursion` after the follower last asked for
it is asked for again, whatever arrived meanwhile. A follower that missed an announcer's first
pass of four objects took one or two symbols from each repetition, each counted as having
arrived, and it never asked again: it held 64 of 113 symbols after six hours (FEASIBILITY.md §26).

**What a new manifest names, the announcer is told.** A follower that adopts a manifest of what it
listens to, root or collection, waits for its announcer's next GOSSIP, or `T_gossip` if it hears
none, and then, after a random wait of up to `T_offer` and whatever its usual cadence, asks for
what the manifest names that its announcer neither lists in its HAVE nor asks for itself and that
is not arriving, asking only for what it never asked for. Its announcer fetches only what its
followers ask for (§2): on the usual cadence alone, a bulletin whose manifest came on the control
carrier just after the followers had asked for something else waited up to `T_want_min` before its
station knew that anyone wanted it, newcomers to a band O cell caught up 1.7 minutes later, and,
with clocks apart, the slowest hour of a living band O network's bulletins took 11 minutes
instead of 2. Asking at once for all of it
made cells that would have overheard a neighbour's pass have their own announcer pass it too: an
ESP-NOW neighbourhood sent 15 % more frames and its slowest tenth of listeners started 5 minutes
later, as asking soon at home had cost while announcers fetched everything they heard of
(FEASIBILITY.md §15, §27). An announcer that fetches the object anyway asks for it in that GOSSIP,
and its followers then ask for nothing.

**Repetition that does not help is repeated ever more slowly.** The first time an object is asked
for again it is passed again at once; each further repetition waits longer after the pass before:
`T_want_min`, then twice, four and eight times that (10, 20, 40 and 80 minutes at the draft), and
never longer. When nobody asks for the object for twice its current wait after a pass, it has
rested and starts over. A repeated pass reaches everyone who missed the last one at once, so
honest followers, who ask at most every `T_want_min` and stop once they hold the object, hardly
notice; a node that keeps asking for everything, under its own id or under made-up ones, gets
one repetition per object every 80 minutes instead of one per minute. The rule looks only at the
object, so it needs no identity. Its ceiling is a trade: a lower one bounds an attacker more
tightly, but a follower who starts listening during an attack waits up to that long for its
first repetition (ABUSE.md, FEASIBILITY.md §11).

**A manifest is passed once unasked, then when asked for.** A manifest new to a carousel (one
its node publishes or adopts, or every manifest a node holds when it becomes announcer) gets one
pass without being asked; after that it is passed when it is wanted, like any object, but before
anything else in the round. A node that lacks a manifest learns its id from `MANIFEST_ANNOUNCE`
and asks. The earlier rule repeated every manifest every `T_always` (5 min) and in every round
in which anything was wanted, and that grew with the catalogue: with an hour of music in
30-second pieces a manifest lists 120 objects, and the repetitions were most of the airtime that
went to passes nobody had asked for. Passing manifests only when new or asked for saved up to 29 %
of all frames in the nine scenarios of FEASIBILITY.md §9.3 at the same delivery (§13 there). A
carousel with nothing to send is silent; the announcer then only beacons. (The first simulator
runs looped manifests forever at the full duty cycle, which wasted the budget and caused
half-duplex losses during uploads.)

**An ask is answered in full.** A follower asks at most every `T_want_min`, but what a manifest it
asked for names is the rest of that ask, not a new one: once it has a manifest it asked for, it
asks for what the manifest names soon, after a random wait of up to `T_offer` (in which a pass
can make the ask unneeded), at most every `T_gossip_min`, and only for what it never asked for.
A manifest it got unasked, from a pass, changes nothing: it asks on its cadence. Two levels of
manifest make this matter: a follower that has to ask, a newcomer or one that missed the pass of
a new root, asks for the root, then the collection manifest, then the pieces, and waited
`T_want_min` before each (FEASIBILITY.md §16). Asking soon after every manifest, asked for or not,
was tried and cost the band O networks more frames than it saved (FEASIBILITY.md §15.3).

**A want is served by the announcer it names.** A follower's WANT names the announcer it
follows (§3.3). Other announcers that overhear it do not serve it: two announcers answering the
same WANT start the same pass at the same instant, and where they cannot hear each other every
frame of both collides at the follower that asked. In one simulated world a follower caught 27
of an object's 216 symbols in twelve hours that way (FEASIBILITY.md §9.8). An announcer still
hears the WANTs, offers and conflict reports of other cells; it only leaves their wants to their
own announcers.

**Repair.** Any node, follower or announcer, that holds at least 80 % of an object, or all of it
but one symbol, and has seen no new symbol for `T_nack_stall` (draft 60 s) sends one NACK listing
the missing symbols. The announcer's carousel answers from its front queue; a holder whose upload
the announcer is missing answers with exactly those symbols. Stall detection is time-based, not
round-based, so it also works when the carousel is idle. A NACK for one symbol is the smallest
repair there is: under the fraction alone an object of two to four symbols, such as a collection
manifest, could not be repaired at all, and one that had lost one of its two symbols waited for
the next round of asking, five minutes at an announcer and `T_want_min` at a follower
(FEASIBILITY.md §20).

**A repair is an ask.** A node can reach 95 % of an object by overhearing a neighbouring cell's
carousel and then want the rest, although nobody was ever granted to it. So a NACK is answered by
the granted uploader at once, and by any other holder after a wait: the wait grows with how many
of its own neighbours the holder hears better than the asker, so **whoever hears the asker best
answers first**, and hearing anyone send those symbols cancels an answer that has not begun.
No absolute signal level enters into it; the rank is relative to the holder's own neighbourhood.
**An answer leaves out what its holder sent since.** A holder that heard a NACK heard its asker,
so the asker hears the holder: every symbol the holder sends, in an upload or in another answer,
leaves the answers it has lined up for that object, and an answer left with nothing ends. A
granted uploader answers at once, but behind the upload the NACK came during, and in one band O
town that upload brought thirteen followers everything they had listed before the answers began,
150 symbols each (FEASIBILITY.md §22).

**Who uploads: ask, offer, grant, send.** An announcer's WANT entry is either an *open ask*
(`grant = NONE`) or a *grant* naming one uploader. Any node that holds the object, in the
announcer's own cell or a neighbouring one, answers an open ask with an *offer*: one small GOSSIP
carrying HAVE, after a random delay of up to `T_offer`, and not at all if it hears another
holder's offer first. **A frame goes where its addressee listens**: an offer to the holder's own
announcer goes out at once, but on a hopping carrier an offer to another cell's announcer waits
for the rendezvous, the only time that announcer listens on a channel the holder can reach. The
announcer grants the first offer it hears and names that holder in its next WANT; only the named
holder uploads, one object at a time (further grants queue), on the announcer's channel and in
its slot. A grant lapses after `T_grant` without a symbol arriving, counted from the grant or
from the last symbol, whichever is later, and the ask becomes open again: an uploader that
delivered once and then fell silent is no more responsible than one that never started. An
announcer's NACK names who answers (§3.5). **An announcer's HAVE is not an offer**: it lists what
its carousel serves, and announcers do not upload. So only a node that follows someone else can
be granted, and only its HAVE silences other holders' offers. Before this rule an announcer could
grant another announcer, which never sent, and the followers that would have offered fell silent
because they had heard the other announcer's HAVE; FEASIBILITY.md §9.6. This is the DHCP pattern, and it replaced "any holder answers after a random wait",
which the simulator showed producing fourteen uploads per object per cell among holders that
could not hear each other's suppression.

**An upload ends when its announcer holds the object.** A holder ends an upload, running or lined
up, as soon as the announcer it uploads to lists the object in a HAVE. An announcer often
completes an object before an upload of it ends, from symbols it overheard the same holder or
another send to a neighbouring cell's announcer, or from another holder, and the holder cannot
tell otherwise. So an announcer lists first in every HAVE, for `T_grant` after it completed them,
the objects it completed, the latest first and at most half the list: a HAVE holds twelve ids
(§3.3), and in the rotation alone a holder that had missed the one HAVE after the completion
uploaded the object whole, minutes later. Without the rule, 13.5 % of the upload frames in one
band O town went to announcers that already held the object, and a quarter once roots travelled on
the control carrier (FEASIBILITY.md §22). Anyone can send a HAVE in an announcer's name;
ABUSE.md lists what a forged name can do.

**The receiver divides its listening time.** Holders on opposite sides of a cell cannot hear
each other, so carrier sensing cannot make them take turns, and their uploads collide at the
announcer. The announcer, the only one that can tell, divides its listening time among those it
asks to speak. Each grant carries a phase, in the WANT entry that names the holder: the phase of
that holder's running grants if it has any, otherwise the lowest one no running grant uses.
Listening time is divided among those who speak, not among the objects they bring: a holder
uploads one object at a time, so a phase per object left most of each cycle idle while a holder
with several objects waited its turn in each of them, and an hour of music in 3-minute pieces
reached a band L neighbourhood in 70 minutes instead of 27 (FEASIBILITY.md §13). The announcer's beacon carries `upload_phases`, K = the highest
phase in use + 1, counting only grants it has named in a WANT and repairs it has reserved: a grant
that did not fit its asks yet raised K in the beacon while its holder kept silent, and the
uploaders that had heard the beacon and one that had not divided the time differently and
collided frame for frame (FEASIBILITY.md §23). A phase lasts `T_upload_phase` (1 s, one permitted
transmission under polite access), and an uploader transmits only in its own phase of each cycle
of K phases. An uploader that has not yet heard the new K takes the larger of its phase + 1 and
the highest phase it heard its announcer grant in a WANT or reserve in a NACK, + 1. One running upload has K = 1
and all the time. K follows the number of running uploads, so as many may run at once as before,
and none overlaps another. When a grant ends its phase is free for the next one, and K shrinks
once the highest phase is released. An announcer grants at most 16 uploads at once; a holder
that offers when all phases are taken is granted on a later WANT. The cost is one byte in the
beacon and one per WANT entry. Hashing object and holder to a phase instead needed K = 16 far too
often (the birthday problem), and fixed phases cut the airtime but made a bulletin 50 % slower;
FEASIBILITY.md §9.6.

An announcer divides its listening time on every radio carrier. Under polite access every
transmission is at most `Ton_max` and followed by a pause, so an upload is spread over minutes and
hidden uploaders overlap. On a carrier without a limit the announcer's one receiver is the only
limit, so dividing its time costs nothing in total; and on a hopping one, every upload to an
announcer waits for the end of the meeting dwell, when its channel comes back, so hidden uploaders
start together. Without phases, 45 % of the upload frames in the ESP-NOW neighbourhood collided
at their own announcer; with them none did, the uploads needed half the frames, and the
scenario's median went from 13.7 to 11.8 minutes (FEASIBILITY.md §15). Under a duty cycle phases
were first left out, because with a phase per grant a lone upload was held to one phase in K and
took K times as long (a median upload of 39 s instead of 4 s in band O); with phases per holder a
lone holder has the whole cycle. And two holders that hear each other do meet there: one that
sends back to back never finds the channel busy, while the other backs off over a window that
doubles each time it does, and in one band O world a source uploaded at a third of its rate for
25 minutes. With phases under the duty cycle too, an hour of music reached a band O neighbourhood
in 20.4 to 22.7 minutes in every world at every size from 14 kB up, where up to five worlds in
eight had taken up to 18 minutes longer (FEASIBILITY.md §18).

**The announcer keeps quiet in the phases it gave away.** A radio that transmits cannot receive,
and carrier sensing does not stop an announcer from talking over an uploader it can decode but
hears below the clear-channel threshold (15 dB above sensitivity, ETSI EN 300 220-2 Table 18).
So an announcer holds its carousel content during a phase whose uploader it has heard in the last
two cycles; control frames still go. In the ring smoke test a quarter of the upload frames had
arrived while the station was transmitting; afterwards 1.7 %, and 43 % fewer upload frames were
needed; FEASIBILITY.md §9.7.

**Every upload to an announcer runs in a phase the announcer named, by the holder it named.** A
grant names both in the WANT. An announcer's NACK names both too (§3.5): the granted uploader and
its phase if the object has one, otherwise the holder it hears best and a phase chosen as for a
grant to that holder, reserved for repairs of that object until it completes or `T_grant` passes
without a symbol. Answers that
nobody named, from holders that cannot hear each other, were nearly all the collisions left in
band L after grants had phases; FEASIBILITY.md §9.6.

**An upload keeps out of the phases other announcers gave away.** Phases are slots of one grid on
the shared time (§6), `T_upload_phase` wide, so the slot tells which phase every announcer is in. A
holder that hears another announcer grant a phase to a holder other than itself, in a WANT, keeps
its uploads to anyone else out of that phase while it hears that announcer, wherever that
announcer listens on the channel it sends on. The phase counts until that announcer's beacon
counts fewer phases or it stops announcing. A holder in range of the next cell's announcer
otherwise collides there with that cell's uploaders, which it cannot hear: in one band O world 7.5 %
of the upload frames were lost at their own announcer to an upload to another, and 15 % once
roots travelled on the control carrier (FEASIBILITY.md §22). The holder yields only where
that leaves it a turn: when none of its next 64 slots would be free, it ignores the other
announcers' phases. Under a duty cycle that is rare, since the cycle holds a holder to a tenth of
the time and the phases leave it more.

**A grant ends with the announcer's role.** Grants belong to the announcer role: a node that
stops announcing drops them, and it never names a holder in the WANT it sends as a follower. A
holder stops every upload to a node, and forgets that node's grants, as soon as it hears that
node say it follows someone else (any GOSSIP whose `announcer_id` is not its sender). Before this
rule, a third of all upload frames in a living band L network went to nodes that had stopped
announcing; FEASIBILITY.md §9.6. **A holder's own change of announcer ends nothing.** What another
cell's announcer granted a holder is still that announcer's when the holder follows someone new:
a running upload goes on, and what is lined up is ranked anew, the new announcer's grants first.
Dropped then, an upload to the next cell ended unfinished and unsaid, its grant ran idle for
`T_grant`, and in one band O world the first piece of a programme reached its listeners last
(FEASIBILITY.md §22).

**Ask only for what is not coming.** An announcer's WANT lists objects that have received no
symbol for `T_nack_stall`; an object whose symbols are arriving is not asked for again.

**How much one round asks for depends on what a round costs.** On a hopping carrier an announcer
asks, and a holder offers to another cell, only in the meeting dwell, once a hop cycle. Asking for
eight objects a round then cost a round per eight pieces: in band L an hour of music in 1-minute
pieces reached its last announcer after 140 minutes, against 50 in 10-minute pieces, while the
uploads themselves took seconds (FEASIBILITY.md §14). So where rounds are dear, followers ask their
announcer, announcers ask and grant, and holders offer in sets (§3.3): a whole collection in one
round, and a holder learns every object granted to it at once and uploads them one after the
other in its phase. Where asking is cheap, on a carrier that does not hop, a round asks for a
few objects and the next round comes as soon as they have arrived; there the order of arrival
matters more than the number of rounds, and asking for everything at once put a cell's speech
and music up side by side, so that speech arrived a third later. A holder whose granted upload
is flowing is then not asked for a second object until it is done.

**Earlier pieces first.** A listener plays a collection from its first piece (§2), so wherever an
announcer chooses what to ask for, the earlier place goes first. Asking by name, it asks for the
earlier place first, and among equal places for the most listeners per byte. Asking in sets, of
which a frame holds four (§3.3), every collection gets a set before any gets a second;
collections in progress, a piece of which arrived in the last `T_want_min` or is granted, go
before the others; and within that the earliest place goes first, then the set asked for longest
ago. What is flowing is not asked for, so a collection in progress gives up its place once its
uploads run: as many collections are in flight as arrive, and one that stopped arriving takes
turns with the rest.

**Asking in turns.** A frame names eight objects or four sets, and what nobody in reach holds is
wanted for good: in the order above alone, such wants took the frame round after round, and the
rest was never asked for. In a sparse band L network an announcer wanted a median of 24 objects
when a follower left it for ignoring one, and in 351 of 389 such leaves it had never asked for that
object, which it wanted too, in 50 minutes (FEASIBILITY.md §25). So what is **stuck**, asked for
and neither granted nor arriving for `T_excursion`, goes after everything else, and among what is
stuck the one asked for longest ago goes first; a set goes there once every piece it names is
stuck. Everything else keeps the order above, so what can be had is asked for as
before. Turning everything, so that what was asked for in the last `T_want_min` went after what
was not, also asked for every want, but undid the order above wherever more was wanted than a frame
holds and all of it could be had: in a band O network over 15 km² with 7 kB pieces, playback
started after 22.4 minutes instead of 8.8.

Each part answers a measurement (FEASIBILITY.md §19). In the order of manifest ids, the sets of
the lowest ids and their grants filled the frame round after round, and one announcer asked three
times in 26 minutes for a source whose manifest id sorted last, against 41 and 130 times for the
other two. The earliest place first alone put every collection side by side, and where a band L
network was asked for twelve albums at once they shared its uploads so thinly that listeners
could play one through only 10.5 minutes later than before; with collections in progress first,
0.4 minutes sooner than before, and the first piece 10.7 minutes sooner. By name, most listeners
per byte first put a source's speech before the music it plays between. Together, listeners in a
band O town could start a programme at its first piece and hear it through without waiting
7.9 minutes sooner, in a band L town 5.3 minutes sooner. What it costs: speech on its own
arrives later (in the band O town after 17.2 minutes instead of 8.0, while music came 5 minutes
sooner), and in band L the median listener held all of a programme up to 2.3 minutes later; at
the 90th percentile within a minute of before.

**A holder uploads the earlier place first** (§2), whatever grant brought it, and among equal
places the smallest first: Smith's rule as far as a holder can know it, since pieces at the same
place have about the same listeners. The earlier rule sorted only what one grant brought,
smallest first and then in the collection's order, behind everything earlier grants had lined
up; in a 15 km² band L network one holder uploaded the first piece of a programme after 33
later pieces of it (FEASIBILITY.md §19). **Manifests come first**: a manifest a holder is
granted goes before the pieces it already lined up, after any repair it was named for. Nothing of
a collection can be read without its manifest, and a manifest is a few symbols; lined up behind
pieces, a collection manifest granted to a holder in a 15 km² band L network waited almost six
minutes (FEASIBILITY.md §16). **Its own announcer comes first** after that: what its own
announcer granted a holder uploads before what another cell's announcer granted it. Its own cell
is where it is heard best, and every follower there that gets the object becomes a holder for the
neighbouring cells; in the order the grants arrived, a source served another cell for ten minutes
while its own waited (FEASIBILITY.md §15).

**A root brings what changed in it.** A holder that lists a root manifest in a HAVE lists with it,
in the same frame, the collection manifests the root flags as changed (§2) that it holds. An
announcer that grants the root on that HAVE takes those as granted to the same holder, in the
same phase, once it holds the root and wants them; the holder uploads them right after the root,
which the announcer must hold before it can read them. A root and the collection manifests new in
it then cost one round of asking, not two: asked for separately, a collection manifest reached a
band L station 200 seconds after its root, a meeting dwell or two (FEASIBILITY.md §16). The holder
uploads exactly what it listed with the root and the announcer counts exactly that, so both agree
whose phase the uploads use.
**Ask first for the most listeners per byte**, the carousel's rule applied one step earlier: a
holder uploads one object at a time, so the order of asking is the order of arriving, and a
3-minute track must not wait behind a 540 kB object from the same source. (Asking in object-id
order, as Phase 0 did, delayed small objects in mixed traffic about threefold.)
Several holders may upload different objects to one announcer at the same time: each spends its
own regulatory budget, and serialising them (tried in Phase 0) halves the cell's inbound rate.
Holders on opposite sides of a cell that cannot hear each other's CCA are the known residual
source of upload collisions (FEASIBILITY.md §7.5.0, rounds 9–10). An
object that is nearly complete (§4) is never re-asked in full: it is repaired by NACK, which
the granted uploader answers for as long as the announcer keeps asking. On frequency-agile
carriers an announcer's NACKs, like its gossip, go out in the meeting dwell, because its
uploader may live in another cell on another sequence.

**A holder that is uploading offers last.** It waits `T_offer` longer than a free holder would.
A free holder's offer then comes first and silences it, and an object that nobody else holds is
still offered, granted, and waits behind the current upload instead of a new round of asking.
Offering at the same time as free holders silenced them, and everything granted to the busy one
queued behind one radio while theirs stayed idle: a source in a small cell next to a large one
uploaded each of its objects to both announcers in turn, and the large cell waited for it while
nine of its neighbours already held the objects (FEASIBILITY.md §12). Not offering at all while
uploading, the rule that fixed that, left an object that only the busy holder had unasked until a
later round (FEASIBILITY.md §13).

**Content crosses wherever a link does.** Two cells are joined by any pair of nodes, one in each,
that hear each other. Each kind of pair has its own way across:

- *A follower of one cell hears the other cell's announcer, and holds what it asks for*: it
  answers the open ask with an offer in the rendezvous, and uploads when granted (above).
- *It hears the other cell ask, for its listeners, for something it does not hold*: it relays.
  An announcer marks in its asks what its own followers asked for, and what it listens to itself
  (§3.3): an announcer is a listener too. Marking only its followers' asks, an announcer that
  followed a channel nobody else in its cell followed asked for its bulletins unmarked for ten
  hours, and nobody relayed them (FEASIBILITY.md §28). Once such an ask has gone unmet long
  enough (below), a follower that hears it and can name the object, because a
  collection manifest or root it holds names it (its own, or the menu it keeps, §2), wants the
  piece or cover itself, fetches it in its own cell like anything it wants, keeps it, and answers
  the next ask with an offer. A set it cannot read names a collection manifest of a channel it
  does not follow: it fetches that manifest first, from its own announcer, which holds the
  manifests of every channel it hears of (§2) and answers only for names it knows, and relays
  the pieces once it can name them; they have waited as long as the asks for the manifest. It
  relays for listeners, not to fill another announcer's library, and only what nobody met,
  since most asks are answered by a holder within a round. Followers of single collections
  carry less than followers of whole channels, and with every listener of a 15 km² band L
  network following one album of four the chain of carriers of an album broke: in one world
  14 % of its listeners never had it. With relaying every listener had its album, and where
  nothing was missing it cost 6 % more frames at most (FEASIBILITY.md §17). Relaying only for
  channels it follows left a sparse network without a chain: where a channel has a handful of
  followers in the whole network, most cells between its source and a listener have none, and a
  band L network over 15 km² delivered 92.0 % of its bulletins in their period; relaying for
  any channel, 99.7 %, with 22 % fewer frames (FEASIBILITY.md §27). What a follower relays it
  keeps while it is asked for or used and `want_ttl` longer, within its carry budget (below);
  kept for good, a long-lived node would have kept everything it ever relayed. **A relay ends
  when the cell that asked for it holds the object:** once the asking announcer lists it, a
  follower that has not fetched it yet no longer wants it. Kept until `want_ttl` instead, relays
  passed the ask on from cell to cell: a relayer's own announcer asked for the object, marked for
  its listeners, the next cell relayed that, and so on, long after the listeners had it. More
  than half of all asks marked for listeners came from cells with no listener of the object, and
  a bulletin with a handful of listeners was fetched by some forty nodes. Withdrawing also when
  the asking announcer granted the object to someone else, delivery fell: a grant is not yet
  delivery (FEASIBILITY.md §28). How long is enough, a node learns from the asks it hears: **a
  relay waits as long as others would likely have met the ask.** For every age bucket (under 1,
  2, 4 and so on up to 64 minutes, and over) it counts the other cells' asks it heard reach that
  age, and those of them that someone else met before they were twice as old: the asking
  announcer granted the object to a holder, or lists it as held. An ask that it relayed itself,
  or forgot, counts by half and as not met, in the bucket where it left. It relays an ask once
  fewer than `relay_risk` of the asks of its age were met so, and at the latest after
  `want_ttl`. Before it has heard any it expects what `T_relay_wait` says: asks met half the
  time in a bucket that ends within it and never after, with the weight of four asks; and what
  it learned halves whenever 512 asks have reached the first bucket, so that what the
  neighbourhood does now counts most. In band O a holder or a follower in a neighbouring cell
  meets most asks within half an hour, and nodes wait longer than before; in a sparse band L
  network nobody else meets most of them, and nodes relay within the first minutes. Waiting a
  fixed number of the asking announcer's rounds instead made band O send 10 to 28 % more
  frames: where rounds of asking are cheap they say little about how soon holders answer. With
  both rules a sparse band L network delivered as much as before, 99.8 % of its bulletins in
  their period, with 4 % fewer frames, and its median bulletin arrived after 10 minutes instead
  of 19 (FEASIBILITY.md §28).
- *A follower hears another cell's announcer that has what it wants, which its own cannot get*:
  it goes there. A follower whose want has brought no symbol for `T_excursion` (draft 40 min),
  whose announcer has not granted the object to any uploader in that time and does not list it
  itself (a busy cell delivers late; only a cell that cannot get an object sends its followers
  out for it, and an announcer that lists an object either serves it or is not followed, §5.2),
  and that has heard another announcer list the object in its HAVE, follows that announcer, for
  what it has rather than for how well it is heard, until it holds everything it wanted that the
  announcer has, or until the visit brings nothing for `T_excursion`. Then it follows by signal
  again, and is back in its own cell as a holder, where its announcer's ask finds it. This is
  an **excursion**. On a visit the follower asks for what a new manifest names soon, after a
  random wait of up to `T_offer`: on a visit only the visitor asks (at home, see above), and
  waiting `T_want_min` to ask for what the manifest just fetched named kept one two-cluster
  world waiting nine minutes longer (FEASIBILITY.md §15). An excursion is the last way in,
  not a shortcut: at 20 minutes, followers in slow but working multi-cell networks went out
  dozens of times a day and every visit cost the visited carousel a repeated pass (11 % more
  airtime for a minute of median); at 40 minutes they go only where their cell cannot get the
  object at all (FEASIBILITY.md §12). A visit that brought not one symbol means the announcer
  lists what it does not serve, and the follower ignores it for `want_ttl` (§5.2). Without
  excursions, a cell whose only link to the rest was its own announcer, which never uploads,
  kept a source's content to itself for twelve hours (FEASIBILITY.md §12).
- *Two followers hear each other, neither announcer hears the other cell*: the cells are not in
  conflict, so they usually share a colour and a channel, and the follower overhears the other
  cell's uploads. What it misses it repairs from the holder it heard, by name (§3.5); the holder
  answers on the channel the follower listens on, even though only an announcer's NACK is otherwise
  answered by holders: one named holder answering is no storm. Without the named repair, two
  followers in one cluster of two had each overheard 112 of 113 symbols of three objects and had
  nobody to ask for the last one (FEASIBILITY.md §12).

Between two announcers alone there is no way across: an announcer neither uploads nor makes
excursions. Two announcers that hear each other merge when they share a cell (§5.2); two cells
joined by nothing but their announcers' link are the case left open (§9, question 11).

**You carry what you listen to.** A node registers, collects and keeps the objects of the
channels it follows (and, as announcer, what it holds of every channel it serves, §2). Objects
that no manifest of interest references any more, because the channel was unfollowed or the
object left the channel's window, are evicted, and offers and uploads of them dropped; own
objects are kept, and offered and uploaded, whether a source follows its own channel or not (a
source that did not dropped its queued uploads whenever it adopted another channel's manifest;
FEASIBILITY.md §15). An object leaves the window when the manifest that drops it is held,
not when it is announced (§2). A node that restarts, or stops announcing, evicts nothing
for `want_ttl`: a station back from a power cut, or an announcer that steps down for minutes,
would otherwise drop the library of every channel it does not follow itself and fetch it again
when it announces once more, which in a living band L network with nodes coming and going was
most of what was fetched twice (FEASIBILITY.md §13). Content crosses cells through nodes that
follow the channel and through nodes that relay it for listeners (above). A node that follows
a channel again asks for its manifest if it no longer holds it: its announcer announces nothing
it does not already know of.

**What you carry for others has a budget.** Besides its own objects and what it listens to, a
node holds what it relays and, as announcer, what its followers asked for. Of that it keeps at
most `carry_budget` bytes, chosen per device from what it can spare, and the menu it keeps of
channels it neither follows nor serves (§2) counts too; the manifests of what it follows or
serves do not, being small and how it knows what it plays and serves. An object is *of use*
while it arrives, is asked for or is sent, and for `want_ttl` after; the menu of a channel a
node neither follows nor serves is of use while it names another cell's ask, not when it
arrives. When the budget is full, what has not been of use gives way, and so does a relay
another cell's announcer lists as held; each keeps its metadata, so that it can be fetched again
by name: **first the menu, then relays another cell's announcer lists, then the least recently
used.** The menu comes by again for free, and a relay an announcer listed was needed again far
less often than other content; ordered by use alone, the menu filled small budgets and kept
relays out (FEASIBILITY.md §28). What is of use stays, even over the budget, and the node takes on
no more relays until there is room. Evicting
whatever was least recently used made a small budget evict what had just been fetched for
another cell before it was handed on, and fetch it again: at 64 kB per node a sparse band L
network delivered 77.8 % of its bulletins, against 99.5 % without a budget, and sent 7.7 times
the frames (FEASIBILITY.md §27). A budget of 0 relays nothing; an announcer still fetches what
its followers ask for, and lets it go once it has not been of use for `want_ttl`.

**Fresh before repeated.** The first copy of an object into a cell (an upload, or the carousel's
first pass) is worth more than its second and third pass. Fresh content is paced at the full
budget and admitted like metadata; repeated passes take the throttled rate and yield first. (The
simulator found five carousels on one channel throttling every transmitter to the floor,
including the sources uploading new tracks, so that a channel's last tracks never entered the
mesh.)

**Upload**: a source that has an object the announcer lacks sends GOSSIP with HAVE. The announcer
replies with GOSSIP WANT. The source then transmits the object's symbols as `BULK` frames under the
same gating; everyone in range collects them, not just the announcer. The source stops when it has
uploaded the object and the announcer reports HAVE (that announcer, whichever cell it serves), when
it hears anyone else send the object, or when it passes the object itself as announcer. A
HAVE alone is a claim: an announcer that lists a source's object although nobody has been heard
sending it is not believed, and not followed (§5.2). A source that hears no announcer for
`T_silence` (see §5) may become the announcer itself.

## 5. Announcer election and healing

Every node runs this state machine on every carrier independently (a node may be announcer on
ESP-NOW and follower on sub-GHz).

### 5.1 Score

`score` is a u16 computed locally, draft weights:

| Term | Weight | Source |
|---|---|---|
| distinct `node_id`s heard more than once in the last hour | 4 per node, capped at 64 nodes | GOSSIP / BEACON reception |
| mains powered | +64 | hardware |
| fraction of regulatory airtime budget unused | 0–64 | EtherDiscipline accounting |
| objects in the carousel set the node can serve | 1 per object, capped at 32 | library |
| has IP uplink | +16 | discovery |

The weights add up to `score_max` = 432.

A station on a roof with an SX1302, mains and internet scores near the maximum; a battery dongle
in a drawer scores low.

**Capability and circumstance.** Two of the terms say what a node *is*: mains power and an IP
uplink. Its *capability* is these two as a number, mains outranking an uplink (`2 × mains +
uplink`), and every beacon carries it (§3.1). The other terms say what a node *experiences* in its
role, and the role changes them: an announcer transmits, so it hears less than its followers
(a radio that sends cannot receive), and it spends its airtime budget while they keep theirs.
Scores are therefore compared only between nodes in the same role. Two announcers compare
capability first and score second; a follower compares only capability with its announcer
(§5.2). When followers challenged on score, a follower in the middle of three new cells reached
120 against its announcers' 72 to 76 within three minutes of a failover, purely by hearing the
uploaders of all three, took over, and then lost to a neighbour on the tie-break; its followers
and those of the announcers it displaced waited out `N_miss` beacons each time
(FEASIBILITY.md §12). A node id counts only once it has been heard a second time: names are not
authenticated in GOSSIP, and when every made-up name counted at once, one node sending WANTs
under a fresh name each minute inflated the scores of everyone who heard it, and the election in
one simulated world changed roles 27,335 times in 72 hours instead of 259 (FEASIBILITY.md §11).

### 5.2 States

```
FOLLOWER  ── no BEACON for N_miss expected intervals, another announcer audible ──────────▶ FOLLOWER of that one
FOLLOWER  ── no BEACON for N_miss expected intervals, nobody audible ─────────────────────▶ CANDIDATE
FOLLOWER  ── more capable than its announcer for `challenge_beacons` beacons, and
             no announcer heard is as capable ────────────────────────────────────────────▶ CANDIDATE (challenge)
FOLLOWER  ── a want stalled and ungranted for T_excursion, another announcer has it ──────▶ FOLLOWER of that one (excursion, §4)
FOLLOWER  ── its announcer lists what it does not serve ──────────────────────────────────▶ FOLLOWER of another, or CANDIDATE
CANDIDATE ── timer expires, still no BEACON it stands down for ───────────────────────────▶ ANNOUNCER
CANDIDATE ── hears BEACON of an announcer at least as capable ────────────────────────────▶ FOLLOWER
ANNOUNCER ── hears BEACON of a more capable announcer ────────────────────────────────────▶ FOLLOWER
ANNOUNCER ── hears BEACON, same capability, clearly higher score ─────────────────────────▶ FOLLOWER
ANNOUNCER ── hears BEACON, same capability, similar score and lower id, in its own cell ──▶ FOLLOWER
```

**Following is by signal and by evidence, stepping up is by capability and score.** A follower
needs to *receive* its announcer's carousel, so it follows the announcer it hears best (RSSI,
averaged) and switches only for one at least `rssi_hysteresis` (draft 6 dB) stronger, unless that
announcer does not serve what it lists (below). Capability and score decide who steps up when
nobody is heard and who yields when two announcers meet; capability alone decides when a follower
challenges its announcer.

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
- **Candidates step up in the order announcers yield in**: capability first, then score, then
  chance. A span of time is divided into four bands, one per capability (mains and uplink, mains,
  uplink, neither); within its band a candidate waits less the higher its score, over two thirds
  of the band, plus a jitter over the last third. A more capable node therefore always speaks
  before a less capable one; when a jitter that spanned the bands let a battery node step up a
  few seconds before the station, the station stepped up anyway, the battery node yielded, and
  its new followers waited out `N_miss` beacons (FEASIBILITY.md §12).
- **A candidate steps up where every other candidate hears it.** On a carrier that does not hop,
  that is any moment, and the span is `T_base + T_jitter` (draft 60 s + 10 s) after the candidacy
  began. Two candidates collide only if they step up within one beacon's airtime of each other,
  milliseconds; the span is long enough to keep them apart and short enough that a network of
  battery nodes, which waits out the bands of the stations it does not have, fails over in about 200
  s (FEASIBILITY.md §12). On a hopping carrier (§5.3) candidates are spread over the channels: a
  node that follows nobody scans slowly, and a new announcer's first beacon goes out on its own hop
  sequence, where almost nobody listens. Candidates there meet only in the meeting dwell, so that is
  where they step up: after the announcers' own meeting beacons (`T_dwell / 5`), over the next seven
  tenths of the dwell. The first to step up is heard by every candidate in range at once. Before
  this rule, when all fifty nodes of a band L neighbourhood started together, between 5 and 22 of
  them were announcer five minutes later and every node had been one; a failover produced up to 13
  announcers and settled after 13 minutes, once after more than an hour (FEASIBILITY.md §12).
- **A candidate stands down for an announcer at least as capable as itself.** While waiting it
  listens; a BEACON from such an announcer makes it a FOLLOWER. A more capable candidate keeps
  waiting and steps up, and the less capable announcer yields to it. (A challenger hears its own
  announcer's beacons all the time; when any beacon cancelled a candidacy, a challenge on a hopping
  carrier, where the announcer beacons every dwell, could never complete.)
- **Tie-break**: if an ANNOUNCER hears another BEACON, it compares capability first: it yields to
  a more capable announcer, near or far, and never to a less capable one. Between equals it
  compares scores: it yields if the other score exceeds its own by more than `H` (draft 10 % of
  the max), or if scores are within `H` and the other `announcer_id` is numerically lower and the
  other is in its own cell. Two announcers that cannot hear each other but
  are both heard by a node in between are detected by that node's GOSSIP (`announcer_id` field
  differs from the announcer's own id). That report makes them colour themselves apart (§5.3);
  it is not a reason to yield. An announcer yields only to a beacon it hears itself: it cannot
  follow an announcer it cannot hear, and yielding on reports let any node that repeated such a
  report make announcers step down and come back, over and over (ABUSE.md; FEASIBILITY.md §11).
  Removing it changed nothing in any scenario without an attacker. Convergence to one announcer
  per connected cell takes at most a few beacon intervals.
- **An announcer that does not serve, or cannot get, is not followed.** Beacons and HAVE are
  claims; serving is evidence. An announcer does one of two things with an object its follower
  wants: it serves it, or, lacking it, gets it, by granting it to an uploader. A follower ignores its
  announcer for `want_ttl`, as it ignores one whose excursion brought nothing (§4), when the
  announcer has done neither for an object the follower wants, whether it lists the object or
  not, so that the follower has never received one symbol of it, for longer than an honest
  announcer can take to pass an object it was asked for: its repetition ceiling (§4, eight
  `T_want_min`) and one `T_want_min` for the ask, 90 minutes at the draft, counted from when the
  follower began to follow it. Or when it lists one of the follower's own objects that nobody has
  been heard sending for that long since the follower published it. Not one symbol, because a
  follower on a channel shared with a neighbouring cell overhears that cell's symbols, which say
  nothing for its own announcer. Longer than the ceiling, because under a WANT flood an honest
  announcer passes an object only that often: with a 40-minute window, followers in a living band L
  network under one attacker left honest announcers 56 to 320 times in three days (FEASIBILITY.md
  §12). From when it began to follow, because what it waited for under another announcer, or as one,
  is no evidence against this one. Whether listed or not, because a false announcer that lists only
  what a follower cannot want yet (the objects of a manifest the follower lacks) held followers that
  wanted only that manifest for good once manifests were no longer repeated (FEASIBILITY.md §13).
  **Sooner, the follower asks for proof.** When a want the announcer has listed since the follower
  began to wait for it has stalled for `T_want_min`, the follower sends its announcer a `NACK` for
  one symbol of it that it lacks, naming the announcer as the one to answer, every
  `T_nack_stall`. (What the announcer listed only before, it may have dropped since.) An honest
  announcer answers it from the front of its carousel at once, whatever its repetition backoff;
  one that has sent not one symbol of that object `T_want_min` after the first `NACK` serves
  nothing. What it does not list it cannot answer for: a want it has named no uploader for in
  `T_excursion` gets no `NACK`, and the follower waits as long, `T_want_min`, before it leaves.
  Asked anyway, the followers of a sparse band L network sent 54,687 `NACK`s in eight worlds;
  asked only for what was listed, 73 (FEASIBILITY.md §25). Waiting alone is no
  evidence: an honest announcer whose repetitions a WANT flood holds back is silent on that
  object too, for up to its ceiling. Only the named announcer answers such a `NACK`, so a symbol
  of the object is its answer even on a channel the follower shares with other cells. The rule
  first asked only once not one `BULK` frame of anything had arrived for `T_excursion`; any frame
  of anyone else restarted that wait, and in a busy band L network a false announcer kept its
  followers for 88 minutes once they held the manifests and wanted only what it claimed
  (FEASIBILITY.md §22). Asking only after `T_excursion` let five false announcers hand followers
  on from one to the next, and playback started 21 minutes later. What an announcer does not list
  it cannot answer, so that waits `T_excursion` still: asked sooner, honest announcers that had not
  yet asked for what their followers wanted lost them, and a band O network over 15 km² had 72 %
  more role changes. An announcer that has answered a follower once has shown that it serves, and
  is asked by it again only `T_excursion` after that answer (counted from the start of the wait,
  as the simulator first did, the next `NACK` could follow every answer at once; FEASIBILITY.md
  §25). An answer proves that the announcer serves, and is not
  counted as progress on the object, so the follower asks for the rest as usual (§4): one symbol
  per answer, counted as progress, had kept a follower from asking, and it held 24 of 216 symbols
  after four hours (FEASIBILITY.md §24). A source's own object stops being pending, as in the
  upload rule of §4, when the source hears anyone send it, passes it itself as announcer, or sees
  an announcer it uploaded the object to list it, its own or a neighbouring cell's; otherwise a
  source that had announced its own objects, or uploaded them to a neighbouring cell, took the next
  honest announcer it followed for a liar (FEASIBILITY.md §13). The follower then
  follows the best other announcer it hears; hearing none, it becomes a candidate, since an area
  whose only announcer serves nothing has none. Before this rule a follower only escaped an
  announcer that served nothing by challenging it on score, which a false beacon defeats by
  claiming the maximum: five such beacons in a 15 km² band L network left 66 % of deliveries done in
  twelve hours, 60 % in the worst world; with the rule, 95 % and 87 % (FEASIBILITY.md §12), and with
  the form above 96 % and 94 % (§13 there).
  **Asking is not getting.** The rule first counted an announcer that asked for the object itself
  as one that serves. An announcer that asks and finds no holder in reach cannot get the object,
  honest or not, and its follower does better elsewhere: following the next announcer it hears or,
  hearing none, leading a cell of its own, it takes its want where other holders are, and as an
  announcer its own asks reach them. It is not a verdict that the announcer lies, and the
  simulator counts these leaves apart. In a sparse band L network (24 channels, each node
  following 2) honest announcers that asked kept their followers, and 87.6 % of bulletins arrived
  within their period (82.0 % in the worst of eight worlds); leaving them, 92.2 % (88.5 %)
  (FEASIBILITY.md §25). It is leading a cell that carries the want: leaving only for another
  announcer it heard, 83.7 %; visiting the strongest other announcer instead of leaving, 81.7 %.
  What it costs: where everybody wants the same large objects and they take longer than
  `T_excursion` to cross the network, followers leave announcers that would have had them soon:
  in a band L network over 15 km² with 846 kB objects, 19 % more frames and playback 1.9 minutes
  later, and where listeners pick single collections, 13 % more frames and a 90th percentile
  5.8 minutes later. The matrix, the living networks and the networks under attack did not
  change. And an announcer can no longer keep its followers by asking forever for what it never
  gets (ABUSE.md, "election capture").
- **Challenge on capability, not on circumstance**: a follower more capable than its announcer
  (a station back from a power cut, following the battery node that took over) for
  `challenge_beacons` consecutive beacons becomes a candidate; the incumbent hears the more
  capable beacon and yields. It does not challenge while it hears any announcer at least as
  capable as itself: it would yield to that one once it stepped up, follow the weaker one again
  and challenge it again, a cycle the simulator found in a town with three stations (127
  challenges in one day). Followers of equal capability never challenge, whatever their scores.
  A rebooted node always starts as a follower.
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
  and offers. Candidates step up there too (§5.2).

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
One mechanism, in frequency where possible and in time where necessary. Time slots apply only on
carriers the regulator does not cap: under a duty cycle or polite access the cap already bounds
what every announcer adds up to, and slots on top of it only added idle time (FEASIBILITY.md
§7.7).

EtherDiscipline's per-200 kHz accounting is unchanged: a random sequence spends about `1/n` of
the airtime in each slice. (Sequences derived by a fixed offset per announcer never coincide and
made nodes unable to find each other; the simulator caught this.)

### 5.4 Timing jitter

Every control or metadata frame on a bulk carrier waits a random `0..T_jitter` (draft 500 ms)
before transmission, beacons excepted. CCA cannot see a transmitter at the edge of range (a few
dB above sensitivity), and without jitter two nodes whose timers are both aligned to dwell
boundaries collide every single time.

Content frames are paced by the EtherFatsoen budget (ETHERFATSOEN.md), and a node that waits for
budget waits a random part of that wait again, up to half of it. The budget keeps accruing
meanwhile, so the rate stays and only the instant wanders. Paced exactly, two announcers that
cannot hear each other, once in step, stayed in step frame for frame: in a band O town a follower
that heard both lost 2533 of the 2615 frames of one object that its announcer sent it after the
first hour, and never completed two objects in twelve hours (FEASIBILITY.md §22).

### 5.5 What a listener experiences

During the outage, playback of already-collected objects continues from the schedule. New content
arrives a few minutes later than it otherwise would. That is the entire user-visible effect.

## 6. Time

### 6.1 The shared time

Schedules run on a **shared time** in milliseconds: hop dwells and the rendezvous (§5.3), the time
slots of announcers that share a channel (§5.3), the upload phases (§4), the control window (§3), and playback (§6.3).
Durations, every wait and timer of the protocol, run on each node's own clock. A node without GPS,
phone or a battery-backed clock starts counting from zero whenever it is switched on, and its
crystal runs a little fast or slow: the ESP32-S3 asks for a 40 MHz crystal within ±10 ppm (ESP32-S3
Hardware Design Guidelines, schematic checklist, "External Crystal Clock Source"), and in LoRaWAN
use the SX1262's 32 MHz reference should stay within about ±30 ppm in all conditions (SX1261/2
datasheet rev 1.2, §3.4, Table 3-4). Two free-running clocks at ±10 ppm drift 100 ms apart in about
83 minutes (2 × 10 ppm × 5,000 s; an estimate). The schedules need agreement to well within a
second: dwells last 20 s, slots 10 s, the window 4 s.

Every BEACON tells its sender's shared time as the frame begins, in milliseconds (`time`, §3.1). A
receiver adds the frame's airtime, which it knows from the length: that is the time as the frame
ends, which is when it receives it. LoRaWAN Class B devices take the time of their beacons the same
way, beacon time plus time on air (LoRaMac-node, `LoRaMacClassB.c`).

### 6.2 How nodes agree

- **A follower keeps its announcer's time**, whichever way it differs, at every beacon of it.
- **Announcers keep the latest time any of them tells**, and never go back. An announcer takes a
  later time at its own next beacon: that beacon goes out on the schedule its followers still
  keep, with the new time in it, and the cell moves at once. Among announcers this is maximum
  consensus (He, Cheng, Shi, Chen, Sun, "Time Synchronization in WSNs: A Maximum-Value-Based
  Consensus Approach", IEEE Transactions on Automatic Control 59(3), 2014): every group that hears
  itself converges to its fastest clock. Taking only later times everywhere, a follower whose
  clock ran ahead never heard back from anyone, since followers do not beacon, and kept a time of
  its own: in a band O neighbourhood started with clocks up to an hour apart, playback could start
  after 7.7 minutes instead of 6.0 (FEASIBILITY.md §26).
- **A node following nobody keeps the time of the announcer it heard last**, whom it is about to
  find. Taking only later times, a node whose clock ran ahead looked for that announcer on the
  wrong channel and led a cell of its own instead.

Two groups that do not share a time meet on the control carrier only when their windows meet, and
where the cell's carrier hops, their hop sequences differ too, and a node that knows no time finds
nobody on it. The control carrier carries the time across:

- **Every announcer tells its time on the control carrier in every window.** Another group's
  window falls on one of them now and then (§3), and an announcer that hears a later time there
  takes it at its next beacon (above): the two groups become one. Announcers used to listen
  instead through a whole period every `T_watch` (30 minutes), and sooner if they had started the
  time themselves, while their windows stayed put. Deaf on the bulk carrier for that minute, they
  cost newcomers in a band O neighbourhood 1.4 minutes more to catch up than now, and band L at a
  256 kB budget delivered 99.4 % instead of 99.9 % with twice the role changes (FEASIBILITY.md
  §29). Without either, two groups of 26 and 24 nodes in one band L neighbourhood kept times nine
  minutes apart for over an hour.
- **Where the cell's carrier hops, a node that knows no time listens on the control carrier all
  the time** until it hears a beacon there, and takes that announcer's time. One that has heard
  none steps up only after `T_acquire` of listening, two control periods and a window, the longest
  it can wait for a whole window (§3), and is then a source of time itself: it tells it at once
  on the control carrier, outside the window, where whoever else knows none is listening, and
  keeps listening there for `T_acquire`, for a source that started before it. A control period
  and its window was enough while windows stayed put; with windows that wander, a station back from
  a power cut sometimes heard no time in it and started one of its own.
- **Where the cell's carrier hops, a candidate whose time moved steps up in the next rendezvous of
  its new time**, where the other candidates and announcers are. Planned by its old time, it
  stepped up where nobody heard it, and in a town started with clocks apart three cells too many
  stayed side by side.

On a carrier that does not hop, a node that knows no time hears its cell's announcer on the cell's
channel, and with it the time, without knowing it first: it needs no acquisition there, and a
candidate no rendezvous. Its announcer still tells its time wherever a node listens on the
control carrier only in the window: neighbouring cells whose bulk carriers do not reach each other
are joined by the control carrier's longer range, and only in the windows they share. Telling only
where the carrier hops, as first specified, left band O with real clocks without that join: four in
five pairs of announcers that heard each other only on the control carrier never shared a window,
in every world and for the whole of it, and one announcer missed a channel's new root for 21 hours
(FEASIBILITY.md §28.5, §29).

**A frame keeps `T_guard` from the edges** of a hop dwell and of the control window: a receiver
whose clock is a few milliseconds off has retuned there already, or still listens on the other
carrier. Sent at the very end of the window, the beacons of a band O announcer met followers a
millisecond behind it still listening on the control carrier; they missed three in a row and stood
for election: 408 role changes in that world, 128 with the guard.

Over the whole validation, with clocks up to an hour apart and ±20 ppm, every scenario delivered
within a point of what it did with one clock; a cold start, where every node first has to learn the time, began playback
1 to 5 minutes later (a band L town after 28.6 minutes instead of 24.7), and once agreed the nodes
kept within a few milliseconds of each other (FEASIBILITY.md §26).

A false announcer can pull every announcer that hears it to a later time, as maximum consensus
lets the fastest clock lead (He et al. 2014, Remark 3.8, name the same weakness); see ABUSE.md,
"Time pulled ahead". GPS and a phone or NTP would give a better time than the mesh's own
(`time_quality` 3 and 2); how such a time takes precedence is not specified yet, because the
simulator has no node with one.

### 6.3 Playback

- Manifests schedule objects at UTC times, which nodes read on the shared time. Nodes play an
  object at its scheduled time if they have it complete; otherwise they skip it (or play the
  previous complete object in the channel, a channel-level option). A shared time that came from
  no GPS or phone is not UTC: in such a mesh a schedule is only as right as the clock that happened
  to lead (open question 6).
- A device that cannot decode asks for the rendition of an object `T_render_ahead` before its
  slot (§1.2), so a radio fetches what is on next and nothing else.
- Accuracy needed: seconds, not milliseconds. Two neighbours playing the same track one second
  apart is acceptable; one minute apart is not.

## 7. Security model (summary)

- Authenticity: manifests are signed by the channel key; a node never plays or schedules an
  object that is not referenced by a valid manifest of a followed channel.
- Integrity: full object hash; v1 verifies chunk by chunk against the object's BLAKE3 tree (§1).
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
- What a node keeps of what it hears is bounded. It keeps what it can use only: an ask for an
  object it cannot name, by id or as a rendition it knows of, is not recorded by a carousel, a
  follower notes only those of its announcer's asks that concern objects it knows, and only asks
  for objects it can name, or for a collection manifest a set names, count towards relaying.
  Tables keyed by names nobody checks have caps (§8) and give way evidence first: a name heard
  once before one heard twice, then the oldest. What a node carries is what it follows, and for
  others no more than its carry budget (§4).
- Node ids are not authenticated either, and anyone can build a node that keeps none of these
  rules. What such a node can do beyond requests (poisoned symbols, names it does not own, time,
  ignoring the regulations, malformed frames, firmware images, what listeners give away) and what
  the design must do about each is in ABUSE.md, "Someone else's firmware".

## 8. Parameters (draft, to be tuned in simulation)

| Name | Draft | Meaning |
|---|---|---|
| `T` | 200 B | symbol size |
| `K_max` | 1024 | symbols per source block |
| `T_beacon` | 60 s | beacon interval on the bulk carrier |
| `N_miss` | 3 | missed beacons before switching or election |
| `T_base`, election jitter | 60 s, 10 s | together the span of a candidate's wait on a carrier that does not hop |
| `H` | 10 % of `score_max` | yield hysteresis between announcers of equal capability |
| `challenge_beacons` | 3 | beacons from a less capable announcer before a follower challenges it |
| step-up order | span in 4 capability bands; in a band, 2/3 by score + 1/3 jitter | span `T_base + T_jitter` from the candidacy, or 7/10 of the meeting dwell after its first fifth on a hopping carrier |
| `T_excursion` | 40 min | a want without a symbol, and without a grant by our announcer, this long sends a follower to another announcer that has it; a visit without a symbol this long ends, and one that brought none is not repeated for `want_ttl`; with nobody to visit, and one `T_want_min` more, it makes the follower leave its announcer (§5.2); and what was asked for and neither granted nor arriving this long is stuck, and asked for last (§4) |
| `rssi_hysteresis` | 6 dB | a follower switches announcer only for a clearly stronger one |
| `near_rssi` | sensitivity + 17 dB | beacon strength that means "same cell" for the tie-break |
| `max_passes` | 1 | carousel passes per object unless re-wanted |
| repetition spacing | 0, then `T_want_min` × 1, 2, 4, 8 | wait before an object is passed again; the level climbs with each repetition and resets after a rest of twice the wait |
| `T_nack_stall` | 60 s | no progress on a nearly complete object (≥ 80 %, or all but one symbol) before a NACK |
| `T_want_min` | 10 min | minimum interval between a follower's WANT frames, except an ask for what a new manifest names (§4); also how long a want its announcer lists may bring nothing before the follower asks it for proof (§5.2) |
| `T_relay_wait` | 10 min | what a node expects of other cells' asks before it has heard any: met half the time within it, never after (§4) |
| `relay_risk` | 5 % | a node relays another cell's ask once fewer than this share of the asks of its age were met by others before twice that age, as it learned from the asks it heard (§4) |
| `carry_budget` | per device | bytes a node keeps for others, beyond what it listens to; what has not been of use for `want_ttl` gives way, the menu first, then relays another cell's announcer lists, then the least recently used, and a full budget takes on no more relays (§4). The simulator's default is no limit |
| `cell_keep` | 24 h | an announcer serves a channel this long after a follower of its cell last asked for anything of it (§2) |
| `T_gossip`, `T_gossip_min` | 5 min, 30 s | announcer/source gossip cadence and its floor |
| `control_reserve` | 10 % | share of the band budget kept free for control frames |
| own share | `min(regulatory, occ_high_own / (announcers heard + 1))` | content pacing ceiling; derived, not configured |
| `T_dwell` | 20 s | hop dwell on frequency-agile carriers |
| `meet_every` | 5 | every fifth dwell is on the common control-plane sequence |
| `T_ctrl_period`, `T_ctrl_window` | 60 s, 4 s | the control window: control-carrier frames go only in it, and a radio shared with a sub-GHz bulk carrier listens on the control carrier only then; where it falls in each period follows from the period's number (§3) |
| `T_acquire` | 124 s | where the cell's carrier hops: how long a node that knows no shared time listens on the control carrier before it may step up by its own clock, two control periods and a window, the longest it can wait for a whole window (§3, §6) |
| `T_guard` | 50 ms | how far a frame keeps from the edges of a hop dwell and of the control window, for clocks a few milliseconds apart (§6) |
| `T_offer` | 0–3 s | random delay before a holder offers on an open ask |
| repair wait | `T_suppress × (neighbours heard better than the asker) / (all neighbours)` + jitter | ungranted NACK answer |
| `T_upload_phase` | 1 s | one upload phase: uploaders to one announcer take turns this long each |
| `T_render_ahead` | 30 min | a device that cannot decode asks for a rendition this long before its slot |
| `T_grant` | 10 min | a grant lapses this long after its last symbol (or after the grant, if none came) |
| `T_slot` | 10 s | time slot when announcers in conflict share a channel |
| `conflict_ttl`, `T_report_min` | 30 min, 60 s | conflict report lifetime and follower report rate limit |
| `T_jitter` (tx) | 0–500 ms | random delay before control/metadata frames |
| repair overhead (v1) | 10 % | RaptorQ repair symbols per block |
| gossip cap | 12 have + 12 want | per frame |
| `max_neighbours` | 256 | neighbours a node keeps; a name heard once gives way first, then the one heard longest ago (§7) |
| `max_offered_ids` | 8192 | ids offered by all neighbours together; what the neighbour heard longest ago offered goes first |
| `max_conflicts` | 64 | announcers reported in conflict; the report heard longest ago goes first |
| `max_askers_per_object` | 32 | askers a carousel keeps per object for its order; the one that asked longest ago goes first |
| `max_relay_asks` | 1024 | other cells' asks a node keeps to relay (§4); one heard once gives way first, then the one heard longest ago |

## 9. Open questions

1. Symbol size versus LoRa airtime: 200 B is right for GFSK; should LoRa-only cells use 64 B?
2. Should the carousel prioritise by schedule proximity (what plays soonest)? It now orders by
   listeners served per byte (§4); a playback deadline could weight that, but has not been
   needed yet.
3. Per-symbol authentication in v0 rather than v1, given that anyone can inject BULK frames?
4. Multi-announcer cells on purpose (two bulk channels, two announcers) in dense areas?
5. How does a node learn a channel id in the first place without internet? (QR code, spoken
   over the mesh in a "directory" channel that every node follows by default, or both.)
6. Time without GPS or phone in a fully offline mesh: the shared time of §6 keeps schedules
   together, but it is not UTC. How a GPS or phone time takes precedence, and what a mesh without
   one does with a manifest's UTC schedule, is open.
7. *(resolved in Phase 0: any holder answers any announcer's WANT, with suppression; see §4.)*
8. Nodes with two bulk carriers (GFSK and ESP-NOW) run two elections; the simulator models one
   bulk carrier per node so far.
9. zsync-style delta transfer for updated objects (web bundles, firmware): the receiver compares
   the block lists of the old and new object and wants only the changed symbols.
10. Fountain coding would remove the 62 % of received symbols that are duplicates, but the
    experiment (FEASIBILITY.md §7.7.1) broke cells with several announcers. The first guess, that
    announcers fill up by overhearing and so can never complete, was measured and is wrong: on
    main 57 to 100 % of what an announcer receives arrives on request, and it holds every object.
    Measured apart, fountain coding alone is equal where one announcer serves a cell and worse
    where several do, and the comparison corrected the premise: the duplicates come from a node
    hearing several announcers send the *same* symbols, not from a carousel repeating itself
    (FEASIBILITY.md §7.7.1). Not merged. Next hypothesis: a symbol range per announcer, so that
    two carousels a node hears are never redundant.
11. Cross-cell fetching among cells that cannot hear each other is the weakest part of the
    protocol, and the Phase 0 answer is mixed. Conflict colouring with granted uploads took band O
    at town scale from a 10.5-hour to a 7.3-hour median at the same complete delivery, and the
    ESP-NOW neighbourhood from 78 % to 86–91 %; but it took band L at town scale from 100 % in
    11.4 hours to 66 % in 24 hours, because random hopping over fifteen channels was already good
    and the grant round-trips at meeting-dwell cadence are not (FEASIBILITY.md §7.5). Serialised
    and turn-taking uploads were tried and rejected. Next: measure where the band L town path
    stalls (grant latency, coinciding upload channels, or one-object-per-holder), and find a rule
    that is cheap where channels are plentiful without being unsafe where they are scarce.
    After the codec change the cause was found with an ensemble and a want-list trace
    (FEASIBILITY.md §9): holders sent offers for another cell's announcer at once, on their own
    hop sequence, where that announcer never listens; and a grant whose uploader had delivered
    once never lapsed, so an announcer kept naming a holder that had gone quiet. Offers now wait
    for the rendezvous and grants lapse without progress; the band L neighbourhood went from
    95–99 % on average (one seed at 66 %) to 100 % on every seed.
    Once elections settled cleanly (§5.2), it turned out that some of this had been carried by
    churn: followers that changed announcer took objects with them. With stable cells the paths
    across were named one by one, by the kind of pair that joins two cells, and each got a rule
    (§4, "Content crosses wherever a link does"; FEASIBILITY.md §12). Still open: two cells joined
    only by their two announcers hearing each other. An announcer that uploaded to another
    announcer as a last resort would close it, at the cost of leaving its own channel; no
    simulated world has needed it yet.
12. *(resolved: renditions, on demand and for the last hop; §1.2.)* A device that cannot run the
    neural decoder, such as a LilyGo T-Deck Pro, can play Opus but not SNAC. Carrying Opus
    alongside every programme was measured: with half the programmes also as Opus the network
    spends three to six times the airtime (FEASIBILITY.md §9.5). So Opus never travels with the
    codes. A device that needs it asks for the rendition of what it is about to play; a node in
    its cell that can decode makes it, checked against the id the source signed, and the cell's
    carousel carries it once to whoever asked (FEASIBILITY.md §10).
13. **Collections: albums, series, episodes.** Settled in §2: a provider's channel has one signed
    root manifest that names its collections (album, series, singles; a station is a series with
    a schedule), each with a collection manifest that lists its pieces in order and a cover of
    content type 6 (JPEG); a node follows a whole channel or single collections. The second level
    is nearly free where it matters: a root brings the collection manifests new in it in the same
    round of asking (§4), a holder uploads manifests first, and a follower asks for what a new
    manifest names at once (§4). What it still costs, a few tenths of a minute
    across networks of many cells, is in FEASIBILITY.md §16. Size needs no rule (FEASIBILITY.md
    §13): the carousel serves the most listeners per byte first, so a large object only arrives
    later, and abuse through size is the channel flood and store exhaustion of ABUSE.md. Retention
    stays the provider's choice: what leaves a collection leaves the mesh in time. How a player
    uses collections (follow the schedule like radio, newest first, in order, shuffled, move on
    to the next episode) is the app's business; the network delivers in the order the collection
    manifest lists, the earlier place first (§2, §4), and listeners in a band O town could start
    playing a programme 7.9 minutes sooner than when it went by size (FEASIBILITY.md §19). Open:
    the `integrity` slot of a piece (reserved, §1).
14. **A menu that scales: how everyone learns what is on the mesh.** Partly settled in §2 and
    §3.4: an announcer serves what its cell listens to and publishes, not every channel it hears
    of, and announces the roots it adopted last first. With 400 channels nobody followed beside
    the 24 that were, a sparse band L network delivered 98.9 % of its bulletins over 24 worlds,
    against 99.8 % without them and 88.8 % when every announcer served everything; with 1000,
    97.5 % against 59.3 % (FEASIBILITY.md §30). It still delivers less than without them, and every
    node still keeps every root that comes by. What is left is a direction, to be specified here
    before it is simulated, in three layers (sources in FEASIBILITY.md §30 and PRIOR-ART.md):
    - *What a node follows* stays on the device and is never sent: receivers do not transmit.
    - *What is new* on what a cell follows: small heads (channel, seq, root id prefix) repeated in
      tiers under a fixed share of the control budget, what the cell follows and what changed
      lately often, the rest seldom, as SAP (RFC 2974) and DVB service information do. Between
      announcers, reconciling sets (rateless IBLT, PinSketch) instead of rotating lists, if
      rotation proves too slow.
    - *What exists*: no directory held by the project and no project key. A provider describes
      its channel when it publishes, in its signed root and in a fixed taxonomy (kind: music,
      podcast, radio, speech; then genre, language and the like), and a node's guide is the merge
      of the self-descriptions its neighbours carry, ordered by that taxonomy and by how widely a
      channel is followed. Nobody can edit another's description; a listener can order and hide
      locally, and a curated list is a channel like any other, found the same way or by QR code.
    Open: the taxonomy and its encoding, the heads budget at tens of thousands of channels, and
    how far popularity can rank without inviting a flood of self-promoting channels (ABUSE.md).
