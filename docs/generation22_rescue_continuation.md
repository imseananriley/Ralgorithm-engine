# Generation 22: Exact rescue continuation

## Single-pass escalation

The former exact-rescue schedule ran every unresolved hand at 20,000 states and
then restarted each capped failure from its initial state at 60,000. Restarting
is unnecessary. With deterministic traversal, one 60,000-state run:

- stops at the same success for hands that succeed before 20,000;
- exhausts at the same point for hands whose complete graph is below 20,000;
- continues capped searches without repeating their first 20,000 states.

Single-pass maximum-limit rescue is now the default. `--staged-exact-rescue`
retains the old two-pass behavior when the diagnostic initial-cap count is
required.

## Corpus result

Both modes used four workers and the same 656 unresolved hands. Per-game rescue
hits and final capped game IDs are identical.

| Exact rescue | Staged 20k + 60k | Single 60k | Improvement |
| --- | ---: | ---: | ---: |
| Wall time | 4.807 s | 3.343 s | 1.44x |
| CPU time | 18.811 s | 13.222 s | 1.42x |
| Rescue hits | 273 | 273 | exact parity |
| Final caps | 47 | 47 | exact parity |

The complete validated 1,000-game pipeline moved from the generation-21
starting point of 20.477 seconds wall and 28.532 CPU-seconds to 5.353 seconds
wall and 19.689 CPU-seconds. That is 3.83x wall throughput and 31.0% less CPU,
with the same 617 deterministic successes.

## Rejected alternate-witness accelerator

A validation-state search was prototyped for invalid packed witnesses. It
tracked recorded-order removals, post-shuffle uncertainty, and terminal use of
uncertain draws. The unbounded form exceeded 30 seconds in packed replay alone.
A 2,000-state bound recovered four additional confirmations but increased packed
CPU from 6.54 to 8.14 seconds, avoiding only about 0.1 seconds of exact rescue.
The implementation was removed.

The next useful exact-path work is compact collision-safe mana-dominance keys and
better cross-worker funding-cache reuse. Both should be benchmarked against the
single-pass baseline rather than the obsolete staged schedule.
