# Generation 31: fixed-ratio Nick Fury optimization

## Objective

Optimize the supplied 99-card Nick Fury tournament list for a mildly Rhystic-favored
turn-one/turn-two engine objective without reducing its protected interaction, value, or
win-condition slots. The score weights were:

- Rhystic Study turn one: 1.00
- Rhystic Study turn two: 0.75
- Heartwood Storyteller turn one: 0.70
- Heartwood Storyteller turn two: 0.55

Only ordinary lands, mana acceleration, and tutors were eligible cuts. Emergence Zone,
Sink into Stupor, and all dedicated interaction/value cards remained protected.

## Semantic audit

Before inference, the Rust engine's opening-hand tutor target tables were expanded to cover
candidate mana, dorks, fetchlands, and creature tutors. Compact deck fixture handling was
fixed in the comparison and mulligan-policy drivers. The default full-policy Rust binary now
resolves to the workspace release target rather than the removed pre-fork target directory.

The full Rust suite passed 147 tests after the semantic changes.

## Selection funnel

1. A 1,073-swap broad screen used 600 paired raw hands per swap and changed-slot relevance.
   This was a discovery screen only; it deliberately omitted tutor-access effects.
2. Twenty-nine promoted arms received 1,200 paired raw hands with typed relevance and the
   expanded tutor model.
3. Five predeclared finalists received 3,000 fresh typed-relevance hands.
4. Full weighted mulligan-policy screens narrowed the field to Birds of Paradise, Flooded
   Strand, and Street Wraith packages.
5. A 2,000-game shared-policy package test used a fresh seed and 100,000-state cap reruns.
6. The winning package received a final 4,000-game replication on another fresh seed, with
   256 threshold-training hands per stage and independently trained baseline/candidate
   mulligan policies.

Raw-hand results were used only for elimination and candidate selection. Final inference is
based on the full mulligan-policy simulations.

## Shared-policy package test

| Package | Binary delta (95% CI) | Weighted delta (95% bootstrap CI) |
|---|---:|---:|
| Scrubland + Diabolic Intent -> Birds + Flooded Strand | +2.40 pp (+1.15, +3.65) | +0.0138 (+0.0051, +0.0224) |
| Scrubland + Rain of Filth -> Birds + Street Wraith | +1.55 pp (+0.38, +2.72) | +0.0068 (-0.0014, +0.0150) |
| Diabolic Intent + Rain of Filth -> Flooded Strand + Street Wraith | +1.30 pp (+0.16, +2.44) | +0.0086 (+0.0004, +0.0170) |
| All three changes | +2.25 pp (+0.89, +3.61) | +0.0127 (+0.0030, +0.0224) |

Street Wraith did not improve the two-card leader and is not included in the recommendation.

## Independent replication

| Deck | Successes | Binary rate | Weighted score | Remaining caps |
|---|---:|---:|---:|---:|
| Supplied list | 2,838 / 4,000 | 70.95% | 0.49460 | 12 |
| Birds + Flooded package | 2,912 / 4,000 | 72.80% | 0.50319 | 11 |

The paired binary improvement is **+1.85 percentage points** (95% CI +1.00 to +2.70),
with 187 candidate-only wins and 113 baseline-only wins (exact McNemar p = 2.29e-5).
The weighted improvement is **+0.00859** (95% normal CI +0.0026 to +0.0145; bootstrap
CI +0.0026 to +0.0146).

Outcome counts changed as follows:

| Outcome | Supplied | Optimized | Delta |
|---|---:|---:|---:|
| Rhystic Study turn one | 183 | 180 | -3 |
| Rhystic Study turn two | 1,606 | 1,581 | -25 |
| Heartwood Storyteller turn one | 93 | 93 | 0 |
| Heartwood Storyteller turn two | 956 | 1,058 | +102 |

The package therefore improves the specified mild weighted objective and total engine rate,
but its gain is concentrated in turn-two Heartwood outcomes. It should not be described as
an improvement to Rhystic Study frequency in isolation.

## Recommendation

Apply exactly these two swaps:

- `Scrubland -> Birds of Paradise`
- `Diabolic Intent -> Flooded Strand`

The package leaves the land count unchanged, exchanges one tutor for one mana creature, and
does not cut interaction. The optimized fixture is
`fixtures/decks/nick_fury_generation31_optimized.json`.

Confidence intervals condition on the two learned mulligan policies and do not include every
possible source of model misspecification. The result is nevertheless replicated across a
shared-policy selection run and a larger fresh-seed independent-policy run, with matching
positive direction and intervals excluding zero.
