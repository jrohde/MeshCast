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
  all is tracked by count alone, as every large object is.
- **Integrity**: v0 verifies the full object hash on completion and discards the object on
  mismatch. v1 option: a Merkle root over symbol hashes in the manifest so a poisoned symbol can be
  rejected on arrival (costs 4 bytes per symbol in the manifest).

### 1.1 Content types and the two audio codecs

Every object has a one-byte **content type**, carried in the manifest entry that lists it
(§2). Frames never carry it: a BULK frame names an object by short id, and a node that wants the
object already has the manifest that says what it is.

| Code | Name | MIME | Content |
|---|---|---|---|
| 1 | manifest | `application/meshcast-manifest` | a channel manifest (§2) |
| 2 | text | `text/plain; charset=utf-8` | text pages |
| 3 | firmware | `application/octet-stream` | firmware images |
| 4 | renditions | `application/meshcast-renditions` | a channel's rendition table (§1.2) |
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
  wants one for itself. If it can make it, it does; otherwise it asks, in its own cell and in the
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

## 2. Channels and manifests

A **channel** is an Ed25519 public key. Its **channel id** is the first 8 bytes of BLAKE3(pubkey).

A **manifest** is an object of MIME `application/meshcast-manifest` containing, CBOR-encoded:

| Field | Type | Meaning |
|---|---|---|
| `chan` | 32 B | channel public key |
| `seq` | u32 | monotonically increasing; a node keeps only the highest valid seq per channel |
| `title`, `desc` | text | channel metadata |
| `objects` | list of {`id` 32 B, `len` u32, `kind` u8 content type (§1.1), `title`, `blocks` u16, `enc` bool} | the channel's catalogue (or a window of it) |
| `schedule` | list of {`id` 8 B, `start` u64 UTC seconds, `repeat` optional} | when to play what |
| `prev` | 32 B optional | id of the previous manifest, for history |
| `renditions` | {`id` 32 B, `len` u32} optional | the channel's rendition table (§1.2) |
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
| 1 | 1 | flags: bulk carrier kind (3 bits) |
| 2 | 4 | `announcer_id` (first 4 bytes of the node's public key hash) |
| 6 | 2 | `score` (see §5) |
| 8 | 2 | `next_ms` — milliseconds until the next beacon from this announcer |
| 10 | 2 | `round` — carousel round counter |
| 12 | 8 | `utc` — UTC seconds if known, else 0 |
| 20 | 1 | `time_quality` — 0 none, 1 mesh-derived, 2 phone/NTP, 3 GPS |
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
| 1 | 1 | flags: `n_heard` |
| 2 | 4 | `node_id` |
| 6 | 4 | `announcer_id` — the announcer this node currently follows (its own id if it announces, 0 if none heard) |
| 10 | 1 | `announcer_colour` — that announcer's colour, so holders can reach it without having heard its beacon |
| 11 | 1 | `announcer_colours` — the number of colours in its cycle |
| 12 | 1 | `n_have`, complete objects listed |
| 13 | 1 | `n_want` |
| 14 | 6 × n_heard | other announcers this node hears: id (4), colour (1), colours (1); the conflict report (§5) |
| … | 8 × n_have | short ids the node has completely |
| … | 13 × n_want | wants: short id (8), granted holder (4; 0 = open ask), upload phase of the grant (1; §4) |
| … | 2 | CRC-16 |

Draft cap: 3 heard + 12 have + 8 want per frame, 234 bytes. For larger libraries a node rotates through
its list across gossip rounds, most recently completed and most wanted first. Followers that are
not sources send GOSSIP only when they have something new to offer that the announcer lacks (the
"upload" case) or, rarely, a WANT for an object the announcer has never included; the default is
silence.

### 3.4 `MANIFEST_ANNOUNCE`

`channel_id` (8) + `manifest_short_id` (8) + `seq` (4), repeated up to 12 times, + CRC.

### 3.5 `NACK` (v0 repair)

| Offset | Size | Field |
|---|---|---|
| 1 | 1 | flags: the upload phase answers use (low 4 bits; §4), set by an announcer |
| 2 | 4 | `node_id` of the asker |
| 6 | 8 | `object_short_id` |
| 14 | 2 | `block` |
| 16 | 4 | `answerer` — the holder an announcer names to answer (0: any holder, after a wait) |
| 20 | 1 | `n_ranges` |
| 21 | 4 × n_ranges | missing source symbols as (first `esi` u16, count u16) runs |
| … | 2 | CRC-16 |

Draft cap 40 ranges, 183 bytes. Sent by a node that is nearly complete on an object (draft 80 %)
and has seen no progress for `T_nack_stall`. A follower's NACK goes to its announcer, whose
carousel puts the missing symbols at the front of the next round. An announcer's NACK names
who answers, and in which phase: its granted uploader if the object has one, otherwise the holder
of the object it hears best (holders say what they have in GOSSIP HAVE). The named holder answers
at once; only if the announcer knows no holder does any holder answer, after a wait. This is the whole repair mechanism in v0; it costs one small control frame per object
per asker at most, which is negligible next to the object itself.

v1 replaces most NACKs with RaptorQ repair symbols (`esi >= K`) generated by the announcer at a
configurable overhead (draft 10 %), so that receivers that missed any `≤ 10 %` of a block recover
without transmitting anything.

## 4. Carousel

The announcer maintains a **carousel set**: every object (including manifests) that a follower in
the cell wants, as learned from GOSSIP and MANIFEST_ANNOUNCE, that the announcer has. A round is:

1. `BEACON` on the bulk carrier.
2. For each object in the set, manifests first and then **the most listeners served per byte**:
   emit its symbols, one `BULK` frame each, subject to EtherFatsoen and EtherDiscipline gating
   between frames. The listeners of an object are the followers asking for it; ordering by
   listeners divided by size is Smith's rule, which minimises the total time listeners wait on
   one shared transmitter. Among objects of one size it is simply most-wanted first; a small
   object no longer waits behind a large one; and every wanted object is still sent every round,
   so nothing starves.
3. Merge NACKs received during the round; symbols named in NACKs are queued at the front of the
   next round.
4. Objects that every heard follower reports complete leave the set.

The round never waits for anyone. A follower that joins mid-round starts collecting and completes
the object next round. Symbols are idempotent, so a slow announcer simply takes more rounds.

**One pass, then repair.** Followers never report HAVE, so the announcer cannot know when
everyone is done. Each object gets one full pass (`max_passes` = 1) and then leaves the carousel
unless a new WANT arrives after that pass: a follower that caught most of it repairs the rest
with a NACK, one that caught little asks again. Three passes per object, the earlier draft, cost
35 to 49 % more airtime in every scenario and bought no speed; the repair does the work of the
repeated passes, aimed (FEASIBILITY.md §9.8). Manifests are repeated at most every `T_always`
(draft 5 min) when nothing else is wanted. A carousel with nothing to send is silent; the
announcer then only beacons. (The first simulator runs looped manifests forever at the full duty
cycle, which wasted the budget and caused half-duplex losses during uploads.)

**A want is served by the announcer it names.** A follower's WANT names the announcer it
follows (§3.3). Other announcers that overhear it do not serve it: two announcers answering the
same WANT start the same pass at the same instant, and where they cannot hear each other every
frame of both collides at the follower that asked. In one simulated world a follower caught 27
of an object's 216 symbols in twelve hours that way (FEASIBILITY.md §9.8). An announcer still
hears the WANTs, offers and conflict reports of other cells; it only leaves their wants to their
own announcers.

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

**The receiver divides its listening time.** Holders on opposite sides of a cell cannot hear
each other, so carrier sensing cannot make them take turns, and their uploads collide at the
announcer. The announcer, the only one that can tell, divides its listening time among those it
asks to speak. Each grant carries a phase: the lowest one no running grant uses, in the WANT
entry that names the holder. The announcer's beacon carries `upload_phases`, K = the highest
phase in use + 1. A phase lasts `T_upload_phase` (1 s, one permitted transmission under polite
access), and an uploader transmits only in its own phase of each cycle of K phases; an uploader
that has not yet heard the new K after a grant uses its phase + 1. One running upload has K = 1
and all the time. K follows the number of running uploads, so as many may run at once as before,
and none overlaps another. When a grant ends its phase is free for the next one, and K shrinks
once the highest phase is released. An announcer grants at most 16 uploads at once; a holder
that offers when all phases are taken is granted on a later WANT. The cost is one byte in the
beacon and one per WANT entry. Hashing object and holder to a phase instead needed K = 16 far too
often (the birthday problem), and fixed phases cut the airtime but made a bulletin 50 % slower;
FEASIBILITY.md §9.6.

An announcer divides its listening time only under polite access, and announces K = 1 elsewhere.
Under polite access every transmission is at most `Ton_max` and followed by a pause, so an upload
is spread over minutes and hidden uploaders overlap. Under a duty cycle budgeted per hour, or on a
carrier without a limit, an upload is a burst of seconds at the full rate that rarely meets
another; holding it to one phase in K made it K times slower (a median upload of 39 s instead of
4 s in band O) and the bulletin a third to two fifths slower. This is the rule for time slots
(§5.3) turned around: slots are for carriers the regulator does not cap, phases for the one where
its cap makes uploads long.

**The announcer keeps quiet in the phases it gave away.** A radio that transmits cannot receive,
and carrier sensing does not stop an announcer from talking over an uploader it can decode but
hears below the clear-channel threshold (15 dB above sensitivity, ETSI EN 300 220-2 Table 18).
So an announcer holds its carousel content during a phase whose uploader it has heard in the last
two cycles; control frames still go. In the ring smoke test a quarter of the upload frames had
arrived while the station was transmitting; afterwards 1.7 %, and 43 % fewer upload frames were
needed; FEASIBILITY.md §9.7.

**Every upload to an announcer runs in a phase the announcer named, by the holder it named.** A
grant names both in the WANT. An announcer's NACK names both too (§3.5): the granted uploader and
its phase if the object has one, otherwise the holder it hears best and a phase reserved for
repairs of that object until it completes or `T_grant` passes without a symbol. Answers that
nobody named, from holders that cannot hear each other, were nearly all the collisions left in
band L after grants had phases; FEASIBILITY.md §9.6.

**A grant ends with the announcer's role.** Grants belong to the announcer role: a node that
stops announcing drops them, and it never names a holder in the WANT it sends as a follower. A
holder stops every upload to a node, and forgets that node's grants, as soon as it hears that
node say it follows someone else (any GOSSIP whose `announcer_id` is not its sender). Before this
rule, a third of all upload frames in a living band L network went to nodes that had stopped
announcing; FEASIBILITY.md §9.6.

**Ask only for what is not coming.** An announcer's WANT lists objects that have received no
symbol for `T_nack_stall`; an object whose symbols are arriving is not asked for again, and a
holder whose granted upload is flowing is not asked for a second object until it is done.
**Ask first for the most listeners per byte**, the carousel's rule applied one step earlier: a
holder uploads one object at a time, so the order of asking is the order of arriving, and a
3-minute track must not wait behind a 540 kB object from the same source. (Asking in object-id
order, as Phase 0 did, delayed small objects in mixed traffic about threefold.)
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
- A device that cannot decode asks for the rendition of an object `T_render_ahead` before its
  slot (§1.2), so a radio fetches what is on next and nothing else.
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
| `max_passes` | 1 | carousel passes per object unless re-wanted |
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
| `T_upload_phase` | 1 s | one upload phase: uploaders to one announcer take turns this long each |
| `T_render_ahead` | 30 min | a device that cannot decode asks for a rendition this long before its slot |
| `T_grant` | 10 min | a grant lapses this long after its last symbol (or after the grant, if none came) |
| `T_slot` | 10 s | time slot when announcers in conflict share a channel |
| `conflict_ttl`, `T_report_min` | 30 min, 60 s | conflict report lifetime and follower report rate limit |
| `T_jitter` (tx) | 0–500 ms | random delay before control/metadata frames |
| repair overhead (v1) | 10 % | RaptorQ repair symbols per block |
| gossip cap | 12 have + 12 want | per frame |

## 9. Open questions

1. Symbol size versus LoRa airtime: 200 B is right for GFSK; should LoRa-only cells use 64 B?
2. Should the carousel prioritise by schedule proximity (what plays soonest)? It now orders by
   listeners served per byte (§4); a playback deadline could weight that, but has not been
   needed yet.
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
12. *(resolved: renditions, on demand and for the last hop; §1.2.)* A device that cannot run the
    neural decoder, such as a LilyGo T-Deck Pro, can play Opus but not SNAC. Carrying Opus
    alongside every programme was measured: with half the programmes also as Opus the network
    spends three to six times the airtime (FEASIBILITY.md §9.5). So Opus never travels with the
    codes. A device that needs it asks for the rendition of what it is about to play; a node in
    its cell that can decode makes it, checked against the id the source signed, and the cell's
    carousel carries it once to whoever asked (FEASIBILITY.md §10).
