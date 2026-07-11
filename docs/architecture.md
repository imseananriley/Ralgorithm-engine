# Next-Generation Architecture

## Objective

Estimate early Rhystic Study and Heartwood Storyteller performance under decisions that use only visible information, then compare legal deck variants with paired randomness and fixed interaction/value constraints.

## Components

- `CardSpec`: one semantic registry for legality, card properties, action templates, tutor predicates, UI metadata, and generated tests.
- `PackedState`: fixed-size observable zones and permanent status represented by bitsets.
- `PackedLibrary`: exact unknown-card mask plus a small known-top stack. Semantic-class quotienting is added only after equivalence is mechanically verified.
- `ManaClosure`: collapses commuting resource-action orders into Pareto-optimal resource outcomes while retaining a payment witness.
- `ReferenceSolver`: information-state expectimax with explicit chance nodes and memoization.
- `PolicyEvaluator`: frozen no-lookahead mulligan and action policy used for bulk rollouts.
- `BatchEvaluator`: paired slot permutations, multi-variant cohorts, influence tracking, and compact discordant output.
- `MultiFidelityEstimator`: fast-policy estimate plus a random strict-solver correction sample.

## Correctness Rule

The fixed-library solver is an oracle upper bound, not the publication estimand. No next-generation action-selection API may receive an ordered unknown library. Known top cards are explicit observations; all other draws and random discards are chance outcomes.

## Performance Gates

Every retained pass records:

- states expanded and deduplicated;
- strategic and resource actions generated;
- mana-closure calls and outcomes;
- transposition hits;
- chance nodes;
- allocated bytes where available;
- p50, p95, and p99 game latency;
- games per second at 1, 2, 4, 8, and pod-scale workers;
- exact parity differences or documented intentional semantic differences.
