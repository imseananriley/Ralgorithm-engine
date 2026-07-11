# Overhaul Status And Pre-Fork Benchmark

## Scope

The intended overhaul is the architecture in `docs/architecture.md`: a visible-information production engine with a semantic card registry, fixed-size observable state, explicit chance nodes, a compiled policy, paired batch evaluation, and multifidelity correction.

The exact pre-fork baseline is commit `11104ea`, the clean snapshot used to initialize this repository. The current candidate is generation seven at commit `6e722dc` with `RALGORITHM_PAYMENT_DIRECTED=packed`.

## Goal Status

| Goal | Status | Remaining work |
| --- | --- | --- |
| Clean, reproducible engine fork | Complete | Keep source manifests and retained benchmark summaries current. |
| Shared card registry | Partial | Flags, action templates, and payment gates drive production dispatch. Commander legality, tutor predicates, UI metadata, and generated semantic coverage are not yet owned by one registry. |
| Fixed-size `PackedState` and `PackedLibrary` | Partial production | `PackedStateV2` now has canonical permanent instances, duplicate tokens, attachments, counters, and commander state. The concrete opening model uses it, but bulk production still uses `FastState`. |
| Packed mana/payment closure | Partial production | Cost-specific plans are integrated, but paid actions are materialized back into `FastState`. Offer, sacrifice effects, Crop Rotation, Beseech, LED tutor lines, active Ragavan, Rain of Filth, City floating, and several combinations fall back to the legacy graph. |
| Information-state reference solver | Partial production | The concrete land/engine opening slice uses explicit chance draws and weighted terminal values. Most Nick Fury card actions remain unsupported. |
| Compiled no-lookahead policy | Kernel only | No production action or mulligan policy implements `CompiledPolicy`; bulk experiments still use the bridge evaluator. |
| Paired `BatchEvaluator` | Missing | Slot permutations, multi-variant cohorts, influence tracking, compact discordant records, and delta-only execution remain in scripts or are absent. |
| Multifidelity estimator | Kernel only | The accumulator is tested, but no production pipeline draws strict correction samples against fast policy results. |
| Performance gates | Partial | Aggregate throughput and parity are retained. State/action counters, allocation totals, p50/p95/p99 latency, and 1/2/4/8/pod worker scaling are not collected by the production benchmark. |
| Publication estimand | Incomplete | The current fixed-library solver remains an oracle/policy approximation and can receive hidden library order. It is not the visible-information estimand required by the architecture. |

The practical result is a substantially better bridge engine, not a completed next-generation engine. Generations two through seven improved immutable library handling, registry dispatch, canonicalization, packed payment planning, tutors, and state deduplication while preserving a differential oracle.

## Large Local Benchmark

Machine: Apple M4, 10 cores, 16 GiB RAM, macOS arm64. The benchmark used one nice-adjusted worker, a 15-second initial idle, two seconds between child processes, and four balanced `AB/BA` timing repetitions. Process startup is included.

Workloads use the champion fixture, seed `2026071101`, turn-two resilient Rhystic/Heartwood objective, stochastic simplified Gamble, a 20,000-state initial cap, and a 60,000-state policy rerun cap. Both binaries received identical requests. Repetitions improve timing precision; they do not increase the 3,000-hand or 100-game statistical sample.

| Workload | Pre-fork | Current | Observed success delta | Caps | Median time | Throughput speedup |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 3,000 fixed hands | 1,080/3,000 (36.00%) | 1,145/3,000 (38.17%) | +2.17 pp | 378 -> 210 | 70.953 s -> 41.083 s | 1.727x |
| 100 full-policy games | 63/100 (63%) | 72/100 (72%) | +9 pp | 2 cap misses -> 0 | 49.823 s -> 18.797 s | 2.651x |

Fixed-hand throughput increased from 42.28 to 73.02 hands/s; full-policy throughput increased from 2.01 to 5.32 games/s. Paired median speedups were `1.729x` and `2.633x`, respectively.

An outcome-only replay recovered paired directions omitted by the timing harness:

| Workload | Current-only wins | Pre-fork-only wins | Paired delta 95% normal CI | Exact McNemar p |
| --- | ---: | ---: | ---: | ---: |
| Fixed hands | 66 | 1 | +1.64 to +2.70 pp | `9.22e-19` |
| Full policy | 9 | 0 | +3.36 to +14.64 pp | `0.00391` |

There were 29 fixed hands and six policy games where both implementations succeeded but selected a different successful outcome. The single pre-fork-only fixed-hand result should be traced before claiming the current search is a monotonic coverage superset. The 100-game policy rate is still a benchmark-scale estimate, not a publication-quality deck-strength estimate.

Raw local timing output: `benchmarks/results/current_vs_prefork_large_local.json` (ignored machine-local artifact). Candidate source digest: `fdaed2749f24977761999120861606355709512f4372faef`.

## Completion Plan

1. Define `PackedStateV2` around observable card zones plus a bounded permanent-instance array. It must represent tokens, repeated generated objects, attachments, counters, tapped/fresh status, commander state, and turn resources without allocating.
2. Move card semantics into one declarative `CardSpec`: legality and color identity, costs, types, tutor predicates, mana activations, strategic transitions, random effects, UI metadata, and generated rule tests.
3. Implement a concrete Nick Fury information model in vertical slices. Port lands and deterministic mana first, then engines and tutors, then sacrifice/priority/random lines. Differentially compare every reachable transition against `FastState`, while explicit chance nodes replace access to unknown order.
4. Apply payment witnesses directly to packed states. Remove the current funded-`FastState` materialization and eliminate legacy fallbacks one interaction family at a time.
5. Implement and freeze a visible-information mulligan/action policy, then build the paired batch evaluator with common randomness, slot-aligned variants, influence tracking, compact discordances, and resumable shards.
6. Connect multifidelity estimation: bulk compiled-policy rollouts plus a random, independently selected strict-reference correction sample with reported uncertainty.
7. Instrument the required performance gates and paired inference. Add candidate-only/baseline-only counts and McNemar tests to generation benchmarks, plus latency quantiles, search counters, allocations, and worker-scaling curves.

Further `FastState` micro-optimization should be limited to changes that immediately reduce experiment cost. The primary engineering path is now the complete packed information-state migration; otherwise the project will continue making the oracle bridge faster without reaching the intended estimand.
