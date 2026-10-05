# Spam, flooding and other abuse

MeshCast is robust where it signs and hashes, and weak where it trusts a request. This page is
the honest inventory: what an attacker can do today, how much it costs them, how much it costs
us, and which of it is fixed. It is written from the attacker's side on purpose; a threat you
cannot state precisely you cannot defend against.

The measure that matters is **amplification**: how many bytes of other people's airtime one byte
of yours can spend. Anything above one is a lever.

## What already holds

- **You cannot forge a channel.** A manifest is signed by the channel key; without it you cannot
  add, remove or reorder anything in someone's channel, and a manifest with a lower sequence
  number than the one a node holds is ignored.
- **You cannot forge a collection.** A collection manifest is not signed: the root manifest
  names it by its full hash, so a node reads one only once the adopted, signed root names it,
  and a collection manifest that no adopted root names is an unknown object like any other.
- **You cannot poison content.** Objects are named by their hash. A symbol that does not fit is
  discarded when the object completes, the object is dropped and collected again. You cannot
  change what people hear this way, but you can keep it from arriving (Someone else's firmware,
  below).
- **You cannot make the audience transmit for you.** Followers never transmit unless they hold
  something an announcer asked for, so there is no reflection through the crowd. They do fetch in
  their own cell what another cell asks for its listeners and nobody met: the relay ask below,
  bounded by each node's carry budget.
- **You cannot replay.** Manifest sequence numbers only go up, and only a signed manifest moves a
  node's sequence number; an announced one does not (below).

## What does not hold

| Attack | What you send | What it costs us | Amplification |
|---|---|---|---|
| **Channel flood** | Many signed channels with large catalogues | An announcer keeps current the root and collection manifests of the channels its cell asks for, publishes or follows, and every node, announcers too, keeps what comes by of the others (PROTOCOL.md §2); pieces and covers an announcer fetches only when a follower asks for them | At every node its carry budget; a follower, or a node posing as one, makes its announcer serve a channel for `cell_keep` by asking for it. Content only on a listener's ask, which is the WANT flood. Before, an announcer kept every channel it heard of current, and 400 channels nobody followed brought a sparse band L network from 99.8 % to 88.8 % delivered (FEASIBILITY.md §30); before that, it fetched every piece of every channel it heard of |
| **WANT flood** | One 50-byte gossip asking for an object, or one 23-byte set asking for up to 64 pieces where sets are used (PROTOCOL.md §3.3) | The announcer puts a 42 kB track in its carousel (540 kB before the codec change), or every piece of a collection | ~800× per object; a set asks for more per frame, but the repetition backoff still applies per piece |
| **Rendition flood** | WANTs, as a device that cannot decode, for the rendition of every object a channel lists | The cell's carousel carries each as Opus (PROTOCOL.md §1.2): 367 kB for a 3-minute song at 16 kbit/s | ~7 000× |
| **NACK amplification** | One 30-byte NACK, claiming to be an announcer, or naming a holder as a follower whose announcer cannot repair | The holder named (or, unnamed, the best-placed one) sends up to 40 symbols | ~300× |
| **Grant hijack** | An offer, then silence | The announcer waits `T_grant` (10 min) before reassigning, once per object | Stalls delivery |
| **Election capture** | Beacons claiming mains power and an uplink, or the maximum score, and a HAVE listing everything | Every announcer that hears you yields and its followers follow you; you serve nothing | Each follower is held until it has asked you for one symbol of what it waits for and got none for `T_want_min`, about 20 minutes after it began to wait (FEASIBILITY.md §24), or for 90 minutes; then it ignores you for an hour, and may follow another false announcer next. Asking for what you lack and never naming an uploader holds them no longer (PROTOCOL.md §5.2, FEASIBILITY.md §25). Listing what you lack, you are waited for as long as ever; listing nothing, you hold a follower longer only for what its earlier announcers could not get either, since it waits twice as long for that under each next one (PROTOCOL.md §5.2, FEASIBILITY.md §31) |
| **Excursion lure** | In the rendezvous, a beacon and a HAVE listing objects you do not have | Followers whose own cell cannot get those objects visit you for `T_excursion` and get nothing | Delay of what was missing anyway, once per follower and hour |
| **Conflict poisoning** | A report naming announcers with a high colour count | Everyone's slot cycle grows to that count and each announcer idles all but one slot of it | Was measured at 8/9 idle by accident alone |
| **Store exhaustion** | A huge catalogue on a channel someone follows | Followers fetch and keep it | Bounded by what they follow |
| **Changed collections** | A root of your own channel that flags every collection manifest as changed (PROTOCOL.md §2) | Every holder of the root lists them with it and uploads them after it, up to a HAVE frame of them per root | Bounded by your own channel: the channel flood |
| **Relay ask** | As an announcer, asks marked as for listeners (PROTOCOL.md §3.3), for every piece of every channel, repeated for `T_relay_wait` | Every follower that hears you and can name the pieces, or fetch the collection manifest a set of them names from its own announcer, fetches what you asked for in its own cell, and keeps it while you ask and `want_ttl` longer | Bounded by each node's carry budget, once per object and node: a full budget takes on no more relays (PROTOCOL.md §4). Without a budget, by every channel a follower's announcer holds. A made-up piece is not relayed; a made-up collection manifest in a set costs a table entry, at most `max_relay_asks` of them, and an ask to the follower's own announcer, which does not know it and does not record it |
| **Relay deterrence** | As an announcer, asks marked as for listeners (PROTOCOL.md §3.3) that you grant, or list as held, at ages you choose | Nodes that hear you learn that others meet such asks at those ages, and relay other cells' asks later (PROTOCOL.md §4) | Relays at most `want_ttl` late; once you stop, what they learned from you halves with every 512 further asks they hear |
| **Relay cancellation** | A HAVE under the id of an announcer whose cell asked for something, listing it (PROTOCOL.md §4) | Nodes relaying for that cell give up what they had not fetched yet | Until that announcer asks again, when they take it on again; ids are not authenticated (Someone else's firmware, below) |
| **Window jamming** | Any signal on the control carrier through every control window, 4 s a minute (PROTOCOL.md §3) | No root or announcement crosses between cells while it lasts, and cells learn of new content later: as if nobody listened on the control carrier (FEASIBILITY.md §23) | 15 times: 6.7 % airtime blocks what continuous jamming used to |
| **Time pulled ahead** | Beacons, as announcer, that tell a later shared time than everyone else's (PROTOCOL.md §6) | Every announcer that hears one takes the later time at its next beacon, and its cell with it; cells that have not heard it yet no longer share a rendezvous or a window with those that have, until a window of theirs falls on one of the others' (PROTOCOL.md §3), or a node that knows no time carries it across. Repeated, it splits a network into groups of different times: on a hopping carrier their hop sequences differ, on any carrier their control windows (PROTOCOL.md §6). Maximum consensus has this weakness in general (He et al. 2014, Remark 3.8). Not measured | One beacon per jump |

Two of these are structural rather than incidental. An announcer is generous by design: it serves
whatever its cell asks for, which is exactly what an attacker needs. And control frames are free
to send and expensive to answer.

## The rule this asks for

> **A request may not spend more of someone else's airtime than the sender spent on it.**

That is the same thought as EtherFatsoen's other rules, pointed outward: politeness includes not
letting someone else be impolite through you. It turns every attack above into one that starves
the attacker first. Concretely it means a per-neighbour budget for work requested, refilled
slowly, so a peer that asks for more than it has earned simply waits, and an ordinary node never
notices the limit exists.

## What is fixed

- **Time slots only where the regulator does not already cap everyone.** On a duty-cycled or
  polite band the cap is the bound, so a poisoned colour count cannot idle the band. This also
  removed the accidental version of that attack, which was costing a town eight ninths of its
  airtime.
- **Only an announcer's NACK is answered by arbitrary holders.** A follower's repair comes from
  its own announcer's carousel, which removes the easiest amplification path and, incidentally,
  960 000 repair answers from a 200-node town. A follower names one holder only when its own
  announcer is asking for the object and has granted it to nobody, and only a holder in another
  cell (PROTOCOL.md §3.5); a lying follower can still make that one holder answer, which is the
  NACK row above.
- **Content verification on completion**, so poisoned symbols cost airtime and nothing else.
- **Repetition that does not help is repeated ever more slowly** (PROTOCOL.md §4). A WANT flood
  brought every object back for another pass as often as it was asked for: one simulated
  attacker, about 700 WANTs in 12 hours, made a band O cell carry 25 times its normal traffic and
  its announcer transmit at the legal limit, although every listener still got everything on
  time. Now the first repetition of an object comes at once and each further one waits 10, 20, 40
  and at most 80 minutes. The rule looks at the object, not at who asks, so made-up ids do not
  get around it (FEASIBILITY.md §11 has the numbers).
- **Announcers yield only to announcers they hear.** Yielding to an announcer named in a report
  let anyone who repeated such a report make announcers step down and come back; a WANT flood
  tripled the role changes in one simulated world. Reports now only feed the colouring.
- **Renditions only around their slot.** An announcer serves a rendition of a scheduled programme
  only from twice `T_render_ahead` before its slot until it has played, so a rendition flood gets
  at most what a listener of every channel at once would get.
- **A name counts once it has been heard twice.** Node ids are not authenticated, and every
  made-up id counted towards the score of each node that heard it: one node sending WANTs under
  a fresh id each minute made one simulated election change roles 27,335 times in 72 hours. A
  neighbour now counts only from its second frame, so each made-up name costs the attacker a
  frame every time it is used.

- **An announcer that lists what it does not serve is not followed** (PROTOCOL.md §5.2). A
  follower holds its announcer, and any announcer it visits on an excursion, to what it serves
  rather than what it claims: one that lists objects the follower wants and delivers none of them
  is ignored for an hour, and the follower follows another announcer or becomes a candidate. A
  source no longer stops offering an object because its announcer claims to have it, only when it
  uploaded the object or hears someone send it. With five false announcers in a 15 km² band L
  network, all claiming the maximum score, the design before this round had delivered 66 % in
  twelve hours, 60 % in the worst world, because the followers they captured escaped only by
  challenging on score; now 95 % and 87 %. Against false announcers claiming nothing, 97 %
  before and 98 % now; and an attacker flooding WANTs, which slows honest announcers, makes nobody
  leave one (FEASIBILITY.md §12). Two parts of that defence had leant on the manifest repetition
  that §13 of FEASIBILITY.md removed: a follower that wanted only a manifest had no evidence
  against a false announcer listing tracks, and the quick test of a silent channel took an honest
  announcer, silenced by a WANT flood, for a false one. Now an announcer that neither serves, nor
  asks for, nor grants what its follower wants is not followed, listed or not, and a follower asks a
  silent announcer for one symbol before it believes the silence. Against five false announcers
  claiming the maximum, 96 % delivered and 94 % in the worst world; under a WANT flood, no role
  changes.
- **An announcement is a hint, not a fact** (PROTOCOL.md §2). `MANIFEST_ANNOUNCE` is not signed,
  and a node took an announced sequence number for the channel's newest. One frame naming the
  highest sequence number and a made-up manifest id made every node that heard it ignore all real
  announcements of that channel and refuse its real, signed manifests, and announcers passed the
  claim on to their cells: a channel frozen for the whole mesh by one 33-byte frame (the smoke
  test `a_false_announcement_blocks_nothing` shows it on the earlier code). Now a node fetches an
  announced manifest but believes only one it has checked; the latest announcement replaces a
  pending one unless that one is arriving; and announcers announce only manifests they hold. A
  false announcement now costs a follower a want that nobody answers, until the next real
  announcement replaces it. This was found by reading, not by an attack in the simulator.

## Someone else's firmware

The code is public, so anyone can build a node that speaks MeshCast and keeps none of its rules:
it can send any frame, under any id, at any time and power, and lie in every field. The requests
above already assume such a node. This section takes the rest of what it can do, class by class,
and states what the design must do about it. These are requirements, not results: none of them
is simulated (the simulator's attackers stay the request floods and false announcers of
FEASIBILITY.md §11 and §12), and each is to be designed and measured like the rest of the
protocol before it becomes a rule.

**1. Poisoned symbols.** A node checks an object only when it is complete, and a mismatch drops
all of it (PROTOCOL.md §1). So a foreign node that sends one wrong symbol of an object on a cell's
channel, during each pass or under the id of a granted uploader, keeps that object from ever
completing at the nodes that hear it, for one frame per pass: about 200 times its own airtime for
a 42 kB track in 200-byte symbols. What people hear stays authentic; whether they hear it does
not.

*Requirement: verify an object in pieces as they arrive, against what its id already
authenticates, and discard only the piece that fails.* The object id is the BLAKE3 hash of the
object, and BLAKE3 hashes a tree over 1024-byte chunks whose parent nodes hold the chaining
values of their children
([BLAKE3 specification](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf);
the [Bao](https://github.com/oconnor663/bao/blob/master/docs/spec.md) outboard encoding is that
tree without the chunks). A node that holds an object's parent nodes can check each chunk on
arrival against the id it already has; the parent nodes are themselves checked against the id.
They would travel as an object of their own, named in the `integrity` slot of the piece's entry
in its collection manifest (PROTOCOL.md §2), which the signed root authenticates. A binary tree
over n chunks has n − 1 parent nodes of two 32-byte chaining values each: for a 42 kB track,
41 parents, 2.6 kB, about 6 % of the track. Open and to be measured: that cost against what it
saves, whether a symbol should divide a chunk (200 bytes do not divide 1024), or a tag per symbol
instead, which must then be at least 64 bits: the 4-byte tag PROTOCOL.md §1 first drafted is
matched by trying about 2^32 candidate symbols.

**2. Names that are not checked.** A node id is 4 bytes and nothing binds it to the node that
sends it. Under an announcer's id a foreign node can grant uploads it never asked for, tell every
holder that the announcer follows someone else (holders then stop uploading to it and forget its
grants, PROTOCOL.md §4), list in a HAVE what holders are uploading to it (they end those uploads,
§4), grant phases to holders that do not exist (holders that hear it keep their uploads to other
announcers out of those phases, though never out of all their turns, §4), send NACKs that make
holders answer (the NACK row above), or beacon with another score or colour. Under a holder's
id it can offer and fall silent (the grant hijack row), and under anyone's id what it sends
counts as that node's: the evidence rules of PROTOCOL.md §5.2 hold a node's frames, offers and
lists against the id they came under.

*Requirement: a frame that makes others act, or that counts as evidence against its sender, must
be attributable to that sender.* An Ed25519 signature is 64 bytes, more than a quarter of the
largest gossip frame (234 bytes, PROTOCOL.md §3.3), on every grant. The candidate is TESLA
([RFC 4082](https://www.rfc-editor.org/rfc/rfc4082),
2005): a sender commits once, in a signed frame, to a one-way chain of keys, attaches to each
frame a MAC under the key of the current time interval, and discloses that key some intervals
later; a receiver accepts a frame only once the key has arrived, and only if by its own clock the
key could not yet have been disclosed when the frame came in, which requires an upper bound on how
far its clock lags the sender's. Asking, granting and repair are delay-tolerant and can wait an
interval; carrier sensing cannot, and stays unauthenticated. Open and to be measured: bytes per
frame, the disclosure delay against `T_grant` and the meeting-dwell cycle, verification time on
an ESP32-S3, and the clock bound (next item). Until then, every rule that acts on evidence per id
must leave a wrongly accused honest node a way back, as the hour-long shun does.

**3. Time.** Beacons carry UTC, followers without GPS or a phone take their announcer's, renditions
are served only in a window around their slot (PROTOCOL.md §1.2), and TESLA needs a bound on clock
lag. A foreign beacon can claim any time.

*Requirement: time from the air is a hint within a bound.* A node with a trusted source (GPS, a
phone over BLE, NTP over IP) never moves its clock for a beacon; one without accepts beacon time
only from its own announcer, authenticated once item 2 exists, and only within the drift its own
clock can have accumulated since it last had trusted time. The drift bound comes from the
oscillator of the board, to be taken from its datasheet in Phase 1.

**4. Ignoring the regulations.** A foreign node can send at any power, any duty cycle, on any
channel. EtherDiscipline binds only our own transmitters (ETHERDISCIPLINE.md); stopping anyone
else is the regulator's business and the operator's liability, and no protocol can.

*Requirement: the protocol does not reward it.* A node already measures the channel occupancy
others cause (ETHERFATSOEN.md). An announcer whose own measured airtime exceeds its region's
limit, over a window as long as the limit's own (an hour for a duty cycle), is not followed and
not counted in scores, like an announcer that serves nothing. A node hears only part of what
another sends and two senders under one id add up, so the measurement under-counts and over-
counts in turn; it is acted on only for an announcer the node would otherwise follow, and only
with a margin, to be set by measurement.

**5. Malformed frames, and tables that grow.** Every frame is untrusted input. Rust rules out
memory corruption, not a panic, a large allocation or a table that never stops growing. Today the
parsers check lengths, and what a node keeps per neighbour is capped (512 offered ids, 16 sets it
could not read yet), but how many neighbours, announced channels, relay asks, askers in a
carousel and known objects it keeps is bounded only by expiry: a foreign node that uses a new id
in every frame makes every node in range keep an hour of names (`neighbor_ttl`).

*Requirements:* (a) decoding never panics and never allocates out of proportion to its input, for
every frame type and every CBOR object (root and collection manifests, rendition tables),
checked by fuzzing in CI; (b) every table filled from the air has a size cap and an eviction rule
that keeps what has evidence first (a name heard twice before one heard once, the announcer a
node follows before one it only hears), so that a node's memory is bounded whatever arrives.

(a) holds now. A test (`core/tests/robust_input.rs`) mangles valid encodings of every frame type,
a root and a collection manifest and a rendition table 800,000 times with a seeded generator (bit
flips, bytes that start large CBOR lengths, truncations, insertions, removals) and requires that
no decode panics or allocates more than eight times its input plus 8 KiB; it runs in under a
second, with the other tests, on the stable toolchain. It found the collection, root and rendition
decoders reserving room for as many entries as the input claimed: seven bytes claiming 4,096
pieces made the collection decoder allocate 262,144 bytes. They now grow a list as its entries
arrive, and the largest allocation of any decode in the test is 885 bytes, for 277 bytes of input.
(b) holds for every table that names nobody checks can fill. The simulator reports the largest
size each table filled from the air reached in any honest node (sim/README.md). Over the nine
scenarios, the collection variants, pieces of 7 kB, the living network and its attacks, honest
nodes kept at most 155 neighbours, 5,437 ids offered by them, 23 announcers in conflict and about
35 askers per object in a carousel; five attackers asking under made-up names every five seconds
raised the neighbours a node kept to 2,362 and the askers in one carousel to 22,296, and each
made-up name stayed for an hour, so a node on its own firmware sending faster fills them without
end. A node now keeps only what it can use (PROTOCOL.md §7): an ask for an object it cannot name
is not recorded, nor an announcer's ask for one, and only asks for objects it can name, or for a
collection manifest a set names, count towards relaying. And the tables keyed by names have
caps that give way evidence first (PROTOCOL.md §8): 256 neighbours, 8,192 offered ids, 64
announcers in conflict, 32 askers per object, 1,024 relay asks (smoke test
`made_up_relay_asks_are_kept_within_bounds`). Under the same five attackers a node kept 256
neighbours and the carousel 870 askers, and everything was still delivered, with 0.4 % more
frames; without them, the smoke test `made_up_names_are_kept_within_bounds` shows the same flood
filling both tables past their caps.
The nine scenarios, the collections, the size sweep, the living network and the false announcers
moved within the spread of their worlds (band O 15 km² over sixteen worlds with 14 and 42 kB
pieces: playback start 9.7 and 12.8 minutes against 9.7 and 12.6). What a node carries (its wants,
its store, its grants) grows with what it follows and serves, up to 366 entries for an hour of
music in 360 pieces, and is bounded by item 4, bounded generosity; what it holds for others, by
its carry budget (PROTOCOL.md §4).

**6. Firmware images.** An object can be a firmware image (PROTOCOL.md §1), and updates over the
carousel are planned (ROADMAP.md).

*Requirement: a node installs only an image signed by a key it was built to trust,* never one
because it arrived in a followed channel, and never treats content as code.

**7. What a listener gives away.** A node that only listens sends nothing. A follower whose cell
lacks something asks, and every ask carries its node id and what it wants, readable by anyone in
range; an id that stays the same for days lets anyone with a receiver follow one listener's
interests and movements.

*Requirements:* (a) node ids are random and replaced regularly, at least at every start; what is
counted per id (neighbours, evidence) must then relearn, and what that costs is measured before
the period is chosen; (b) a follower asks for no more than its cell does not carry, as now, and
for a collection in sets rather than piece by piece; (c) encrypted channels keep content from
strangers (PROTOCOL.md §7), but the short ids in asks still link the listeners of one
object; whether an encrypted channel's objects need names only its listeners can link is open.

## What is not fixed yet, in the order it should be

1. **Per-neighbour request budgets.** The rule above, as a token bucket per peer, on WANT, NACK
   and offers. The repetition backoff now bounds what repeated asks cost without needing to know
   who asks, which is what made-up names require; a budget per neighbour would still bound first
   passes of objects nobody else wants, for attackers that keep one name.
2. **Evidence before belief, the rest.** Neighbour counts, reports and an announcer's HAVE now
   need evidence (above). Still open: a colour count is accepted only up to the number of
   distinct announcers the node has heard itself; and capability and score in a beacon are still
   believed until the announcer is caught serving nothing, so a false announcer captures each
   follower for 50 to 90 minutes before it is ignored, and makes every announcer that hears it
   yield.
   An announcer that claims mains power should have to show it, for example by staying on the air
   through the hours a battery node could not.
3. **Renditions at the speed of listening.** A device cannot play faster than real time, so a
   node that asks for renditions of more audio per hour than an hour holds is not listening.
   An announcer serves each follower renditions at most at the rate its profile plays; this is
   the request budget of item 1 with a limit that follows from what renditions are for.
4. **Bounded generosity.** An announcer serves at most so many channels and collections, chosen
   by how many distinct followers asked and for how long, rather than everything it hears of.
   *Partly done:* an announcer fetches pieces and covers only when a follower asks for them, and
   keeps current only the channels its cell asked for within `cell_keep`, publishes or follows
   (PROTOCOL.md §2); what a node holds for others, the menu of channels it does not follow
   included, is bounded by its carry budget (§4). Open: how many channels one follower can make
   its announcer serve, which today only `cell_keep` bounds.
5. **Someone else's firmware.** The rest of the requirements above, cheapest first: signed
   firmware images, verification in chunks, ids that change, airtime as evidence, and
   authenticated announcer frames with the bound on time they need. The decoders and the tables
   keyed by names are done; what a node carries is item 4.
6. **Known peers, optionally.** A cell may require that requests come from a node whose key it
   has seen before, which makes the attacks above cost an identity rather than nothing. This is
   a deployment choice, not a default: MeshCast is meant to work with strangers.

None of this protects against jamming, which is a radio problem and not a protocol one, or
against an attacker with a licence and a kilowatt.
