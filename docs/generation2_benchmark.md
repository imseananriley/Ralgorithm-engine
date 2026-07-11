# Generation 2 Benchmark

## Compared Implementations

- Baseline: commit `11104ea`, inherited `Vec<CardId>` library state and name-based card classification.
- Candidate: shared immutable library storage with copy-on-write mutation, cached content hashes, cached canonical hidden-tail views, registry-backed card metadata, and the next-generation reference/policy kernels.

Both binaries received identical JSON requests, deck order, seeds, state limits, objective weights, and mulligan thresholds. Timed runs alternated baseline/candidate order (`AB`, then `BA`) to reduce thermal and ordering bias. Process startup is included for both.

## Final Local Result

Configuration: champion 99-card fixture, turn-two resilient Rhystic/Heartwood objective, 20,000-state initial cap, 60,000-state cap rerun, stochastic simplified Gamble, and three timed repetitions.

| Workload | Outcome parity | Baseline median | Candidate median | Speedup |
| --- | ---: | ---: | ---: | ---: |
| 300 fixed hands | 300/300 exact | 7.417 s | 6.578 s | 1.127x |
| 30 full policy games | 30/30 exact | 21.106 s | 19.773 s | 1.067x |

Fixed hands produced 110/300 successes in both engines: 36.67%, Wilson 95% interval 31.41%-42.26%. Full policy produced 17/30 successes in both engines: 56.67%, Wilson 95% interval 39.20%-72.62%. Cap counts also matched: 40 fixed-hand caps and one full-policy cap in each generation.

The five-repeat microbenchmark moved fast-state clone/hash performance from 37.7x to 64.1x relative to the original string fixture representation. Fast action generation moved from 11.5x to 13.1x. The packed next-generation state remained 144 bytes and measured 164.7 million state operations per second.

Zero discordance demonstrates exact agreement on this paired corpus; it does not prove equivalence for every possible state. The 30-game policy rate is a correctness/performance check and is too small for a precise deck-strength estimate.

## Reproduction

Build commit `11104ea` and the candidate in separate worktrees, then run:

```bash
python3 scripts/benchmark_solver_generations.py \
  --baseline-bin /path/to/baseline/target/release/rhystic-core-smoke \
  --hands 300 \
  --policy-games 30 \
  --repeats 3 \
  --state-limit 20000 \
  --actual-rerun-state-limit 60000 \
  --out benchmarks/results/solver_generation_final.json
```

Raw benchmark JSON is intentionally ignored because machine-local timing artifacts should not be treated as source. Retained summaries must include source manifests and the exact command.
