# Mild Rhystic/Heartwood Optimization Experiment

## Objective

Optimize the current bloodstained/Rain baseline for early Rhystic Study or Heartwood Storyteller resolution while keeping the interaction/win-card structure mostly intact.

Baseline deck:

`data/moxfield_tPWeAfl5uXGJdejEnaIwYw_current_rain_over_mindbreak_20260701.json`

Primary weighted score, on a 100-point per-game scale:

- Rhystic Study turn 1: 100
- Rhystic Study turn 2: 75
- Heartwood Storyteller turn 1: 65
- Heartwood Storyteller turn 2: 50
- Miss/cap miss: 0

This keeps Rhystic materially preferred, but Heartwood is no longer treated as a small consolation prize. Sensitivity checks should rerun finalists at `100/80/70/55` and `100/70/55/40`.

The Rust full-sim path now supports weighted policy EV. The Python-only screen path can still report weighted outcomes after the fact, but publication-grade weighted optimization should use `--rust-full-sim --weighted-policy-ev`.

## Fixed Simulator Assumptions

- Target: `rhystic_heartwood`.
- Engine policy: `resilient`.
- Gamble: `stochastic` with simplified legal random discard in Rust full-sim.
- Gemstone Caverns live rate: `0.75`, pre-sampled and known before mulligan decisions.
- Production solver semantics: normal hidden-library behavior, with strict shuffle-diff only for manual audit diagnostics.
- Commander legal cards only.

## Stage 0: Weighted Policy EV Mode

For true weighted-objective runs, the Rust policy evaluator scores visible-hand samples by normalized outcome value:

- Rhystic Study turn 1: 1.00
- Rhystic Study turn 2: 0.75
- Heartwood Storyteller turn 1: 0.65
- Heartwood Storyteller turn 2: 0.50
- Miss/cap miss: 0.00, unless a separate cap upper-bound sensitivity is requested

Implemented behavior:

- `score_ev` and `selection_score` should average weighted sample value, not just binary hits.
- `hits` and raw success rate should still be reported separately.
- Deterministic visible hits should only short-circuit the bottom search when they achieve the maximum possible weighted value, otherwise other bottoms can still dominate.
- Final weighted inference should use Rust full-sim. Python-only runs should be labeled binary-policy/post-hoc-weighted.

## Stage 1: Locked-Card Exploration

Purpose: cheaply identify which cards are worth deeper paired tests by forcing a candidate into each visible mulligan hand. This is not an unbiased deck-rate estimate. This current locked-card runner remains binary-policy with weighted final-outcome reporting unless separately ported to Rust full-sim.

Candidate file:

`data/rhystic_study_turn12/mild_objective_locked_candidates_20260704.txt`

Suggested command:

```bash
python3 scripts/rhystic_locked_mulligan_search.py \
  --deck-json data/moxfield_tPWeAfl5uXGJdejEnaIwYw_current_rain_over_mindbreak_20260701.json \
  --out-dir data/rhystic_study_turn12/mild_locked_screen_20260704 \
  --candidate-file data/rhystic_study_turn12/mild_objective_locked_candidates_20260704.txt \
  --target rhystic_heartwood \
  --games 300 \
  --threshold-hands 40 \
  --samples-per-bottom 1 \
  --validation-samples 1 \
  --state-limit 30000 \
  --actual-rerun-state-limit 100000 \
  --workers 10 \
  --chunks-per-worker 8 \
  --seed 2026070401 \
  --gemstone-caverns-live-rate 0.75 \
  --engine-success-policy resilient \
  --gamble-mode stochastic \
  --rhystic-t1-weight 100 \
  --rhystic-t2-weight 75 \
  --heartwood-t1-weight 65 \
  --heartwood-t2-weight 50 \
  --normalize-no-caverns-gemstone-key
```

Keep cards for Stage 2 if they are top-ranked in locked weighted score, have a low bottomed rate when kept, or expose a suspicious simulator blind spot worth manual audit.

## Stage 2: Single-Swap Paired Screen

Purpose: estimate actual deck-rate deltas for candidate swaps under paired random streams using Rust weighted policy EV.

Primary swap file:

`data/rhystic_study_turn12/mild_objective_stage1_paired_swaps_20260704.txt`

Optional one-interaction-flex file:

`data/rhystic_study_turn12/mild_objective_stage1_interaction_flex_swaps_20260704.txt`

Suggested local command:

```bash
RHYSTIC_SIMPLIFIED_GAMBLE=1 python3 scripts/rhystic_paired_rate_compare.py \
  --deck-json data/moxfield_tPWeAfl5uXGJdejEnaIwYw_current_rain_over_mindbreak_20260701.json \
  --out-dir data/rhystic_study_turn12/mild_paired_screen_20260704 \
  --target rhystic_heartwood \
  --swap-file data/rhystic_study_turn12/mild_objective_stage1_paired_swaps_20260704.txt \
  --threshold-hands 80 \
  --eval-games 10000 \
  --samples-per-bottom 1 \
  --validation-samples 1 \
  --state-limit 40000 \
  --actual-rerun-state-limit 120000 \
  --workers 10 \
  --chunks-per-worker 8 \
  --seed 2026070402 \
  --gemstone-caverns-live-rate 0.75 \
  --engine-success-policy resilient \
  --gamble-mode stochastic \
  --independent-thresholds \
  --rust-full-sim \
  --rust-full-sim-games-per-shard 2500 \
  --rust-full-sim-shard-workers 4 \
  --weighted-policy-ev \
  --bootstrap-samples 1000 \
  --rate-half-width 0.0025 \
  --score-half-width 0.002 \
  --rhystic-t1-weight 1.0 \
  --rhystic-t2-weight 0.75 \
  --heartwood-t1-weight 0.65 \
  --heartwood-t2-weight 0.50 \
  --normalize-no-caverns-gemstone-key \
  --adaptive-threshold-sampling
```

For RunPod, use a Rust-capable job wrapper that copies `rust/rhystic_core`, builds `rhystic-core-smoke` on the pod, and runs the same `--rust-full-sim --weighted-policy-ev` command. The older Python-only paired RunPod wrapper should be treated as a screen, not final weighted-policy inference.

## Stage 3: Beam Package Search

Build packages from Stage 2 winners rather than taking all positive one-card swaps at face value.

Procedure:

1. Start from baseline.
2. Keep the top 6 to 10 swaps by paired weighted-score delta, preferring swaps with positive bootstrap lower CI or high win/loss discordance.
3. Generate packages of 2 to 4 non-conflicting swaps.
4. Evaluate each package against baseline and against its parent package.
5. Keep only packages that improve weighted score without a large raw success-rate loss.

Use semicolon-separated grouped swaps in the swap file for packages, for example:

```text
package_a: Rain of Filth=Beseech the Mirror; Glimmervoid=Sea of Clouds
```

## Stage 4: Final Validation

For the final 1 to 3 deck candidates, run Rust full-sim absolute validation for baseline and each finalist:

```bash
RHYSTIC_SIMPLIFIED_GAMBLE=1 python3 scripts/rhystic_belief_mulligan_sim.py \
  --target rhystic_heartwood \
  --deck-json <finalist-deck-json> \
  --threshold-hands 500 \
  --eval-games 200000 \
  --samples-per-bottom 1 \
  --validation-samples 1 \
  --state-limit 200000 \
  --actual-rerun-state-limit 400000 \
  --seed 2026070403 \
  --gemstone-caverns-live-rate 0.75 \
  --engine-success-policy resilient \
  --gamble-mode stochastic \
  --rust-full-sim \
  --weighted-policy-ev \
  --rhystic-t1-weight 1.0 \
  --rhystic-t2-weight 0.75 \
  --heartwood-t1-weight 0.65 \
  --heartwood-t2-weight 0.50 \
  --rust-full-sim-games-per-shard 25000 \
  --rust-full-sim-shard-workers 8 \
  --adaptive-threshold-sampling \
  --normalize-no-caverns-gemstone-key \
  --json-out data/rhystic_study_turn12/<name>.json \
  --suppress-json-stdout
```

Analyze final absolute results by recomputing the same weighted score from `evaluation.game_records` or `validation_records`, and report both weighted score and raw hit rate.

## Decision Rule

Use Stage 1 for discovery only. Use Stage 2 paired deltas for single-swap inference. Use Stage 3 paired package deltas for nonlinear interaction effects. Use Stage 4 Rust full-sim for publication-quality absolute rates.

A swap/package is a recommended optimization only if:

- Paired weighted-score delta is positive after bootstrap CI review.
- It survives the sensitivity weights.
- It does not materially reduce raw Rhystic rate unless the Heartwood gain is intentional and statistically clear.
- It does not consume more than the explicitly allowed interaction-flex slot.
