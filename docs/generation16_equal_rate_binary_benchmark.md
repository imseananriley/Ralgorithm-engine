# Generation 16: Equal-rate binary benchmark

## Question

How fast is the current packed Rust search relative to the pre-fork Rust search
when both identify approximately the same number of successful keeps?

This is deliberately narrower than a probability-estimation benchmark. It replays
the same 1,000 selected post-mulligan keeps and asks each engine for a binary
turn-two engine result.

## Corpus and controls

- Corpus: `prefork_recall_1000_20260712/prefork_corpus.json`
- Seed: `2027071101`
- Games: 1,000 recorded post-mulligan keeps
- Pre-fork limits: 20,000 states, followed by 60,000 states for capped games
- Current limits: depth 14, two action candidates per policy family, adaptive
  discrepancy tiers D0 through D5
- Workers: 8
- The current replay preserves the first three recorded library cards. The fourth
  packed known-top slot remains available for top-deck tutor effects.
- Three complete repetitions were run. Child CPU time includes all solver workers.

## Results

| Metric | Pre-fork | Current |
|---|---:|---:|
| Successful keeps | 619 | 621 |
| Median wall time | 45.618 s | 1.710 s |
| Median CPU time | 44.947 s | 10.548 s |
| Wall-time speedup | 1.00x | 26.67x |
| CPU-work speedup | 1.00x | 4.26x |

Wall-time ranges were 45.601-45.850 seconds pre-fork and 1.463-1.747
seconds current. CPU-time ranges were 44.876-45.078 seconds pre-fork and
9.758-10.684 seconds current.

The hit sets are not equivalent. The current engine recalls 494 of the 619
pre-fork hits, misses 125 of them, and finds a positive-probability branch in 127
games that the pre-fork engine did not classify as wins. Therefore, 26.67x is an
**equal nominal hit-count throughput** result, not an equal realized success-rate,
equal-recall, or semantic-equivalence result.

## Interpretation

The speed improvements are substantial for existential branch discovery: parallel
packed states and adaptive discrepancy search reduce wall time by about 27x and
total CPU work by about 4x. This does not establish the same speedup for exact
realized-order classification or integrated probability estimation.

The disagreement also prevents using this benchmark as evidence that the current
engine is already an unbiased replacement. After the three recorded top cards are
consumed or randomized, the existence solver accepts any positive-probability
branch. Some current-only hits may consequently be optimistic. Conversely, the
125 pre-fork-only hits identify incomplete current search coverage under these
exact-top constraints.
Probability experiments should continue to use bounded chance integration; this
existential benchmark is appropriate only for measuring binary line-search speed.

## Artifacts

- `scripts/benchmark_binary_recall.py`
- `binary_equal_rate_repeat_1.json`
- `binary_equal_rate_repeat_2.json`
- `binary_equal_rate_repeat_3.json`
