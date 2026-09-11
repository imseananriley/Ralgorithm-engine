# Statistical design for deck-optimization experiments

## Scope

These calculations address Monte Carlo sampling error. They do not correct card-model
errors, missed lines, biased mulligan policies, unresolved search caps, or look-ahead
violations. More games make a biased simulator more precise, not more correct.

No fixed sample size guarantees statistical significance. Sample size can target a
chosen power under specified distributional assumptions, after specifying:

- the minimum effect worth detecting;
- the per-game variance or binary discordance;
- the Type I error rate;
- the desired power; and
- the number of hypotheses in the confirmatory family.

The tables below use two-sided alpha 0.05 and 80% power unless stated otherwise.
Family calculations use the conservative Bonferroni planning threshold. Holm's procedure
can be used for the final family-wise analysis.

## Independent unit and randomization

The independent experimental unit is one unique game seed/index. Reusing a seed across
deck variants creates a paired observation and is desirable. Repeating the same seed in
another run does not add a new observation.

If a seed is evaluated under eight replacement-slot couplings, that is one independent
game cluster with eight repeated measurements, not eight independent games. The Serum
Powder control contained 4,000 outputs but only 500 independent game clusters.

Common random numbers reduce variance only when random events are synchronized
semantically. The current in-place replacement method gives an added card the random
position of the removed card. It is unbiased as the number of unique games approaches
infinity, but different cuts expose the added card to different finite samples and make
candidate rankings noisy.

The Rust full-policy evaluator now assigns random keys by game, stage, and card identity
(`policy_card_priority_v1`, added September 2026). Legacy raw-slot screens still use the
older conditional design. The general production design is:

```text
random_key = H(root_seed, game_index, event_domain, stable_card_id)
```

Shared cards retain the same key in every variant. A common added card receives the same
new-card key in every cut arm. Gemstone Caverns, Gamble, policy rollouts, and hidden draws
must use separate event domains.

## Core calculations

For paired binary success, define `D_i = candidate_i - baseline_i`, where `D_i` is -1, 0,
or 1. If `q` is the probability of a discordant pair and `delta = E[D]`, then:

```text
Var(D) = q - delta^2
```

For weighted score, use the empirical variance `s_D^2` of paired per-game score
differences.

The approximate games required for confidence-interval half-width `h` are:

```text
n_precision = z_(1-alpha/2)^2 * s_D^2 / h^2
```

The approximate games required for two-sided power `1-beta` against effect `delta` are:

```text
n_power = (z_(1-alpha/2) + z_(1-beta))^2 * s_D^2 / delta^2
```

For a family of `m` simultaneous claims, replace `alpha` with `alpha/m` for conservative
planning. Final binary inference should still report paired intervals and exact McNemar
tests; final weighted inference should use a paired or cluster bootstrap.

## Absolute deck-rate precision

For a success rate near 70%, estimating one deck without a paired comparator requires:

| Desired 95% half-width | Games |
|---:|---:|
| +/-1.00 percentage point | 8,068 |
| +/-0.50 percentage point | 32,269 |
| +/-0.25 percentage point | 129,074 |
| +/-0.10 percentage point | 806,707 |

This is why absolute-rate estimation is much more expensive than a well-coupled swap
comparison.

## Pre-registered binary swap

`q = 0.03` represents a highly correlated local swap like the Generation 34
counterspell arms. `q = 0.10` represents a more disruptive mulligan or package change
like Serum Powder or the balanced land package.

| Minimum raw effect | Independent decks, games each | Paired `q=0.10` | Paired `q=0.03` |
|---:|---:|---:|---:|
| 0.25 pp | 526,182 | 125,575 | 37,667 |
| 0.50 pp | 131,226 | 31,388 | 9,411 |
| 1.00 pp | 32,644 | 7,842 | 2,347 |
| 2.00 pp | 8,077 | 1,955 | 581 |

Across retained experiments, paired common random numbers reduced raw-delta variance by
3.9x to 12.4x relative to independent deck runs. The correct response is therefore not to
abandon repeated seeds across variants. It is to synchronize their semantic use.

## Multiple-candidate binary families

For a Serum-like discordance of 10%, the games per arm required for 80% power are:

| Minimum effect | One pre-registered claim | Eight-arm family | 72-arm family |
|---:|---:|---:|---:|
| 0.25 pp | 125,575 | 204,591 | 286,727 |
| 0.50 pp | 31,388 | 51,139 | 71,669 |
| 1.00 pp | 7,842 | 12,775 | 17,904 |
| 2.00 pp | 1,955 | 3,185 | 4,463 |

A 500- or 2,000-game sweep cannot support direct claims about sub-1-point differences
across dozens of cards. It can only be an exploratory ranking step followed by a fresh,
pre-registered confirmation.

## Weighted-score comparisons

Observed paired weighted-delta standard deviations were approximately:

- 0.10-0.16 for tightly coupled local swaps;
- 0.16-0.17 for multi-card packages; and
- 0.20-0.23 for Serum Powder and other mulligan-divergent changes.

Games required for a pre-registered weighted claim:

| Minimum weighted effect | SD 0.16 | SD 0.226 |
|---:|---:|---:|
| 0.10 weighted pp | 200,932 | 400,890 |
| 0.25 weighted pp | 32,150 | 64,143 |
| 0.50 weighted pp | 8,038 | 16,036 |
| 1.00 weighted pp | 2,010 | 4,009 |

For an eight-arm family, a 0.50 weighted-point effect requires 13,095 games at SD 0.16
or 26,126 games at SD 0.226. For 72 direct claims those figures become 18,352 and
36,615.

## What existing sample sizes can resolve

The following are approximate 80%-power minimum detectable effects for one
pre-registered comparison:

| Games | Binary `q=0.03` | Binary `q=0.10` | Weighted SD 0.16 | Weighted SD 0.226 |
|---:|---:|---:|---:|---:|
| 500 | 2.17 pp | 3.96 pp | 2.01 pp | 2.83 pp |
| 2,000 | 1.09 pp | 1.98 pp | 1.00 pp | 1.42 pp |
| 10,000 | 0.49 pp | 0.89 pp | 0.45 pp | 0.63 pp |
| 50,000 | 0.22 pp | 0.40 pp | 0.20 pp | 0.28 pp |
| 100,000 | 0.15 pp | 0.28 pp | 0.14 pp | 0.20 pp |

For eight simultaneous Serum-like claims, the binary thresholds at 500, 2,000, 10,000,
and 50,000 games are approximately 5.06, 2.53, 1.13, and 0.51 percentage points.

## Empirical examples

| Experiment | Observed raw effect | Raw games for 80% power | Observed weighted effect | Weighted games for 80% power |
|---|---:|---:|---:|---:|
| Generation 32 balanced package | +1.925 pp | 2,212 | +1.2925 weighted pp | 2,420 |
| Generation 34 Rain -> Dispel | -0.600 pp | 6,973 | -0.595 weighted pp | 3,706 |
| Generation 35 Culling + Rain -> Breach + Freeze | -2.800 pp | 564 | -1.950 weighted pp | 588 |
| Generation 36 Manamorphose -> Powder pilot | +1.050 pp | 7,080 | +0.675 weighted pp | 8,818 |

The Powder figures describe the pilot variance, not a valid Manamorphose-specific effect,
because the replacement-slot control invalidated that ranking.

For very close candidate ranking, the cost is much higher. Generation 34's observed
Culling-versus-Rain weighted gap was 0.1325 weighted percentage points. Its pilot
variance implies about 55,400 games merely for a 95% interval centered at the observed
effect to touch zero, and approximately 115,000 games for 80% power.

## Replacement-slot averaging

In the Serum Powder control, eight identical deck compositions differed only in which
array slot supplied Powder's random position.

- Candidate rates ranged over 4.8 percentage points at 500 games.
- Single-coupling raw paired-delta SD was 0.344.
- Eight-coupling average raw paired-delta SD was 0.213.
- The repeated-measure intraclass correlation was 0.295.
- Treating all 4,000 outputs as independent would overstate information; the effective
  count was approximately 1,306 independent observations.

| Couplings averaged per game | Unique games for +/-0.5 pp | Candidate evaluations | Unique games for 80% power at 1 pp | Candidate evaluations |
|---:|---:|---:|---:|---:|
| 1 | 18,161 | 18,161 | 9,277 | 9,277 |
| 2 | 11,761 | 23,522 | 6,008 | 12,016 |
| 4 | 8,561 | 34,244 | 4,373 | 17,492 |
| 8 | 6,961 | 55,688 | 3,556 | 28,448 |

Multiple couplings reduce variance per unique game but increase total solver work.
They are a defensible fallback and useful audit. A fixed common add-card random key is
the preferred production solution because it should retain one candidate evaluation per
game while removing most replacement-slot noise.

## Recommended experiment classes

### Mechanics and regression checks

Use 100-500 games. Report no probability claim. These runs verify card semantics,
sharding invariance, trace quality, and absence of obvious regressions.

### Exploratory broad screen

Use 1,000-2,000 unique games with fixed semantic coupling or a delta-only relevance
accelerator. Treat rankings and intervals as descriptive. Do not reuse these game seeds
for confirmation.

### Confirming a large change

For a pre-registered effect of at least 2 percentage points, 2,000-5,000 paired games are
normally adequate across the observed variance range.

### Confirming a one-point change

Use approximately 8,000 games for one Serum-like arm, 13,000 for an eight-arm family,
or 18,000 for 72 direct claims. A low-discordance local swap may need only 2,500-5,500.

### Confirming a half-point change

Use approximately 32,000 games for one Serum-like arm, 51,000 for an eight-arm family,
or 72,000 for 72 direct claims. Low-discordance swaps still need roughly 9,500 games for
one pre-registered arm.

### Ranking nearly equivalent finalists

Expect 50,000-150,000 paired games when weighted differences are around 0.1-0.25
percentage points. It may be more useful to report a set of statistically indistinguishable
finalists than to force a total ordering.

## Two-stage publication protocol

1. Freeze the source digest, deck fixture, objective, card semantics, cap policy, and RNG
   domains.
2. Run an exploratory pilot on unique game seeds to estimate discordance and paired score
   variance.
3. Select candidates without making confirmatory claims.
4. Pre-register the confirmation candidate set, minimum meaningful effect, alpha, power,
   sample size, and stopping rule.
5. Use a fresh seed block for confirmation.
6. Analyze binary outcomes with paired intervals and exact McNemar tests.
7. Analyze weighted outcomes with a paired bootstrap clustered by game seed.
8. Apply Holm correction within the pre-registered confirmatory family.
9. Report cap sensitivity and complete-case results.
10. If monitoring results before completion, use a time-uniform confidence sequence or a
    fixed group-sequential design rather than repeatedly checking ordinary 95% intervals.

Increasing 80% power to 90% raises the required game count by approximately 34% under
these normal-approximation plans.

## Remaining unidentified variance

The calculations above condition on a fixed mulligan policy. If each deck receives a
separately trained policy, policy-training seeds become another random level. Current
data do not estimate that variance, so no justified total sample size exists yet for a
claim about independently optimized policies.

That experiment requires independent policy-training replicates, holdout evaluation
seeds, and a hierarchical or cluster analysis. Evaluation games cannot compensate for
one unusually favorable threshold fit.

Likewise, 1.45%-2.4% of pairs in several retained experiments carried at least one capped
search flag. Effects below one percentage point require cap resolution or an unbiased
correction estimator; otherwise search uncertainty can be larger than the target effect.

## Reproducibility

Run:

```bash
python3 scripts/experiment_sample_size_analysis.py \
  --out-dir benchmarks/results/statistical_design
```

Generated tables:

- `absolute_rate_precision.csv`
- `binary_comparison_power.csv`
- `weighted_comparison_power.csv`
- `minimum_detectable_effect.csv`
- `empirical_pilot_power.csv`
- `slot_coupling_efficiency.csv`
- `summary.json`

## References

- Kleijnen, "Analyzing Simulation Experiments with Common Random Numbers,"
  *Management Science* 34(1), 1988, <https://doi.org/10.1287/mnsc.34.1.65>.
- Lehr, "Some Practical Considerations and a Crude Formula for Estimating Sample Size
  for McNemar's Test," 2001, <https://doi.org/10.1177/009286150103500419>.
- Holm, "A Simple Sequentially Rejective Multiple Test Procedure," 1979,
  <https://www.jstor.org/stable/4615733>.
- Howard et al., "Time-Uniform, Nonparametric, Nonasymptotic Confidence Sequences,"
  *Annals of Statistics* 49(2), 2021, <https://doi.org/10.1214/20-AOS1991>.
