# Generation 29: champion versus Rograkh/Silas

## Question

Compare the current Nick Fury champion with the supplied Rograkh/Silas list for
resolving an opening engine by turn two. The shared utility is:

- Rhystic Study turn 1: 1.00
- Rhystic Study turn 2: 0.75
- Heartwood Storyteller turn 1: 0.70
- Heartwood Storyteller turn 2: 0.55

The Rograkh/Silas list has no Heartwood Storyteller, so every success is Rhystic
Study. This is a deck-strategy comparison, not a Rhystic-only comparison under a
separately optimized champion policy.

## Method

- 500 evaluation games per deck with common root seeds and domain-separated
  ChaCha20 Fisher-Yates shuffles.
- Independent frozen mulligan continuation values trained from 64 visible hands
  per London mulligan stage. The policy never sees the next offered hand.
- Mulligans are allowed through the forced three-card keep.
- Gemstone Caverns is live with probability 3/4, sampled before mulligan decisions.
- Exact ordered-library Rust search, 5,000 initial states and a 60,000-state cap
  rerun. Remaining caps are reported as an uncertainty interval.
- Gamble uses the existing seeded stochastic simplified-discard model.
- The supplied library contains 98 cards plus two partner commanders. Rograkh is
  the opening-relevant commander; Silas remains in the command zone and supplies
  blue-black-red color identity.

## Results

| Model | Deck | Success | Cap interval | T1 Rhystic | T2 Rhystic | T1 Heartwood | T2 Heartwood | Weighted score |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Strict | Champion | 60.6% | 60.6-61.2% | 4.0% | 34.0% | 1.6% | 21.0% | 42.17% |
| Strict | Rograkh/Silas | 58.6% | 58.6-60.8% | 6.0% | 52.6% | 0 | 0 | 45.45% |
| Draw-engine proxy | Rograkh/Silas | 59.8% | 59.8-62.6% | 6.0% | 53.8% | 0 | 0 | 46.35% |

For strict binary success, Rograkh/Silas minus champion is -2.0 percentage
points. Paired discordances are 91 Rograkh-only and 101 champion-only;
two-sided exact McNemar p = 0.516. The unresolved-cap bounds overlap.

For strict weighted utility, Rograkh/Silas minus champion is +3.28 points with
an approximate paired 95% interval of -0.83 to +7.39 points. The draw-engine
proxy delta is +4.18 points (naive paired interval +0.19 to +8.17), but this
does not include semantic uncertainty from the draw-engine approximation.

The experiment therefore does not establish a statistically reliable overall
winner. It does show the strategic split clearly: the champion converts 22.6%
of games through Heartwood, while Rograkh/Silas casts Rhystic much more often
and has a two-point turn-one Rhystic advantage.

## New semantics

Opening-relevant behavior was added for Rograkh and Grixis commander identity,
Blood Crypt, Watery Grave, Cabal Ritual, Chromatic Star, Grim Monolith, Jeweled
Amulet, Talisman of Dominance, Vexing Bauble, Birgi, Curse of Opulence,
Demonic Consultation, Tainted Pact, Dramatic Reversal, Infernal Plunge,
Jeska's Will mana, wheels, ritual copies through Flare of Duplication, and
ritual reuse through Flashback. Defense Grid and Grinding Station can be cast
as artifacts for metalcraft; their post-resolution text does not improve the
measured objective.

Oracle-sensitive assumptions:

- Command Tower and Arcane Signet produce only blue, black, or red for the
  partner deck. Mox Amber produces red from Rograkh, not every identity color.
- Demonic Consultation fails when Rhystic is in the mandatory first six exiles.
- Tainted Pact treats the supplied singleton library as deterministic.
- Ragavan always connects, consistent with the established simulator policy.
- Jeska's Will uses a seven-card opponent hand and omits temporary top-three
  cards. Wheel of Fortune and Windfall draw seven under that opponent model.
- Demonic Counsel delirium, Yawgmoth's Will, Underworld Breach, and non-ritual
  Flare/Flashback targets remain conservative omissions.
- Ad Nauseam and Necropotence require life and cleanup-choice state not present
  in the opening abstraction. The strict result disables both. The proxy result
  draws 15 cards for Ad Nauseam and schedules a large Necropotence end-step draw;
  it is sensitivity analysis only. Five proxy wins used Ad Nauseam and six used
  Necropotence.

## Next sample

The observed strict weighted standard error implies approximately 2,100 paired
games for a 2.5-point 95% half-width, or about 4,000 games for 80% power if the
true weighted delta is near 3.3 points. Before that run, implement exact
Ad Nauseam life accounting and bounded Necropotence cleanup selection, then
predeclare strict weighted utility as the primary endpoint and binary success as
secondary.
