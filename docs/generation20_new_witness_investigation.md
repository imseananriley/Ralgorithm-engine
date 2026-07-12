# Generation 20: Investigation of eight new confirmed wins

## Correction

Mox Amber adds mana of a color among legendary creatures and planeswalkers the
player controls. It uses their actual colors, not color identity. Nick Fury is
white, so Nick alone lets Mox Amber make only white.

The initial investigation incorrectly treated Nick's five-color identity as five
Mox Amber colors. Correcting both packed mana paths changes the original 13 new
confirmations as follows:

- Games 213, 436, and 702 become probabilistic rather than deterministic.
- Games 360 and 697 no longer produce packed winning branches through D2.
- Eight deterministic confirmations remain: 174, 344, 493, 518, 638, 743, 894,
  and 919.

## Independent validation

All eight remaining wins capped in the legacy exact engine at 60,000 states. At a
one-million-state limit, the corrected exact engine independently finds all eight
in 2.2 seconds total. They are genuine frontier misses rather than packed-model
false positives.

## Witness summary

| Game | Packed tier | Engine | Principal line |
|---:|---:|---|---|
| 174 | D2 | Heartwood | Mox Diamond, Mystical for GSZ, Nick, Rite, GSZ |
| 344 | D1 | Heartwood | Chrome Mox, Nick, fetch Bayou, Vampiric, fetch Tropical |
| 493 | D2 | Rhystic | fetch Bayou, Vampiric, Sol Ring, Exotic Orchard |
| 518 | D1 | Rhystic | Scheming, Crop Rotation for Ancient Tomb, second land |
| 638 | D0 | Rhystic | Mox Diamond, Vampiric, Nick, fetch Hallowed |
| 743 | D1 | Heartwood | Vampiric, two lands, Elvish Spirit Guide |
| 894 | D0 | Heartwood | Mox Diamond, Scheming, Nick, Mox Amber |
| 919 | D1 | Rhystic | Imperial Seal, City of Brass, City of Traitors |

All eight resolve on turn two: four Rhystic Study and four Heartwood Storyteller.
All eight use a top-deck tutor, four cast Nick Fury, three use Mox Diamond, and one
uses Mox Amber.

## Notable lines

Game 518 is the previously identified Crop Rotation line: cast Scheming Symmetry,
then on turn two tap and sacrifice City of Brass to Crop Rotation for Ancient
Tomb, play Forbidden Orchard, and cast Rhystic Study.

Game 894 remains legal under the corrected Mox Amber rule. Mox Amber supplies
white toward Heartwood's generic mana, while Mox Diamond and Gemstone Mine supply
the two green mana.

The witnesses for games 493 and 743 cast Noxious Revival after drawing the engine.
That action is unnecessary to the final engine resolution; the witnesses are
valid but not action-minimal.

## Engineering result

Nick remains white in both engines. Mox Amber derives available mana from actual
legendary permanent colors. The packed engine now uses one shared color helper for
direct payment planning and explicit artifact activation, and tests require Nick
to enable only white from Mox Amber.
