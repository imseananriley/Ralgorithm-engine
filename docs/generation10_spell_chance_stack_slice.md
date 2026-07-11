# Generation Ten: Spell, Chance, And Stack Slice

## Added Semantics

Generation ten ports the opening spell families that most directly change Rhystic Study and Heartwood Storyteller reachability:

- Elvish and Simian Spirit Guide as direct hand-exile payment sources;
- Dark Ritual and Rite of Flame;
- Demonic Tutor, Imperial Seal, Vampiric Tutor, Enlightened Tutor, Scheming Symmetry, and Wishclaw Talisman;
- Manamorphose with explicit color choices and draw chance;
- Gamble with exact post-search random discard;
- Noxious Revival graveyard recursion to a known library top;
- Green Sun's Zenith for Heartwood and Summoner's Pact for green creatures;
- Crop Rotation;
- Culling the Weak, Diabolic Intent, Infernal Plunge, and Rain of Filth; and
- the valid Demonic Tutor hold-priority Lion's Eye Diamond line.

Live Gemstone Caverns states now expose a pre-turn-one-draw priority window. Only instant-speed actions are generated there. Vampiric or Enlightened Tutor can therefore establish a known first draw without permitting Imperial Seal, Scheming Symmetry, Rite of Flame, or other sorcery-speed actions.

## Chance And Hidden Information

Manamorphose draw and Gamble discard are `InformationTransition::Chance` nodes. Tutor searches remove the selected card, forget prior known-top information when the effect shuffles, then either add the target to hand or establish a new known top. No action receives an ordered unknown library.

## Resource Details

Green Sun's Zenith shuffles itself into the unknown library after putting Heartwood onto the battlefield. Summoner's Pact records a debt. A turn-one Pact must actually pay `2GG` at the next upkeep, consuming untapped battlefield sources and instant-speed hand mana. A turn-two engine result with unpaid Pact is accepted only when the modeled next upkeep payment exists.

Rain of Filth adds both direct sacrifice-for-black and the legal compound line that taps a land for its native mana before sacrificing it for black. Crop Rotation may likewise tap the land for green and sacrifice that same land as the additional cost. Sacrifice spells can use Nick Fury and return it to the command zone.

## Verification

Thirty focused opening-model tests cover the new and prior land/artifact rules. The stack test distinguishes the valid Demonic Tutor plus LED line from the invalid Gamble plus LED proposal: Demonic resolves after LED and puts the target into the emptied hand, whereas Gamble necessarily performs random discard after finding its target.

## Remaining Boundary

Mana creatures and their combat/upkeep assumptions, Eldritch Evolution-style creature chains, Mystical Tutor chains, and several post-engine interaction/value cards remain outside the packed opening model. The legacy engine remains the strict correction oracle for those families.
