# Generation 21: Exact-preserving throughput pass

## Scope

This pass optimizes the validated hybrid classifier without changing its search
width, discrepancy budget, state limits, card semantics, hidden-information
rules, or objective. The benchmark corpus is the fixed 1,000-game pre-fork
recall corpus used by generations 15 through 20.

## Retained changes

- Exact-rescue batches accept an explicit worker count. Each worker owns an
  independent `FastContext`; an atomic work index balances hard hands and
  results are restored to input order.
- D0, D1, and D2 replay can run in one adaptive call. A solver retains its
  transposition table between budgets and a game stops after its first
  full-library-confirmed witness.
- Benchmark replay omits human-readable witness strings. Witness state paths and
  validation remain unchanged, and the UI/audit default still includes actions.
- Existence memo entries now contain the successful witness edge, removing a
  second hash table. Temporary deterministic-state storage is reused, and the
  bounded family set is allocation-free.
- Ordinary land actions share the copy-on-write `PersistentLibrary`. Only fetch,
  tutor, and shuffle effects allocate a changed library.
- Canonical card-ID tails use counting sort for the normal sub-256 registry and
  retain comparison sort as a general fallback.
- Small land-option, City-index, and mana-dominance collections stay inline.

## Correctness gate

The final run exactly matches the starting classifier on:

- 617 deterministic hits and every hit game index;
- 344 confirmed packed hits and 698 raw packed branches;
- every witness-status count;
- every exact-rescue hit game index;
- all 47 final capped game indices.

The Rust suite passes 124 tests, including serial/parallel batch equivalence and
counting-sort equivalence over duplicates and card IDs above 255. Clippy remains
warning-free.

## Performance

The strongest controlled microcomparison uses the same 656 unresolved hands at
the 20,000-state exact tier, strict hidden-shuffle mode, and one worker. Four
balanced ABBA runs compare untouched commit `98271e3` with this pass.

| Exact 20k tier | Baseline | Current | Improvement |
| --- | ---: | ---: | ---: |
| Median wall time | 7.149 s | 6.531 s | 1.095x |
| Median CPU time | 7.141 s | 6.373 s | 1.121x |
| Response parity | - | byte-for-byte | exact |

The representative complete hybrid run includes D0-D2 witness validation,
20,000-state rescue, and 60,000-state reruns:

| Complete 1,000-game run | Starting point | Current, 4 workers |
| --- | ---: | ---: |
| Wall time | 20.477 s | 6.516 s |
| CPU time | 28.532 s | 25.480 s |
| Deterministic hits | 617 | 617 |
| Final caps | 47 | 47 |

The complete-run wall ratio is 3.14x and the observed CPU reduction is 10.7%.
This is a representative end-to-end measurement rather than a balanced timing
series. Four workers is the recommended local setting. Eight workers previously
reached lower wall time but increased CPU materially through cache fragmentation
and memory contention.

## Remaining exact-preserving work

1. Make the 20,000-to-60,000 exact escalation resumable. The current 109 capped
   initial searches restart from zero. A checkpoint must preserve the DFS queue,
   collision-safe seen index, mana-dominance frontier, turn boundary, and search
   order; otherwise outcome parity is not guaranteed.
2. Search for a library-consistent packed witness directly after an invalid or
   probabilistic first witness. Exact rescue remains the fallback, so this can
   reduce rescue volume without lowering recall.
3. Replace full `FastState` mana-dominance keys with a compact structural
   fingerprint plus collision-checked equality. The canonical library prefix and
   multiset tail must remain part of equality.
4. Add a sharded, shared funding-plan cache for parallel exact workers. This may
   recover the serial cache efficiency currently lost at four and eight workers;
   lock contention must be measured before retention.
5. Reuse per-worker action and state scratch buffers and move remaining land and
   tutor dispatch from strings to compiled card metadata.

The first two items offer the largest expected reduction in total compute. GPU
execution is not the next step: the workload is branch-heavy, hash-heavy, and
irregular. Additional CPU cores and memory bandwidth are useful now; GPU work
would require a substantially different batched transition kernel.
