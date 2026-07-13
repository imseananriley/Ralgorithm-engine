# Generation 28: Probe and Street Wraith gas optimization

## Objective

Test Gitaxian Probe and Street Wraith as gas additions without reducing the champion's
dedicated interaction count. Emergence Zone is protected from cuts. Both cards are now
registered in the reusable `gas` candidate preset.

Gitaxian Probe and Street Wraith are absent from the Commander section of Wizards' current
banned list: <https://magic.wizards.com/en/banned-restricted-list>.

## Model changes

- Gitaxian Probe casts for two life and draws at sorcery speed.
- Gitaxian Probe can be countered by An Offer You Can't Refuse to create two Treasures.
- Street Wraith cycles for two life without casting a spell.
- Street Wraith can cycle in the pre-turn Caverns priority window, including after an
  instant-speed top tutor.
- Street Wraith is a black creature card for Chrome Mox; Probe is blue.
- Pregame tutor use now leaves Gemstone Caverns tapped during the first main phase.
- The legacy legality table no longer incorrectly lists Gitaxian Probe as Commander-illegal.

The full Rust suite passes with 131 tests.

## Broad screen

The exploratory screen evaluated 92 swaps on 1,000 paired raw hands: both gas cards against
30 eligible lands and 16 dedicated acceleration cards. Sixteen swaps with point estimates of
at least +0.20 percentage points advanced. Formal family inference uses only the 9,000 hands
not seen during selection and applies Holm correction across those 16 comparisons.

The Probe-to-Offer line was identified after the initial screen. All eight promoted Probe
arms were rerun with corrected semantics before inference.

## Held-out family results

| Swap | Held-out delta (95% CI) | Gains / losses | Holm p | Complete-case delta | Complete-case Holm p |
|---|---:|---:|---:|---:|---:|
| Sink into Stupor -> Gitaxian Probe | +0.51 pp (+0.29, +0.73) | 74 / 28 | 0.0000945 | +0.55 pp | 0.0000131 |
| Sink into Stupor -> Street Wraith | +0.39 pp (+0.17, +0.60) | 66 / 31 | 0.00637 | +0.42 pp | 0.00183 |
| Rite of Flame -> Gitaxian Probe | +0.32 pp (+0.09, +0.56) | 73 / 44 | 0.0934 | +0.34 pp | 0.0342 |
| Rite of Flame -> Street Wraith | +0.28 pp (+0.05, +0.51) | 69 / 44 | 0.212 | +0.29 pp | 0.0967 |

No other positive swap has a held-out interval excluding zero. Dark Ritual and Underground
Sea cuts are harmful. Sink into Stupor is statistically supported, but its spell face is
interaction; it therefore fails the user's no-interaction-cut constraint.

For the Sink cut, Probe records 11 held-out wins that Street Wraith misses and no losses to
Street Wraith (`p = 0.000977`). The difference is attributable to Probe's cast-spell utility,
principally Offer treasure lines; their ordinary cantrip outcomes are nearly identical.

## Independent Rite confirmation

Because Rite of Flame was the strongest clean cut but did not survive the exploratory-family
Holm correction, it received a pre-specified independent confirmation using 20,000 fresh
Fisher-Yates shuffles, fresh 75% Caverns samples, and fresh Gamble seeds.

| Deck | Deterministic hits | Rate | Search caps |
|---|---:|---:|---:|
| Champion | 7,589 / 20,000 | 37.945% | 976 |
| Rite of Flame -> Gitaxian Probe | 7,644 / 20,000 | 38.220% | 1,067 |

The paired gain is **+0.275 percentage points** (95% CI +0.116 to +0.434), with 160
candidate-only wins and 105 baseline-only wins (`p = 0.000875`). Excluding all pairs capped
in either arm leaves 18,907 pairs and gives **+0.376 percentage points** (95% CI +0.226 to
+0.525; `p = 1.02e-6`). The conclusion is not explained by Probe's higher cap count.

## Decision

`Rite of Flame -> Gitaxian Probe` is a statistically justified champion optimization that
does not reduce interaction count. Street Wraith remains in the gas candidate pool, but no
clean Street Wraith swap is established by this experiment. Do not cut Sink into Stupor if
its interaction modality is part of the required interaction budget.
