# Generation 26: updated champion versus supplied list

## Decks

The champion fixture now includes the requested `Glimmervoid -> Eldritch Evolution`
change. Relative to that champion, the supplied 99-card list makes this five-card package:

| Champion only | Supplied only |
|---|---|
| Flooded Strand | Mindbreak Trap |
| Wooded Foothills | Gitaxian Probe |
| Hallowed Fountain | Badlands |
| Tropical Island | Taiga |
| Volcanic Island | Glimmervoid |

The column pairing is a common-random-number alignment, not a claim that these were the
historical one-for-one substitutions. It preserves each deck's exact marginal shuffle
distribution. The updated champion has 31 lands and the supplied list has 29.

## Model audit

Before comparison, the Rust model gained explicit Gitaxian Probe transitions in both the
packed and exact engines. Probe casts for two life and draws the next card; its life payment
cannot affect the turn-one/turn-two Rhystic/Heartwood objective. Badlands and Taiga now have
typed two-color land profiles in the packed engine, and Badlands is recognized as a land in
both Rust and the legacy Python registry. Targeted Probe and dual-land tests plus the full
127-test Rust suite pass.

The supplied list has no unreviewed opening cards: 66 are modeled for the objective, 5 have
the relevant creature-body subset modeled, and 28 interaction/value cards are intentionally
passive before engine resolution.

## Method

- 10,000 paired raw seven-card hands, split into ten independent 1,000-hand shards.
- Identical slot permutations, draw orders, Gemstone Caverns states, and Gamble seeds.
- Packed discrepancy search through budget 2, then full-library exact rescue and witness
  validation.
- Primary interval uses the paired per-hand {-1, 0, 1} difference.
- Exact two-sided McNemar test uses only discordant hand outcomes.
- A complete-case sensitivity analysis excludes every hand capped in either arm.
- This is a strict binary raw-hand screen. It does not include London mulligans or distinguish
  Rhystic/Heartwood and turn-one/turn-two utility weights.

## Result

| Deck | Deterministic hits | Rate | Search caps |
|---|---:|---:|---:|
| Updated champion | 3,772 / 10,000 | 37.72% | 490 |
| Supplied list | 3,684 / 10,000 | 36.84% | 430 |

The supplied-minus-champion paired delta is **-0.88 percentage points** (95% CI -1.25 to
-0.51). There are 133 supplied-only wins and 221 champion-only wins; exact McNemar
`p = 3.37e-6`.

After excluding every hand capped in either arm, 9,444 pairs remain. The complete-case delta
is **-0.94 percentage points** (95% CI -1.31 to -0.58; exact McNemar `p = 5.54e-7`). The
direction and magnitude therefore are not explained by the supplied list having fewer caps.

## Decision

Keep the updated champion over the supplied package for the tested objective. The supplied
list's two-land reduction is the leading structural explanation, but this package comparison
does not identify individual causal contributions. A subsequent component experiment should
test restoring one and then both fetchlands while retaining the red-dual and Probe choices.
