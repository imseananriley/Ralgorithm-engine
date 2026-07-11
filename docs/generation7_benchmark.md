# Generation 7 State Deduplication Optimization

## Retained Changes

Generation seven preserves generation-six search order and outcomes while reducing repeated work in the close-turn search:

- The production visited-state set is replaced by a collision-safe fingerprint index. Each candidate `FastState` is hashed once, buckets store indices into the existing ordered state vector, and full equality is still checked inside a bucket. This removes the second full-state hash and one full-state clone for every accepted transition.
- Mana-dominance insertion uses the hash-map entry API, eliminating the second structural-key hash on a cache miss.
- The close-turn work queue is represented by `Vec` because it is strictly LIFO. This preserves `pop_back`/`push_back` traversal order without `VecDeque` bookkeeping.

An incremental packed-funding Pareto frontier was also implemented and isolated. It was effectively neutral on 500 fixed hands (`1.010x` ratio of medians) and slightly slower on eight policy games (`0.988x`), so it was removed.

## Balanced Paired Result

Baseline: generation-six commit `fba5a81`. Candidate: generation seven. Both use `RALGORITHM_PAYMENT_DIRECTED=packed`.

Configuration: champion fixture, turn-two resilient Rhystic/Heartwood objective, 20,000-state initial cap, 60,000-state cap rerun, stochastic simplified Gamble, four balanced `AB/BA` repetitions, a 15-second initial cooldown, and one second between processes.

| Workload | Outcome parity | Baseline median | Candidate median | Ratio of medians | Paired median |
| --- | ---: | ---: | ---: | ---: | ---: |
| 400 fixed hands | 400/400 exact | 5.554 s | 5.239 s | 1.060x | 1.070x |
| 12 full policy games | 12/12 exact | 3.317 s | 3.147 s | 1.054x | 1.050x |

Fixed hands produced 149 successes and 25 caps in both engines. Full policy produced 8/12 successes and no caps in both engines.

## Larger Outcome Audit

A separate 1,000-hand paired audit used seed `2026071101` and the same fixed-hand settings. Both generations produced 391 successes and 71 caps, with zero turn, cap, label, or unsupported-status discordances.

The release suite contains 45 passing tests and passes `cargo clippy --release --all-targets -- -D warnings`. The new state-index invariant verifies that a forced fingerprint-bucket collision still requires full state equality.

## Reproduction

```bash
RALGORITHM_PAYMENT_DIRECTED=packed python3 scripts/benchmark_solver_generations.py \
  --baseline-bin /tmp/ralgorithm-gen6/target/release/rhystic-core-smoke \
  --candidate-bin target/release/rhystic-core-smoke \
  --hands 400 \
  --policy-games 12 \
  --repeats 4 \
  --cooldown-seconds 1 \
  --out benchmarks/results/generation7_final.json
```

## Next Bottlenecks

The remaining profile is dominated by structural-key construction and hashing, hand/battlefield movement, action-vector allocation, and packed funding-frontier work. The next substantial step should use a compact owned structural key or a collision-safe structural fingerprint index; it should be isolated carefully because canonicalized library tails are part of mana-dominance semantics.
