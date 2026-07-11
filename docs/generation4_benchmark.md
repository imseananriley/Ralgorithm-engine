# Generation 4 Benchmark

## Production Changes

Generation four keeps generation-three search behavior and makes battlefield canonicalization cheaper:

- Permanent extras are compared from packed color, freshness, and counter fields without constructing strings.
- An exhaustive test compares 11,776 packed permanent combinations against the legacy textual ordering.
- Mana-sacrifice helpers no longer sort a battlefield a second time after removal already restored canonical order.
- The paired benchmark supports explicit cooldown intervals for thermally stable local comparisons.

## Cooled Paired Result

Baseline: generation three commit `8a971b7`. Candidate: generation-four default mode with payment shortcuts disabled.

Configuration: champion fixture, turn-two resilient Rhystic/Heartwood objective, 20,000-state initial cap, 60,000-state cap rerun, stochastic simplified Gamble, four balanced `AB/BA` repetitions, a 30-second initial idle, and three seconds between processes.

| Workload | Outcome parity | Baseline median | Candidate median | Ratio of medians | Paired median |
| --- | ---: | ---: | ---: | ---: | ---: |
| 200 fixed hands | 200/200 exact | 3.335 s | 3.200 s | 1.042x | 1.038x |
| 30 full policy games | 30/30 exact | 14.090 s | 13.003 s | 1.084x | 1.077x |

Successes, labels, caps, and mulligan outcomes matched exactly. Fixed hands produced 73/200 successes and 28 caps in each engine. Full policy produced 17/30 successes and one cap in each engine.

The earlier non-cooled three-repeat policy result drifted as the machine heated and is not retained as evidence. Balanced even repetitions and cooldowns are required for future local publication measurements.

## Payment-Directed Research Path

`RALGORITHM_PAYMENT_DIRECTED=generic` computes a Pareto-pruned closure of existing mana transitions, emits free setup actions once, and tests only payable costed actions across the closure. Compiled card metadata stores up to two conservative colored-cost lower bounds for early rejection; authoritative spell resolution still uses the existing card-specific generators.

On a 100-fixed-hand calibration, this path increased successes from 31 to 33 and reduced caps from 10 to 5. The six discordances were all baseline caps; the candidate found two legal wins and exhausted four failures. A separately traced policy line confirmed one newly recognized turn-two Rhystic sequence through Enlightened Tutor, Elvish Spirit Guide, Green Sun's Zenith, and Tinder Wall.

The generic closure was still slower: 0.765x fixed-hand and 0.858x full-policy throughput in that calibration. It therefore remains an opt-in coverage oracle. The production successor should enumerate cost-specific payment plans directly from packed resources instead of rebuilding full `FastState` mana frontiers.

## Reproduction

```bash
python3 scripts/benchmark_solver_generations.py \
  --baseline-bin /path/to/generation3/rhystic-core-smoke \
  --hands 200 \
  --policy-games 30 \
  --repeats 4 \
  --cooldown-seconds 3 \
  --state-limit 20000 \
  --actual-rerun-state-limit 60000 \
  --out benchmarks/results/generation4_cooled_final.json
```
