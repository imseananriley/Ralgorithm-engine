# Generation Thirteen: Paired Mana Screen

## Design

- Champion fixture at generation twelve.
- Root seed `2026071301`.
- 300 Commander London mulligan games per variant.
- Turn-two weighted Rhystic/Heartwood objective.
- Semantic-draw compiled policy, depth 24.
- Ten local workers.
- Nine aligned variants: baseline plus eight single-slot swaps.
- No strict-reference correction; the depth-16 calibration was stopped after exceeding 2 GB and 45 seconds for two games.

The run completed in 161.61 seconds at 16.71 variant-evaluations/s. Baseline mean score was `0.1223608`; 89.0% of games had nonzero expected value. Nonzero rate is not a resolved-engine rate because chance-valued lines count whenever their expected value is positive.

## Paired Results

| Swap | Paired score delta | SE | 95% CI | Changed slot seen | Conditional delta |
| --- | ---: | ---: | ---: | ---: | ---: |
| Commandeer -> Birds of Paradise | -0.000003 | 0.002059 | [-0.004038, +0.004032] | 11.7% | -0.001136 |
| Commandeer -> Ragavan | -0.000932 | 0.001135 | [-0.003157, +0.001292] | 11.7% | -0.007888 |
| Commandeer -> Mox Opal | -0.000001 | 0.002005 | [-0.003930, +0.003928] | 11.7% | -0.000991 |
| Commandeer -> Paradise Mantle | -0.001216 | 0.001125 | [-0.003421, +0.000989] | 11.7% | -0.010717 |
| Commandeer -> Infernal Plunge | +0.000432 | 0.002094 | [-0.003671, +0.004535] | 11.7% | +0.003545 |
| Rain of Filth -> Birds of Paradise | **+0.000354** | 0.000126 | **[+0.000108, +0.000601]** | 17.3% | +0.001140 |
| Rite of Flame -> Birds of Paradise | +0.003031 | 0.001802 | [-0.000501, +0.006562] | 19.0% | +0.015387 |
| Deathrite Shaman -> Birds of Paradise | -0.000138 | 0.000099 | [-0.000331, +0.000056] | 16.7% | -0.000804 |

The Rain comparison has `z = 2.81`, approximately `p = 0.0049` two-sided. A Bonferroni threshold over the eight comparisons is `0.00625`, so this result narrowly survives that conservative family-wise correction. No binary success discordance was significant by exact McNemar testing.

## Interpretation

`Rain of Filth -> Birds of Paradise` is the only provisionally supported change and preserves the interaction/value count. The estimated gain is small: about 0.035 weighted-score percentage points overall. It should be tested first in an independent larger seed window.

`Rite of Flame -> Birds` has the largest point estimate, but its confidence interval includes zero. It is the second follow-up candidate. `Deathrite -> Birds` is slightly negative under the configured assumption that Deathrite usually has an external land to exile, so the current evidence favors retaining Deathrite.

None of the Commandeer replacements is resolved by this sample. Those comparisons are both statistically noisy and strategically costly because they remove interaction. They should not motivate an interaction cut.

## Limitations

This is a low-fidelity paired screen under the frozen compiled policy. Semantic draw quotienting and deterministic policy choices are corrected only in a future strict-reference sample. The screen is suitable for ranking follow-up experiments, not for a publication claim or final deck registration.
