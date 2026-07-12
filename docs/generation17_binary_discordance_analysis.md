# Generation 17: Binary discordance analysis

## Sets

On the 1,000-game final-keep corpus:

- Shared nominal hits: 494
- Pre-fork only: 125
- Current existential only: 127
- Neither: 254
- Naive set union: 746 (74.6%)

The union is not a valid win-rate estimate. Pre-fork hits use realized deck order;
current-only hits mean that at least one positive-probability branch exists after
the three-card deterministic library horizon.

## Pre-fork-only games

These were predominantly ordinary turn-two tutor and mana lines. Of 125 traced
games, all 125 were reproduced by the fast trace engine at 60,000 states. Traced
patterns overlap:

- 75 used a top-deck tutor
- 45 used a hand tutor
- 32 used a ritual
- 24 used Lion's Eye Diamond
- 23 sacrificed a land for mana
- 20 cast Nick Fury
- 17 used a Heartwood creature tutor
- 5 used Offer on the player's own spell

The mean keep size was 6.01 cards. The old outcomes were 102 Rhystic Study and 23
Heartwood Storyteller; 123 resolved on turn two. These misses are best explained
by insufficient action-family/discrepancy coverage under exact known-top replay.

## Current-only branches

These keeps averaged 4.98 cards and were enriched for fetchlands, Crop Rotation,
Mox Diamond, and Manamorphose. The pre-fork engine capped on 32 and returned a
failure on 95. A 60,000-state exact-order fast trace did not reproduce any of the
127 as a completed line; 32 remained capped.

Probability-aware packed replay found positive explored probability mass in all
127, but every result remained capped. Summing the modeled engine probabilities
produced 18.286 expected successes:

- Turn-one Rhystic: 0.011
- Turn-two Rhystic: 11.209
- Turn-one Heartwood: 0.022
- Turn-two Heartwood: 7.043

The per-game median was 3.78%, the mean was 14.40%, and the range was
0.011%-100%. These are possible probabilistic lines, not 127 realized wins.

## Combined rate

- Naive detector union: 74.6% (not a win-rate estimate)
- Pre-fork realized-order result: 61.9%
- Provisional hybrid lower estimate: `(619 + 18.286) / 1000 = 63.73%`

The 63.73% figure mixes realized binary outcomes with integrated residual chance
mass and all 127 packed evaluations were capped. It is useful diagnostically but
is not publication-grade. A full-library deterministic witness replay, followed
by probability integration over all pre-fork failures, is required for a single
unbiased combined estimate.
