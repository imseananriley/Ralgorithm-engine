# Generation 27: Gitaxian Probe champion swap

## Comparison

This experiment tests `Swan Song -> Gitaxian Probe` against the Generation 26 champion.
Swan Song provides a color-matched interaction control: both cards are blue for Chrome Mox
and pitch-card accounting, while only Probe has an opening-objective action.

Gitaxian Probe is Commander legal as of the experiment date. It does not appear in the
Commander section of Wizards' current banned list:
<https://magic.wizards.com/en/banned-restricted-list>.

## Method

- 10,000 paired raw seven-card hands using the champion's exact permutations.
- Identical draw order, Gemstone Caverns status, and Gamble seed in both arms.
- Packed discrepancy search through budget 2, full-library exact rescue, and witness
  validation.
- Exact two-sided McNemar inference over discordant outcomes.
- Complete-case sensitivity excludes every hand capped in either arm.
- This stage does not apply London mulligans or weighted engine/turn utility.

## Result

| Deck | Deterministic hits | Rate | Search caps |
|---|---:|---:|---:|
| Generation 26 champion | 3,772 / 10,000 | 37.72% | 490 |
| Swan Song -> Gitaxian Probe | 3,867 / 10,000 | 38.67% | 545 |

The Probe swap gains **+0.95 percentage points** (95% CI +0.75 to +1.15). There are
101 Probe-only wins and 6 champion-only wins; exact McNemar `p = 2.37e-23`.

After excluding every hand capped in either arm, 9,445 pairs remain. Probe has 91 exclusive
wins and zero losses, for **+0.96 percentage points** (95% CI +0.77 to +1.16; exact McNemar
`p = 8.08e-28`). Search-cap asymmetry therefore does not explain the gain.

Probe is visibly present in the final keep for 83 of the 101 gained hands. Twenty-six of
those 83 hands also contain Enlightened Tutor, Imperial Seal, Mystical Tutor, Scheming
Symmetry, or Vampiric Tutor. This directly supports the proposed top-tutor cracking mechanism,
while the remaining gains include ordinary redraws into relevant cards and lines where Probe
is drawn from the library.

## Decision

`Swan Song -> Gitaxian Probe` is strongly supported for the opening-objective champion. It
converts one interaction slot into velocity, so final promotion remains a deck-construction
tradeoff rather than a purely statistical dominance claim. The canonical champion is left
unchanged pending explicit promotion.
