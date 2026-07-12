# Generation 20: Investigation of 13 new confirmed wins

## Conclusion

All 13 full-library-confirmed packed witnesses are genuine turn-two lines. After
fixing one legacy Mox Amber bug and increasing the legacy frontier for capped
hands, the exact fast engine independently reproduces all 13.

## Why the legacy engine missed them

- Four games were uncapped legacy failures: 213, 360, 697, and 702.
- Nine games were capped at the old limit: 174, 344, 436, 493, 518, 638, 743,
  894, and 919.
- Every uncapped miss uses Mox Amber after resolving Nick Fury.

The legacy engine represented Nick Fury's battlefield color as white and derived
Mox Amber output from that value. Nick is a white card but has five-color identity,
so Mox Amber must make any color. Correcting Nick's battlefield identity lets the
legacy engine find the four uncapped misses plus capped game 894 at 60,000 states.

The remaining eight still cap at 60,000 states. At a one-million-state limit, the
exact engine finds all eight in 2.4 seconds total, confirming that they were
frontier misses rather than packed-model false positives.

## Witness summary

| Game | Packed tier | Engine | Legacy result | Principal line |
|---:|---:|---|---|---|
| 174 | D2 | Heartwood | capped | Mox Diamond, Mystical for GSZ, Nick, Rite, GSZ |
| 213 | D2 | Heartwood | miss | Mox Diamond, Nick, Mox Amber, Dark Ritual, GSZ |
| 344 | D1 | Heartwood | capped | Chrome Mox, Nick, fetch Bayou, Vampiric, fetch Tropical |
| 360 | D2 | Rhystic | miss | Petal, Nick, Mox Amber, Enlightened, Dark Ritual |
| 436 | D2 | Rhystic | capped | Mox Amber, fetch Hallowed, Nick, Scheming, Glimmervoid |
| 493 | D2 | Rhystic | capped | fetch Bayou, Vampiric, Sol Ring, Exotic Orchard |
| 518 | D1 | Rhystic | capped | Scheming, Crop Rotation for Ancient Tomb, second land |
| 638 | D0 | Rhystic | capped | Mox Diamond, Vampiric, Nick, fetch Hallowed |
| 697 | D0 | Rhystic | miss | Nick, Mox Amber, turn-two Tinder Wall |
| 702 | D0 | Heartwood | miss | Nick, Mox Amber, Tinder Wall, GSZ |
| 743 | D1 | Heartwood | capped | Vampiric, two lands, Elvish Spirit Guide |
| 894 | D0 | Heartwood | capped | Mox Diamond, Scheming, Nick, Mox Amber |
| 919 | D1 | Rhystic | capped | Imperial Seal, City of Brass, City of Traitors |

All 13 resolve on turn two. Seven resolve Rhystic Study and six resolve Heartwood
Storyteller. Ten use a top-deck tutor, nine cast Nick Fury, six use Mox Amber, and
four use Mox Diamond.

## Notable lines

Game 518 is the previously identified Crop Rotation line: cast Scheming Symmetry,
then on turn two tap and sacrifice City of Brass to Crop Rotation for Ancient
Tomb, play Forbidden Orchard, and cast Rhystic Study.

Game 697 demonstrates the legacy Mox Amber issue compactly. City of Brass casts
Nick on turn one. On turn two City casts Tinder Wall, Mox Amber makes blue, and
Tinder Wall supplies the two generic mana for Rhystic Study.

Game 702 draws a nominal 1/91 unknown card after a fetch shuffle, but the card is
never used. The line wins with Hallowed Fountain, Mox Amber, and Tinder Wall for
Green Sun's Zenith regardless of that draw, so full-library validation correctly
classifies it as deterministic.

The witnesses for games 493 and 743 cast Noxious Revival after drawing the engine.
That action is unnecessary to the final engine resolution; the witnesses are
valid but not action-minimal.

## Engineering result

The legacy Nick Fury permanent now carries five-color identity for Mox Amber
generation while remaining a white card for casting and imprint rules. A
regression test requires Mox Amber to expose all five colors with Nick on the
battlefield.
