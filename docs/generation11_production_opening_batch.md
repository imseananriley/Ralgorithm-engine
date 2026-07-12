# Generation Eleven: Production Packed Opening Batch

## Complete Execution Path

Generation eleven connects the packed information model to a production JSONL command:

```text
rhystic-core-smoke opening-batch-jsonl
```

Each request supplies one or more slot-aligned variants, a root seed, a resumable sample range, maximum turn, search depth, and either compiled-policy or strict-reference evaluation. The response contains paired summaries, compact discordances, influence statistics, latency quantiles, throughput, and the next sample index.

## Mulligan And Randomness

The adapter implements the Commander London sequence `7, 7, 6, 5, 4, 3`. The first mulligan is free. Every offered seven-card hand comes from an independently domain-separated ChaCha8 slot permutation shared by all variants. Keep and bottom decisions receive only visible information. Bottoms remain a known bottom stack until a shuffle merges them into the unknown library.

Gemstone Caverns live status is sampled once per game at three quarters and is known for every mulligan decision in that game. A live kept Caverns then enumerates visible exile choices. The action policy or reference solver chooses among those starts without observing unknown order.

## Multifidelity

Policy-mode requests may specify a correction numerator and denominator. Membership in the strict-reference correction sample depends only on root seed and sample index. Every low result enters the low estimator; selected games additionally enter the paired high-minus-low correction. The response reports the corrected mean, standard error, low mean, correction mean, and both sample counts per variant.

## Added Creature Coverage

This generation also ports Birds of Paradise, Deathrite Shaman, Ragavan, and Tinder Wall, plus Mystical Tutor and Eldritch Evolution. Birds respects summoning sickness, Tinder can sacrifice immediately, Ragavan uses the project assumption that it connects and creates a Treasure, and Deathrite's external-land availability is an explicit model setting that defaults on. Angel's Grace can satisfy an otherwise lethal Summoner's Pact trigger when white payment exists.

## Smoke Result

A release CLI smoke over a deterministic eight-card fixture completed 20 strict-reference games at 8,668 games/s. It emitted p50/p95/p99 sample latencies of 106/114/286 microseconds, a resumable `next_sample` of 20, and reproducible influence counts.

## Parallel Execution

The request may select a worker count. Workers claim disjoint sample chunks from an atomic queue and share immutable compiled variants. Reports are sorted by sample range before merge, preserving reproducible accumulation order while balancing long-tail searches.

A 200-game champion-list policy check produced the same mean score (`0.0805041335`) at every worker count:

| Workers | Games/s | Wall time | Speedup |
| ---: | ---: | ---: | ---: |
| 1 | 3.50 | 57.13 s | 1.00x |
| 2 | 5.25 | 38.09 s | 1.50x |
| 4 | 6.56 | 30.50 s | 1.87x |

This run followed a release build and used short cooldowns, so it is a functional scaling check rather than a thermally controlled publication benchmark. Merged-shard latency quantiles are conservative powers-of-two histogram bounds.

## Remaining Engineering Work

Generation twelve supersedes the provisional mulligan, outcome, and multifidelity methodology in this document. See `docs/generation12_correctness_and_speed.md`.
