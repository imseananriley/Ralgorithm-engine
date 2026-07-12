# Generation Fourteen: Opening-Solver Recall Overhaul

## Invalidated baseline

The generation-thirteen 2,000-game result is not a deck-strength estimate. Its low tier followed one immediately ranked action at every decision and screened only one London-bottom subset. The narrow depth bounds measured uncertainty along that selected policy; they did not bound the value of legal alternatives the policy never explored.

One exhaustive full-mulligan probe was stopped after 79.08 seconds without completing a game. macOS `time` reported a 2.34 GB maximum resident set and a 6.13 GB peak memory footprint. Exhaustive expectimax therefore remains an audit oracle for small fixtures, not the production evaluator.

## Implemented search family

The production solver now supports nested limited-discrepancy families:

- `D=0`: follow the highest-ranked action.
- `D=1`: permit one departure to another ranked action anywhere in the line.
- `D=2`: permit two departures.

The action-candidate limit is independent of the discrepancy budget. Tutor targets that compile to the same verified semantic class now share one representative after search-and-shuffle effects. Exact search orders actions and stops when it reaches the turn-specific maximum objective value.

The opening batch reports the London mulligan stage of every kept hand. Multifidelity evaluation can run `D=1` over all samples and apply an independently selected paired `D=2 - D=1` correction with a separately trained, frozen mulligan policy.

## Capacity calibration

These measurements use identical champion-deck shuffles, depth 14, one bottom candidate, and two ranked action candidates. They are implementation calibrations, not probability estimates.

| Calibration | Games | Any-engine probability | States | Games/s |
| --- | ---: | ---: | ---: | ---: |
| `D=0` | 8 | 17.77% | 166,274 | 1.254 |
| `D=1` | 8 | 30.70% | 914,535 | 0.235 |
| `D=1`, first paired block | 4 | 46.11% | 838,277 | 0.319 |
| `D=2`, first paired block | 4 | 47.38% | 2,079,279 | 0.135 |

The first two games were particularly policy-sensitive: `D=0` recovered 49.94% probability mass, `D=1/actions=2` recovered 89.83%, and `D=1/actions=4` recovered 91.16%. Two action candidates retained most of that gain at roughly half the state count of four candidates.

The old frozen mulligan table sent five of the first eight `D=0` games and three of the first eight `D=1` games to the forced three-card keep. A fresh eight-hand-per-stage `D=1` pilot reduced forced three-card keeps to two, but is too small for publication use.

## Recall gate

No deck-card comparison is publishable until all of the following pass:

1. Every manually validated line is represented as a permanent regression fixture. The Birds of Paradise, Gamble, Crop Rotation, Ancient Tomb turn-two Rhystic line is now covered, alongside existing Pact/Angel's Grace, tutor/LED, Gamble, hidden-shuffle, and mana-semantics tests.
2. `D=2` preserves every success in an independently sampled legacy-solver corpus. Discordances require trace review; a lower aggregate mean cannot excuse a lost known win.
3. Bottom-screen recall is calibrated at 1, 2, 4, 8, and exhaustive candidates on the same hands. Bottom selection and in-game action selection must be reported separately.
4. A larger independent mulligan pilot is frozen before evaluation shards start. Keep-stage counts must be included in every result.
5. A paired `D=1/D=2` pilot estimates correction variance. The production correction fraction is then selected from the variance and target standard error, rather than chosen heuristically.

## Next experiment

Run 100 paired games over the unchanged champion list with common random numbers:

- low tier: `D=1`, two action candidates;
- correction tier: `D=2`, two action candidates;
- bottom screens: paired 1, 2, 4, and 8-candidate calibrations;
- independent mulligan pilots for each tier;
- per-game outcomes, keep stages, states, and correction deltas retained for audit.

Use the pilot to choose the correction fraction and bottom-screen size for a 10,000-game baseline. Only after the corrected baseline passes the recall gate should card-swap optimization resume.
