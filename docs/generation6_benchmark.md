# Generation 6 Tutor And Terminal Optimization

## Profile-Guided Changes

Generation six keeps generation-five search semantics and removes repeated work in the dominant tutor path:

- Tutor target lists are stored in the established rank/name order, so filtering the library no longer requires sorting the same static candidates on every expansion.
- Tutor action priority avoids repeated substring classification. Imperial Seal and Scheming Symmetry retain target-dependent priority; every other supported tutor has the constant engine/tutor priority.
- Top-of-library tutors construct their persistent library in one allocation instead of filtering into one vector and then copying into a second vector.
- An expansion containing multiple terminal engine actions now selects the established engine rank before returning. Rhystic Study therefore wins an equal-turn tie with Heartwood Storyteller independently of compiler layout or incidental action insertion order.

A direct paid-transition experiment was also implemented and measured. It was neutral on fixed hands (`1.000x`) and changed an equal-priority Rhystic/Heartwood result through traversal order. That experiment was removed rather than retained as unproven complexity.

## Balanced Paired Result

Baseline: generation-five commit `f25ea3b`. Candidate: generation six. Both use `RALGORITHM_PAYMENT_DIRECTED=packed`.

Configuration: champion fixture, turn-two resilient Rhystic/Heartwood objective, 20,000-state initial cap, 60,000-state cap rerun, stochastic simplified Gamble, four balanced `AB/BA` repetitions, a 15-second initial cooldown, and one second between processes.

| Workload | Outcome parity | Baseline median | Candidate median | Ratio of medians | Paired median |
| --- | ---: | ---: | ---: | ---: | ---: |
| 400 fixed hands | 400/400 exact | 6.793 s | 5.512 s | 1.232x | 1.236x |
| 12 full policy games | 12/12 exact | 4.431 s | 3.402 s | 1.303x | 1.309x |

Fixed hands produced 149 successes and 25 caps in both engines. Full policy produced 8/12 successes and no caps in both engines.

## Larger Outcome Audit

A separate 1,000-hand paired audit used seed `2026071101` and the same fixed-hand settings. Generation five and generation six both produced 391 successes and 71 caps, with zero turn, cap, label, or unsupported-status discordances. The single-pass wall times were 16.459 seconds and 13.375 seconds respectively (`1.231x`).

The release suite contains 44 passing tests and passes `cargo clippy --release --all-targets -- -D warnings`. New invariants verify static tutor ordering, tutor-priority equivalence, and deterministic Rhystic-over-Heartwood terminal selection.

## Next Bottlenecks

After removing repeated tutor classification and sorting, profiles are led by full-state hashing, battlefield/hand movement, funding-frontier Pareto pruning, and allocator traffic. The next optimization should reduce state-key size or add cached structural hashes without changing search order. A full `nextgen::PackedState` production switch still requires explicit support for tokens, counters, attachments, and battlefield-derived mana abilities before it can replace `FastState` safely.
