# Generation 3 Benchmark

## Scope

Generation three keeps the generation-two search semantics and state representation while reducing work in the production action generator:

- `PersistentLibrary` caches exact membership for the common card-ID range.
- Each registry card compiles to an `ActionTemplateMask`.
- A single hand scan selects only strategic generator groups that can produce actions.
- Tutor targets use allocation-free ranking while preserving the legacy priority classes.
- Policy benchmark records are sorted by `game_index` before paired comparison.

The legacy action order and substring-based priority behavior remain deliberate compatibility constraints. A shadow test invokes every legacy generator group and compares the ordered action stream against registry dispatch over 96 fixtures.

## Paired Results

Configuration: champion 99-card fixture, turn-two resilient Rhystic/Heartwood objective, 20,000-state initial cap, 60,000-state cap rerun, stochastic simplified Gamble, and three interleaved timed repetitions. Process startup is included.

### Incremental: Generation Two to Generation Three

| Workload | Outcome parity | Generation two median | Generation three median | Speedup |
| --- | ---: | ---: | ---: | ---: |
| 300 fixed hands | 300/300 exact | 5.675 s | 4.665 s | 1.216x |
| 30 full policy games | 30/30 exact | 15.127 s | 11.868 s | 1.275x |

Both engines produced 110/300 fixed-hand successes with 40 caps and 17/30 policy successes with one cap.

### Cumulative: Original Rust Fork to Generation Three

The original baseline is commit `11104ea`. The candidate is the same generation-three source measured above.

| Workload | Outcome parity | Original median | Generation three median | Ratio of medians | Paired median |
| --- | ---: | ---: | ---: | ---: | ---: |
| 300 fixed hands | 300/300 exact | 6.920 s | 4.727 s | 1.464x | 1.487x |
| 30 full policy games | 30/30 exact | 18.197 s | 11.991 s | 1.518x | 1.511x |

The cumulative comparison is Rust-to-Rust. Earlier Python/string-state microbenchmarks measure a different migration boundary and are not included in these ratios.

Zero discordance on 330 paired outcomes is a regression check, not a proof over every reachable state. The 30-game policy sample is intentionally a throughput and parity workload, not a deck-strength estimate.

## Rejected Experiments

Arena-indexed queues, compact state keys, and a generic mana-closure wrapper were exact or nearly exact but did not materially improve representative policy throughput, so they were removed. Payment closure must be integrated at spell-cost generation to avoid recreating the existing mana microstep work around a second abstraction.

## Reproduction

Build each baseline in a separate worktree, then run:

```bash
python3 scripts/benchmark_solver_generations.py \
  --baseline-bin /path/to/baseline/target/release/rhystic-core-smoke \
  --hands 300 \
  --policy-games 30 \
  --repeats 3 \
  --state-limit 20000 \
  --actual-rerun-state-limit 60000 \
  --out benchmarks/results/generation3_final.json
```

Machine-local raw benchmark JSON remains ignored. Retained claims belong in this summary with exact source provenance and commands.
