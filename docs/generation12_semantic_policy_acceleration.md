# Generation Twelve: Semantic Policy Acceleration

## Changes

Unknown draws in the compiled bulk policy may now aggregate cards by a mechanically generated semantic key. The key includes color, flags, action templates, payment gates, opening land/artifact/spell/creature semantics, and explicit special handling for Angel's Grace. Known-top cards remain exact observations.

Exact-slot draws remain available through the batch request. Strict-reference evaluation and multifidelity high samples always use the exact-slot model. Semantic quotienting is therefore a low-fidelity accelerator, not part of the strict estimand.

The visible action policy now scores known-top cards and untapped mana sources. This fixes two slot-order defects: top tutors previously tied across targets, and played lands previously had nearly identical scores regardless of production.

## Benchmark

On 100 paired champion-list policy games at four workers:

| Mode | Games/s | Mean score | Nonzero rate |
| --- | ---: | ---: | ---: |
| Semantic draws, old policy | 14.11 | 0.07714 | 92% |
| Exact-slot draws, old policy | 8.73 | 0.07672 | 92% |
| Semantic draws, known-top policy | 18.11 | 0.09477 | 89% |
| Semantic draws, source-aware policy | 12.46 | 0.13496 | 91% |

The final source-aware policy improves mean objective value by about 42% relative to the known-top-only policy, at a 31% throughput cost. Semantic draws were 1.62x faster than exact-slot draws in the controlled old-policy comparison.

The semantic and exact compiled policies differed slightly because deterministic policy tie-breaking is not invariant to every equivalent slot substitution. This is why semantic draws are restricted to low fidelity. Focused reference expectimax tests still verify equal value for proven equivalent draw classes.

## Strict Reference Limit

An exact depth-16 calibration with two strict corrections exceeded 2 GB and 45 seconds, so it was terminated. The unbounded strict solver is not safe for routine local correction sampling on full champion states. Bulk paired experiments may proceed with the compiled policy, but publication corrections require a bounded/optimized strict solver or remote memory-isolated workers. No capped strict result should be inserted into the correction estimator as though it were exact.
