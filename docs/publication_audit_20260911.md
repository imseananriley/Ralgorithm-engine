# Publication audit, September 2026

## Scope and intent

The maintained repository is `imseananriley/Ralgorithm-engine`, the Rust-focused
successor to the older Python/RunPod workspace. Its useful endpoint is comparing
early Rhystic/Heartwood resolution under explicit Commander assumptions, while
protecting interaction and later-game engine slots. This endpoint does not measure
winning a multiplayer game.

The audit covers production input validation, paired experiment orchestration,
randomization, statistical postprocessing, validator integration, documentation,
and publication hygiene. Existing Serum Powder changes and the sample-size analysis
are retained and included in the verification scope.

## Verified issues and fixes

| Issue | Consequence | Change |
|---|---|---|
| Results checked settings but not deck or supplied thresholds | Reusing an output directory could compare stale results from a different swap | Bind cached results to input-deck digest and shared threshold values; regenerate variant inputs |
| Duplicate IDs overwritten; unmatched IDs silently dropped | Incorrect paired sample and uncertainty | Reject duplicate, invalid, empty, or unmatched game IDs and mismatched seeds |
| Same fixed seed used on every CLI invocation | Apparently new experiments repeated existing observations | Fresh random seed by default; explicit seed for resume/reproduction |
| In-place replacement determined added card's shuffled slot | Noisy slot-dependent rankings of identical deck compositions | Rust full-policy stage order uses identity-keyed 128-bit priorities |
| Full resampling loop for each bootstrap replicate | O(games times replicates) Python work for a few distinct scores | Exact conditional-binomial multinomial bootstrap on Python 3.12+, with compatible fallback |
| Comparison skipped semantic audit | Unsupported cards could be simulated without the documented gate | Run coverage and probes before launching comparison |
| Unknown commanders default to Nick Fury internally | Arbitrary decks could silently use the wrong commander rules | Public CLI rejects unaudited commander configurations |
| Basic lands rejected; malformed entries crashed; legacy secondary commander ignored | Valid inputs rejected or invalid inputs crashed | Basic-land duplicate exception, defensive validation, legacy partner support |
| Validator used obsolete binary and results paths | Replay failed and new runs were absent from UI | Use workspace binary and scan benchmark results; parse compact JSON correctly |
| One observation returned a zero-width interval | Unsupported precision claim | Return unavailable intervals for fewer than two observations |

Identity-keyed order preserves shared-card relative order across swaps. It uses a
game/stage domain plus card name and copy number, with negligible 128-bit collision
probability. It changes the realization of seeded games, so historical and new
results must not be pooled as paired observations. Legacy conditional raw-slot
screens are unchanged and must be independently confirmed with full-policy games.

## Verification and performance

Regression checks cover input errors, partner count, cache invalidation after deck
or policy edits, matching game IDs, shuffled-input invariance, shared-card order,
duplicate-card multiplicity, position frequencies, validator discovery, and exact
binomial bootstrap quantiles. The existing rules, parity, witness, and sharding
tests are retained.

The three-repeat local Python 3.14 benchmark of 10,000 outcomes and 2,000 bootstrap
replicates had median times of 2.559 seconds with individual resampling and 0.00303
seconds with histogram resampling, about 844 times faster for this statistical step. Both
sample the same empirical bootstrap law; individual intervals differ because
their random draws differ. This is not a solver-throughput claim. Reproduce with:

```bash
python3 scripts/benchmark_statistics.py --games 10000 --replicates 2000 --repeats 3
make check
python3 scripts/ralgorithm.py compare --preset smoke --games 10 --seed 2026091101
```

The end-to-end smoke evaluated ten games for a baseline and four variants, all
without remaining caps. Repeating the same invocation reused the baseline cache
and all four candidate results. These ten-game rates are mechanics checks only.
The validator served HTTP 200, discovered the new smoke records, and solved a
known turn-one Rhystic hand using the workspace release binary. Protocol errors
now raise errors rather than being shown as failed hands, and large root seeds
are preserved as strings when sent to browsers.

Gitleaks 8.30.1 scanned all 54 existing commits (14.35 MB) without finding a secret.
The scanner binary's SHA-256 was verified against the release asset digest. The
staged publication snapshot, including retained archives, was separately scanned
(14.28 MB), also with no leaks found. A clean export of that snapshot passes all
20 Python tests. The working-tree verification passes all 159 Rust tests, rustfmt,
Clippy with warnings denied, Python compilation, and diff whitespace checks.

## Remaining research work

1. **Nonanticipating play:** full-library legality does not prove that a chosen
   action was independent of unseen draws. Maintain separate legal-witness,
   sampled-policy, and oracle-reachability labels and evaluate chosen actions on
   independent hidden continuations before making real-play probability claims.
2. **Caps:** report unresolved searches and sensitivity, rather than treating
   complete-case selection as an unbiased repair. Prior tiny swap effects may be
   smaller than uncertainty caused by capped search.
3. **Policy training:** current intervals condition on a trained policy. Compare
   multiple independent training replicates for claims about optimized policies.
4. **Coverage:** the Python classification table and Rust action registry can
   drift. Generate coverage from a single registry and require necessity probes
   for all opening-relevant effects. Commander support is intentionally narrow;
   Rograkh/Silas does not yet model Silas's full recursion ability.
5. **Statistics:** implement family-wise adjusted outputs and predeclared
   sequential stopping in the default experiment loop. Raw exploratory rankings
   and ordinary repeatedly inspected intervals are not confirmatory evidence.
6. **Performance:** profile actual production search before deeper architectural
   changes. Prioritize identical-subproblem reuse with complete state keys,
   allocation reduction, and correction samples for bounded searches. Preserve
   observable decision rules and line recall in every benchmark.

Publishing the code makes these experiments inspectable; it does not certify
historical deck rankings or claim a solved general-purpose Magic engine.
