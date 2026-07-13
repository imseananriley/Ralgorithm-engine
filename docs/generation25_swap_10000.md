# Generation 25: paired 10,000-hand swap screen

## Scope

This confirmatory screen evaluates 24 unresolved `watch_raw` swaps from the established
7-cut by 18-addition candidate pool. Each swap uses the same 10,000 raw seven-card hands,
deck permutations, Gemstone Caverns states, and probabilistic seeds as the baseline.
There are no London mulligans in this stage. The experiment therefore measures conditional
raw-hand value and is a screening step, not a final deck win-rate estimate.

The strict Rust hybrid solver ran packed search through discrepancy budget 2 followed by
full-library exact rescue and witness validation. The run completed 240 shards (240,000
swap-hand evaluations) in 1,587 seconds of elapsed local time. Summed per-process solver
time was 3,141 wall-seconds and 9,070 CPU-seconds.

## Statistical method

- Baseline deterministic success: 3,687 / 10,000 (36.87%).
- Primary effect: paired candidate-minus-baseline deterministic success rate.
- Confidence interval: normal paired interval using the variance of the per-hand {-1, 0, 1}
  difference.
- Hypothesis test: exact two-sided McNemar test over discordant paired hands.
- Family-wise error: Holm correction across all 24 comparisons.
- Cap sensitivity: repeat the paired test after excluding every hand capped in either arm.
  This complete-case result is diagnostic because cap occurrence is not random, but it
  prevents a simple difference in search completion from being counted as a deck gain.

## Positive results

| Swap | Candidate rate | Raw delta (95% CI) | Gains / losses | Holm p | Complete-case delta (95% CI) | Complete-case Holm p |
|---|---:|---:|---:|---:|---:|---:|
| Glimmervoid -> Eldritch Evolution | 37.72% | +0.85 pp (+0.54, +1.16) | 172 / 87 | 0.00000294 | +0.35 pp (+0.06, +0.64) | 0.171 |
| Wishclaw Talisman -> Eldritch Evolution | 37.41% | +0.54 pp (+0.30, +0.78) | 101 / 47 | 0.000182 | +0.43 pp (+0.20, +0.67) | 0.00486 |
| Glimmervoid -> Worldly Tutor | 37.04% | +0.17 pp (-0.10, +0.44) | 102 / 85 | 1.000 | -0.23 pp (-0.48, +0.01) | 0.302 |
| Sink into Stupor -> Ragavan, Nimble Pilferer | 37.01% | +0.14 pp (-0.04, +0.32) | 50 / 36 | 1.000 | +0.21 pp (+0.03, +0.40) | 0.197 |
| Rain of Filth -> Arcane Signet | 36.99% | +0.12 pp (-0.10, +0.34) | 67 / 55 | 1.000 | +0.12 pp (-0.10, +0.33) | 0.987 |

Only `Wishclaw Talisman -> Eldritch Evolution` is positive under both the primary and
complete-case analyses after family-wise correction. `Glimmervoid -> Eldritch Evolution`
has the largest primary effect, but 58 of its 172 gains occur where the baseline search
capped; its cap count falls from 546 to 490. Its uncapped direction remains positive, but
the family-wise corrected complete-case test is inconclusive.

`Sink into Stupor -> Steam Vents` produces only six net deterministic wins (+0.06 pp) and
increases caps to 608. Its positive complete-case test is selection-sensitive and is not
evidence for promotion because the primary paired result is null.

## Negative results

Holm-significant adverse primary effects include:

| Swap | Raw delta | Holm p |
|---|---:|---:|
| Crystal Vein -> Paradise Mantle | -1.11 pp | 4.58e-17 |
| Glimmervoid -> Badlands | -0.88 pp | 6.36e-18 |
| Glimmervoid -> Blood Crypt | -0.87 pp | 1.11e-17 |
| Crystal Vein -> Arcane Signet | -0.63 pp | 0.0000476 |
| Wishclaw Talisman -> Plateau | -0.54 pp | 0.000294 |
| Manamorphose -> Springleaf Drum | -0.42 pp | 0.000699 |
| Wishclaw Talisman -> Grim Tutor | -0.37 pp | 0.00000474 |
| Glimmervoid -> Plateau | -0.36 pp | 0.0000573 |
| Glimmervoid -> Phyrexian Tower | -0.30 pp | 0.00749 |

The remaining swaps are statistically unresolved at 10,000 paired hands. Their full
rankings, cap diagnostics, and exact p-values are in the CSV and JSON artifacts.
The compressed raw-results archive retains all 240 shard outputs and per-hand hit/cap IDs.

## Decision

The strongest robust candidate for the next full-mulligan confirmation is
`Wishclaw Talisman -> Eldritch Evolution`. The larger but cap-sensitive
`Glimmervoid -> Eldritch Evolution` should be included in the same confirmation so the
actual mulligan policy and expanded rescue can decide which single cut is preferable.
`Sink into Stupor -> Ragavan, Nimble Pilferer` is the best secondary exploratory candidate,
but the current 10,000-hand evidence does not establish a positive effect.
