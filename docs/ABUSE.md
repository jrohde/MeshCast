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
- **You cannot replay.** Manifest sequence numbers only go up.

## What does not hold

| Attack | What you send | What it costs us | Amplification |
|---|---|---|---|
| **Channel flood** | Many signed channels with large catalogues | An announcer serves every channel it learns of, so it tries to carry all of them | Unbounded |
| **WANT flood** | One 50-byte gossip asking for an object | The announcer puts a 540 kB track in its carousel | ~10 000× |
| **NACK amplification** | One 30-byte NACK, claiming to be an announcer | Every holder that hears it lines up an answer; the best-placed one sends up to 40 symbols | ~300× |
| **Grant hijack** | An offer, then silence | The announcer waits `T_grant` (10 min) before reassigning, once per object | Stalls delivery |
| **Election capture** | Beacons claiming the maximum score | You become announcer and can then simply not transmit; the cell starves | Denial of a whole cell |
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
  960 000 repair answers from a 200-node town.
- **Content verification on completion**, so poisoned symbols cost airtime and nothing else.

## What is not fixed yet, in the order it should be

1. **Per-neighbour request budgets.** The rule above, as a token bucket per peer, on WANT, NACK
   and offers. This is the general answer and subsumes several rows of the table.
2. **Evidence before belief.** A colour count is accepted only up to the number of distinct
   announcers the node has heard itself; a score is trusted only as far as the beacon's own
   carousel round counter shows the announcer is doing anything. Both are claims that cost
   everyone, so both should need evidence.
3. **Bounded generosity.** An announcer serves at most so many channels, chosen by how many
   distinct followers asked and for how long, rather than everything it hears of.
4. **Known peers, optionally.** A cell may require that requests come from a node whose key it
   has seen before, which makes the attacks above cost an identity rather than nothing. This is
   a deployment choice, not a default: MeshCast is meant to work with strangers.

None of this protects against jamming, which is a radio problem and not a protocol one, or
against an attacker with a licence and a kilowatt.
