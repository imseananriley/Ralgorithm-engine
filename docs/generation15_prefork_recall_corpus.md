# Generation Fifteen: Pre-Fork Recall Corpus

## Corpus

The pre-fork binary at commit `11104ea` evaluated 1,000 full Commander mulligan games with seed `2027071101`, the champion 99-card list, the established fixed threshold policy, a 20,000-state initial limit, and a 60,000-state cap rerun. It reported 619 successes and retained all 1,000 validation records: visible mulligan decisions, selected bottoms, final keeps, libraries, and Gemstone Caverns status.

The current packed solver replayed the exact final keeps. It treated the unseen library as hidden information and integrated over legal draws rather than reading the recorded deck order. This isolates in-game search recall from mulligan-policy differences. It does not make the pre-fork binary success rate and current expected probability the same estimand.

Every pre-fork success was also replayed through the old engine with strict hidden shuffles. All 619 remained successes. Thus this corpus contains no identified pure look-ahead success, although that does not prove the old engine never cheats on other seeds.

## Initial discrepancy

Before action-family diversification:

- `D=0` recovered 589/619 old successes.
- `D=1` recovered 595/619.
- `D=2` recovered 602/619.
- All 24 `D=1` misses contained Summoner's Pact.
- All 17 `D=2` misses contained Summoner's Pact.

The old traces showed that many missed lines did not cast Summoner's Pact. Multiple Pact target transitions occupied both ranked action candidates, preventing the solver from considering Enlightened Tutor, Scheming Symmetry, Crop Rotation, or an ordinary land line.

## Fix

Limited-discrepancy candidate selection now diversifies by the semantic class of the consumed spell or permanent. All Summoner's Pact targets share one action family, so the next-best different strategic action remains eligible. Actions without a consumed card remain distinct.

Post-fix recall on all 30 old successes missed by `D=0` combines with the unchanged 589 `D=0` successes as follows:

| Search family | Recovered | Recall | Wilson 95% |
| --- | ---: | ---: | ---: |
| `D=0` | 589/619 | 95.15% | 93.17-96.58% |
| `D=1` | 605/619 | 97.74% | 96.24-98.65% |
| `D=2` | 616/619 | 99.52% | 98.58-99.84% |
| `D=3` | 618/619 | 99.84% | 99.09-99.97% |
| `D=4` | 619/619 | 100.00% | 99.38-100.00% |

The three lines requiring more than `D=2` were:

- Pact for Heartwood with LED retained to pay the next upkeep.
- Pact plus Crop Rotation, Wishclaw Talisman, commander sacrifice mana, and LED activation for Rhystic Study.
- Offer countering Summoner's Pact, followed by Mystical Tutor, Demonic Tutor, and LED for Rhystic Study.

These are permanent replay regressions. The last line first appears at `D=4`; the other two first appear at `D=3`.

## Old underperformance

The initial current `D=1` replay found positive legal probability in 350 of the 381 pre-fork misses. This does not imply a 94.5% resolution rate: a positive outcome may have very small probability, while the old result was a binary outcome on one realized deck order. It does show that the pre-fork method is unsuitable as a probability baseline and supports the independently observed higher human recall.

## Production method

Use `D=1` over all bulk samples and an independently selected `D=4` correction sample. Estimate

`E[D1] + E[D4 - D1]`

with paired common random numbers and independently trained, frozen mulligan policies. Do not escalate only zero-valued `D=1` hands when estimating probability; that would recover binary wins while failing to correct probability mass on already-positive hands.

The RunPod production script now defaults its correction tier to `D=4`. A pilot must estimate `Var(D4 - D1)` before selecting the correction fraction. Full mulligan-policy parity remains a separate audit because this corpus intentionally reused the old selected keeps to isolate in-game search recall.
