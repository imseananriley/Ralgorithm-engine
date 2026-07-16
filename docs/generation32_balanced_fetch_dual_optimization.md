# Generation 32: balanced fetch/dual optimization

## Constraint

Generation 31's two-card package changed the supplied mana base from eight fetchlands and
six original duals to nine fetchlands and five duals. It also broke the supplied topology's
stronger invariant: every included fetchland could access all five colors through its legal
dual-land targets.

Generation 32 treated both properties as hard constraints:

- exactly eight fetchlands;
- exactly six original dual lands; and
- every included fetchland can access W, U, B, R, and G through those duals.

The engine objective and weights remained Rhystic T1 1.00, Rhystic T2 0.75, Heartwood T1
0.70, and Heartwood T2 0.55.

## Exhaustive topology search

There are only 15 selections of eight fetchlands and six original duals that satisfy the
five-color-access invariant. All 15 were generated and singleton-validated rather than
choosing a replacement dual heuristically.

Each topology was tested with `Diabolic Intent -> Birds of Paradise`, preserving the
interaction/value count and total land count. A 500-game shared-policy screen promoted two
topologies. A fresh-seed 2,000-game paired test selected the Flooded/Tropical/Volcanic
configuration. The selected configuration then received a fresh-seed 4,000-game test with
independently trained baseline and candidate mulligan policies (256 threshold hands per
stage).

## Corrected recommendation

Apply this four-card package to the supplied tournament list:

| Remove | Add |
|---|---|
| Marsh Flats | Flooded Strand |
| Badlands | Tropical Island |
| Bayou | Volcanic Island |
| Diabolic Intent | Birds of Paradise |

The resulting fetchlands are Arid Mesa, Bloodstained Mire, Flooded Strand, Misty Rainforest,
Polluted Delta, Scalding Tarn, Verdant Catacombs, and Windswept Heath.

The resulting duals are Scrubland, Taiga, Tropical Island, Tundra, Underground Sea, and
Volcanic Island. Every one of the eight fetchlands can access all five colors through this
six-dual set.

## Independent validation

| Deck | Successes | Binary rate | Weighted score | Remaining caps |
|---|---:|---:|---:|---:|
| Supplied list | 2,836 / 4,000 | 70.900% | 0.49176 | 10 |
| Balanced package | 2,913 / 4,000 | 72.825% | 0.50469 | 16 |

The paired binary improvement is **+1.925 percentage points** (95% CI +0.924 to +2.926),
with 248 candidate-only wins and 171 baseline-only wins (exact McNemar p = 0.000197).
The weighted improvement is **+0.01293** (95% normal CI +0.0059 to +0.0200; bootstrap CI
+0.0059 to +0.0201).

Outcome counts changed as follows:

| Outcome | Supplied | Balanced | Delta |
|---|---:|---:|---:|
| Rhystic Study turn one | 160 | 158 | -2 |
| Rhystic Study turn two | 1,608 | 1,660 | +52 |
| Heartwood Storyteller turn one | 91 | 90 | -1 |
| Heartwood Storyteller turn two | 977 | 1,005 | +28 |

Unlike the generation 31 package, this gain includes more turn-two Rhystic Study outcomes.
The optimized fixture is `fixtures/decks/nick_fury_generation32_balanced_optimized.json`.
