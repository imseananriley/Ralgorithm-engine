# Generation 5 Packed Payment Planner

## Implementation

Generation five replaces mana-production microsteps with cost-specific payment plans while retaining the established card-resolution code:

- Mana sources compile into compact activation groups with explicit tap, sacrifice, counter, and graveyard-exile dispositions.
- A Pareto frontier preserves colored mana and irreversible resource choices, then stops each branch as soon as the requested cost is payable.
- The selected plan is applied to `FastState` once before dispatching only generators associated with that cost.
- Unsupported interactions fall back to the generation-four one-step graph. These include LED priority lines, Offer treasure lines, sacrifice tutors and rituals, City floating before a land, and Noxious Revival plus Manamorphose.
- `solve-keep-trace-fast-jsonl` replays results through the readable one-step graph for validation.
- An optional `RALGORITHM_FUNDING_FRONTIER_LIMIT` bounds a pathological local frontier and falls back to legacy actions for that state. It is unbounded by default because local timing did not justify a publication default.

`RALGORITHM_PAYMENT_DIRECTED=packed` selects this planner. `generic` selects the older full-state closure oracle. The default remains the generation-four production graph until the packed planner's remaining full-state materialization is removed.

## Rules Validation

The planner has focused tests for Mana Vault surplus, Gemstone Mine's last counter, bounded-frontier fallback, and Manamorphose's red/green hybrid payment model. A 21-state recorded parity corpus also verifies that every action emitted by the supported packed path exists in the generic closure oracle.

The full release suite contains 41 passing tests and passes `cargo clippy --release --all-targets -- -D warnings`.

## Paired Outcome Audit

Configuration: 1,000 fixed hands from the champion fixture, seed `2026071101`, turn-two resilient Rhystic/Heartwood objective, 20,000-state cap, stochastic simplified Gamble, and identical shuffles.

| Mode | Successes | Caps | Legacy wins lost | Wall time |
| --- | ---: | ---: | ---: | ---: |
| Generation-four legacy | 367 | 135 | - | 15.109 s |
| Generation-five packed | 391 | 71 | 0 | 15.932 s |

Packed planning found 24 additional legal wins and reduced caps by 64, while preserving every legacy win. Its measured wall time was 5.4% higher in this single large audit. Earlier 300-hand balanced measurements ranged from a small gain to a small loss; therefore no positive throughput claim is retained.

The important result is reduced state-cap bias without the generation-four generic closure's large slowdown. The remaining bottleneck is cloning and hashing full `FastState` values after payment planning. The next generation should apply paid actions directly to the fixed-size `nextgen::PackedState`, rather than materializing an intermediate funded `FastState` and invoking a second cloning generator.

## Research Controls

- `RALGORITHM_FUNDING_CACHE_LIMIT` changes the cross-state funding-plan cache capacity. Default: `100000`; use `0` to disable it.
- `RALGORITHM_FUNDING_FRONTIER_LIMIT` caps the local payment frontier. Default: unbounded.
- Timing comparisons should use balanced `AB/BA` repetitions and cooldowns. One-pass local frontier sweeps are exploratory and must not support publication claims.
