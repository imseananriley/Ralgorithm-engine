# Generation 34: counterspell cut experiment

## Question

Which non-interaction card in the Generation 32 balanced Nick Fury list should be cut to
add another counterspell while minimizing damage to the turn-one/turn-two Rhystic Study or
Heartwood Storyteller plan?

`Dispel` is the standardized added counterspell. It is absent from the list, Commander
legal, blue for Chrome Mox and pitch effects, and has no proactive engine-search or mana
effect. The simulator does not assign value to protecting a resolved engine, but that value
is common to every tested cut and therefore cancels in the comparison.

The objective weights are Rhystic T1 1.00, Rhystic T2 0.75, Heartwood T1 0.70, and
Heartwood T2 0.55.

## Discovery

The paired changed-slot screen tested 56 current lands, acceleration cards, and tutors. It
used 600 raw hands at each of six London-mulligan hand stages, or 3,600 stage-hands per
cut. Interaction, win conditions, dedicated post-engine value cards, Emergence Zone, and
Sink into Stupor were protected. Gitaxian Probe was added as a gas-control candidate in the
full-policy screen.

The lowest raw weighted costs were:

| Cut for Dispel | Raw score delta |
|---|---:|
| Culling the Weak | -0.450 pp |
| Eldritch Evolution | -0.506 pp |
| Rite of Flame | -0.507 pp |
| Tundra | -0.526 pp |
| Volcanic Island | -0.540 pp |
| Wishclaw Talisman | -0.624 pp |
| Rain of Filth | -0.661 pp |

This screen is for pruning only. It freezes hand stages and does not infer changed mulligan
decisions.

## Full-policy results

The finalists used the learned champion mulligan thresholds, stochastic Gamble, 75%
pregame Gemstone Caverns, full London mulligans through three cards, actual-state cap
reruns to 100,000 states, and common-random-number pairing. The 500-game screen promoted
Culling, Rain, Probe, and Tundra to the final comparison. Each final result used the same
2,000 paired games and baseline.

| Cut for Dispel | Success | Success delta (95% CI) | Weighted score | Score delta (bootstrap 95% CI) |
|---|---:|---:|---:|---:|
| None (baseline) | 72.95% | - | 0.507875 | - |
| Culling the Weak | 72.20% | -0.75 pp (-1.46, -0.04) | 0.503250 | -0.00463 (-0.00965, +0.00020) |
| Rain of Filth | 72.35% | -0.60 pp (-1.38, +0.18) | 0.501925 | -0.00595 (-0.01162, -0.00028) |
| Gitaxian Probe | 72.35% | -0.60 pp (-1.25, +0.05) | 0.502075 | -0.00580 (-0.01050, -0.00122) |
| Tundra | 71.95% | -1.00 pp (-1.86, -0.14) | 0.499250 | -0.00863 (-0.01498, -0.00245) |

Culling's weighted score exceeds Rain by 0.001325, Probe by 0.001175, and Tundra by
0.004000. The paired 95% intervals for those candidate-to-candidate differences all include
zero. At the observed Culling-Rain weighted gap and variance, approximately 55,400 paired
games per candidate would be needed for a 95% interval just narrow enough to exclude zero,
assuming the effect and variance remain stable.

## Recommendation

Cut **Culling the Weak** for **Dispel**.

Culling is the best mildly weighted point estimate, ranked first in the independent raw
screen, preserves the 29-land configuration, and avoids removing a tutor or the free
topdeck-tutor redraw supplied by Gitaxian Probe. Its acceleration is also conditional on a
disposable creature and black mana. Rain and Probe are statistically plausible alternatives,
so this is the best current recommendation rather than proof that Culling is a unique
optimum.

Artifacts:

- `benchmarks/results/generation34_counterspell_slot_600/raw_delta_ranked.csv`
- `benchmarks/results/generation34_counterspell_policy_screen_500/paired_summary.csv`
- `benchmarks/results/generation34_counterspell_culling_2000/paired_summary.csv`
- `benchmarks/results/generation34_counterspell_finalists_2000/paired_summary.csv`
- `benchmarks/results/generation34_counterspell_final_combined.csv`
