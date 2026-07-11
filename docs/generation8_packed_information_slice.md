# Generation 8 Packed Information Slice

## Implemented Boundary

Generation eight begins the production migration rather than further optimizing `FastState`:

- `PackedStateV2` stores unique card zones as masks and battlefield objects in a canonical fixed-capacity permanent set.
- A permanent is four bytes and records a card/token/commander source, unique card/commander attachment target, counters, tapped status, and freshness.
- The separate generated-object namespace supports duplicate Treasure and other token instances without conflating them with singleton deck slots.
- Commander zone, cast count, tax, and flags are explicit state.
- Canonical insertion order, active-entry hashing, zone disjointness, attachment validity, duplicate-card rejection, bounded capacity, and malformed-state rejection are tested.
- `CardSpec` compiles the opening slice's land mana profile. Conditional cards remain unsupported instead of receiving optimistic approximations.

The first concrete `EngineOpeningModel` implements:

- mandatory turn-one and turn-two draws through explicit chance nodes over `PackedLibrary`;
- one land play per turn;
- exact colored/colorless activation for the supported land profiles;
- Rhystic Study and Heartwood Storyteller payment;
- turn cleanup and untapping;
- the mildly weighted objective: Rhystic `1.00/0.75`, Heartwood `0.70/0.55` on turns one/two.

The model supports 21 of 99 champion-list slots: both engines and 19 unconditional or objective-equivalent land profiles. Unsupported cards are inert. Fetchlands, City of Traitors, Crystal Vein, Glimmervoid, fast mana, tutors, creatures, pregame Gemstone Caverns, and other conditional effects are not yet part of this model.

## State Benchmark

Five local repetitions used 50 million operations per path on an Apple M4. The median measurements were:

| Path | State size | Throughput | Relative to V1 |
| --- | ---: | ---: | ---: |
| Minimal `PackedState` transition/hash | 144 bytes | 154.30M ops/s | 1.000x |
| Equivalent `PackedStateV2` transition/hash | 176 bytes | 90.26M ops/s | 0.588x |
| V2 commander, two tokens, attachment, counter, removal, and hash | 176 bytes | 33.51M ops/s | 0.217x |

The richer state is 22.2% larger. Active-entry hashing and reducing the turn-two permanent capacity from 24 to 16 improved the equivalent V2 path from 46.1M to approximately 90M ops/s. Even the stress path remains orders of magnitude above complete solver throughput.

## Information Solver Benchmark

The benchmark fixture has Ancient Tomb and Rhystic Study in hand with Command Tower among three unknown cards. Mandatory draws on turns one and two produce weighted expected value `0.5`.

Across four final 100,000-solve repetitions, median throughput was 14,919 solves/s. Each solve expanded 132 states, recorded 61 transposition hits, and evaluated 12 chance nodes.

## Verification

The release suite contains 54 passing tests and passes:

```bash
cargo test -p rhystic_core --release
cargo clippy -p rhystic_core --release --all-targets -- -D warnings
```

The dedicated benchmarks are:

```bash
target/release/rhystic-core-smoke bench-state-v2 50000000
target/release/rhystic-core-smoke bench-opening-model 100000
```

A production bridge guard compared generation seven with this source on 1,000 fixed hands and 20 policy games. Outcomes were exact in all 1,020 cases: both produced 391 fixed successes with 71 caps and 12 policy successes with no cap misses. Policy median timing was neutral (`1.006x`). Fixed timing contained one candidate outlier and is retained only as a no-regression guard, not a speed claim.

## Next Migration Slice

The next slice should add exact fetchland search/shuffle transitions and conditional land state for City of Traitors, Crystal Vein, Glimmervoid, Gemstone Mine, and pregame Gemstone Caverns. After land parity is mechanically established, port zero-mana artifacts and direct packed payment witnesses. This expands real hand coverage while keeping chance semantics and card registry ownership testable.
