# Generation 33: Dramatic Reversal ramp test

## Objective

Test Dramatic Reversal as an acceleration option in the Generation 32 balanced Nick Fury
list under the existing mildly Rhystic-favored objective: Rhystic T1 1.00, Rhystic T2 0.75,
Heartwood T1 0.70, and Heartwood T2 0.55.

## Semantic audit

Dramatic Reversal costs `1U`, goes to the graveyard on resolution, untaps all nonland
permanents, does not untap lands, and does not remove summoning sickness. It can untap Mana
Vault, Sol Ring, Moxen, Birds of Paradise, and Deathrite Shaman when those permanents are
otherwise eligible mana sources.

A regression test now covers tapped rocks, a fresh Birds, and a tapped land. The audit also
removed an unrelated global Rhystic Study gate from the turbo-opening action generator;
Nick Fury simulations were not affected by that gate because the deck contains Rhystic.

## Policy screen

Eight plausible substitutions received 500 paired full mulligan-policy games under the
same learned baseline thresholds.

| Cut for Dramatic Reversal | Binary delta | Weighted delta |
|---|---:|---:|
| Culling the Weak | -0.4 pp | -0.0022 |
| Rain of Filth | -0.2 pp | -0.0022 |
| Mystical Tutor | -0.6 pp | -0.0045 |
| Manamorphose | -1.2 pp | -0.0064 |
| Rite of Flame | -1.0 pp | -0.0096 |
| Tinder Wall | -1.2 pp | -0.0103 |
| Simian Spirit Guide | -1.4 pp | -0.0138 |
| Elvish Spirit Guide | -3.2 pp | -0.0180 |

The two least-negative swaps advanced to a fresh-seed 2,000-game paired confirmation.

## Confirmation

| Swap | Binary delta (95% CI) | Weighted delta (95% CI) | Bootstrap weighted CI |
|---|---:|---:|---:|
| Culling the Weak -> Dramatic Reversal | -0.75 pp (-1.45, -0.05) | -0.0052 (-0.0101, -0.0002) | (-0.0102, -0.0003) |
| Rain of Filth -> Dramatic Reversal | -0.75 pp (-1.53, +0.03) | -0.0072 (-0.0128, -0.0016) | (-0.0127, -0.0015) |

The Culling comparison had 18 candidate-only wins and 33 baseline-only wins (McNemar
`p = 0.0489`). The Rain comparison had 24 candidate-only wins and 39 baseline-only wins
(`p = 0.0769`). Both weighted intervals exclude zero in the harmful direction.

## Decision

Do not add Dramatic Reversal for the turn-one/turn-two Rhystic/Heartwood objective. Its
`1U` setup cost requires multiple retained nonland mana permanents before it produces net
mana. The deck's expendable rituals, guides, and sacrifice sources work without that board
state, while spent Petals, LED, guides, and rituals cannot be recovered by the untap effect.

The result does not claim Dramatic Reversal lacks later-game utility or combo applications;
it establishes that no tested ramp-slot substitution improves this opening-engine objective.
