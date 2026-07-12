# Overhaul Status And Pre-Fork Benchmark

> Historical generation-seven snapshot. For the current mulligan, bounded-search, multifidelity, validation, and throughput status, see `docs/generation12_correctness_and_speed.md`.

## Scope

The intended overhaul is the architecture in `docs/architecture.md`: a visible-information production engine with a semantic card registry, fixed-size observable state, explicit chance nodes, a compiled policy, paired batch evaluation, and multifidelity correction.

The exact pre-fork baseline is commit `11104ea`, the clean snapshot used to initialize this repository. The current candidate is generation seven at commit `6e722dc` with `RALGORITHM_PAYMENT_DIRECTED=packed`.

## Goal Status

| Goal | Status | Remaining work |
| --- | --- | --- |
| Clean, reproducible engine fork | Complete | Keep source manifests and retained benchmark summaries current. |
| Shared card registry | Partial | Flags plus opening land, artifact, spell, and creature semantics drive the packed production path. Commander legality and UI metadata are not yet fully registry-owned. |
| Fixed-size `PackedState` and `PackedLibrary` | Partial production | `PackedStateV2` owns the exact land and principal artifact opening slice. Unsupported spell families and the bulk CLI still require migration from `FastState`. |
| Packed mana/payment closure | Partial production | Engine, artifacts, commander, equip, rituals, tutors, sacrifice spells, Rain, Crop, and Demonic-plus-LED apply directly to `PackedStateV2`; City floating is retained explicitly. Offer and creature-chain combinations remain. |
| Information-state reference solver | Partial production | Lands, principal artifacts, deterministic tutors/rituals, Manamorphose, Gamble, Noxious, GSZ, Pact upkeep, Crop, Rain, Rhystic, and Heartwood use visible information and explicit chance. Mana creatures and creature-chain tutors remain. |
| Compiled no-lookahead policy | Production screening | The action policy scores visible known tops and mana sources; the mulligan policy sees only the visible hand. Semantic draw quotienting accelerates low-fidelity screens, while strict samples retain exact slots. |
| Paired `BatchEvaluator` | Production | The `opening-batch-jsonl` adapter implements aligned variants, Commander mulligans, resumable ranges, influence, discordances, paired inference, latency histograms, and merge-equivalent multiworker execution. |
| Multifidelity estimator | Production | The same adapter runs policy bulk samples and seed-only independent strict-reference corrections with uncertainty output. |
| Performance gates | Partial | Opening direct/micro parity, counters, mergeable latency histograms, throughput, exact-result 1/2/4-worker scaling, and semantic low-fidelity acceleration are present. The strict full-state reference still needs memory/time bounds and optimization. |
| Publication estimand | Implemented for covered semantics | Packed batch decisions receive only visible hands/states and explicit chance outcomes. Unsupported early card lines must still be corrected or ported before a full-deck claim. |

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
