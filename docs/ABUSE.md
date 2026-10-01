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
- **You cannot poison content.** Objects are named by their hash. A symbol that does not fit is
  discarded when the object completes, the object is dropped and collected again. You can waste
  airtime this way but you cannot change what people hear.
- **You cannot make the audience relay for you.** Followers never transmit unless they hold
  something an announcer asked for, so there is no reflection through the crowd.
- **You cannot replay.** Manifest sequence numbers only go up, and only a signed manifest moves a
  node's sequence number; an announced one does not (below).

## What does not hold

| Attack | What you send | What it costs us | Amplification |
|---|---|---|---|
| **Channel flood** | Many signed channels with large catalogues | An announcer serves every channel it learns of, so it tries to carry all of them | Unbounded |
| **WANT flood** | One 50-byte gossip asking for an object | The announcer puts a 42 kB track in its carousel (540 kB before the codec change) | ~800× |
| **Rendition flood** | WANTs, as a device that cannot decode, for the rendition of every object a channel lists | The cell's carousel carries each as Opus (PROTOCOL.md §1.2): 367 kB for a 3-minute song at 16 kbit/s | ~7 000× |
| **NACK amplification** | One 30-byte NACK, claiming to be an announcer, or naming a holder as a follower whose announcer cannot repair | The holder named (or, unnamed, the best-placed one) sends up to 40 symbols | ~300× |
| **Grant hijack** | An offer, then silence | The announcer waits `T_grant` (10 min) before reassigning, once per object | Stalls delivery |
| **Election capture** | Beacons claiming mains power and an uplink, or the maximum score, and a HAVE listing everything | Every announcer that hears you yields and its followers follow you; you serve nothing | Each follower is held until it has asked you for one symbol and got nothing, 50 minutes after your channel fell silent, or for 90 minutes on a shared channel; then it ignores you for an hour |
| **Excursion lure** | In the rendezvous, a beacon and a HAVE listing objects you do not have | Followers whose own cell cannot get those objects visit you for `T_excursion` and get nothing | Delay of what was missing anyway, once per follower and hour |
| **Conflict poisoning** | A report naming announcers with a high colour count | Everyone's slot cycle grows to that count and each announcer idles all but one slot of it | Was measured at 8/9 idle by accident alone |
| **Store exhaustion** | A huge catalogue on a channel someone follows | Followers fetch and keep it | Bounded by what they follow |

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
4. **Bounded generosity.** An announcer serves at most so many channels, chosen by how many
   distinct followers asked and for how long, rather than everything it hears of.
5. **Known peers, optionally.** A cell may require that requests come from a node whose key it
   has seen before, which makes the attacks above cost an identity rather than nothing. This is
   a deployment choice, not a default: MeshCast is meant to work with strangers.

None of this protects against jamming, which is a radio problem and not a protocol one, or
against an attacker with a licence and a kilowatt.
