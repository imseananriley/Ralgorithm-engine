# Generation Nine: Direct Packed Payments And Paired Pipeline

## Scope

Generation nine completes the exact opening land slice, ports the principal zero- and one-mana artifacts, applies engine payments directly to `PackedStateV2`, and adds the production experiment kernels required by the architecture.

The supported opening permanents now include fetchlands, typed duals, City of Traitors, Crystal Vein, Glimmervoid, Gemstone Mine, pregame Gemstone Caverns, Lotus Petal, Lion's Eye Diamond, Chrome Mox, Mox Diamond, Mox Opal, Mox Amber, Paradise Mantle, Sol Ring, and Mana Vault. Rule tests cover imprints, discards, metalcraft, commander identity, equipment summoning sickness, counters, sacrifice modes, and the Mana Vault untap exception. End-to-end successful and unsuccessful fixtures agree with the legacy `FastState` oracle.

## Direct Payment

The opening model now builds packed mana witnesses from battlefield sources and applies their tap or sacrifice disposition directly to `PackedStateV2`. Direct payment covers engine casts, Sol Ring and Mana Vault, commander casts, and Paradise Mantle equip costs. City of Traitors retains a narrow floating-mana transition because tapping it before a second land is played cannot be reconstructed from the final battlefield alone. The full resource-microstep graph remains selectable as a differential oracle.

A cooled local benchmark used 5,000 identical solves per mode. Direct payment expanded 42 states per solve versus 132 for the prior opening graph and produced 19,764 to 23,202 solves/s. The same-build microstep mode produced 9,784 to 9,889 solves/s, for a direct speedup of 2.02x to 2.35x. Both modes returned expected value 0.5.

## Visible Policy And Experiment Pipeline

`CompiledPolicy` now receives the observable transitions rather than only their count. `VisibleOpeningPolicy` ranks only public state consequences, and `OpeningMulliganPolicy` scores only the visible hand. Tests verify that changing hidden library order cannot change the keep decision.

`BatchEvaluator` adds:

- deterministic ChaCha8 slot permutations shared by every aligned variant;
- resumable sample ranges and progress callbacks;
- paired score deltas and standard errors;
- candidate-only and baseline-only success counts with exact McNemar p-values;
- influence counts and conditional influenced deltas;
- bounded discordance records; and
- p50, p95, p99 latency plus aggregate throughput.

The multifidelity pipeline evaluates every low-fidelity sample and selects strict-reference corrections with an independent seed-only predicate. This prevents outcomes or variant identity from influencing correction membership.

## Remaining Semantic Boundary

The production infrastructure is present, but the packed opening model does not yet cover every card in the 99-card Nick Fury list. Rituals, mana creatures, tutors, random discard, sacrifice tutors, priority-sensitive LED lines, and delayed Summoner's Pact payment remain the next vertical slices. Until those slices pass differential tests, full-deck publication experiments must continue to label the packed model's unsupported cards and use the legacy solver as the high-fidelity correction oracle.
