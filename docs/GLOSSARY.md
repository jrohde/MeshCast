# Glossary

**Announcer.** The one node in a cell that transmits the carousel. Elected automatically by score;
every node runs the same protocol and any node can become announcer. See PROTOCOL.md §5.

**Beacon.** The announcer's heartbeat frame: identity, score, time, spectrum weather, and when the
next beacon comes. Its absence triggers an election.

**Bridge node.** A node that hears the announcers of two cells and therefore lets content cross
between them via its HAVE gossip.

**Bulk carrier.** A carrier used for content symbols: GFSK on sub-GHz, ESP-NOW LR, IP, SD.

**Carousel.** The announcer's endless loop over the objects its cell wants, rarest-first. A
receiver joining at any time eventually has everything. From DVB/FLUTE practice.

**Carrier.** A physical way to move frames: sub-GHz GFSK, sub-GHz LoRa, ESP-NOW LR (2.4 GHz),
IP, SD card. MeshCast is carrier-agnostic.

**CCA (clear channel assessment).** Listening on the channel before transmitting and deferring if
it is busy. A legal requirement in polite regimes; always done in MeshCast.

**Cell.** The set of nodes that hear the same announcer on a given carrier. Cells are per carrier
and emerge from radio reach; nobody defines them.

**Channel.** A publishing identity: an Ed25519 key pair. The owner signs manifests; followers
store the public key. Not to be confused with a radio channel.

**Codes.** The integers a neural codec turns audio into and back, 12 bits each for SNAC. Audio
objects carry codes, not a waveform; only the device that plays them decodes (PROTOCOL.md §1.1).
Also called tokens.

**Content type.** One byte in a manifest entry that says what an object is: manifest, text,
firmware, speech (SNAC 24 kHz) or music (SNAC 32 kHz). PROTOCOL.md §1.1.

**Control carrier.** The LoRa channel used for beacons, gossip and manifest announcements:
long range, tiny throughput.

**Duty cycle.** The fraction of time a transmitter may be on, per hour, in regimes that use it
(e.g. 10 % in EU band O).

**Edge node.** An ESP32-S3 + SX1262 board: dongle, Heltec, LilyGo.

**EtherDiscipline.** Dutch loanword, "ether discipline": legal limits per region, encoded as a
profile and enforced in firmware. See ETHERDISCIPLINE.md.

**EtherFatsoen.** Dutch loanword, "ether decency": the spectrum-etiquette layer, how nodes share
the air politely without a coordinator. See ETHERFATSOEN.md.

**Follower.** A node that follows at least one channel and therefore wants objects. Followers
never transmit unless they have something the announcer lacks.

**Gossip.** Small HAVE/WANT frames by which sources and the announcer learn what exists and what
is needed. Followers stay silent by default.

**Manifest.** A signed catalogue and schedule for a channel; itself an object.

**Object.** An immutable, hash-identified blob: track, bulletin, manifest, firmware image, page.

**Polite spectrum access.** The ETSI alternative to duty cycle: CCA, max 1 s on, 100 ms off,
100 s per hour per 200 kHz, more with frequency agility (AFA).

**Repair symbol.** A RaptorQ-encoded symbol (v1) that lets a receiver reconstruct a block from any
sufficiently large subset of symbols.

**Score.** A node's self-computed suitability to be announcer: neighbours heard, mains power,
unused airtime budget, library size, internet uplink.

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
