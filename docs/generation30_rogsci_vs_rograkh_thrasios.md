# Generation 30: Rograkh/Silas versus Rograkh/Thrasios

## Question

Compare the supplied Rograkh/Silas turbo list (RogSci) with the supplied
Rograkh/Thrasios creature-mana list for resolving Rhystic Study or Heartwood
Storyteller by turn two.

The predeclared utility is mildly weighted toward Rhystic Study:

- Rhystic Study turn 1: 1.00
- Rhystic Study turn 2: 0.75
- Heartwood Storyteller turn 1: 0.70
- Heartwood Storyteller turn 2: 0.55

## Method

- Common root seeds and domain-separated ChaCha20 Fisher-Yates shuffles.
- Independent frozen London-mulligan policies through a forced three-card keep.
- 128 visible hands per mulligan stage and four independent continuations per
  visible hand. The policy cannot inspect the next offered hand.
- Gemstone Caverns is live with probability 3/4, sampled before mulligan choices.
- Exact ordered-library Rust search, with stochastic Gamble discard.
- Primary sample: 1,000 matched games, 5,000 initial states and a 60,000-state
  cap rerun.
- Sensitivity sample: the first 250 matched games rerun with a 300,000-state cap
  using the same seeds and policy estimator.

## Results

| Run | Deck | Success | Cap interval | R T1 | R T2 | H T1 | H T2 | Weighted |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| 1,000-game primary | Rograkh/Silas | 46.1% | 46.1-52.7% | 4.2% | 41.9% | 0 | 0 | 35.63% |
| 1,000-game primary | Rograkh/Thrasios | 37.1% | 37.1-58.0% | 2.4% | 9.9% | 2.2% | 22.6% | 23.80% |
| 250-game, 300k cap | Rograkh/Silas | 46.8% | 46.8-48.0% | 3.2% | 43.6% | 0 | 0 | 35.90% |
| 250-game, 300k cap | Rograkh/Thrasios | 35.6% | 35.6-42.4% | 2.4% | 12.4% | 1.6% | 19.2% | 23.38% |

In the higher-cap paired sample, Rograkh/Thrasios minus Rograkh/Silas is -11.2
percentage points for binary success (paired approximate 95% interval -19.6 to
-2.8 points). There are 73 Silas-only wins and 45 Thrasios-only wins; exact
two-sided McNemar p = 0.0126. Weighted utility differs by -12.52 points (paired
95% interval -18.65 to -6.39 points).

The primary run alone is not conclusive because the creature list creates many
more capped states. The deeper sensitivity run supports Rograkh/Silas: even if
all 17 unresolved Thrasios games succeeded, its point ceiling is 42.4%, below
the observed 46.8% Rograkh/Silas rate in that matched sample.

## Oracle-sensitive implementation

The comparison adds target-relevant rules support for the full supplied list.
The most important cases are:

- Dryad Arbor is played or fetched as a land, is also a green creature, and has
  summoning sickness. Earthbend grants haste to its target.
- Badgermole Cub earthbends a land on entry. Its mana trigger applies only when
  a creature activates its own tap mana ability, not when Earthcraft,
  Springleaf Drum, or Gene Pollinator taps another creature.
- Seymour Guado is treated as its canonical card, Kinnan, Bonder Prodigy.
  Kinnan adds mana only when a nonland permanent activates its own tap mana
  ability. Its 5GU activation examines the actual top five cards.
- Gene Pollinator must itself be able to tap, but its second tapped permanent
  may be summoning sick. That second tap does not trigger Badgermole or Kinnan.
- Cryptolith Rite gives creatures their own tap mana ability, so summoning
  sickness, Kinnan, and Badgermole all apply. Earthcraft may tap a fresh
  creature but can untap only the literal basic Forest in this deck.
- Gaea's Cradle counts all creatures, including fresh creatures and earthbent
  lands. Shimmerwilds Growth adds the chosen color when its land is tapped.
- Chord of Calling uses exact colored convoke, including fresh creatures;
  Finale of Devastation, Green Sun's Zenith, Summoner's Pact, Worldly Tutor,
  Nature's Rhythm, and Crop Rotation use typed legal target sets.
- Shifting Woodland checks for a Forest when entering and can become Heartwood
  with delirium. Hydroelectric Specimen is a nonland in the library and its
  back face is available as the land play from hand.
- Mockingbird observes mana spent and legal mana-value copy targets. Storm-Kiln
  Artist, Birgi, Jeska's Will, Underworld Breach, Thrasios, and Valley
  Floodcaller have opening-relevant cast, mana, draw, or untap behavior.

The Badgermole and Kinnan trigger distinctions follow their Oracle text and
rulings: [Badgermole Cub](https://api.scryfall.com/cards/named?exact=Badgermole%20Cub)
and [Kinnan, Bonder Prodigy](https://api.scryfall.com/cards/named?exact=Kinnan%2C%20Bonder%20Prodigy).

## Scope limits

Pure protection and interaction are legal cards, colored Chrome Mox imprints,
and Offer bait where appropriate, but opponent interaction is outside this
goldfish objective. Copy Enchantment and Clever Impersonator do not improve the
first-engine endpoint after that engine has already resolved. Underworld Breach
enumerates relevant engine and Rite of Flame escapes; Flare of Duplication
copies relevant ritual lines. Shifting Woodland enumerates Heartwood as its
relevant permanent-copy target. Thrasios uses an observable, no-lookahead scry
policy. These restrictions avoid inventing opponent-dependent value while
retaining lines that can change engine resolution by turn two.
