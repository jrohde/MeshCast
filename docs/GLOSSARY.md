# Glossary

**Announcer.** The one node in a cell that transmits the carousel. Elected automatically, by
capability first and score second;
every node runs the same protocol and any node can become announcer. See PROTOCOL.md §5.

**Beacon.** The announcer's heartbeat frame: identity, capability, score, time, spectrum weather,
and when the next beacon comes. Its absence triggers an election.

**Bridge node.** A node that hears into a neighbouring cell and so lets content cross: it offers
what that cell's announcer asks for, relays what that cell's listeners lack, goes on an excursion
to it, or repairs from a holder there whose uploads it overheard (PROTOCOL.md §4).

**Capability.** What a node is, as opposed to what it experiences in its role: mains power and an
internet uplink, carried in every beacon. Elections compare it before the score; a follower
challenges its announcer only on capability (PROTOCOL.md §5.1).

**Bulk carrier.** A carrier used for content symbols: GFSK on sub-GHz, ESP-NOW LR, IP, SD.

**Carousel.** The announcer's endless loop over the objects its cell wants, rarest-first. A
receiver joining at any time eventually has everything. From DVB/FLUTE practice.

**Carrier.** A physical way to move frames: sub-GHz GFSK, sub-GHz LoRa, ESP-NOW LR (2.4 GHz),
IP, SD card. MeshCast is carrier-agnostic.

**CCA (clear channel assessment).** Listening on the channel before transmitting and deferring if
it is busy. A legal requirement in polite regimes; always done in MeshCast.

**Cell.** The set of nodes that hear the same announcer on a given carrier. Cells are per carrier
and emerge from radio reach; nobody defines them.

**Channel.** A publishing identity: an Ed25519 key pair, one per provider. The owner signs its
root manifest; followers store the public key. Not to be confused with a radio channel.

**Codes.** The integers a neural codec turns audio into and back, 12 bits each for SNAC. Audio
objects carry codes, not a waveform; only the device that plays them decodes (PROTOCOL.md §1.1).
Also called tokens.

**Collection.** What a provider publishes under its channel: an album (pieces in order), a series
(episodes that come and go; with a schedule, a station) or singles. A node follows a whole channel
or single collections of it (PROTOCOL.md §2).

**Content type.** One byte in a manifest entry that says what an object is: root or collection
manifest, text, firmware, speech (SNAC 24 kHz), music (SNAC 32 kHz), a cover image (PROTOCOL.md
§1.1).

**Control carrier.** The LoRa channel that carries manifest announcements between cells: long
range, tiny throughput. Beacons, gossip and everything else cell-local travel on the bulk
carrier (PROTOCOL.md §3).

**Cover.** A collection's image, a JPEG object named in the root manifest (PROTOCOL.md §1.1, §2).

**Duty cycle.** The fraction of time a transmitter may be on, per hour, in regimes that use it
(e.g. 10 % in EU band O).

**Edge node.** An ESP32-S3 + SX1262 board: dongle, Heltec, LilyGo.

**EtherDiscipline.** Dutch loanword, "ether discipline": legal limits per region, encoded as a
profile and enforced in firmware. See ETHERDISCIPLINE.md.

**EtherFatsoen.** Dutch loanword, "ether decency": the spectrum-etiquette layer, how nodes share
the air politely without a coordinator. See ETHERFATSOEN.md.

**Excursion.** A follower following another cell's announcer for a while, for an object its own
cell cannot get, and then coming back to upload it there (PROTOCOL.md §4).

**Follower.** A node that follows at least one channel and therefore wants objects. Followers
never transmit unless they have something the announcer lacks.

**Gossip.** Small HAVE/WANT frames by which sources and the announcer learn what exists and what
is needed. Followers stay silent by default.

**Manifest.** An object that lists other objects. A channel's *root manifest* is its signed index
of collections; a *collection manifest* lists one collection's pieces and schedule, and is as
authentic as the root that names it by its full hash. A node believes a root it has checked
(*adopted*); one it has only heard announced is something to fetch (PROTOCOL.md §2).

**Object.** An immutable, hash-identified blob: track, bulletin, manifest, firmware image, page.

**Place.** A piece's position in the list of its collection manifest, the order a listener plays
it in; 0 for every piece of singles and for anything that is not a piece. Asking and uploading go
by it, the earlier place first (PROTOCOL.md §2, §4).

**Polite spectrum access.** The ETSI alternative to duty cycle: CCA, max 1 s on, 100 ms off,
100 s per hour per 200 kHz, more with frequency agility (AFA).

**Repair symbol.** A RaptorQ-encoded symbol (v1) that lets a receiver reconstruct a block from any
sufficiently large subset of symbols.

**Score.** A node's self-computed suitability to be announcer: neighbours heard, mains power,
unused airtime budget, library size, internet uplink. Only comparable between nodes in the same
role; see Capability.

**Set.** A HAVE, WANT or grant for many pieces of one collection at once: its collection
manifest's short id and a bitmap over its list. Used where a round of asking is dear
(PROTOCOL.md §3.3, §4).

**Short id.** The first 8 bytes of an object's BLAKE3 hash, used on the air.

**SNAC.** The neural audio codec MeshCast uses: the 24 kHz model for speech, the 32 kHz model for
music, each pinned to exact weights.

**Source.** A node that has an object the announcer does not yet have; it uploads by gossiping
HAVE and then sending symbols.

**Spectrum weather.** Measured channel occupancy per bulk channel, carried in beacons as shared
observation so nodes can choose quiet channels and hours.

**Station.** A Linux node, typically an upcycled Helium miner with an SX1302; not a role, just a
node that usually wins the election.

**Symbol.** A fixed-size piece of an object (draft 200 bytes), one per bulk frame.
