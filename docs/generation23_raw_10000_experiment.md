# Generation 23: 10,000-hand capacity experiment

## Estimand

This experiment evaluates 10,000 independent raw seven-card hands from the
champion deck, seed `2027072301`. Each sample retains the complete shuffled
library order and a presampled 75% live Gemstone Caverns status. It uses packed
D0-D2 search, full-library witness validation, and single-pass 60,000-state exact
rescue with four workers.

This is a raw-hand capacity and classifier experiment. It does not include
London mulligan decisions and must not be reported as the deck's mulligan-policy
success probability.

## Execution

The initial monolithic packed pass completed, but aggregating 7,981 exact-rescue
requests in one driver process exceeded the practical memory limit. The same
10,000 indexed games were therefore evaluated as ten disjoint 1,000-game shards.
The retained aggregate contains 10,000 unique sample indices from 0 through
9,999 with no duplicate successful index.

## Results

| Metric | Result |
| --- | ---: |
| Deterministic successes | 3,687 / 10,000 |
| Deterministic lower-bound rate | 36.87% |
| Wilson 95% interval | 35.93%-37.82% |
| Final capped searches | 546 / 10,000 |
| Upper rate if every cap succeeds | 42.33% |
| Packed confirmed witnesses | 2,019 |
| Exact-rescue successes | 1,668 |
| Raw packed existential hits | 5,748 |
| Invalid packed witnesses | 442 |
| Probabilistic packed witnesses | 3,287 |

Ten sequential shards consumed 95.94 seconds wall and 364.21 CPU-seconds. Median
shard wall time was 9.55 seconds, or 104.24 classified hands per wall-second.

## Implications

The optimized hybrid runs stably at ten times the prior validation corpus, but
raw seven-card hands are harder than policy-selected final keeps: 5.46% still
reach the 60,000-state cap. Publication estimates should report the deterministic
lower bound and cap upper bound until those cases receive a higher-limit audit or
an unbiased correction sample.

The driver now streams exact-rescue requests and results in bounded 1,000-hand
chunks. A 100-hand chunk parity rerun reproduced every hit, witness status,
rescue hit, and cap ID from shard zero. The Rust search scaled; the failed
monolithic attempt was caused by driver-side aggregation, not search semantics.
