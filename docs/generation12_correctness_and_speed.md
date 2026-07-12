# Generation Twelve: Correctness And Throughput

Generation twelve replaces the provisional opening estimand with an auditable, nonanticipating pipeline.

## Correctness Changes

- London-mulligan bottoms are a known bottom stack. Normal draws consume known top, then uniform unknown cards, then known bottoms. Tutors can find bottomed cards, and shuffles merge both known stacks into the unknown library.
- Gemstone Caverns live status is sampled once and is visible to every mulligan decision. Pregame exile choices are evaluated without access to unknown order.
- Mulligan continuation values are trained on a domain-separated pilot stream. Evaluation hands compare their best visible keep with the frozen next-stage expectation; they never inspect the next offered hand.
- Strict evaluation enumerates every legal bottom subset. Bulk policy evaluation visibly ranks every subset and solves the configured top `policy_bottom_candidate_limit`; independent strict corrections estimate the approximation error.
- Outcomes retain weighted EV, Rhystic and Heartwood identity, and exact turn-one/turn-two resolution probabilities through chance nodes.
- Depth and cycle truncation produce lower and upper bounds. `capped_rate` exposes unresolved searches instead of counting them as ordinary failures.

## Statistical Changes

- Multifidelity standard errors include covariance between the all-low mean and the overlapping correction sample.
- Each candidate has a directly paired corrected-delta estimator. This preserves baseline/candidate covariance instead of subtracting marginal estimates.
- McNemar's exact tail uses a stable linear-time log recurrence.
- Pilot and evaluation permutations use common random numbers across aligned variants.

## Publication Guards

Production mode requires a 99-card singleton library and validates card color masks against the commander's identity. Candidate influence slots must exactly equal changed slots; baseline influence slots must equal their union. Results include model and request digests plus supported, inert, and unsupported-opening card manifests.

`publication_mode=true` initially rejected the champion fixture because `An Offer You Can't Refuse` and `Ranger-Captain of Eos` had opening relevance but were not implemented. Generation thirteen ports those semantics and the fixture now passes with an empty unsupported-opening manifest. Structural Commander validation still does not replace an external, date-stamped banned-list oracle.

## Performance Changes

- Bottom and Caverns roots share solver and policy transposition tables.
- Duplicate deterministic actions are removed before recursion.
- Workers claim small dynamic chunks, eliminating contiguous-shard tail imbalance while deterministic report ordering preserves reproducibility.
- Bulk bottom screening reduces low-fidelity cost while strict samples remain exhaustive.
- A bounded thread-local payment cache reuses `(model, packed state, cost)` closures without cross-variant sharing or mutex contention.

On the same champion sample at depth 12, exhaustive low-fidelity bottoms took 8.78 seconds. Four visible-ranked candidates took 4.42 seconds, and one took 1.66 seconds; the one- and four-candidate outcomes matched on that sample. This is a latency calibration, not an accuracy claim.

Adding payment-plan reuse reduced the one-candidate calibration from 1.656 to 1.560 seconds (5.8%) with identical weighted output. The recorded search generated 35,243 strategic actions and removed only six duplicate deterministic actions, confirming that payment reuse and bottom screening matter more than action deduplication on this hand.

An eight-game, four-worker smoke with one bottom candidate and four pilot hands completed in 5.01 measured seconds (1.60 games/s). The run is not inferential: only eight games were sampled and 62.5% had a nonzero depth interval at depth 12. Model digests now include the relevant engine source bytes as well as deck and configuration, so later semantic builds cannot reuse this smoke's pre-source-binding identifier.

## Next Publication Gate

Calibrate bottom-candidate and compiled-policy bias with an independently selected exhaustive strict sample, then run paired deck variants to a preregistered confidence-interval half-width. See `docs/generation13_large_production_experiment.md` for the first 2,000-game production baseline.
