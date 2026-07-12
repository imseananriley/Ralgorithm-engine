# Generation Thirteen: Large Production Baseline

## Configuration

- Deck: `fixtures/decks/champion_working_list.json`
- Samples: 2,000 evaluation games in 20 resumable 100-game shards
- Seed: `2026071301`
- Objective horizon: turn two
- Search depth: 14
- Policy pilot: 64 independent hands, frozen across every shard
- Bottom screen: top one visible-ranked legal subset; all subsets were ranked
- Compute: RunPod CPU, 32 vCPU, 256 GiB RAM
- Model digest: `77083cf15a0ba11a3b88bbcaebefd08a`
- Publication mode: enabled

The packed support manifest was clean after implementing `An Offer You Can't Refuse`, `Ranger-Captain of Eos`, and `Esper Sentinel`. A live Scryfall collection query resolved all 99 slots and reported no Commander-illegal cards. The structural in-engine check and the external legality check serve different purposes and were both required.

## Results

| Outcome | Estimate | 95% shard-t interval |
| --- | ---: | ---: |
| Rhystic turn one | 1.79% | 1.36–2.22% |
| Rhystic by turn two | 16.50% | 15.73–17.28% |
| Heartwood turn one | 1.23% | 0.83–1.63% |
| Heartwood by turn two | 12.58% | 11.74–13.42% |
| Any engine turn one | 3.02% | 2.37–3.67% |
| Any engine by turn two | 29.08% | 27.90–30.26% |

Turn-two-only contributions were 14.71% Rhystic, 11.35% Heartwood, and 26.06% for either engine. Weighted EV was `0.19930`; its raw game-level standard error was `0.00540`. The weighted lower/upper means were `0.19930` and `0.19934`, an average unresolved width of only `0.0000432`.

The capped-root rate was 4.5% (95% shard-t interval 3.40–5.60%), but its tiny aggregate bound width means depth truncation cannot materially explain the result.

## Performance

The 20 evaluation shards consumed 559.75 measured seconds in total, or 3.57 games/s. The one-time pilot and remote build were outside that total. Dynamic scheduling completed all shards without a digest or resume-boundary mismatch, and the pod was deleted after the merged summary was local.

The search expanded 52.3 million states, generated 823.8 million strategic actions, recorded 6.78 million transposition hits, and evaluated 676,680 chance nodes. These counts identify action generation and policy evaluation as the next performance targets.

## Interpretation

The old `success_rate` field counted a game whenever its evaluated value was positive. In this experiment, 94.45% of games had some positive-probability path, while the probability-weighted any-engine rate was 29.08%. Therefore old 70%+ positive-line figures and this probability estimate are different estimands and must not be compared as if they were the same resolution rate.

This run is a production bulk-policy baseline, not the final strict-reference estimate. Before optimizing deck slots against it, run an independently selected exhaustive-bottom/strict sample to estimate policy and bottom-screen bias. The frozen policy table and 100-game shards make that correction reproducible and resumable.
