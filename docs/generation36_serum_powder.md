# Generation 36: Serum Powder

## Question

Does Serum Powder improve the mildly weighted turn-one/turn-two Rhystic Study or
Heartwood Storyteller rate of the Generation 32 balanced Nick Fury list?

The tested objective weights remain Rhystic T1 1.00, Rhystic T2 0.75, Heartwood T1
0.70, and Heartwood T2 0.55.

## Model

Serum Powder is a three-mana artifact that taps for colorless. Its mulligan ability is
resolved after the current mulligan stage's required bottoms:

1. Select the stage's required bottom cards while retaining Serum Powder.
2. Exile the resulting keep-sized hand.
3. Draw that many cards from the current ordered library without shuffling.
4. Keep that redraw or continue to the next normal mulligan stage.

Bottomed cards remain in the library and are included in later shuffles. Exiled cards
remain unavailable. The Powder decision uses only the visible hand and its calibrated
continuation value; it does not inspect the replacement cards. Powder bottoms preserve
engines, tutors, and mana before interaction or post-engine cards.

## Screen

An exploratory 500-game paired screen tested Powder over Culling the Weak, Rain of
Filth, Rite of Flame, Gitaxian Probe, Manamorphose, Birds of Paradise, Wishclaw
Talisman, and Eldritch Evolution. Only `Manamorphose -> Serum Powder` produced a
materially positive weighted point estimate: +2.20 percentage points raw and +0.0168
weighted. Its intervals still crossed zero.

The original 200-game implementation screen was discarded because it incorrectly drew
seven cards at every Powder stage rather than the current post-bottom hand size.

## Independent validation

Four finalists used a fresh seed, 2,000 paired games, common random numbers, 64
threshold hands per stage, stochastic Gamble, 75% live Gemstone Caverns, and
actual-state cap reruns to 100,000 states.

| Swap | Success | Raw delta (95% CI) | Weighted delta (bootstrap 95% CI) |
|---|---:|---:|---:|
| Baseline | 67.45% | - | - |
| Manamorphose -> Serum Powder | 68.50% | +1.05 pp (-0.33, +2.43) | +0.00675 (-0.00323, +0.01673) |
| Eldritch Evolution -> Serum Powder | 66.95% | -0.50 pp (-1.79, +0.79) | +0.00368 (-0.00543, +0.01268) |
| Rain of Filth -> Serum Powder | 67.55% | +0.10 pp (-1.13, +1.33) | -0.00150 (-0.01025, +0.00735) |
| Wishclaw Talisman -> Serum Powder | 67.00% | -0.45 pp (-1.79, +0.89) | -0.00538 (-0.01498, +0.00395) |

For the Manamorphose swap, Powder lost 13 turn-one successes and gained 34 turn-two
successes. The paired discordance was 110 Powder-only wins to 89 baseline-only wins
with exact McNemar `p = 0.156`.

## Replacement-slot control

The Manamorphose result conflicted with earlier cut-cost experiments, where Manamorphose
was more valuable than several other tested acceleration cards. The comparison method
replaces a card in place, so Powder inherits that card's shuffled index. Different cuts
therefore expose Powder to different finite samples of mulligan hands.

A fresh 500-game control constructed eight copies of the exact same
`Manamorphose -> Serum Powder` deck. Only the array position of Powder changed. The
candidate rates ranged from 64.6% to 69.4%, a 4.8 percentage-point spread, and weighted
deltas ranged from -0.0114 to +0.0202. The Manamorphose position was tied for worst in
this fresh control despite appearing best in the earlier seeds.

Cluster-averaging all eight placements by game produced a raw delta of +0.325 percentage
points (95% CI -1.54 to +2.19) and a weighted delta of +0.00208 (95% CI -0.01141 to
+0.01556). The apparent Manamorphose-specific advantage is therefore replacement-slot
coupling variance, not evidence of a card interaction.

## Recommendation

Do not change the list based on this experiment. No cut has a statistically supported
Powder gain, and the initial cut ranking is not reliable.

Before retesting, assign added cards a common random key across every cut arm or average
multiple replacement-slot couplings and bootstrap by game. This holds Powder exposure
constant enough to compare the cost of removing different cards.

Artifacts:

- `benchmarks/generation36_serum_powder_screen.txt`
- `benchmarks/generation36_serum_powder_finalists.txt`
- `benchmarks/generation36_serum_powder_slot_control.txt`
- `benchmarks/results/generation36_serum_powder_screen_500/paired_summary.csv`
- `benchmarks/results/generation36_serum_powder_finalists_2000/paired_summary.csv`
- `benchmarks/results/generation36_serum_powder_slot_control_500/paired_summary.csv`
