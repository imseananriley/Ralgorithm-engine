# Rhystic/Heartwood Paired Simulation Methodology

## Objective

Estimate how single-card substitutions affect early advantage-engine deployment in the Nick Fury five-color Commander shell.

Primary endpoint:

- Weighted early-engine score per shuffled context:
  - Rhystic Study turn 1: 100
  - Rhystic Study turn 2: 60
  - Heartwood Storyteller turn 1: 20
  - Heartwood Storyteller turn 2: 10
  - Miss: 0

Secondary endpoints:

- Binary success by turn 2.
- Turn-1 success.
- Engine-specific counts.
- Cap-miss upper bound.

## Experimental Design

Use paired simulation contexts for all card comparisons.

For each `game_index` and mulligan `stage`, the simulator generates the visible hand from a stable seed. When variants preserve card positions in the deck list, unchanged cards remain aligned across variants. This creates paired outcomes:

```text
context_i -> baseline outcome_i
context_i -> candidate outcome_i
delta_i = candidate_score_i - baseline_score_i
```

Paired analysis is preferred over independent samples because hand difficulty is shared across variants, reducing variance.

## Card Replacement Semantics

Two tests answer different questions:

- Blank ablation: replace a card with an inert Commander-legal blank such as `Scour from Existence`. This estimates the isolated contribution of the removed card under the model.
- Real substitution: replace a card with an actual candidate such as `Swan Song`. This estimates the real deckbuilding swap and includes all side effects, including Chrome Mox imprint value, blue pitch density, and support-card utility.

Both are valid, but they must not be mixed in interpretation.

## Statistical Inference

For binary success:

- Report baseline rate, candidate rate, paired rate delta, and normal CI over per-context deltas.
- Report candidate-only and baseline-only successes.
- Use an exact McNemar/binomial sign test on discordant binary outcomes.

For weighted score:

- Report mean paired score delta.
- Report normal CI over per-context score deltas.
- For final publication-grade runs, also report a paired bootstrap percentile CI using a fixed bootstrap seed.

For sample-size planning:

```text
n_half_width = ceil((1.96 * sd(delta) / desired_half_width)^2)
```

Observed-power planning:

```text
n_power ~= ceil(((1.96 + 0.8416) * sd(delta) / abs(mean_delta))^2)
```

Interpret observed-power estimates cautiously; they are planning aids, not evidence.

## Sequential Plan

Use staged paired runs:

1. Smoke: 40-200 contexts, catches model bugs and gross direction errors.
2. Pilot: 2,000-5,000 contexts, estimates variance and discordance.
3. Main: enough contexts to hit the preselected CI half-width.
4. Confirmation: rerun high-impact conclusions under a different seed window and, where possible, a stricter state cap.

Avoid claiming small deltas from the smoke or pilot stage.

## Multiple Comparisons

When screening many swaps, report all tested swaps and treat p-values as descriptive unless a correction or pre-registered comparison set is used.

Recommended practice:

- Use false-discovery control for broad screens.
- Use unadjusted paired CIs only for predeclared primary comparisons.
- Confirm any deckbuilding recommendation in a second paired seed window.

## Capped Search States

Primary rates count capped unresolved hands as misses.

Every report must include:

- Candidate cap misses.
- Baseline cap misses.
- Actual-rerun state limit.
- Upper-bound sensitivity where relevant.

If cap misses are asymmetric and comparable to the claimed effect size, the result is inconclusive until rerun at a higher cap.

## Reproducibility Requirements

Each run must persist:

- Deck JSON snapshot.
- Variant deck JSONs.
- Simulator settings.
- Seed.
- Threshold policy.
- Compact per-game paired deltas.
- Summary CSV and Markdown report.

The current runner is:

```bash
python3 scripts/rhystic_paired_rate_compare.py \
  --deck-json data/moxfield_ggafAahWI3KipH2u48GdVQ.json \
  --out-dir data/rhystic_study_turn12/paired_rate_example \
  --eval-games 10000 \
  --threshold-hands 80 \
  --samples-per-bottom 2 \
  --validation-samples 2 \
  --state-limit 20000 \
  --actual-rerun-state-limit 60000 \
  --workers 24 \
  --swap 'Mox Diamond=Swan Song' \
  --swap 'Rain of Filth=Swan Song'
```

RunPod wrapper:

```bash
SWAPS='Mox Diamond=Swan Song;Rain of Filth=Swan Song;Mox Opal=Swan Song' \
EVAL_GAMES=100000 \
scripts/runpod_rhystic_paired_rate_compare.sh
```

## Exploratory Sequential Runs

Use `scripts/rhystic_sequential_paired_compare.py` for exploratory swap screens
before committing to full 20k paired validation. It runs independent paired
shards, aggregates per-game paired deltas, and stops a variant when a
conservative interim confidence interval resolves the requested objective.

Example:

```bash
python3 scripts/rhystic_sequential_paired_compare.py \
  --deck-json data/moxfield_tPWeAfl5uXGJdejEnaIwYw_current_rain_over_mindbreak_20260701.json \
  --out-dir data/rhystic_study_turn12/sequential_screen_example \
  --objective rate \
  --chunk-games 2000 \
  --min-games 4000 \
  --max-games 20000 \
  --workers 16 \
  --swap 'beseech_test:Birds of Paradise=Beseech the Mirror'
```

By default the wrapper uses a Bonferroni interim bound across the planned
number of looks. That makes repeated peeking conservative enough for pruning
bad candidates while preserving the full paired simulator as the source of
truth. Final publication-grade claims should still use the fixed-N paired
validation output.

## Benchmark First

Before renting long-running compute, benchmark the exact simulator settings on
the target machine:

```bash
python3 scripts/benchmark_rhystic_sim.py \
  --label local_8_workers_smoke \
  --deck-json data/moxfield_tPWeAfl5uXGJdejEnaIwYw_current_rain_over_mindbreak_20260701.json \
  --threshold-hands 20 \
  --eval-games 200 \
  --workers 8 \
  --repeat 3
```

The output CSV reports elapsed seconds and games/second. Use the same benchmark
on CPU-only RunPod pods and GPU-attached CPU pods to choose the cheapest
throughput source.

## Publication Standard

For a paper-level claim, include:

- Exact decklist and card legality assumptions.
- Complete simulator version and commit hash.
- Full method for mulligan policy, bottom selection, Gemstone Caverns pre-sampling, stochastic Gamble, and state caps.
- Predeclared primary endpoint and primary comparisons.
- Paired CIs, McNemar test, cap sensitivity, and second-window confirmation.
- A public artifact bundle with compact paired deltas sufficient to reproduce every table.
