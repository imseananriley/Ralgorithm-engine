# Generation 18: Structured families and exact rescue

## Changes

The packed discrepancy solvers previously grouped transitions by only the semantic
class of a card consumed from hand. Every target of the same tutor consequently
shared one family, and all but the highest-ranked target could be discarded before
search.

Transition families now incorporate:

- consumed card classes
- cards added to hand
- permanents added to the battlefield
- the known top-card class
- resulting floating mana
- changed turn/resource flags
- changed commander state

This keeps role-distinct tutor, land, mana, and payment outcomes separate while
the existing full-state deduplication still removes identical successors.

The benchmark loop also has an adaptive exact rescue stage. After packed D0-D2,
only unresolved keeps are sent to the current exact-order fast solver at 20,000
states. Capped rescue cases are retried at 60,000 states.

## Regression corpus

The 125 pre-fork-only games were used as a focused regression corpus.

- Structured packed search, width 8 through D5: 82/125
- Exact rescue evaluated the remaining 43
- Exact rescue hits: 43/43
- Final recall: 125/125
- Rescue wall time: 0.166 seconds in the calibration run

The remaining packed misses are caused by crowded full-deck search frontiers, not
missing line semantics. A stepwise regression confirms that the packed model and
existence solver can execute the representative Vampiric Tutor, Dark Ritual,
Rhystic Study line in a reduced state.

## Production profile

Candidate-width and discrepancy sweeps selected:

- depth: 16
- action candidate width: 4
- maximum discrepancy: D2
- deterministic recorded top: 3 cards
- workers: 8
- exact rescue: 20,000 states, then 60,000 for caps

D3-D5 increased runtime more than they reduced rescue work.

## Fresh 1,000-game benchmark

Three complete repetitions recomputed both engines, including capped reruns.

| Metric | Pre-fork | Hybrid current |
|---|---:|---:|
| Pre-fork wins recalled | 619 | 619 |
| Median wall time | 51.663 s | 11.359 s |
| Median CPU time | 50.012 s | 19.004 s |
| Median paired wall speedup | 1.00x | 4.64x |
| Median paired CPU speedup | 1.00x | 2.64x |

The packed stages produced 697 nominal positive branches and recalled 524 of the
619 pre-fork wins. Exact rescue evaluated 303 unresolved keeps, recovered the
remaining 95 pre-fork wins, and reran 46 capped cases at the larger limit.

The hybrid output reports 792 nominal hits, but 173 are packed-only existential
branches rather than exact-order confirmed wins. They must remain separate from
the 619 confirmed realized-order wins until deterministic witness validation is
implemented for packed positives.

## Next accuracy task

Exact rescue fixes false negatives without sacrificing most of the packed speed
gain. The remaining correctness task is symmetric validation of packed positives:
retain a compact witness action skeleton, replay it against the complete recorded
library order, and send failed witnesses to exact search. That will turn the
hybrid from a recall accelerator into a complete exact classifier.
