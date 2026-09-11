#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import os
import re
import subprocess
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]


LANDS = {
    "Ancient Tomb": ("modeled", "land", "two-colorless land"),
    "Arid Mesa": ("modeled", "fetch_land", "fetches supported typed dual/shock targets"),
    "Badlands": ("modeled", "land", "typed BR dual"),
    "Bayou": ("modeled", "land", "typed BG land"),
    "Bloodstained Mire": ("modeled", "fetch_land", "fetches supported typed dual/shock targets"),
    "Blood Crypt": ("modeled", "land", "typed BR shock"),
    "Breeding Pool": ("modeled", "land", "typed UG shock"),
    "Boseiju, Who Endures": ("modeled", "land", "green land; channel text not modeled"),
    "City of Brass": ("modeled", "land", "five-color land"),
    "City of Traitors": ("modeled", "land", "two-colorless land with sacrifice-on-next-land"),
    "Command Tower": ("modeled", "land", "five-color commander-identity land"),
    "Crystal Vein": ("modeled", "land", "taps for C or sacrifices for CC"),
    "Emergence Zone": ("partial", "land", "colorless land modeled; flash activation intentionally not modeled for this objective"),
    "Exotic Orchard": ("modeled", "land", "approximated as five-color land"),
    "Flooded Strand": ("modeled", "fetch_land", "fetches supported typed dual/shock targets"),
    "Forest": ("modeled", "land", "basic Forest"),
    "Forbidden Orchard": ("modeled", "land", "five-color land"),
    "Gemstone Caverns": ("modeled", "pregame_land", "pre-sampled live at configured rate"),
    "Gemstone Mine": ("modeled", "land", "three counters"),
    "Glimmervoid": ("modeled", "land", "unconditional five-color tap; sacrifices at end step if no artifact is controlled"),
    "Glittering Caves of Aglarond": ("modeled", "pregame_land_alias", "Gemstone Caverns alias; pre-sampled live at configured rate, colorless without luck counter"),
    "Hallowed Fountain": ("modeled", "land", "typed UW shock"),
    "Hydroelectric Specimen": ("modeled", "mdfc_land_creature", "blue land face and castable blue creature face"),
    "Mana Confluence": ("modeled", "land", "five-color land"),
    "Marsh Flats": ("modeled", "fetch_land", "fetches supported typed dual/shock targets"),
    "Misty Rainforest": ("modeled", "fetch_land", "fetches supported typed dual/shock targets"),
    "Otawara, Soaring City": ("modeled", "land", "blue land; channel text not modeled"),
    "Phyrexian Tower": ("modeled", "land", "taps for C or sacrifices a creature for BB"),
    "Plateau": ("modeled", "land", "typed RW dual"),
    "Polluted Delta": ("modeled", "fetch_land", "fetches supported typed dual/shock targets"),
    "Scalding Tarn": ("modeled", "fetch_land", "fetches supported typed dual/shock targets"),
    "Scrubland": ("modeled", "land", "typed BW dual"),
    "Sea of Clouds": ("modeled", "land", "UW land"),
    "Shifting Woodland": ("modeled", "land", "enters tapped and taps for green; delirium copy text is outside this objective"),
    "Sink into Stupor": ("modeled", "mdfc_land", "modeled as untapped blue MDFC land for this objective"),
    "Starting Town": ("modeled", "land", "five-color land"),
    "Steam Vents": ("modeled", "land", "typed RU shock"),
    "Taiga": ("modeled", "land", "typed RG dual"),
    "Tarnished Citadel": ("modeled", "land", "five-color land"),
    "Tropical Island": ("modeled", "land", "typed UG dual"),
    "Tundra": ("modeled", "land", "typed UW dual"),
    "Underground Sea": ("modeled", "land", "typed UB dual"),
    "Verdant Catacombs": ("modeled", "fetch_land", "fetches supported typed dual/shock targets"),
    "Volcanic Island": ("modeled", "land", "typed UR dual"),
    "Watery Grave": ("modeled", "land", "typed UB shock"),
    "Windswept Heath": ("modeled", "fetch_land", "fetches supported typed dual/shock targets"),
    "Wooded Foothills": ("modeled", "fetch_land", "fetches supported typed dual/shock targets"),
}

ACTION_CARDS = {
    "Ad Nauseam": ("partial", "draw", "turbo context uses a documented conservative fifteen-card draw proxy"),
    "An Offer You Can't Refuse": ("modeled", "self_counter", "can counter supported own spells for two Treasures"),
    "Angel's Grace": ("modeled", "pact_survival", "modeled for Pact upkeep survival checks"),
    "Arcane Signet": ("modeled", "mana_artifact", "casts for 2, taps for any color"),
    "Beseech the Mirror": ("modeled", "tutor", "normal, bargained, and LED-priority tutor lines"),
    "Badgermole Cub": ("modeled", "mana_creature", "earthbends a land on entry and applies the supported land-creature mana bonuses"),
    "Brain Freeze": ("modeled", "storm_mill", "counts prior spells and mills the actual ordered library"),
    "Birds of Paradise": ("modeled", "mana_creature", "casts for G, taps next turn for any color"),
    "Birgi, God of Storytelling": ("modeled", "mana_creature", "casts for 2R and adds R after later spells cast that turn"),
    "Chrome Mox": ("modeled", "mana_artifact", "imprints colored nonartifact nonland cards"),
    "Crop Rotation": ("modeled", "land_tutor", "sacrifices land and finds supported mana lands"),
    "Culling the Weak": ("modeled", "ritual", "sacrifices creature for BBBB"),
    "Cabal Ritual": ("modeled", "ritual", "1B to BBB, or BBBBB with threshold"),
    "Chromatic Star": ("modeled", "mana_artifact", "casts for 1; pays 1, sacrifices, filters one mana, and draws"),
    "Chord of Calling": ("modeled", "creature_tutor", "uses colored/generic convoke and puts supported creatures onto the battlefield"),
    "Cryptolith Rite": ("modeled", "mana_enchantment", "non-summoning-sick creatures tap for any color"),
    "Curse of Opulence": ("modeled", "combat_mana", "Rograkh or Ragavan combat creates the opening-relevant Treasure"),
    "Dark Ritual": ("modeled", "ritual", "B to BBB"),
    "Deathrite Shaman": ("modeled", "mana_creature", "casts for B/G, taps next turn using land-grave count"),
    "Demonic Tutor": ("modeled", "tutor", "finds Rhystic/Heartwood and relevant intermediate targets through Mystical"),
    "Demonic Consultation": ("partial", "tutor", "finds Rhystic from the actual singleton library when it survives the first six exiles"),
    "Defense Grid": ("partial", "artifact_body", "castable artifact body; opponent tax text is outside the proactive objective"),
    "Diabolic Intent": ("modeled", "tutor", "sacrifices creature and finds engine"),
    "Eldritch Evolution": ("modeled", "creature_tutor", "sacrifices creature into Heartwood"),
    "Elvish Spirit Guide": ("modeled", "free_mana", "exiles for G"),
    "Dramatic Reversal": ("modeled", "untap", "untaps nonland permanents without removing summoning sickness"),
    "Dryad Arbor": ("modeled", "land_creature", "green Forest creature with summoning sickness and creature-tutor interactions"),
    "Earthcraft": ("modeled", "mana_enchantment", "taps creatures to untap a tapped basic Forest"),
    "Enlightened Tutor": ("modeled", "top_tutor", "top-tutors Rhystic in rhystic_heartwood mode"),
    "Esper Sentinel": ("modeled", "artifact_creature", "castable W artifact-creature body; draw trigger not modeled"),
    "Faerie Mastermind": ("partial", "creature_body", "castable MV2 blue creature body; draw text not modeled"),
    "Finale of Devastation": ("modeled", "creature_tutor", "finds supported creatures from library or graveyard for XGG"),
    "Flare of Duplication": ("partial", "ritual_copy", "free mode copies supported ritual spells by sacrificing a red creature"),
    "Flashback": ("partial", "graveyard_recursion", "supported mode recasts Dark Ritual, Rite of Flame, or Cabal Ritual"),
    "Gamble": ("modeled", "tutor", "simplified stochastic discard mode"),
    "Green Sun's Zenith": ("modeled", "creature_tutor", "finds Heartwood or supported green mana creatures"),
    "Gitaxian Probe": ("modeled", "draw", "casts for two life and draws the next card"),
    "Grim Tutor": ("modeled", "tutor", "finds engine to hand"),
    "Gaea's Cradle": ("modeled", "mana_land", "taps for green equal to controlled creature count"),
    "Gene Pollinator": ("modeled", "mana_creature", "casts for G and taps itself plus another permanent for any color"),
    "Grim Monolith": ("modeled", "mana_artifact", "casts for 2 and taps for CCC; untap cost is irrelevant by turn two"),
    "Grinding Station": ("partial", "artifact_body", "castable artifact body; mill-combo activation is not modeled for this objective"),
    "Heartwood Storyteller": ("modeled", "engine", "native objective engine"),
    "Idyllic Tutor": ("modeled", "tutor", "finds Rhystic to hand"),
    "Ignoble Hierarch": ("modeled", "mana_creature", "casts for G, taps next turn for B/R/G"),
    "Imperial Seal": ("modeled", "top_tutor", "top-tutors engine"),
    "Lion's Eye Diamond": ("modeled", "mana_artifact", "casts for 0; priority-hold lines with supported tutors"),
    "Infernal Plunge": ("modeled", "ritual", "R plus a creature sacrifice produces RRR"),
    "Jeska's Will": ("partial", "ritual_exile", "with a commander, assumes seven opposing cards and exposes three playable cards until end of turn"),
    "Jeweled Amulet": ("modeled", "mana_artifact", "stores one available mana and releases that color after untapping"),
    "Lotho, Corrupt Shirriff": ("modeled", "mana_creature", "casts for BW; creates Treasure on second spell"),
    "Lotus Petal": ("modeled", "mana_artifact", "casts for 0, sacrifices for any color"),
    "Mana Vault": ("modeled", "mana_artifact", "casts for 1, taps for CCC"),
    "Manamorphose": ("modeled", "ritual_draw", "filters two mana and draws next known card"),
    "Mockingbird": ("modeled", "copy_creature", "copies supported nonlegendary creatures within the paid X restriction"),
    "Mox Amber": ("modeled", "mana_artifact", "casts for 0, taps from legendary colors"),
    "Mox Diamond": ("modeled", "mana_artifact", "casts for 0 by discarding a land, taps any color"),
    "Mox Opal": ("modeled", "mana_artifact", "casts for 0, metalcraft taps any color"),
    "Mystical Tutor": ("modeled", "top_tutor", "top-tutors supported instants/sorceries"),
    "Neoform": ("modeled", "creature_tutor", "sacrifices MV2 creature into Heartwood"),
    "Noble Hierarch": ("modeled", "mana_creature", "casts for G, taps next turn for U/W/G"),
    "Noxious Revival": ("modeled", "graveyard_recursion", "recurs named graveyard cards to the top of library; also remains supported as Offer bait"),
    "Nature's Rhythm": ("modeled", "creature_tutor", "uses the creature-tutor mode and graveyard Harmonize cost/reduction rules"),
    "Necropotence": ("partial", "draw", "turbo context uses a thirty-card end-step draw proxy"),
    "Orcish Bowmasters": ("partial", "creature_body", "castable MV2 black creature body; trigger text not modeled"),
    "Paradise Mantle": ("modeled", "mana_artifact", "casts for 0; equips for 1; equipped creature taps if not summoning sick"),
    "Ragavan, Nimble Pilferer": ("modeled", "mana_creature", "casts for R; always-connect Treasure attack next turn"),
    "Rain of Filth": ("modeled", "ritual", "lands can be sacrificed for B after cast"),
    "Ranger-Captain of Eos": ("modeled", "creature_tutor", "casts for 1WW and finds Esper Sentinel"),
    "Relic of Legends": ("modeled", "mana_artifact", "casts for 3, taps itself or legendary creatures for any color"),
    "Rhystic Study": ("modeled", "engine", "native objective engine"),
    "Rite of Flame": ("modeled", "ritual", "R to RR"),
    "Scheming Symmetry": ("modeled", "top_tutor", "top-tutors engine"),
    "Serum Powder": ("modeled", "mulligan_mana_artifact", "casts for 3 and taps for C; after required bottoms, low-EV hands may be exiled to redraw the current hand size"),
    "Simian Spirit Guide": ("modeled", "free_mana", "exiles for R"),
    "Sol Ring": ("modeled", "mana_artifact", "casts for 1, taps for CC"),
    "Springleaf Drum": ("modeled", "mana_artifact", "casts for 1; taps any untapped creature for any color"),
    "Seymour Guado": ("modeled", "mana_creature_alias", "uses Kinnan semantics: GU 2/2 and bonus mana from nonland tap sources"),
    "Shimmerwilds Growth": ("modeled", "mana_enchantment", "enchants a land and adds one chosen color when that land is tapped"),
    "Street Wraith": ("modeled", "draw", "cycles for two life, including during the pre-turn priority window"),
    "Storm-Kiln Artist": ("modeled", "mana_creature", "creates Treasure for supported instant and sorcery casts"),
    "Summoner's Pact": ("modeled", "creature_tutor", "finds Heartwood or supported green mana creatures with Pact survival check"),
    "The Cabbage Merchant": ("partial", "creature_body", "castable MV3 legendary green body; Food text not modeled"),
    "Tinder Wall": ("modeled", "mana_creature", "casts for G, sacrifices for RR"),
    "Tainted Pact": ("partial", "tutor", "uses singleton-library determinism to find Rhystic"),
    "Talisman of Dominance": ("modeled", "mana_artifact", "casts for 2 and taps for colorless, blue, or black"),
    "Underworld Breach": ("modeled", "graveyard_engine", "escapes supported engines, tutors, rituals, artifacts, and Brain Freeze lines"),
    "Vexing Bauble": ("modeled", "artifact_draw", "blocks zero-mana spells while active and pays 1 to sacrifice and draw"),
    "Valley Floodcaller": ("partial", "creature_body", "castable MV3 blue creature body; flash/untap text not modeled"),
    "Vampiric Tutor": ("modeled", "top_tutor", "top-tutors engine"),
    "Wild Cantor": ("modeled", "mana_creature", "casts for R/G, sacrifices for any color"),
    "Wishclaw Talisman": ("modeled", "tutor_artifact", "casts for 1B, activates for 1, including LED-priority line"),
    "Wheel of Fortune": ("modeled", "wheel", "discards the hand and draws seven from the actual library"),
    "Windfall": ("partial", "wheel", "uses a seven-card draw proxy after discarding the hand"),
    "Worldly Tutor": ("modeled", "top_tutor", "top-tutors Heartwood or supported mana creatures"),
}

PASSIVE_BY_DESIGN = {
    "Borne Upon a Wind": "post-engine/stack-timing card; not needed for own-turn Rhystic/Heartwood objective",
    "Chain of Vapor": "interaction/bounce not modeled as pre-engine acceleration",
    "Commandeer": "interaction/pitch card",
    "Copy Enchantment": "copy engine excluded from current objective by request",
    "Deflecting Swat": "interaction/pitch card",
    "Daze": "interaction/pitch card",
    "Dispel": "interaction",
    "Disrupting Shoal": "interaction/pitch card",
    "Faerie Mastermind": "see partial creature-body row",
    "Fierce Guardianship": "interaction/pitch card",
    "Firestorm": "interaction/discard outlet not modeled for this objective",
    "Flash Photography": "copy engine excluded from current objective by request",
    "Flusterstorm": "interaction",
    "Force of Negation": "interaction/pitch card",
    "Force of Will": "interaction/pitch card",
    "Mental Misstep": "interaction",
    "Mindbreak Trap": "interaction/pitch card",
    "Misdirection": "interaction/pitch card",
    "Molten Disaster": "win/protection card, not pre-engine acceleration",
    "Mystic Remora": "not optimized in current Rhystic/Heartwood objective",
    "Mnemonic Betrayal": "opponent graveyards are outside the opening-engine objective",
    "Orcish Bowmasters": "see partial creature-body row",
    "Orim's Chant": "interaction/protection",
    "Pact of Negation": "interaction",
    "Pyroblast": "interaction",
    "Praetor's Grasp": "opponent libraries are outside the opening-engine objective",
    "Red Elemental Blast": "interaction",
    "Silence": "interaction/protection",
    "Smothering Tithe": "not a target in current Rhystic/Heartwood objective",
    "Snapback": "interaction/pitch card",
    "Subtlety": "interaction/pitch card",
    "Sudden Substitution": "win/protection card, not pre-engine acceleration",
    "Swan Song": "interaction",
    "Thassa's Oracle": "win condition rather than pre-engine acceleration",
    "Clever Impersonator": "copy engine excluded from the current objective",
    "Into the Flood Maw": "interaction/bounce",
    "The Cabbage Merchant": "see partial creature-body row",
    "Valley Floodcaller": "see partial creature-body row",
    "Wipe Away": "interaction",
    "Word of Seizing": "win/protection card, not pre-engine acceleration",
}

PROBES: dict[str, dict[str, Any]] = {
    "Beseech the Mirror": {
        "hand": ["City of Brass", "Lotus Petal", "Dark Ritual", "Mox Amber", "Beseech the Mirror"],
        "library": ["Rhystic Study"],
        "expect": "Rhystic Study",
    },
    "Wishclaw Talisman": {
        "hand": ["City of Brass", "Ancient Tomb", "Mana Vault", "Lotus Petal", "Wishclaw Talisman"],
        "library": ["Rhystic Study"],
        "expect": "Rhystic Study",
    },
    "Grim Tutor": {
        "hand": ["City of Brass", "Ancient Tomb", "Mana Vault", "Lotus Petal", "Chrome Mox", "Fierce Guardianship", "Grim Tutor"],
        "library": ["Rhystic Study"],
        "expect": "Rhystic Study",
    },
    "Idyllic Tutor": {
        "hand": ["City of Brass", "Ancient Tomb", "Mana Vault", "Chrome Mox", "Fierce Guardianship", "Idyllic Tutor"],
        "library": ["Rhystic Study"],
        "expect": "Rhystic Study",
    },
    "Worldly Tutor": {
        "hand": ["Tropical Island", "Ancient Tomb", "Lotus Petal", "Worldly Tutor"],
        "library": ["Heartwood Storyteller"],
        "expect": "Heartwood Storyteller",
    },
    "Springleaf Drum": {
        "hand": ["Starting Town", "Lotus Petal", "Mana Vault", "Springleaf Drum", "Rhystic Study"],
        "library": [],
        "expect": "Rhystic Study",
    },
    "Paradise Mantle": {
        "hand": ["Tropical Island", "Ancient Tomb", "Birds of Paradise", "Paradise Mantle", "Rhystic Study"],
        "library": [],
        "expect": "Rhystic Study",
    },
    "Mox Opal": {
        "hand": ["Ancient Tomb", "Mox Opal", "Lotus Petal", "Mox Amber", "Rhystic Study"],
        "library": [],
        "expect": "Rhystic Study",
    },
    "Arcane Signet": {
        "hand": ["Ancient Tomb", "Arcane Signet", "Rhystic Study"],
        "library": [],
        "expect": "Rhystic Study",
    },
    "Relic of Legends": {
        "hand": ["Ancient Tomb", "Mana Vault", "Relic of Legends", "Rhystic Study"],
        "library": [],
        "expect": "Rhystic Study",
    },
    "Manamorphose": {
        "hand": ["Tropical Island", "Ancient Tomb", "Manamorphose"],
        "library": ["Rhystic Study"],
        "expect": "Rhystic Study",
    },
    "Noble Hierarch": {
        "hand": ["Bayou", "City of Brass", "Noble Hierarch", "Rhystic Study"],
        "library": [],
        "expect": "Rhystic Study",
    },
    "Ignoble Hierarch": {
        "hand": ["Bayou", "City of Brass", "Ignoble Hierarch", "Heartwood Storyteller"],
        "library": [],
        "expect": "Heartwood Storyteller",
    },
    "Wild Cantor": {
        "hand": ["Tropical Island", "Ancient Tomb", "Wild Cantor", "Heartwood Storyteller"],
        "library": [],
        "expect": "Heartwood Storyteller",
    },
    "Faerie Mastermind": {
        "hand": ["Tropical Island", "Ancient Tomb", "Mox Diamond", "Tundra", "Faerie Mastermind", "Neoform"],
        "library": ["Heartwood Storyteller"],
        "expect": "Heartwood Storyteller",
    },
    "Orcish Bowmasters": {
        "hand": ["Bayou", "Ancient Tomb", "Mox Diamond", "Tundra", "Orcish Bowmasters", "Neoform", "Lotus Petal"],
        "library": ["Heartwood Storyteller"],
        "expect": "Heartwood Storyteller",
    },
    "The Cabbage Merchant": {
        "hand": ["Tropical Island", "Ancient Tomb", "Mana Vault", "Lotus Petal", "Mox Diamond", "Bayou", "The Cabbage Merchant", "Eldritch Evolution"],
        "library": ["Heartwood Storyteller"],
        "expect": "Heartwood Storyteller",
    },
    "Valley Floodcaller": {
        "hand": ["Tropical Island", "Ancient Tomb", "Mox Diamond", "Tundra", "Mana Vault", "Valley Floodcaller", "Eldritch Evolution"],
        "library": ["Heartwood Storyteller"],
        "expect": "Heartwood Storyteller",
    },
    "Noxious Revival": {
        "hand": ["Ancient Tomb", "Lotus Petal", "Gamble", "Noxious Revival", "Blank"],
        "negative_hand": ["Ancient Tomb", "Lotus Petal", "Gamble", "Blank", "Blank"],
        "library": ["Blank", "Blank", "Rhystic Study"],
        "expect": "Rhystic Study",
    },
    "Gemstone Caverns": {
        "hand": ["Gemstone Caverns", "Ancient Tomb", "Vampiric Tutor", "Blank"],
        "negative_hand": ["Gemstone Caverns", "Ancient Tomb", "Vampiric Tutor", "Blank"],
        "library": ["Blank", "Rhystic Study"],
        "expect": "Rhystic Study",
        "gemstone_live": True,
        "negative_gemstone_live": False,
        "max_turns": 1,
    },
    "Crop Rotation": {
        "hand": ["Tropical Island", "City of Brass", "Crop Rotation", "Rhystic Study"],
        "library": ["Ancient Tomb"],
        "expect": "Rhystic Study",
    },
    "Glittering Caves of Aglarond": {
        "hand": ["Glittering Caves of Aglarond", "Ancient Tomb", "Rhystic Study", "Blank"],
        "negative_hand": ["Glittering Caves of Aglarond", "Ancient Tomb", "Rhystic Study", "Blank"],
        "library": ["Blank"],
        "expect": "Rhystic Study",
        "gemstone_live": True,
        "negative_gemstone_live": False,
        "max_turns": 1,
    },
    "Gitaxian Probe": {
        "hand": ["Ancient Tomb", "Lotus Petal", "Gitaxian Probe"],
        "negative_hand": ["Ancient Tomb", "Lotus Petal", "Blank"],
        "library": ["Blank", "Rhystic Study"],
        "expect": "Rhystic Study",
        "max_turns": 1,
    },
}


def read_deck_names(path: Path) -> list[str]:
    payload = json.loads(path.read_text()[path.read_text().find("{") :])
    if isinstance(payload.get("deck"), list):
        return [str(name) for name in payload["deck"]]
    cards = ((payload.get("boards") or {}).get("mainboard") or {}).get("cards") or {}
    names: list[str] = []
    for entry in cards.values():
        qty = int(entry.get("quantity", 1))
        names.extend([entry["card"]["name"]] * qty)
    return names


def parse_swaps(path: Path) -> tuple[dict[str, set[str]], dict[str, set[str]]]:
    cuts: dict[str, set[str]] = defaultdict(set)
    adds: dict[str, set[str]] = defaultdict(set)
    if not path.exists():
        return cuts, adds
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        label = line.split(":", 1)[0].strip() if ":" in line else ""
        body = line.split(":", 1)[1].strip() if ":" in line else line
        if ";" in body:
            pairs = [part.split("=", 1) for part in body.split(";") if "=" in part]
        elif body.count("=") == 1:
            pairs = [body.split("=", 1)]
        else:
            pairs = re.findall(r"([^=,]+?)=([^,]+)(?:,|$)", body)
        for cut, add in pairs:
            cuts[cut.strip()].add(label)
            adds[add.strip()].add(label)
    return cuts, adds


def classify(card: str) -> tuple[str, str, str]:
    if card in ACTION_CARDS:
        return ACTION_CARDS[card]
    if card in LANDS:
        return LANDS[card]
    if card in PASSIVE_BY_DESIGN:
        return ("passive_by_design", "nonobjective", PASSIVE_BY_DESIGN[card])
    return ("missing_review", "unknown", "no explicit audit classification")


def run_probe(binary: Path, card: str, spec: dict[str, Any]) -> dict[str, Any]:
    def solve(hand: list[str], *, gemstone_live: bool) -> dict[str, Any]:
        req = {
            "hand": hand,
            "library": [*spec.get("library", []), *["Blank"] * 20],
            "gemstone_live": gemstone_live,
            "state_limit": 120_000,
            "max_turns": spec.get("max_turns", 2),
            "goal": "engine",
            "engine_target_count": 1,
            "engine_success_policy": "resilient",
            "remora_upkeep_payments": 0,
            "action_sort": True,
            "gamble_mode": "stochastic",
            "gamble_seed": 1,
            "simplified_gamble": True,
        }
        env = dict(os.environ)
        env["RHYSTIC_SIMPLIFIED_GAMBLE"] = "1"
        process = subprocess.run(
            [str(binary), "solve-keep-fast-jsonl"],
            input=json.dumps(req) + "\n",
            text=True,
            capture_output=True,
            env=env,
            check=True,
        )
        return json.loads(process.stdout)

    response = solve(spec["hand"], gemstone_live=bool(spec.get("gemstone_live", False)))
    negative = (
        solve(spec["negative_hand"], gemstone_live=bool(spec.get("negative_gemstone_live", spec.get("gemstone_live", False))))
        if "negative_hand" in spec
        else None
    )
    ok = response.get("label") == spec["expect"] and response.get("turn") in (1, 2) and not response.get("unsupported")
    if negative is not None:
        ok = ok and negative.get("turn") is None and not negative.get("unsupported")
    return {
        "card": card,
        "ok": ok,
        "turn": response.get("turn"),
        "label": response.get("label"),
        "capped": response.get("capped"),
        "unsupported": response.get("unsupported"),
        "unsupported_reason": response.get("unsupported_reason"),
        "negative_turn": negative.get("turn") if negative else None,
    }


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fieldnames = [
        "card",
        "in_base",
        "swap_cut_count",
        "swap_add_count",
        "status",
        "category",
        "notes",
        "probe",
        "probe_ok",
        "probe_turn",
        "probe_label",
    ]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames, lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow({key: row.get(key, "") for key in fieldnames})


def write_report(
    path: Path,
    rows: list[dict[str, Any]],
    probes: list[dict[str, Any]],
    deck: Path,
    swap_file: Path | None,
) -> None:
    counts = Counter(row["status"] for row in rows)
    critical = [row for row in rows if row["status"] == "missing_review" and (row["in_base"] or row["swap_add_count"])]
    failed_probes = [probe for probe in probes if not probe["ok"]]
    lines = [
        "# Rust coverage audit",
        "",
        f"Deck: `{deck}`",
        f"Swap file: `{swap_file}`" if swap_file else "Swap file: none",
        "",
        "## Summary",
        "",
    ]
    for status, count in sorted(counts.items()):
        lines.append(f"- {status}: {count}")
    lines.extend(["", "## Probe results", ""])
    lines.append("| card | ok | turn | label | capped | unsupported |")
    lines.append("|---|---:|---:|---|---:|---:|")
    for probe in probes:
        lines.append(
            f"| {probe['card']} | {probe['ok']} | {probe.get('turn')} | {probe.get('label')} | {probe.get('capped')} | {probe.get('unsupported')} |"
        )
    lines.extend(["", "## Critical review items", ""])
    if critical:
        for row in critical:
            lines.append(f"- {row['card']}: {row['notes']}")
    else:
        lines.append("- None in the current base deck or stage-one add set.")
    lines.extend(["", "## Failed probes", ""])
    if failed_probes:
        for probe in failed_probes:
            lines.append(f"- {probe['card']}: {probe}")
    else:
        lines.append("- None.")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Audit simulator semantics only for cards reachable from a deck and optional swap file."
    )
    parser.add_argument("--deck-json", required=True)
    parser.add_argument("--swap-file", default=None)
    parser.add_argument("--binary", default="target/release/rhystic-core-smoke")
    parser.add_argument("--csv-out", default=None)
    parser.add_argument("--report-out", default=None)
    parser.add_argument(
        "--fail-on-unsupported",
        action="store_true",
        help="Return a failure status when a base/add card has no explicit semantic classification.",
    )
    args = parser.parse_args()

    deck_path = (ROOT / args.deck_json).resolve()
    swap_path = (ROOT / args.swap_file).resolve() if args.swap_file else None
    binary = (ROOT / args.binary).resolve()
    names = read_deck_names(deck_path)
    cuts, adds = parse_swaps(swap_path) if swap_path else (defaultdict(set), defaultdict(set))
    all_cards = sorted(set(names) | set(cuts) | set(adds))
    probes_by_card: dict[str, dict[str, Any]] = {}
    probe_results: list[dict[str, Any]] = []
    for card in sorted(set(PROBES) & set(all_cards)):
        spec = PROBES[card]
        result = run_probe(binary, card, spec)
        probes_by_card[card] = result
        probe_results.append(result)

    rows: list[dict[str, Any]] = []
    base_set = set(names)
    for card in all_cards:
        status, category, notes = classify(card)
        probe = probes_by_card.get(card)
        rows.append(
            {
                "card": card,
                "in_base": card in base_set,
                "swap_cut_count": len(cuts.get(card, ())),
                "swap_add_count": len(adds.get(card, ())),
                "status": status,
                "category": category,
                "notes": notes,
                "probe": "yes" if probe else "",
                "probe_ok": probe["ok"] if probe else "",
                "probe_turn": probe.get("turn") if probe else "",
                "probe_label": probe.get("label") if probe else "",
            }
        )

    if args.csv_out:
        write_csv((ROOT / args.csv_out).resolve(), rows)
    if args.report_out:
        write_report(
            (ROOT / args.report_out).resolve(),
            rows,
            probe_results,
            Path(args.deck_json),
            Path(args.swap_file) if args.swap_file else None,
        )
    unsupported = [
        row["card"]
        for row in rows
        if row["status"] == "missing_review" and (row["in_base"] or row["swap_add_count"])
    ]
    summary = {
        "rows": len(rows),
        "statuses": dict(sorted(Counter(row["status"] for row in rows).items())),
        "failed_probes": [probe["card"] for probe in probe_results if not probe["ok"]],
        "unsupported": unsupported,
        "csv": args.csv_out,
        "report": args.report_out,
    }
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 1 if summary["failed_probes"] or (args.fail_on_unsupported and unsupported) else 0


if __name__ == "__main__":
    raise SystemExit(main())
