# Generation Eleven: Production Packed Opening Batch

## Complete Execution Path

Generation eleven connects the packed information model to a production JSONL command:

```text
rhystic-core-smoke opening-batch-jsonl
```

Each request supplies one or more slot-aligned variants, a root seed, a resumable sample range, maximum turn, search depth, and either compiled-policy or strict-reference evaluation. The response contains paired summaries, compact discordances, influence statistics, latency quantiles, throughput, and the next sample index.

## Mulligan And Randomness

The adapter implements the Commander London sequence `7, 7, 6, 5, 4, 3`. The first mulligan is free. Every offered seven-card hand comes from an independently domain-separated ChaCha8 slot permutation shared by all variants. Keep and bottom decisions receive only the visible hand. Bottoms return to the unknown library.

Gemstone Caverns live status is sampled once per game at three quarters and is known for every mulligan decision in that game. A live kept Caverns then enumerates visible exile choices. The action policy or reference solver chooses among those starts without observing unknown order.

## Multifidelity

Policy-mode requests may specify a correction numerator and denominator. Membership in the strict-reference correction sample depends only on root seed and sample index. Every low result enters the low estimator; selected games additionally enter the paired high-minus-low correction. The response reports the corrected mean, standard error, low mean, correction mean, and both sample counts per variant.

## Added Creature Coverage

This generation also ports Birds of Paradise, Deathrite Shaman, Ragavan, and Tinder Wall, plus Mystical Tutor and Eldritch Evolution. Birds respects summoning sickness, Tinder can sacrifice immediately, Ragavan uses the project assumption that it connects and creates a Treasure, and Deathrite's external-land availability is an explicit model setting that defaults on. Angel's Grace can satisfy an otherwise lethal Summoner's Pact trigger when white payment exists.

## Smoke Result

A release CLI smoke over a deterministic eight-card fixture completed 20 strict-reference games at 8,668 games/s. It emitted p50/p95/p99 sample latencies of 106/114/286 microseconds, a resumable `next_sample` of 20, and reproducible influence counts.

## Remaining Engineering Work

The packed production path is complete for single-process execution. Parallel worker orchestration remains in the existing shard/run infrastructure rather than this Rust adapter. Allocation totals and formal 1/2/4/8 worker scaling curves remain benchmark work. Post-engine interaction/value cards remain inert by design unless they participate in an early engine line; any newly discovered early line must be added with a focused transition test and a legacy differential fixture.
