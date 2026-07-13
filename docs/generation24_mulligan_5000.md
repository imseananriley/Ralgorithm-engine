# Generation 24: 5,000-game frozen-policy validation

The frozen mulligan evaluator successfully generated 5,000 complete game and
validation records in approximately 7 minutes 28 seconds. Its final JSON was
18.4 MB. It did not fail to aggregate; an in-progress zero-byte output file was
mistaken for termination before the still-running process was identified.

The evaluator's cost comes from solving up to six mulligan stages per game,
17,082 visible-hand cache entries, 623 high-limit reruns, four independent shard
contexts, and retaining 10,000 detailed records until final serialization.

The optimized validated hybrid classified the completed corpus in 30.39 seconds
wall and 118.79 CPU-seconds:

| Metric | Result |
| --- | ---: |
| Strict deterministic successes | 3,017 / 5,000 |
| Strict deterministic rate | 60.34% |
| Wilson 95% interval | 58.98%-61.69% |
| Final caps | 228 / 5,000 |
| Upper rate if every cap succeeds | 64.90% |
| Packed confirmations | 1,688 |
| Exact-rescue successes | 1,329 |
| Legacy labels recalled | 2,991 / 3,071 |
| New strict successes | 26 |

The 80 old-only labels and 228 caps require separate audit; the strict result is
a deterministic lower bound, not evidence that those hands are true failures.
