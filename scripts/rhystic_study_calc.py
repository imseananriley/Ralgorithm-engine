#!/usr/bin/env python3
from __future__ import annotations

import argparse
import atexit
from collections import Counter, deque
from functools import lru_cache
import hashlib
from itertools import combinations
import json
import math
import os
import random
import subprocess
import sys
from pathlib import Path
from typing import Iterable, NamedTuple


TARGET = "Rhystic Study"
ENGINE_GOAL = "engine"
MANA_CAP = 10
ZERO_MANA = (0, 0, 0, 0, 0, 0)
COLORS = "BRUWG"
RHYSTIC_COST = (2, 0, 0, 1, 0, 0)  # generic, B, R, U, W, G
DRAW_ORDER_MATTERS_CARDS = frozenset(
    {"Gitaxian Probe", "Manamorphose", "Street Wraith", "Tataru Taru", "Wheel of Fortune"}
)
ROOT = Path(__file__).resolve().parents[1]


DECKS: dict[str, dict[str, list[str]]] = {
    "fury": {
        "commanders": ["Nick Fury, Agent of S.H.I.E.L.D."],
        "mainboard": [
            "Birds of Paradise",
            "Deathrite Shaman",
            "Elvish Spirit Guide",
            "Esper Sentinel",
            "Faerie Mastermind",
            "Heartwood Storyteller",
            "Lotho, Corrupt Shirriff",
            "Orcish Bowmasters",
            "Ragavan, Nimble Pilferer",
            "Ranger-Captain of Eos",
            "Simian Spirit Guide",
            "Subtlety",
            "Tataru Taru",
            "The Cabbage Merchant",
            "Tinder Wall",
            "Valley Floodcaller",
            "Beseech the Mirror",
            "Demonic Tutor",
            "Diabolic Intent",
            "Eldritch Evolution",
            "Flash Photography",
            "Green Sun's Zenith",
            "Imperial Seal",
            "Infernal Plunge",
            "Molten Disaster",
            "Neoform",
            "Rite of Flame",
            "Scheming Symmetry",
            "Angel's Grace",
            "An Offer You Can't Refuse",
            "Borne Upon a Wind",
            "Chain of Vapor",
            "Commandeer",
            "Crop Rotation",
            "Culling the Weak",
            "Dark Ritual",
            "Deflecting Swat",
            "Disrupting Shoal",
            "Enlightened Tutor",
            "Fierce Guardianship",
            "Firestorm",
            "Flusterstorm",
            "Force of Negation",
            "Force of Will",
            "Into the Flood Maw",
            "Manamorphose",
            "Mental Misstep",
            "Mindbreak Trap",
            "Misdirection",
            "Orim's Chant",
            "Pact of Negation",
            "Pyroblast",
            "Silence",
            "Snapback",
            "Sudden Substitution",
            "Summoner's Pact",
            "Swan Song",
            "Vampiric Tutor",
            "Chrome Mox",
            "Lotus Petal",
            "Mana Vault",
            "Mox Amber",
            "Mox Diamond",
            "Sol Ring",
            "Wishclaw Talisman",
            "Copy Enchantment",
            "Mirrormade",
            "Mystic Remora",
            "Nature's Chosen",
            "Necropotence",
            "Rhystic Study",
            "Smothering Tithe",
            "Ancient Tomb",
            "Bayou",
            "Boseiju, Who Endures",
            "City of Brass",
            "City of Traitors",
            "Command Tower",
            "Crystal Vein",
            "Emergence Zone",
            "Exotic Orchard",
            "Flooded Strand",
            "Forbidden Orchard",
            "Gemstone Caverns",
            "Gemstone Mine",
            "Glimmervoid",
            "Mana Confluence",
            "Marsh Flats",
            "Misty Rainforest",
            "Polluted Delta",
            "Scalding Tarn",
            "Scrubland",
            "Starting Town",
            "Tarnished Citadel",
            "Tropical Island",
            "Tundra",
            "Underground Sea",
            "Verdant Catacombs",
            "Volcanic Island",
            "Wooded Foothills",
        ],
    },
    "trigger_farm": {
        "commanders": ["Ishai, Ojutai Dragonspeaker", "Rograkh, Son of Rohgahh"],
        "mainboard": [
            "Birgi, God of Storytelling",
            "Clever Impersonator",
            "Esper Sentinel",
            "Faerie Mastermind",
            "Flesh Duplicate",
            "Hullbreaker Horror",
            "Mockingbird",
            "Phyrexian Metamorph",
            "Ragavan, Nimble Pilferer",
            "Ranger-Captain of Eos",
            "Simian Spirit Guide",
            "Storm-Kiln Artist",
            "Subtlety",
            "Tataru Taru",
            "Valley Floodcaller",
            "Wan Shi Tong, Librarian",
            "Flash Photography",
            "Gamble",
            "Idyllic Tutor",
            "Infernal Plunge",
            "Jeska's Will",
            "Rite of Flame",
            "Sevinne's Reclamation",
            "An Offer You Can't Refuse",
            "Borne Upon a Wind",
            "Brain Freeze",
            "Chain of Vapor",
            "Commandeer",
            "Deflecting Swat",
            "Dispel",
            "Disrupting Shoal",
            "Enlightened Tutor",
            "Fierce Guardianship",
            "Firestorm",
            "Flare of Duplication",
            "Flusterstorm",
            "Force of Negation",
            "Force of Will",
            "Gifts Ungiven",
            "Into the Flood Maw",
            "Intuition",
            "Mental Misstep",
            "Mindbreak Trap",
            "Misdirection",
            "Mystical Tutor",
            "Orim's Chant",
            "Pact of Negation",
            "Pyroblast",
            "Red Elemental Blast",
            "Redirect Lightning",
            "Silence",
            "Sink into Stupor",
            "Snapback",
            "Swan Song",
            "Apple of Eden, Isu Relic",
            "Arcane Signet",
            "Chaos Emerald",
            "Chrome Mox",
            "Lion's Eye Diamond",
            "Mana Vault",
            "Mox Amber",
            "Mox Diamond",
            "Mox Opal",
            "Paradise Mantle",
            "Relic of Legends",
            "Sol Ring",
            "Springleaf Drum",
            "Copy Enchantment",
            "Curse of Opulence",
            "Mirrormade",
            "Mystic Remora",
            "Rhystic Study",
            "Smothering Tithe",
            "Underworld Breach",
            "Ancient Tomb",
            "Arid Mesa",
            "Bloodstained Mire",
            "City of Brass",
            "City of Traitors",
            "Command Tower",
            "Flooded Strand",
            "Glittering Caves of Aglarond",
            "Hallowed Fountain",
            "Mana Confluence",
            "Marsh Flats",
            "Misty Rainforest",
            "Otawara, Soaring City",
            "Plateau",
            "Polluted Delta",
            "Scalding Tarn",
            "Sea of Clouds",
            "Starting Town",
            "Steam Vents",
            "Tarnished Citadel",
            "Tundra",
            "Volcanic Island",
            "Windswept Heath",
            "Wooded Foothills",
        ],
    },
}


FETCH_TYPES = {
    "Arid Mesa": {"Plains", "Mountain"},
    "Bloodstained Mire": {"Swamp", "Mountain"},
    "Flooded Strand": {"Plains", "Island"},
    "Marsh Flats": {"Plains", "Swamp"},
    "Misty Rainforest": {"Forest", "Island"},
    "Polluted Delta": {"Island", "Swamp"},
    "Scalding Tarn": {"Island", "Mountain"},
    "Verdant Catacombs": {"Swamp", "Forest"},
    "Windswept Heath": {"Forest", "Plains"},
    "Wooded Foothills": {"Mountain", "Forest"},
}

LAND_TYPES = {
    "Badlands": {"Swamp", "Mountain"},
    "Bayou": {"Swamp", "Forest"},
    "Blood Crypt": {"Swamp", "Mountain"},
    "Hallowed Fountain": {"Plains", "Island"},
    "Plateau": {"Plains", "Mountain"},
    "Savannah": {"Forest", "Plains"},
    "Scrubland": {"Plains", "Swamp"},
    "Steam Vents": {"Island", "Mountain"},
    "Taiga": {"Mountain", "Forest"},
    "Tropical Island": {"Forest", "Island"},
    "Tundra": {"Plains", "Island"},
    "Underground Sea": {"Island", "Swamp"},
    "Volcanic Island": {"Island", "Mountain"},
    "Watery Grave": {"Island", "Swamp"},
}
LAND_COLOR_BY_TYPE = {"Swamp": "B", "Mountain": "R", "Island": "U", "Plains": "W", "Forest": "G"}


def is_theoretical_rainbow_land(card: str) -> bool:
    return card.startswith("Theoretical Rainbow Land ")

LANDS = {
    "Ancient Tomb",
    "Arid Mesa",
    "Badlands",
    "Bayou",
    "Boseiju, Who Endures",
    "Bloodstained Mire",
    "City of Brass",
    "City of Traitors",
    "Command Tower",
    "Crystal Vein",
    "Emergence Zone",
    "Exotic Orchard",
    "Flooded Strand",
    "Forbidden Orchard",
    "Gemstone Caverns",
    "Gemstone Mine",
    "Glimmervoid",
    "Glittering Caves of Aglarond",
    "Hallowed Fountain",
    "Mana Confluence",
    "Marsh Flats",
    "Misty Rainforest",
    "Otawara, Soaring City",
    "Plateau",
    "Polluted Delta",
    "Phyrexian Tower",
    "Savannah",
    "Scalding Tarn",
    "Scrubland",
    "Sea of Clouds",
    "Starting Town",
    "Steam Vents",
    "Taiga",
    "Tarnished Citadel",
    "Tropical Island",
    "Tundra",
    "Underground Sea",
    "Verdant Catacombs",
    "Volcanic Island",
    "Windswept Heath",
    "Wooded Foothills",
}

MDFC_LANDS = {"Sink into Stupor": "U", "Sink into Stupor // Soporific Springs": "U"}
UNTAPPED_MDFC_LANDS = {"Sink into Stupor", "Sink into Stupor // Soporific Springs"}
ANY_COLOR_LANDS = {
    "City of Brass",
    "Command Tower",
    "Exotic Orchard",
    "Forbidden Orchard",
    "Gemstone Mine",
    "Glimmervoid",
    "Mana Confluence",
    "Starting Town",
    "Tarnished Citadel",
}
COLOR_LANDS = {
    "Boseiju, Who Endures": "G",
    "Otawara, Soaring City": "U",
    "Sea of Clouds": "UW",
}
GEMSTONE_CAVERNS_NAMES = {"Gemstone Caverns", "Glittering Caves of Aglarond"}
COLORLESS_LANDS = {"Emergence Zone", "Gemstone Caverns", "Glittering Caves of Aglarond"}
CC_LANDS = {"Ancient Tomb", "City of Traitors"}

ARTIFACTS = {
    "Apple of Eden, Isu Relic",
    "Arcane Signet",
    "Chaos Emerald",
    "Chrome Mox",
    "Lion's Eye Diamond",
    "Lotus Petal",
    "Mana Vault",
    "Mox Amber",
    "Mox Diamond",
    "Mox Opal",
    "Paradise Mantle",
    "Relic of Legends",
    "Sol Ring",
    "Springleaf Drum",
    "Wishclaw Talisman",
}

CARD_COLORS = {
    TARGET: "U",
    "Angel's Grace": "W",
    "An Offer You Can't Refuse": "U",
    "Beseech the Mirror": "B",
    "Birgi, God of Storytelling": "R",
    "Birds of Paradise": "G",
    "Borne Upon a Wind": "U",
    "Brain Freeze": "U",
    "Chain of Vapor": "U",
    "Clever Impersonator": "U",
    "Commandeer": "U",
    "Copy Enchantment": "U",
    "Crop Rotation": "G",
    "Culling the Weak": "B",
    "Curse of Opulence": "R",
    "Dark Ritual": "B",
    "Deathrite Shaman": "BG",
    "Deflecting Swat": "R",
    "Demonic Tutor": "B",
    "Diabolic Intent": "B",
    "Dispel": "U",
    "Disrupting Shoal": "U",
    "Eldritch Evolution": "G",
    "Elvish Spirit Guide": "G",
    "Enlightened Tutor": "W",
    "Esper Sentinel": "W",
    "Faerie Mastermind": "U",
    "Fierce Guardianship": "U",
    "Firestorm": "R",
    "Flash Photography": "U",
    "Flesh Duplicate": "U",
    "Flusterstorm": "U",
    "Force of Negation": "U",
    "Force of Will": "U",
    "Flashback": "R",
    "Gamble": "R",
    "Gifts Ungiven": "U",
    "Gitaxian Probe": "U",
    "Green Sun's Zenith": "G",
    "Grim Tutor": "B",
    "Heartwood Storyteller": "G",
    "Hullbreaker Horror": "U",
    "Idyllic Tutor": "W",
    "Imperial Seal": "B",
    "Infernal Plunge": "R",
    "Ignoble Hierarch": "G",
    "Intuition": "U",
    "Into the Flood Maw": "U",
    "Ishai, Ojutai Dragonspeaker": "UW",
    "Jeska's Will": "R",
    "Lotho, Corrupt Shirriff": "BW",
    "Manamorphose": "RG",
    "Mental Misstep": "U",
    "Mindbreak Trap": "U",
    "Mirrormade": "U",
    "Misdirection": "U",
    "Mockingbird": "U",
    "Molten Disaster": "R",
    "Mystic Remora": "U",
    "Mystical Tutor": "U",
    "Nature's Chosen": "G",
    "Necropotence": "B",
    "Neoform": "UG",
    "Nick Fury, Agent of S.H.I.E.L.D.": "W",
    "Noble Hierarch": "G",
    "Noxious Revival": "G",
    "Orcish Bowmasters": "B",
    "Orim's Chant": "W",
    "Pact of Negation": "U",
    "Phyrexian Metamorph": "U",
    "Pyroblast": "R",
    "Ragavan, Nimble Pilferer": "R",
    "Rain of Filth": "B",
    "Ranger-Captain of Eos": "W",
    "Red Elemental Blast": "R",
    "Redirect Lightning": "R",
    "Rite of Flame": "R",
    "Rograkh, Son of Rohgahh": "R",
    "Scheming Symmetry": "B",
    "Sevinne's Reclamation": "W",
    "Silence": "W",
    "Simian Spirit Guide": "R",
    "Sink into Stupor": "U",
    "Sink into Stupor // Soporific Springs": "U",
    "Smothering Tithe": "W",
    "Snapback": "U",
    "Storm-Kiln Artist": "R",
    "Street Wraith": "B",
    "Strike It Rich": "R",
    "Subtlety": "U",
    "Sudden Substitution": "U",
    "Summoner's Pact": "G",
    "Swan Song": "U",
    "Tataru Taru": "W",
    "The Cabbage Merchant": "G",
    "Tinder Wall": "G",
    "Underworld Breach": "R",
    "Valley Floodcaller": "U",
    "Vampiric Tutor": "B",
    "Wan Shi Tong, Librarian": "U",
    "Wild Cantor": "RG",
    "Worldly Tutor": "G",
}

CREATURE_COSTS = {
    "Birds of Paradise": (0, 0, 0, 0, 0, 1),
    "Deathrite Shaman": None,
    "Esper Sentinel": (0, 0, 0, 0, 1, 0),
    "Faerie Mastermind": (1, 0, 0, 1, 0, 0),
    "Heartwood Storyteller": (1, 0, 0, 0, 0, 2),
    "Ignoble Hierarch": (0, 0, 0, 0, 0, 1),
    "Lotho, Corrupt Shirriff": (0, 1, 0, 0, 1, 0),
    "Noble Hierarch": (0, 0, 0, 0, 0, 1),
    "Orcish Bowmasters": (1, 1, 0, 0, 0, 0),
    "Ragavan, Nimble Pilferer": (0, 0, 1, 0, 0, 0),
    "Ranger-Captain of Eos": (1, 0, 0, 0, 2, 0),
    "Tataru Taru": (1, 0, 0, 0, 1, 0),
    "The Cabbage Merchant": (2, 0, 0, 0, 0, 1),
    "Tinder Wall": (0, 0, 0, 0, 0, 1),
    "Wild Cantor": None,
    "Birgi, God of Storytelling": (2, 0, 1, 0, 0, 0),
    "Clever Impersonator": (2, 0, 0, 2, 0, 0),
    "Flesh Duplicate": (1, 0, 0, 1, 0, 0),
    "Mockingbird": (1, 0, 0, 1, 0, 0),
    "Phyrexian Metamorph": (3, 0, 0, 1, 0, 0),
    "Storm-Kiln Artist": (3, 0, 1, 0, 0, 0),
    "Valley Floodcaller": (2, 0, 0, 1, 0, 0),
    "Wan Shi Tong, Librarian": (0, 0, 0, 2, 0, 0),
}

EARLY_CREATURES = {
    "Birds of Paradise",
    "Deathrite Shaman",
    "Esper Sentinel",
    "Ignoble Hierarch",
    "Lotho, Corrupt Shirriff",
    "Noble Hierarch",
    "Ragavan, Nimble Pilferer",
    "Tataru Taru",
    "Tinder Wall",
    "Wild Cantor",
    "Birgi, God of Storytelling",
}
EARLY_CREATURE_SPECIAL_COSTS = {
    "Deathrite Shaman": ((0, 1, 0, 0, 0, 0), (0, 0, 0, 0, 0, 1)),
    "Wild Cantor": ((0, 0, 1, 0, 0, 0), (0, 0, 0, 0, 0, 1)),
    "Wan Shi Tong, Librarian": ((0, 0, 0, 2, 0, 0),),
}
EARLY_CREATURE_CAST_OPTIONS = tuple(
    (card, EARLY_CREATURE_SPECIAL_COSTS.get(card, (cost,)))
    for card, cost in CREATURE_COSTS.items()
    if card in EARLY_CREATURES
)

EARLY_CROP_TARGETS = {
    "Ancient Tomb",
    "City of Traitors",
    "Crystal Vein",
    "City of Brass",
    "Command Tower",
    "Mana Confluence",
    "Phyrexian Tower",
    "Tropical Island",
    "Tundra",
    "Underground Sea",
    "Volcanic Island",
}
EARLY_CROP_TARGET_ORDER = tuple(sorted(EARLY_CROP_TARGETS))

COMMANDER_COSTS = {
    "Nick Fury, Agent of S.H.I.E.L.D.": ("NICK", (0, 0, 0, 0, 1, 0), "W"),
    "Rograkh, Son of Rohgahh": ("ROG", (0, 0, 0, 0, 0, 0), "R"),
    "Ishai, Ojutai Dragonspeaker": ("ISHAI", (2, 0, 0, 1, 1, 0), "UW"),
}

HAND_TUTORS = {
    "Demonic Tutor": (1, 1, 0, 0, 0, 0),
    "Idyllic Tutor": (2, 0, 0, 0, 1, 0),
    "Diabolic Intent": (1, 1, 0, 0, 0, 0),
    "Grim Tutor": (1, 2, 0, 0, 0, 0),
}
TOP_TUTORS = {
    "Enlightened Tutor": (0, 0, 0, 0, 1, 0),
    "Imperial Seal": (0, 1, 0, 0, 0, 0),
    "Mystical Tutor": (0, 0, 0, 1, 0, 0),
    "Scheming Symmetry": (0, 1, 0, 0, 0, 0),
    "Vampiric Tutor": (0, 1, 0, 0, 0, 0),
    "Worldly Tutor": (0, 0, 0, 0, 0, 1),
}
MYSTICAL_TUTOR_TARGETS = {
    "An Offer You Can't Refuse",
    "Beseech the Mirror",
    "Crop Rotation",
    "Culling the Weak",
    "Dark Ritual",
    "Demonic Tutor",
    "Diabolic Intent",
    "Eldritch Evolution",
    "Enlightened Tutor",
    "Gitaxian Probe",
    "Street Wraith",
    "Green Sun's Zenith",
    "Grim Tutor",
    "Idyllic Tutor",
    "Imperial Seal",
    "Infernal Plunge",
    "Jeska's Will",
    "Manamorphose",
    "Neoform",
    "Rain of Filth",
    "Rite of Flame",
    "Scheming Symmetry",
    "Strike It Rich",
    "Summoner's Pact",
    "Vampiric Tutor",
    "Worldly Tutor",
}
WORLDLY_TUTOR_TARGETS = {
    "Birds of Paradise",
    "Deathrite Shaman",
    "Heartwood Storyteller",
    "Ignoble Hierarch",
    "Noble Hierarch",
    "Tinder Wall",
    "Wild Cantor",
}
DRAW_ONE = {"Gitaxian Probe", "Street Wraith"}

OFFER_COST = (0, 0, 0, 1, 0, 0)
OFFER_COUNTERABLE_COSTS = {
    "Summoner's Pact": ((0, 0, 0, 0, 0, 0),),
    "Gitaxian Probe": ((0, 0, 0, 0, 0, 0),),
    "Lotus Petal": ((0, 0, 0, 0, 0, 0),),
    "Chaos Emerald": ((0, 0, 0, 0, 0, 0),),
    "Chrome Mox": ((0, 0, 0, 0, 0, 0),),
    "Lion's Eye Diamond": ((0, 0, 0, 0, 0, 0),),
    "Mox Amber": ((0, 0, 0, 0, 0, 0),),
    "Mox Diamond": ((0, 0, 0, 0, 0, 0),),
    "Mox Opal": ((0, 0, 0, 0, 0, 0),),
    "Paradise Mantle": ((0, 0, 0, 0, 0, 0),),
    "Sol Ring": ((1, 0, 0, 0, 0, 0),),
    "Mana Vault": ((1, 0, 0, 0, 0, 0),),
    "Springleaf Drum": ((1, 0, 0, 0, 0, 0),),
    "Arcane Signet": ((2, 0, 0, 0, 0, 0),),
    "Wishclaw Talisman": ((1, 1, 0, 0, 0, 0),),
    "Relic of Legends": ((3, 0, 0, 0, 0, 0),),
    "Dark Ritual": ((0, 1, 0, 0, 0, 0),),
    "Rite of Flame": ((0, 0, 1, 0, 0, 0),),
    "Manamorphose": ((1, 0, 1, 0, 0, 0), (1, 0, 0, 0, 0, 1)),
    "Demonic Tutor": ((1, 1, 0, 0, 0, 0),),
    "Grim Tutor": ((1, 2, 0, 0, 0, 0),),
    "Idyllic Tutor": ((2, 0, 0, 0, 1, 0),),
    "Enlightened Tutor": ((0, 0, 0, 0, 1, 0),),
    "Imperial Seal": ((0, 1, 0, 0, 0, 0),),
    "Mystical Tutor": ((0, 0, 0, 1, 0, 0),),
    "Scheming Symmetry": ((0, 1, 0, 0, 0, 0),),
    "Vampiric Tutor": ((0, 1, 0, 0, 0, 0),),
    "Worldly Tutor": ((0, 0, 0, 0, 0, 1),),
    "Beseech the Mirror": ((1, 3, 0, 0, 0, 0),),
    "Green Sun's Zenith": ((0, 0, 0, 0, 0, 1),),
    TARGET: (RHYSTIC_COST,),
    "Mystic Remora": ((0, 0, 0, 1, 0, 0),),
    "Nature's Chosen": ((0, 0, 0, 0, 0, 1),),
    "Noxious Revival": ((0, 0, 0, 0, 0, 0),),
    "Copy Enchantment": ((2, 0, 0, 1, 0, 0),),
    "Mirrormade": ((1, 0, 0, 2, 0, 0),),
    "Flash Photography": ((2, 0, 0, 2, 0, 0),),
    "Necropotence": ((0, 3, 0, 0, 0, 0),),
    "Smothering Tithe": ((3, 0, 0, 0, 1, 0),),
    "Strike It Rich": ((0, 0, 1, 0, 0, 0),),
}
OFFER_ENGINE_BAIT_EXCLUSIONS = {
    TARGET,
    "Mystic Remora",
    "Esper Sentinel",
    "Heartwood Storyteller",
    "Copy Enchantment",
    "Mirrormade",
    "Flash Photography",
    "Clever Impersonator",
    "Necropotence",
    "Smothering Tithe",
}


class Perm(NamedTuple):
    name: str
    tapped: bool = False
    extra: str = ""


class State(NamedTuple):
    hand: tuple[str, ...]
    library: tuple[str, ...]
    battlefield: tuple[Perm, ...]
    mana: tuple[int, int, int, int, int, int] = ZERO_MANA
    land_played: bool = False
    land_grave_count: int = 0
    mantle_attached: tuple[str, str] = ()
    nature_attached: tuple[str, str] = ()
    nature_untap_used: bool = False
    nature_tap_used: bool = False
    rain_active: bool = False
    spells_this_turn: int = 0
    pact_debt: int = 0
    turn: int = 0
    engine_count: int = 0
    engine_targets: tuple[str, ...] = ()
    engine_names: tuple[str, ...] = ()


def perm_to_json(perm: Perm) -> dict[str, object]:
    return {
        "name": perm.name,
        "tapped": perm.tapped,
        "extra": perm.extra,
    }


def state_to_json(state: State) -> dict[str, object]:
    return {
        "hand": list(state.hand),
        "library": list(state.library),
        "battlefield": [perm_to_json(perm) for perm in state.battlefield],
        "mana": list(state.mana),
        "land_played": state.land_played,
        "land_grave_count": state.land_grave_count,
        "mantle_attached": list(state.mantle_attached),
        "nature_attached": list(state.nature_attached),
        "nature_untap_used": state.nature_untap_used,
        "nature_tap_used": state.nature_tap_used,
        "rain_active": state.rain_active,
        "spells_this_turn": state.spells_this_turn,
        "pact_debt": state.pact_debt,
        "turn": state.turn,
        "engine_count": state.engine_count,
        "engine_targets": list(state.engine_targets),
        "engine_names": list(state.engine_names),
    }


def state_from_json(payload: dict[str, object]) -> State:
    battlefield = tuple(
        Perm(str(item.get("name", "")), bool(item.get("tapped", False)), str(item.get("extra", "")))
        for item in payload.get("battlefield", [])
    )
    return State(
        hand=norm(str(card) for card in payload.get("hand", [])),
        library=tuple(str(card) for card in payload.get("library", [])),
        battlefield=norm_battlefield(battlefield),
        mana=cap_mana(tuple(int(value) for value in payload.get("mana", ZERO_MANA))),
        land_played=bool(payload.get("land_played", False)),
        land_grave_count=max(0, min(4, int(payload.get("land_grave_count", 0)))),
        mantle_attached=tuple(str(value) for value in payload.get("mantle_attached", [])),
        nature_attached=tuple(str(value) for value in payload.get("nature_attached", [])),
        nature_untap_used=bool(payload.get("nature_untap_used", False)),
        nature_tap_used=bool(payload.get("nature_tap_used", False)),
        rain_active=bool(payload.get("rain_active", False)),
        spells_this_turn=min(5, int(payload.get("spells_this_turn", 0))),
        pact_debt=max(0, min(2, int(payload.get("pact_debt", 0)))),
        turn=max(0, min(8, int(payload.get("turn", 0)))),
        engine_count=max(0, min(3, int(payload.get("engine_count", 0)))),
        engine_targets=tuple(sorted(set(str(value) for value in payload.get("engine_targets", [])))),
        engine_names=tuple(sorted(set(str(value) for value in payload.get("engine_names", [])))),
    )


def canonical_tuple(items: Iterable):
    values = items if isinstance(items, tuple) else tuple(items)
    for index in range(1, len(values)):
        if values[index - 1] > values[index]:
            return tuple(sorted(values))
    return values


def norm(cards: Iterable[str]) -> tuple[str, ...]:
    values = cards if isinstance(cards, tuple) else tuple(cards)
    for index in range(1, len(values)):
        if values[index - 1] > values[index]:
            return tuple(sorted(values))
    return values


def norm_battlefield(perms: Iterable[Perm]) -> tuple[Perm, ...]:
    values = perms if isinstance(perms, tuple) else tuple(perms)
    for index in range(1, len(values)):
        if values[index - 1] > values[index]:
            return tuple(sorted(values))
    return values


def remove_card(cards: tuple[str, ...], card: str) -> tuple[str, ...]:
    items = list(cards)
    items.remove(card)
    return tuple(items)


UNKNOWN_SHUFFLE_DRAW_BARRIER = 20


def strict_shuffle_hidden_enabled() -> bool:
    return os.environ.get("RHYSTIC_STRICT_SHUFFLE_HIDDEN", "").lower() in {"1", "true", "yes", "on"}


def obscure_library_top_after_shuffle(library: tuple[str, ...]) -> tuple[str, ...]:
    if not strict_shuffle_hidden_enabled():
        return library
    if not library:
        return library
    leading_blanks = 0
    for card in library:
        if card != "Blank":
            break
        leading_blanks += 1
    if leading_blanks >= UNKNOWN_SHUFFLE_DRAW_BARRIER:
        return library
    return ("Blank",) * (UNKNOWN_SHUFFLE_DRAW_BARRIER - leading_blanks) + library


def known_top_library_after_shuffle(target: str, library: tuple[str, ...]) -> tuple[str, ...]:
    return (target, *obscure_library_top_after_shuffle(library))


@lru_cache(maxsize=32768)
def canonical_library(library: tuple[str, ...]) -> tuple[str, ...]:
    return tuple(sorted(library))


@lru_cache(maxsize=32768)
def library_order_matters_for_hand(hand: tuple[str, ...]) -> bool:
    return any(card in hand for card in DRAW_ORDER_MATTERS_CARDS)


@lru_cache(maxsize=65536)
def action_priority(goal: str, action: str) -> int:
    if action.startswith(("tap ", "sac ", "exile ")):
        return 1000
    if TARGET in action or any(x in action for x in ("Tutor", "Wishclaw", "Beseech", "Gamble")):
        return 900
    if goal == ENGINE_GOAL and any(
        x in action
        for x in (
            "Heartwood",
            "Mystic Remora",
            "Smothering Tithe",
            "Esper Sentinel",
            "Copy Enchantment",
            "Mirrormade",
            "Flash Photography",
            "Clever Impersonator",
            "Green Sun's Zenith",
            "Eldritch Evolution",
            "Summoner's Pact",
            "Neoform",
            "Ranger-Captain",
        )
    ):
        return 900
    if any(x in action for x in ("Lotus", "Mox", "Sol Ring", "Mana Vault", "Chrome", "Diamond", "Opal")):
        return 850
    if action.startswith("play "):
        return 800
    return 100


def default_rust_action_bin() -> Path:
    workspace_bin = ROOT / "target" / "release" / "rhystic-core-smoke"
    if workspace_bin.exists():
        return workspace_bin
    return ROOT / "rust" / "rhystic_core" / "target" / "release" / "rhystic-core-smoke"


class RustActionAccelerator:
    def __init__(self, bin_path: str | None = None, *, fast: bool = False, command: str | None = None):
        self.bin_path = Path(bin_path or os.environ.get("RHYSTIC_RUST_ACTION_BIN") or default_rust_action_bin())
        self.fast = fast
        if not self.bin_path.exists():
            raise FileNotFoundError(f"Rust action binary not found: {self.bin_path}")
        command = command or ("expand-actions-fast-jsonl" if fast else "expand-actions-jsonl")
        self.command = command
        self.process = subprocess.Popen(
            [str(self.bin_path), command],
            cwd=ROOT,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        atexit.register(self.close)

    def close(self) -> None:
        process = getattr(self, "process", None)
        if process is None or process.poll() is not None:
            return
        process.terminate()
        try:
            process.wait(timeout=1)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=1)
        self.process = None

    def actions(self, state: State) -> list[tuple[State, str]]:
        payload = self._send(state_to_json(state), "action accelerator")
        return [(state_from_json(item["next_state"]), item["label"]) for item in payload]

    def close_turn(self, states: Iterable[State], search: "RhysticSearch") -> tuple[list[State], bool, bool, str | None]:
        payload = self._send(
            {
                "states": [state_to_json(state) for state in states],
                "state_limit": search.state_limit,
                "max_turns": search.max_turns,
                "goal": search.goal,
                "engine_target_count": search.engine_target_count,
                "engine_success_policy": search.engine_success_policy,
                "remora_upkeep_payments": search.remora_upkeep_payments,
                "action_sort": search.action_sort,
            },
            "close-turn accelerator",
        )
        return (
            [state_from_json(item) for item in payload["closed"]],
            bool(payload["success"]),
            bool(payload["hit_limit"]),
            payload.get("label"),
        )

    def solve_keep(
        self,
        hand: list[str],
        library: list[str],
        *,
        gemstone_live: bool,
        search: "RhysticSearch",
        gamble_seed: int | None = None,
    ) -> tuple[int | None, bool, str | None, bool, str | None]:
        payload = self._send(
            {
                "hand": list(hand),
                "library": list(library),
                "gemstone_live": gemstone_live,
                "state_limit": search.state_limit,
                "max_turns": search.max_turns,
                "goal": search.goal,
                "engine_target_count": search.engine_target_count,
                "engine_success_policy": search.engine_success_policy,
                "remora_upkeep_payments": search.remora_upkeep_payments,
                "action_sort": search.action_sort,
                "gamble_mode": search.gamble_mode,
                "gamble_seed": search.gamble_seed if gamble_seed is None else gamble_seed,
                "simplified_gamble": search.simplified_gamble,
            },
            "solve-keep accelerator",
        )
        return (
            payload.get("turn"),
            bool(payload["capped"]),
            payload.get("label"),
            bool(payload.get("unsupported")),
            payload.get("unsupported_reason"),
        )

    def solve_keep_batch(
        self,
        requests: list[dict[str, object]],
    ) -> list[tuple[int | None, bool, str | None, bool, str | None]]:
        payload = self._send(requests, "solve-keep batch accelerator")
        if not isinstance(payload, list):
            raise RuntimeError(f"Rust solve-keep batch accelerator returned non-list payload: {payload!r}")
        return [
            (
                item.get("turn"),
                bool(item["capped"]),
                item.get("label"),
                bool(item.get("unsupported")),
                item.get("unsupported_reason"),
            )
            for item in payload
        ]

    def earliest(
        self,
        deck_order: list[str],
        bottom_count: int,
        *,
        gemstone_live: bool,
        search: "RhysticSearch",
    ) -> tuple[int | None, bool, str | None, bool, str | None]:
        payload = self._send(
            {
                "deck_order": list(deck_order),
                "bottom_count": bottom_count,
                "gemstone_live": gemstone_live,
                "state_limit": search.state_limit,
                "max_turns": search.max_turns,
                "goal": search.goal,
                "engine_target_count": search.engine_target_count,
                "engine_success_policy": search.engine_success_policy,
                "remora_upkeep_payments": search.remora_upkeep_payments,
                "action_sort": search.action_sort,
                "gamble_mode": search.gamble_mode,
                "gamble_seed": search.gamble_seed,
                "simplified_gamble": search.simplified_gamble,
            },
            "earliest accelerator",
        )
        return (
            payload.get("turn"),
            bool(payload["capped"]),
            payload.get("label"),
            bool(payload.get("unsupported")),
            payload.get("unsupported_reason"),
        )

    def visible_hand_batch(self, request: dict[str, object]) -> list[dict[str, object]]:
        payload = self._send(request, "visible-hand batch accelerator")
        if not isinstance(payload, list):
            raise RuntimeError(f"Rust visible-hand batch accelerator returned non-list payload: {payload!r}")
        return payload

    def policy_sim(self, request: dict[str, object]) -> dict[str, object]:
        payload = self._send(request, "policy sim accelerator")
        if not isinstance(payload, dict):
            raise RuntimeError(f"Rust policy sim accelerator returned non-dict payload: {payload!r}")
        return payload

    def policy_eval(self, request: dict[str, object]) -> dict[str, object]:
        payload = self._send(request, "policy eval accelerator")
        if not isinstance(payload, dict):
            raise RuntimeError(f"Rust policy eval accelerator returned non-dict payload: {payload!r}")
        return payload

    def _send(self, payload: object, purpose: str) -> object:
        if self.process is None:
            raise RuntimeError(f"Rust {purpose} is closed")
        if self.process.poll() is not None:
            stderr = self.process.stderr.read() if self.process.stderr is not None else ""
            raise RuntimeError(f"Rust {purpose} exited with code {self.process.returncode}: {stderr}")
        if self.process.stdin is None or self.process.stdout is None:
            raise RuntimeError(f"Rust {purpose} pipes are not available")
        self.process.stdin.write(json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            stderr = self.process.stderr.read() if self.process.stderr is not None else ""
            raise RuntimeError(f"Rust {purpose} produced no output: {stderr}")
        response = json.loads(line)
        if isinstance(response, dict) and response.get("error"):
            raise RuntimeError(f"Rust {purpose} error: {response['error']}")
        return response


def action_multiset(actions: Iterable[tuple[State, str]]) -> Counter:
    return Counter((next_state, label) for next_state, label in actions)


def engine_target_priority(target: str) -> tuple[int, str]:
    priority = {
        TARGET: 0,
        "Rhystic Study": 0,
        "Heartwood Storyteller": 1,
        "Mystic Remora": 2,
        "Smothering Tithe": 3,
        "Esper Sentinel": 4,
        "Copy Enchantment": 5,
        "Mirrormade": 6,
        "Flash Photography": 7,
        "Clever Impersonator": 8,
    }.get(target, 50)
    return (priority, target)


@lru_cache(maxsize=8192)
def add_mana(mana, add):
    return (
        min(MANA_CAP, mana[0] + add[0]),
        min(MANA_CAP, mana[1] + add[1]),
        min(MANA_CAP, mana[2] + add[2]),
        min(MANA_CAP, mana[3] + add[3]),
        min(MANA_CAP, mana[4] + add[4]),
        min(MANA_CAP, mana[5] + add[5]),
    )


def cap_mana(mana):
    if (
        isinstance(mana, tuple)
        and mana[0] <= MANA_CAP
        and mana[1] <= MANA_CAP
        and mana[2] <= MANA_CAP
        and mana[3] <= MANA_CAP
        and mana[4] <= MANA_CAP
        and mana[5] <= MANA_CAP
    ):
        return mana
    return (
        min(MANA_CAP, mana[0]),
        min(MANA_CAP, mana[1]),
        min(MANA_CAP, mana[2]),
        min(MANA_CAP, mana[3]),
        min(MANA_CAP, mana[4]),
        min(MANA_CAP, mana[5]),
    )


@lru_cache(maxsize=None)
def mana_for_color(color: str):
    return {"B": (1, 0, 0, 0, 0, 0), "R": (0, 1, 0, 0, 0, 0), "U": (0, 0, 1, 0, 0, 0), "W": (0, 0, 0, 1, 0, 0), "G": (0, 0, 0, 0, 1, 0)}[color]


@lru_cache(maxsize=None)
def mana_for_option(option: str):
    if option in COLORS:
        return mana_for_color(option)
    if option == "C":
        return (0, 0, 0, 0, 0, 1)
    if option == "CC":
        return (0, 0, 0, 0, 0, 2)
    if option == "CCC":
        return (0, 0, 0, 0, 0, 3)
    raise ValueError(option)


@lru_cache(maxsize=8192)
def pay_options(mana, cost):
    generic, black, red, blue, white, green = cost
    b, r, u, w, g, c = mana
    b -= black
    r -= red
    u -= blue
    w -= white
    g -= green
    if b < 0 or r < 0 or u < 0 or w < 0 or g < 0:
        return ()
    if b + r + u + w + g + c < generic:
        return ()
    if generic == 0:
        return ((b, r, u, w, g, c),)
    out = []
    for xb in range(min(b, generic) + 1):
        rem_b = generic - xb
        for xr in range(min(r, rem_b) + 1):
            rem_r = rem_b - xr
            for xu in range(min(u, rem_r) + 1):
                rem_u = rem_r - xu
                for xw in range(min(w, rem_u) + 1):
                    rem_w = rem_u - xw
                    for xg in range(min(g, rem_w) + 1):
                        rem_g = rem_w - xg
                        if rem_g <= c:
                            out.append((b - xb, r - xr, u - xu, w - xw, g - xg, c - rem_g))
    return tuple(out)


ENGINE_NATIVE = {
    TARGET: (RHYSTIC_COST, "ENCH", Perm("ENGINE_ENCH")),
    "Mystic Remora": ((0, 0, 0, 1, 0, 0), "ENCH", Perm("ENGINE_ENCH")),
    "Esper Sentinel": ((0, 0, 0, 0, 1, 0), "ART", Perm("ESPER", False, "W*")),
    "Heartwood Storyteller": ((1, 0, 0, 0, 0, 2), "CREATURE", Perm("HEARTWOOD", False, "G*")),
}
ENGINE_NATIVE_CREATURES = {"Esper Sentinel", "Heartwood Storyteller"}
ENGINE_COPY_SPELLS = {
    "Copy Enchantment": ((2, 0, 0, 1, 0, 0), ("ENCH",)),
    "Mirrormade": ((1, 0, 0, 2, 0, 0), ("ENCH", "ART")),
    "Flash Photography": ((2, 0, 0, 2, 0, 0), ("ENCH", "ART", "CREATURE")),
    "Clever Impersonator": ((2, 0, 0, 2, 0, 0), ("ENCH", "ART", "CREATURE")),
}
ENCHANTMENT_TUTOR_TARGETS = {TARGET, "Mystic Remora", "Copy Enchantment", "Mirrormade"}
ENLIGHTENED_TARGETS = {TARGET, "Mystic Remora", "Esper Sentinel", "Copy Enchantment", "Mirrormade"}
CREATURE_MV_BY_PERM = {
    "NICK": 1,
    "ROG": 0,
    "ISHAI": 4,
    "BIRD": 1,
    "DEATHRITE": 1,
    "TINDER": 1,
    "TATARU": 2,
    "RAGAVAN": 1,
    "LOTHO": 2,
    "BIRGI": 3,
    "ESPER": 1,
    "HEARTWOOD": 3,
    "WAN": 2,
    "CREATURE": 1,
}
CREATURE_PERM_NAMES = frozenset(
    {
        "NICK",
        "ROG",
        "ISHAI",
        "BIRD",
        "DEATHRITE",
        "TINDER",
        "CANTOR",
        "NOBLE",
        "IGNOBLE",
        "TATARU",
        "RAGAVAN",
        "LOTHO",
        "BIRGI",
        "ESPER",
        "HEARTWOOD",
        "WAN",
        "CREATURE",
    }
)
LEGENDARY_PERM_NAMES = frozenset({"NICK", "ROG", "ISHAI", "TATARU", "RAGAVAN", "LOTHO", "BIRGI", "WAN"})
ARTIFACT_PERM_NAMES = frozenset(
    {
        "ARTIFACT",
        "PETAL",
        "TREASURE",
        "LED",
        "AMBER",
        "OPAL",
        "MANTLE",
        "DIAMOND",
        "CHROME",
        "SOL",
        "VAULT",
        "SIGNET",
        "WISHCLAW",
        "DRUM",
        "RELIC",
        "ESPER",
    }
)
LAND_PERM_NAMES = frozenset({"LAND", "CCLAND", "CITY", "GLIMMER", "CAVERN", "MINE", "TOWER", "VEIN"})


class RhysticSearch:
    def __init__(
        self,
        deck_key: str,
        *,
        max_turns: int,
        state_limit: int,
        optimistic_gamble: bool = False,
        gamble_mode: str = "off",
        goal: str = "rhystic",
        engine_target_count: int = 1,
        engine_success_policy: str = "count",
        remora_upkeep_payments: int = 2,
        action_sort: bool = True,
        rust_action_mode: str | None = None,
        rust_close_mode: str | None = None,
        rust_solver_mode: str | None = None,
        rust_action_bin: str | None = None,
    ):
        if optimistic_gamble and gamble_mode == "off":
            gamble_mode = "optimistic"
        if gamble_mode not in {"off", "optimistic", "stochastic"}:
            raise ValueError(f"Unknown gamble_mode: {gamble_mode}")
        self.deck_key = deck_key
        self.commanders = DECKS[deck_key]["commanders"]
        self.mainboard = DECKS[deck_key]["mainboard"]
        self.max_turns = max_turns
        self.state_limit = state_limit
        self.optimistic_gamble = gamble_mode == "optimistic"
        self.gamble_mode = gamble_mode
        self.gamble_seed = 0
        self.simplified_gamble = os.environ.get("RHYSTIC_SIMPLIFIED_GAMBLE", "").lower() in {"1", "true", "yes", "on"}
        self.goal = goal
        self.engine_target_count = engine_target_count
        self.engine_success_policy = engine_success_policy
        self.remora_upkeep_payments = remora_upkeep_payments
        self.action_sort = action_sort
        self.rust_action_mode = (rust_action_mode or os.environ.get("RHYSTIC_RUST_ACTIONS") or "off").lower()
        if self.rust_action_mode not in {"off", "verify", "require"}:
            raise ValueError(f"Unknown rust_action_mode: {self.rust_action_mode}")
        self.rust_close_mode = (rust_close_mode or os.environ.get("RHYSTIC_RUST_CLOSE") or "off").lower()
        if self.rust_close_mode not in {"off", "verify", "require"}:
            raise ValueError(f"Unknown rust_close_mode: {self.rust_close_mode}")
        self.rust_solver_mode = (rust_solver_mode or os.environ.get("RHYSTIC_RUST_SOLVER") or "off").lower()
        if self.rust_solver_mode not in {"off", "verify", "require"}:
            raise ValueError(f"Unknown rust_solver_mode: {self.rust_solver_mode}")
        self._rust_action_accelerator: RustActionAccelerator | None = None
        if self.rust_action_mode != "off":
            self._rust_action_accelerator = RustActionAccelerator(rust_action_bin, fast=self.rust_action_mode == "require")
        self._rust_close_accelerator: RustActionAccelerator | None = None
        if self.rust_close_mode != "off":
            self._rust_close_accelerator = RustActionAccelerator(
                rust_action_bin,
                command=os.environ.get("RHYSTIC_RUST_CLOSE_COMMAND", "close-turn-jsonl"),
            )
        self._rust_solver_accelerator: RustActionAccelerator | None = None
        if self.rust_solver_mode != "off":
            self._rust_solver_accelerator = RustActionAccelerator(
                rust_action_bin,
                command=os.environ.get("RHYSTIC_RUST_SOLVER_COMMAND", "solve-keep-jsonl"),
            )
        self._rust_solver_batch_accelerator: RustActionAccelerator | None = None
        solver_batch_command = os.environ.get("RHYSTIC_RUST_SOLVER_BATCH_COMMAND")
        if self.rust_solver_mode != "off" and solver_batch_command:
            self._rust_solver_batch_accelerator = RustActionAccelerator(rust_action_bin, command=solver_batch_command)
        self._rust_earliest_accelerator: RustActionAccelerator | None = None
        earliest_command = os.environ.get("RHYSTIC_RUST_EARLIEST_COMMAND")
        if self.rust_solver_mode != "off" and earliest_command:
            self._rust_earliest_accelerator = RustActionAccelerator(rust_action_bin, command=earliest_command)
        self._rust_visible_accelerator: RustActionAccelerator | None = None
        visible_command = os.environ.get("RHYSTIC_RUST_VISIBLE_COMMAND")
        if self.rust_solver_mode != "off" and visible_command:
            self._rust_visible_accelerator = RustActionAccelerator(rust_action_bin, command=visible_command)
        self.identity = "".join(c for c in COLORS if c in set("".join(CARD_COLORS.get(cn, "") for cn in self.commanders))) or COLORS
        self.commander_specs = [(name, *COMMANDER_COSTS[name]) for name in self.commanders]
        self._pact_survival_cache: dict[tuple, bool] = {}
        self._remora_keep_cache: dict[tuple[State, int], bool] = {}

    def close(self) -> None:
        if self._rust_action_accelerator is not None:
            self._rust_action_accelerator.close()
            self._rust_action_accelerator = None
        if self._rust_close_accelerator is not None:
            self._rust_close_accelerator.close()
            self._rust_close_accelerator = None
        if self._rust_solver_accelerator is not None:
            self._rust_solver_accelerator.close()
            self._rust_solver_accelerator = None
        if self._rust_solver_batch_accelerator is not None:
            self._rust_solver_batch_accelerator.close()
            self._rust_solver_batch_accelerator = None
        if self._rust_earliest_accelerator is not None:
            self._rust_earliest_accelerator.close()
            self._rust_earliest_accelerator = None
        if self._rust_visible_accelerator is not None:
            self._rust_visible_accelerator.close()
            self._rust_visible_accelerator = None

    def earliest(self, deck_order: list[str], bottom_count: int, *, gemstone_live: bool = False, gamble_seed: int | None = None) -> tuple[int | None, bool]:
        if gamble_seed is not None:
            self.gamble_seed = gamble_seed
        if self.rust_solver_mode != "off" and self._rust_earliest_accelerator is not None and self._rust_solver_supported():
            rust_turn, rust_capped, _rust_label, unsupported, _reason = self._rust_earliest_accelerator.earliest(
                deck_order,
                bottom_count,
                gemstone_live=gemstone_live,
                search=self,
            )
            if not unsupported:
                if self.rust_solver_mode == "verify":
                    py_turn, py_capped = self._earliest_python(
                        deck_order,
                        bottom_count,
                        gemstone_live=gemstone_live,
                        gamble_seed=gamble_seed,
                    )
                    if (py_turn, py_capped) != (rust_turn, rust_capped):
                        raise RuntimeError(
                            "Rust earliest parity mismatch: "
                            f"python={(py_turn, py_capped)} rust={(rust_turn, rust_capped)} "
                            f"bottom_count={bottom_count} gemstone_live={gemstone_live} hand7={deck_order[:7]}"
                        )
                    return py_turn, py_capped
                return rust_turn, rust_capped
        return self._earliest_with_keep_solver(
            deck_order,
            bottom_count,
            gemstone_live=gemstone_live,
            gamble_seed=gamble_seed,
        )

    def _earliest_with_keep_solver(self, deck_order: list[str], bottom_count: int, *, gemstone_live: bool = False, gamble_seed: int | None = None) -> tuple[int | None, bool]:
        best_turn = None
        truncated = False
        if gamble_seed is not None:
            self.gamble_seed = gamble_seed
        hand7 = deck_order[:7]
        rest = deck_order[7:]
        bottom_choices = combinations(range(7), bottom_count) if bottom_count else [()]
        for bottom_idx in bottom_choices:
            bottom_set = set(bottom_idx)
            hand = [card for i, card in enumerate(hand7) if i not in bottom_set]
            library = [*rest, *[hand7[i] for i in bottom_idx]]
            turn, hit_limit = self._earliest_for_keep(hand, library, gemstone_live=gemstone_live)
            truncated = truncated or hit_limit
            if turn is not None and (best_turn is None or turn < best_turn):
                best_turn = turn
                if best_turn == 1:
                    break
        return best_turn, truncated

    def _earliest_python(self, deck_order: list[str], bottom_count: int, *, gemstone_live: bool = False, gamble_seed: int | None = None) -> tuple[int | None, bool]:
        best_turn = None
        truncated = False
        if gamble_seed is not None:
            self.gamble_seed = gamble_seed
        hand7 = deck_order[:7]
        rest = deck_order[7:]
        bottom_choices = combinations(range(7), bottom_count) if bottom_count else [()]
        for bottom_idx in bottom_choices:
            bottom_set = set(bottom_idx)
            hand = [card for i, card in enumerate(hand7) if i not in bottom_set]
            library = [*rest, *[hand7[i] for i in bottom_idx]]
            turn, hit_limit, _label = self._earliest_labeled_for_keep_python(hand, library, gemstone_live=gemstone_live)
            truncated = truncated or hit_limit
            if turn is not None and (best_turn is None or turn < best_turn):
                best_turn = turn
                if best_turn == 1:
                    break
        return best_turn, truncated

    def _starting_state_options(self, hand: Iterable[str], library: Iterable[str], *, gemstone_live: bool = False) -> Iterable[tuple[State, tuple[str, ...]]]:
        hand_tuple = norm(hand)
        library_tuple = tuple(library)
        yield State(hand=hand_tuple, library=library_tuple, battlefield=()), ()
        gemstone_card = next((card for card in hand_tuple if card in GEMSTONE_CAVERNS_NAMES), None)
        if not gemstone_live or gemstone_card is None:
            return
        for exile in sorted(c for c in set(hand_tuple) if c != gemstone_card):
            exile_hand = remove_card(remove_card(hand_tuple, gemstone_card), exile)
            start = State(hand=exile_hand, library=library_tuple, battlefield=(Perm("CAVERN", False, COLORS),))
            path = (f"begin with {gemstone_card} exiling {exile}",)
            yield start, path
            yield from self._preturn_caverns_top_tutor_options(start, path)

    def _preturn_caverns_top_tutor_options(self, state: State, path: tuple[str, ...]) -> Iterable[tuple[State, tuple[str, ...]]]:
        if not any(perm.name == "CAVERN" for perm in state.battlefield):
            return
        for tutor in ("Enlightened Tutor", "Mystical Tutor", "Vampiric Tutor", "Worldly Tutor"):
            if tutor not in state.hand:
                continue
            for target in self._tutor_targets(tutor, state, top=True):
                if target not in state.library:
                    continue
                next_state = self._replace(
                    state,
                    hand=remove_card(state.hand, tutor),
                    library=known_top_library_after_shuffle(target, tuple(card for card in state.library if card != target)),
                    mana=ZERO_MANA,
                    spells_this_turn=0,
                )
                yield next_state, (*path, f"preturn cast {tutor} for {target}")

    def _earliest_for_keep(self, hand: list[str], library: list[str], *, gemstone_live: bool = False, gamble_seed: int | None = None) -> tuple[int | None, bool]:
        turn, truncated, _label = self._earliest_labeled_for_keep(
            hand,
            library,
            gemstone_live=gemstone_live,
            gamble_seed=gamble_seed,
        )
        return turn, truncated

    def _earliest_labeled_for_keep(self, hand: list[str], library: list[str], *, gemstone_live: bool = False, gamble_seed: int | None = None) -> tuple[int | None, bool, str | None]:
        if self.rust_solver_mode != "off" and self._rust_solver_supported():
            if self._rust_solver_accelerator is None:
                raise RuntimeError("Rust solve-keep accelerator is not initialized")
            rust_turn, rust_capped, rust_label, unsupported, _reason = self._rust_solver_accelerator.solve_keep(
                hand,
                library,
                gemstone_live=gemstone_live,
                search=self,
                gamble_seed=gamble_seed,
            )
            if not unsupported:
                if self.rust_solver_mode == "verify":
                    py_turn, py_capped, py_label = self._earliest_labeled_for_keep_python(
                        hand,
                        library,
                        gemstone_live=gemstone_live,
                        gamble_seed=gamble_seed,
                    )
                    if (py_turn, py_capped, py_label) != (rust_turn, rust_capped, rust_label):
                        raise RuntimeError(
                            "Rust solve-keep parity mismatch: "
                            f"python={(py_turn, py_capped, py_label)} rust={(rust_turn, rust_capped, rust_label)} "
                            f"hand={hand} gemstone_live={gemstone_live}"
                        )
                    return py_turn, py_capped, py_label
                return rust_turn, rust_capped, rust_label
        return self._earliest_labeled_for_keep_python(
            hand,
            library,
            gemstone_live=gemstone_live,
            gamble_seed=gamble_seed,
        )

    def _rust_solver_supported(self) -> bool:
        if self.goal != ENGINE_GOAL:
            return False
        if self.engine_success_policy != "resilient" or self.engine_target_count != 1:
            return False
        if set(ENGINE_NATIVE) != {TARGET, "Heartwood Storyteller"} or ENGINE_COPY_SPELLS:
            return False
        return True

    def _earliest_labeled_for_keep_python(self, hand: list[str], library: list[str], *, gemstone_live: bool = False, gamble_seed: int | None = None) -> tuple[int | None, bool, str | None]:
        if gamble_seed is not None:
            self.gamble_seed = gamble_seed
        chancellor = "Chancellor of the Tangle" in hand
        states: dict[State, None] = {state: None for state, _path in self._starting_state_options(hand, library, gemstone_live=gemstone_live)}
        truncated = False
        for turn in range(1, self.max_turns + 1):
            turn_states = {}
            for state in states:
                begun = self._begin_turn(state)
                begun = self._replace(begun, turn=turn)
                if begun.pact_debt <= 0:
                    drawn = self._draw(begun)
                    if turn == 1 and chancellor:
                        drawn = self._replace(drawn, mana=add_mana(drawn.mana, (0, 0, 0, 0, 1, 0)))
                    turn_states[drawn] = None
                else:
                    for upkeep_paid in self._pay_upkeep_pacts(begun):
                        drawn = self._draw(upkeep_paid)
                        if turn == 1 and chancellor:
                            drawn = self._replace(drawn, mana=add_mana(drawn.mana, (0, 0, 0, 0, 1, 0)))
                        turn_states[drawn] = None
            closed, success, hit_limit, label = self._close_turn_labeled(turn_states)
            truncated = truncated or hit_limit
            if success:
                return turn, truncated, label
            states = {self._end_turn(state): None for state in closed}
        return None, truncated, None

    def _begin_turn(self, state: State) -> State:
        battlefield = norm_battlefield(
            Perm(p.name, False if p.name not in {"VAULT", "MONOLITH"} else p.tapped, p.extra.removesuffix("*"))
            for p in state.battlefield
        )
        return State(
            state.hand,
            state.library,
            battlefield,
            ZERO_MANA,
            False,
            state.land_grave_count,
            state.mantle_attached,
            state.nature_attached,
            False,
            False,
            False,
            0,
            state.pact_debt,
            state.turn,
            state.engine_count,
            state.engine_targets,
            state.engine_names,
        )

    def _end_turn(self, state: State) -> State:
        battlefield = state.battlefield
        if any(p.name == "GLIMMER" for p in battlefield) and not any(self._is_artifact(p) for p in battlefield):
            battlefield = tuple(p for p in battlefield if p.name != "GLIMMER")
        return State(
            state.hand,
            state.library,
            battlefield,
            ZERO_MANA,
            False,
            state.land_grave_count,
            state.mantle_attached,
            state.nature_attached,
            state.nature_untap_used,
            state.nature_tap_used,
            False,
            0,
            state.pact_debt,
            state.turn,
            state.engine_count,
            state.engine_targets,
            state.engine_names,
        )

    def _draw(self, state: State, count: int = 1) -> State:
        hand = list(state.hand)
        lib = list(state.library)
        for _ in range(count):
            if lib:
                hand.append(lib.pop(0))
        return self._replace(state, hand=tuple(hand), library=tuple(lib))

    def _close_turn(self, start_states: dict[State, None]) -> tuple[dict[State, None], bool, bool]:
        closed, success, hit_limit, _label = self._close_turn_labeled(start_states)
        return closed, success, hit_limit

    def _close_turn_labeled(self, start_states: dict[State, None]) -> tuple[dict[State, None], bool, bool, str | None]:
        if self.rust_close_mode == "off" or not self._rust_close_supported(start_states):
            return self._close_turn_labeled_python(start_states)
        if self._rust_close_accelerator is None:
            raise RuntimeError("Rust close-turn accelerator is not initialized")
        start_state_list = list(start_states)
        rust_closed, rust_success, rust_hit_limit, rust_label = self._rust_close_accelerator.close_turn(start_state_list, self)
        if self.rust_close_mode == "verify":
            py_closed, py_success, py_hit_limit, py_label = self._close_turn_labeled_python(start_states, force_python_actions=True)
            py_closed_list = list(py_closed)
            if (
                py_success != rust_success
                or py_hit_limit != rust_hit_limit
                or py_label != rust_label
                or py_closed_list != rust_closed
            ):
                first_diff = None
                for index, (py_state, rust_state) in enumerate(zip(py_closed_list, rust_closed)):
                    if py_state != rust_state:
                        first_diff = {
                            "index": index,
                            "python": state_to_json(py_state),
                            "rust": state_to_json(rust_state),
                        }
                        break
                if first_diff is None and len(py_closed_list) != len(rust_closed):
                    first_diff = {
                        "python_len": len(py_closed_list),
                        "rust_len": len(rust_closed),
                    }
                raise RuntimeError(
                    "Rust close-turn parity mismatch: "
                    f"python_success={py_success} rust_success={rust_success} "
                    f"python_hit_limit={py_hit_limit} rust_hit_limit={rust_hit_limit} "
                    f"python_label={py_label!r} rust_label={rust_label!r} "
                    f"python_closed={len(py_closed_list)} rust_closed={len(rust_closed)} "
                    f"first_diff={first_diff} "
                    f"start_state_sample={[state_to_json(state) for state in start_state_list[:5]]}"
                )
            return py_closed, py_success, py_hit_limit, py_label
        return dict.fromkeys(rust_closed), rust_success, rust_hit_limit, rust_label

    def _rust_close_supported(self, start_states: dict[State, None]) -> bool:
        if self.goal != ENGINE_GOAL:
            return False
        if self.engine_success_policy != "resilient" or self.engine_target_count != 1:
            return False
        if set(ENGINE_NATIVE) != {TARGET, "Heartwood Storyteller"} or ENGINE_COPY_SPELLS:
            return False
        return all(self._rust_supported_state(state) for state in start_states)

    def _close_turn_labeled_python(
        self,
        start_states: dict[State, None],
        *,
        force_python_actions: bool = False,
    ) -> tuple[dict[State, None], bool, bool, str | None]:
        queue = deque(start_states)
        seen = dict(start_states)
        best_mana: dict[tuple, list[tuple[int, ...]]] = {}
        hit_limit = False
        while queue:
            state = queue.pop()
            label = self._success_label(state)
            if label is not None:
                return seen, True, hit_limit, label
            if self.action_sort:
                actions = list(self._actions_python(state) if force_python_actions else self._actions(state))
                actions.sort(key=lambda item: self._priority(item[1]))
            else:
                actions = self._actions_python(state) if force_python_actions else self._actions(state)
            for next_state, _action in actions:
                if next_state in seen or self._mana_dominated(next_state, best_mana):
                    continue
                label = self._success_label(next_state)
                if label is not None:
                    return seen, True, hit_limit, label
                if len(seen) >= self.state_limit:
                    hit_limit = True
                    continue
                seen[next_state] = None
                queue.append(next_state)
        return seen, False, hit_limit, None

    def _priority(self, action: str) -> int:
        return action_priority(self.goal, action)

    def _rust_supported_state(self, state: State) -> bool:
        # Stochastic Gamble discard is intentionally kept in Python because it
        # depends on CPython's seeded Random(payload) behavior.
        return "Gamble" not in state.hand

    def _actions(self, state: State) -> Iterable[tuple[State, str]]:
        if self.rust_action_mode == "off" or not self._rust_supported_state(state):
            yield from self._actions_python(state)
            return
        if self._rust_action_accelerator is None:
            raise RuntimeError("Rust action accelerator is not initialized")
        rust_actions = self._rust_action_accelerator.actions(state)
        if self.rust_action_mode == "verify":
            python_actions = list(self._actions_python(state))
            rust_counts = action_multiset(rust_actions)
            python_counts = action_multiset(python_actions)
            if rust_counts != python_counts:
                missing = python_counts - rust_counts
                extra = rust_counts - python_counts
                missing_labels = [label for (_state, label), count in missing.items() for _ in range(count)]
                extra_labels = [label for (_state, label), count in extra.items() for _ in range(count)]
                raise RuntimeError(
                    "Rust action accelerator parity mismatch: "
                    f"missing={missing_labels[:12]} extra={extra_labels[:12]} "
                    f"state={state_to_json(state)}"
                )
            if rust_actions != python_actions:
                rust_labels = [label for _next_state, label in rust_actions]
                python_labels = [label for _next_state, label in python_actions]
                raise RuntimeError(
                    "Rust action accelerator order mismatch: "
                    f"python={python_labels[:20]} rust={rust_labels[:20]} state={state_to_json(state)}"
                )
            yield from python_actions
            return
        yield from rust_actions

    def _actions_python(self, state: State) -> Iterable[tuple[State, str]]:
        hand_set = set(state.hand)
        yield from self._mana_actions(state)
        yield from self._engine_actions(state)

        for name, perm_name, cost, colors in self.commander_specs:
            if any(p.name == perm_name for p in state.battlefield):
                continue
            for mana in pay_options(state.mana, cost):
                ns = self._replace(state, battlefield=(*state.battlefield, Perm(perm_name, False, colors + "*")), mana=mana)
                yield self._after_cast(state, ns), f"cast {name}"

        if not state.land_played:
            for card in state.hand:
                if card not in LANDS and card not in MDFC_LANDS and not is_theoretical_rainbow_land(card):
                    continue
                for perm, library, grave_inc, label in self._land_options(card, state.library):
                    # City of Traitors sacrifices itself when you play any later land.
                    battlefield = [p for p in state.battlefield if p.name != "CITY"]
                    ns = self._replace(
                        state,
                        hand=remove_card(state.hand, card),
                        library=library,
                        battlefield=(*battlefield, perm),
                        land_played=True,
                        land_grave_count=state.land_grave_count + grave_inc,
                    )
                    yield ns, f"play {card}{label}"

        for card, perm in (("Lotus Petal", "PETAL"), ("Chaos Emerald", "PETAL"), ("Lion's Eye Diamond", "LED"), ("Mox Amber", "AMBER"), ("Mox Opal", "OPAL"), ("Paradise Mantle", "MANTLE")):
            if card in hand_set:
                ns = self._replace(state, hand=remove_card(state.hand, card), battlefield=(*state.battlefield, Perm(perm)))
                yield self._after_cast(state, ns), f"cast {card}"

        if "Chrome Mox" in hand_set:
            for imprint in state.hand:
                if (
                    imprint == "Chrome Mox"
                    or imprint in LANDS
                    or is_theoretical_rainbow_land(imprint)
                    or imprint in ARTIFACTS
                    or not CARD_COLORS.get(imprint)
                ):
                    continue
                ns = self._replace(
                    state,
                    hand=remove_card(remove_card(state.hand, "Chrome Mox"), imprint),
                    battlefield=(*state.battlefield, Perm("CHROME", False, CARD_COLORS[imprint])),
                )
                yield self._after_cast(state, ns), f"cast Chrome Mox imprint {imprint}"

        if "Mox Diamond" in hand_set:
            for land in state.hand:
                if land not in LANDS and not is_theoretical_rainbow_land(land):
                    continue
                ns = self._replace(
                    state,
                    hand=remove_card(remove_card(state.hand, "Mox Diamond"), land),
                    battlefield=(*state.battlefield, Perm("DIAMOND")),
                    land_grave_count=state.land_grave_count + 1,
                )
                yield self._after_cast(state, ns), f"cast Mox Diamond discard {land}"

        for card, perm, cost in (
            ("Sol Ring", "SOL", (1, 0, 0, 0, 0, 0)),
            ("Mana Vault", "VAULT", (1, 0, 0, 0, 0, 0)),
            ("Arcane Signet", "SIGNET", (2, 0, 0, 0, 0, 0)),
            ("Relic of Legends", "RELIC", (3, 0, 0, 0, 0, 0)),
            ("Wishclaw Talisman", "WISHCLAW", (1, 1, 0, 0, 0, 0)),
            ("Springleaf Drum", "DRUM", (1, 0, 0, 0, 0, 0)),
        ):
            if card in hand_set:
                for mana in pay_options(state.mana, cost):
                    ns = self._replace(state, hand=remove_card(state.hand, card), battlefield=(*state.battlefield, Perm(perm)), mana=mana)
                    yield self._after_cast(state, ns), f"cast {card}"

        for card, costs in EARLY_CREATURE_CAST_OPTIONS:
            if card not in hand_set:
                continue
            if self.goal == ENGINE_GOAL and card in ENGINE_NATIVE_CREATURES:
                continue
            perm = self._creature_perm(card)
            hand_without_card = remove_card(state.hand, card)
            for cst in costs:
                for mana in pay_options(state.mana, cst):
                    ns = self._replace(state, hand=hand_without_card, battlefield=(*state.battlefield, perm), mana=mana)
                    if card == "Tataru Taru":
                        ns = self._draw(ns)
                        ns = self._replace(ns, battlefield=(*ns.battlefield, Perm("TREASURE", True)))
                    yield self._after_cast(state, ns), f"cast {card}"

        if "Nature's Chosen" in hand_set and not any(p.name == "NATURE" for p in state.battlefield):
            for creature_i in self._unique_creatures(state):
                creature = state.battlefield[creature_i]
                for mana in pay_options(state.mana, (0, 0, 0, 0, 0, 1)):
                    ns = self._replace(
                        state,
                        hand=remove_card(state.hand, "Nature's Chosen"),
                        battlefield=(*state.battlefield, Perm("NATURE")),
                        mana=mana,
                        nature_attached=self._mantle_key(creature),
                        nature_untap_used=False,
                        nature_tap_used=False,
                    )
                    yield self._after_cast(state, ns), f"cast Nature's Chosen enchanting {creature.name}"

        if any(p.name == "MANTLE" for p in state.battlefield):
            for creature_i in self._unique_creatures(state):
                creature = state.battlefield[creature_i]
                for mana in pay_options(state.mana, (1, 0, 0, 0, 0, 0)):
                    ns = self._replace(state, mana=mana, mantle_attached=self._mantle_key(creature))
                    yield ns, f"equip Paradise Mantle to {creature.name}"

        if "Simian Spirit Guide" in hand_set:
            ns = self._replace(state, hand=remove_card(state.hand, "Simian Spirit Guide"), mana=add_mana(state.mana, (0, 1, 0, 0, 0, 0)))
            yield ns, "exile Simian Spirit Guide"
        if "Elvish Spirit Guide" in hand_set:
            ns = self._replace(state, hand=remove_card(state.hand, "Elvish Spirit Guide"), mana=add_mana(state.mana, (0, 0, 0, 0, 1, 0)))
            yield ns, "exile Elvish Spirit Guide"

        if "Strike It Rich" in hand_set:
            for mana in pay_options(state.mana, (0, 0, 1, 0, 0, 0)):
                ns = self._replace(state, hand=remove_card(state.hand, "Strike It Rich"), battlefield=(*state.battlefield, Perm("TREASURE")), mana=mana)
                yield self._after_cast(state, ns), "cast Strike It Rich"

        for card, cost, add in (
            ("Dark Ritual", (0, 1, 0, 0, 0, 0), (3, 0, 0, 0, 0, 0)),
            ("Rite of Flame", (0, 0, 1, 0, 0, 0), (0, 2, 0, 0, 0, 0)),
        ):
            if card in hand_set:
                for mana in pay_options(state.mana, cost):
                    ns = self._replace(state, hand=remove_card(state.hand, card), mana=add_mana(mana, add))
                    yield self._after_cast(state, ns), f"cast {card}"

        if "Rain of Filth" in hand_set:
            for mana in pay_options(state.mana, (0, 1, 0, 0, 0, 0)):
                ns = self._replace(state, hand=remove_card(state.hand, "Rain of Filth"), mana=mana, rain_active=True)
                yield self._after_cast(state, ns), "cast Rain of Filth"

        if "Jeska's Will" in hand_set:
            for mana in pay_options(state.mana, (2, 0, 1, 0, 0, 0)):
                ns = self._replace(state, hand=remove_card(state.hand, "Jeska's Will"), mana=add_mana(mana, (0, 7, 0, 0, 0, 0)))
                yield self._after_cast(state, ns), "cast Jeska's Will mana"

        for card, cost, add in (("Infernal Plunge", (0, 0, 1, 0, 0, 0), (0, 3, 0, 0, 0, 0)), ("Culling the Weak", (0, 1, 0, 0, 0, 0), (4, 0, 0, 0, 0, 0))):
            if card in hand_set:
                for creature_index in self._unique_creatures(state):
                    for mana in pay_options(state.mana, cost):
                        base = self._sac_creature(state, creature_index)
                        ns = self._replace(base, hand=remove_card(base.hand, card), mana=add_mana(mana, add))
                        yield self._after_cast(base, ns), f"cast {card}"

        if "Manamorphose" in hand_set:
            for cost in ((1, 0, 1, 0, 0, 0), (1, 0, 0, 0, 0, 1)):
                for mana in pay_options(state.mana, cost):
                    for a, b in {("U", c) for c in COLORS} | {("W", "U"), ("B", "U"), ("R", "U"), ("G", "U")}:
                            ns = self._replace(state, hand=remove_card(state.hand, "Manamorphose"), mana=add_mana(add_mana(mana, mana_for_color(a)), mana_for_color(b)))
                            ns = self._draw(ns)
                            yield self._after_cast(state, ns), "cast Manamorphose"

        if "Gitaxian Probe" in hand_set:
            ns = self._draw(self._replace(state, hand=remove_card(state.hand, "Gitaxian Probe")))
            yield self._after_cast(state, ns), "cast Gitaxian Probe"

        if "Street Wraith" in hand_set:
            ns = self._draw(self._replace(state, hand=remove_card(state.hand, "Street Wraith")))
            yield ns, "cycle Street Wraith"

        yield from self._offer_self_counter_actions(state)

        if "Wheel of Fortune" in hand_set:
            for mana in pay_options(state.mana, (2, 0, 1, 0, 0, 0)):
                ns = self._replace(state, hand=(), mana=mana)
                ns = self._draw(ns, 7)
                yield self._after_cast(state, ns), "cast Wheel of Fortune"

        if "Summoner's Pact" in hand_set:
            for target in ("Tinder Wall", "Birds of Paradise", "Deathrite Shaman", "Wild Cantor", "Noble Hierarch", "Ignoble Hierarch"):
                if target in state.library:
                    ns = self._replace(
                        state,
                        hand=norm((*remove_card(state.hand, "Summoner's Pact"), target)),
                        library=obscure_library_top_after_shuffle(remove_card(state.library, target)),
                        pact_debt=state.pact_debt + 1,
                    )
                    yield self._after_cast(state, ns), f"cast Summoner's Pact for {target}"
            if self.goal == ENGINE_GOAL and "Heartwood Storyteller" in ENGINE_NATIVE and "Heartwood Storyteller" in state.library:
                ns = self._replace(
                    state,
                    hand=norm((*remove_card(state.hand, "Summoner's Pact"), "Heartwood Storyteller")),
                    library=obscure_library_top_after_shuffle(remove_card(state.library, "Heartwood Storyteller")),
                    pact_debt=state.pact_debt + 1,
                )
                yield self._after_cast(state, ns), "cast Summoner's Pact for Heartwood"

        if "Green Sun's Zenith" in hand_set:
            for target in ("Tinder Wall", "Birds of Paradise", "Deathrite Shaman", "Wild Cantor", "Noble Hierarch", "Ignoble Hierarch"):
                if target in state.library:
                    for mana in pay_options(state.mana, (1, 0, 0, 0, 0, 1)):
                        ns = self._replace(state, hand=remove_card(state.hand, "Green Sun's Zenith"), library=obscure_library_top_after_shuffle(remove_card(state.library, target)), battlefield=(*state.battlefield, self._creature_perm(target)), mana=mana)
                        yield self._after_cast(state, ns), f"cast Green Sun's Zenith for {target}"
            if self.goal == ENGINE_GOAL and "Heartwood Storyteller" in ENGINE_NATIVE and "Heartwood Storyteller" in state.library:
                for mana in pay_options(state.mana, (3, 0, 0, 0, 0, 1)):
                    ns = self._replace(state, hand=remove_card(state.hand, "Green Sun's Zenith"), library=obscure_library_top_after_shuffle(remove_card(state.library, "Heartwood Storyteller")), battlefield=(*state.battlefield, ENGINE_NATIVE["Heartwood Storyteller"][2]), mana=mana)
                    ns = self._add_engine(self._after_cast(state, ns), "CREATURE", "Heartwood Storyteller")
                    yield ns, "cast Green Sun's Zenith for Heartwood"

        if self.goal == ENGINE_GOAL and "Ranger-Captain of Eos" in hand_set and "Esper Sentinel" in state.library:
            for mana in pay_options(state.mana, (1, 0, 0, 0, 2, 0)):
                ns = self._replace(
                    state,
                    hand=norm((*remove_card(state.hand, "Ranger-Captain of Eos"), "Esper Sentinel")),
                    library=obscure_library_top_after_shuffle(remove_card(state.library, "Esper Sentinel")),
                    battlefield=(*state.battlefield, Perm("CREATURE", False, "W*")),
                    mana=mana,
                )
                yield self._after_cast(state, ns), "cast Ranger-Captain for Esper Sentinel"

        if self.goal == ENGINE_GOAL and "Heartwood Storyteller" in ENGINE_NATIVE and "Eldritch Evolution" in hand_set and "Heartwood Storyteller" in state.library:
            for creature_index in self._unique_creatures(state):
                perm = state.battlefield[creature_index]
                if CREATURE_MV_BY_PERM.get(perm.name, 0) + 2 < 3:
                    continue
                for mana in pay_options(state.mana, (1, 0, 0, 0, 0, 2)):
                    base = self._sac_creature(state, creature_index)
                    ns = self._replace(base, hand=remove_card(base.hand, "Eldritch Evolution"), library=obscure_library_top_after_shuffle(remove_card(base.library, "Heartwood Storyteller")), battlefield=(*base.battlefield, ENGINE_NATIVE["Heartwood Storyteller"][2]), mana=mana)
                    ns = self._add_engine(self._after_cast(base, ns), "CREATURE", "Heartwood Storyteller")
                    yield ns, "cast Eldritch Evolution for Heartwood"

        if self.goal == ENGINE_GOAL and "Heartwood Storyteller" in ENGINE_NATIVE and "Neoform" in hand_set and "Heartwood Storyteller" in state.library:
            for creature_index in self._unique_creatures(state):
                perm = state.battlefield[creature_index]
                if CREATURE_MV_BY_PERM.get(perm.name, 0) + 1 != 3:
                    continue
                for mana in pay_options(state.mana, (0, 0, 0, 1, 0, 1)):
                    base = self._sac_creature(state, creature_index)
                    ns = self._replace(base, hand=remove_card(base.hand, "Neoform"), library=obscure_library_top_after_shuffle(remove_card(base.library, "Heartwood Storyteller")), battlefield=(*base.battlefield, ENGINE_NATIVE["Heartwood Storyteller"][2]), mana=mana)
                    ns = self._add_engine(self._after_cast(base, ns), "CREATURE", "Heartwood Storyteller")
                    yield ns, "cast Neoform for Heartwood"

        if "Crop Rotation" in hand_set and any(self._is_land_perm(p) for p in state.battlefield):
            library_set = set(state.library)
            for mana in pay_options(state.mana, (0, 0, 0, 0, 0, 1)):
                for land_index, perm in enumerate(state.battlefield):
                    if not self._is_land_perm(perm):
                        continue
                    for target in EARLY_CROP_TARGET_ORDER:
                        if target not in library_set:
                            continue
                        for target_perm, lib, grave_inc, label in self._land_options(target, remove_card(state.library, target), already_removed=True):
                            base = self._sac_land(state, land_index)
                            ns = self._replace(base, hand=remove_card(base.hand, "Crop Rotation"), library=obscure_library_top_after_shuffle(lib), battlefield=(*base.battlefield, target_perm), mana=mana, land_grave_count=base.land_grave_count + grave_inc)
                            yield self._after_cast(state, ns), f"cast Crop Rotation for {target}{label}"

        for tutor, cost in HAND_TUTORS.items():
            if tutor in hand_set:
                if tutor == "Diabolic Intent":
                    creature_indices = self._unique_creatures(state)
                else:
                    creature_indices = [None]
                for creature_index in creature_indices:
                    for target in self._tutor_targets(tutor, state):
                        for mana in pay_options(state.mana, cost):
                            base = self._sac_creature(state, creature_index) if creature_index is not None else state
                            ns = self._replace(base, hand=norm((*remove_card(base.hand, tutor), target)), library=obscure_library_top_after_shuffle(remove_card(base.library, target)), mana=mana)
                            yield self._after_cast(base, ns), f"cast {tutor} for {target}"
                            yield from self._led_tutor_line(state, tutor, cost, creature_index, target)

        if "Beseech the Mirror" in hand_set:
            for target in self._beseech_targets(state):
                for mana in pay_options(state.mana, (1, 3, 0, 0, 0, 0)):
                    ns = self._replace(state, hand=norm((*remove_card(state.hand, "Beseech the Mirror"), target)), library=obscure_library_top_after_shuffle(remove_card(state.library, target)), mana=mana)
                    yield self._after_cast(state, ns), f"cast Beseech the Mirror for {target}"
                yield from self._led_beseech_line(state, target)

            bargain_indices = [i for i, p in enumerate(state.battlefield) if self._is_artifact(p) or self._is_enchantment(p)]
            for idx in bargain_indices:
                for target in self._beseech_targets(state):
                    for mana in pay_options(state.mana, (1, 3, 0, 0, 0, 0)):
                        base = self._sac_perm(state, idx)
                        ns = self._replace(base, hand=remove_card(base.hand, "Beseech the Mirror"), library=obscure_library_top_after_shuffle(remove_card(base.library, target)), mana=mana)
                        ns = self._after_cast(state, ns)
                        ns = self._resolve_free_engine(ns, target)
                        yield ns, f"cast bargained Beseech for {target}"

        for tutor, cost in TOP_TUTORS.items():
            if tutor in hand_set:
                for target in self._tutor_targets(tutor, state, top=True):
                    if target not in state.library:
                        continue
                    for mana in pay_options(state.mana, cost):
                        ns = self._replace(state, hand=remove_card(state.hand, tutor), library=known_top_library_after_shuffle(target, tuple(c for c in state.library if c != target)), mana=mana)
                        yield self._after_cast(state, ns), f"cast {tutor} for {target}"

        if any(p.name == "WISHCLAW" and not p.tapped for p in state.battlefield):
            for i, p in enumerate(state.battlefield):
                if p.name != "WISHCLAW" or p.tapped:
                    continue
                for target in self._tutor_targets("Wishclaw Talisman", state):
                    for mana in pay_options(state.mana, (1, 0, 0, 0, 0, 0)):
                        bf = list(state.battlefield)
                        bf.pop(i)
                        ns = self._replace(state, battlefield=bf, hand=norm((*state.hand, target)), library=obscure_library_top_after_shuffle(remove_card(state.library, target)), mana=mana)
                        yield ns, f"activate Wishclaw for {target}"
                        yield from self._led_wishclaw_line(state, i, target)

        if self.gamble_mode != "off" and "Gamble" in hand_set:
            for mana in pay_options(state.mana, (0, 0, 1, 0, 0, 0)):
                for target in self._gamble_targets(state, mana):
                    searched_hand = norm((*remove_card(state.hand, "Gamble"), target))
                    if self.gamble_mode == "optimistic":
                        ns = self._replace(state, hand=searched_hand, library=obscure_library_top_after_shuffle(remove_card(state.library, target)), mana=mana)
                        yield self._after_cast(state, ns), f"cast optimistic Gamble for {target}"
                        continue
                    discard = self._gamble_discard(state, target, searched_hand)
                    ns = self._replace(
                        state,
                        hand=remove_card(searched_hand, discard),
                        library=obscure_library_top_after_shuffle(remove_card(state.library, target)),
                        mana=mana,
                    )
                    yield self._after_cast(state, ns), f"cast Gamble for {target} discard {discard}"

    def _offer_self_counter_actions(self, state: State):
        if "An Offer You Can't Refuse" not in state.hand:
            return
        for bait in state.hand:
            if bait == "An Offer You Can't Refuse" or bait not in OFFER_COUNTERABLE_COSTS:
                continue
            for bait_cost in OFFER_COUNTERABLE_COSTS[bait]:
                if not self._offer_bait_can_help(state, bait, bait_cost):
                    continue
                for after_bait_mana in pay_options(state.mana, bait_cost):
                    bait_cast = self._after_cast(
                        state,
                        self._replace(state, hand=remove_card(state.hand, bait), mana=after_bait_mana),
                    )
                    for after_offer_mana in pay_options(bait_cast.mana, OFFER_COST):
                        ns = self._replace(
                            bait_cast,
                            hand=remove_card(bait_cast.hand, "An Offer You Can't Refuse"),
                            mana=after_offer_mana,
                            battlefield=(*bait_cast.battlefield, Perm("TREASURE"), Perm("TREASURE")),
                        )
                        yield self._after_cast(bait_cast, ns), f"cast {bait}, counter it with An Offer You Can't Refuse"

    def _offer_bait_can_help(self, state: State, bait: str, bait_cost: tuple[int, int, int, int, int, int]) -> bool:
        if bait in OFFER_ENGINE_BAIT_EXCLUSIONS:
            return False
        if bait == "Noxious Revival" and state.land_grave_count <= 0:
            return False
        if sum(bait_cost) <= 1:
            return True
        return any(p.name in {"BIRGI", "LOTHO"} for p in state.battlefield)

    def _success(self, state: State) -> bool:
        return self._success_label(state) is not None

    @lru_cache(maxsize=131072)
    def _success_label(self, state: State) -> str | None:
        if self.goal == ENGINE_GOAL:
            label = self._engine_success_label(state)
        elif state.engine_count:
            label = TARGET
        else:
            label = TARGET if TARGET in state.hand and bool(pay_options(state.mana, RHYSTIC_COST)) else None
        if label is None:
            return None
        return label if self._can_survive_next_pact_upkeep(state) else None

    def _engine_success(self, state: State) -> bool:
        return self._engine_success_label(state) is not None

    def _engine_success_label(self, state: State) -> str | None:
        if self.engine_success_policy == "count":
            if state.engine_count < self.engine_target_count:
                return None
            return self._best_engine_name(state) or "engine"
        if self.engine_success_policy != "resilient":
            raise ValueError(f"Unknown engine_success_policy: {self.engine_success_policy}")
        candidates: list[tuple[int, int, str]] = []
        for item in state.engine_names:
            name, sep, turn_text = item.rpartition("@")
            if not sep:
                continue
            try:
                turn = int(turn_text)
            except ValueError:
                continue
            if name in {TARGET, "Heartwood Storyteller", "Smothering Tithe"} and turn <= 2:
                candidates.append((engine_target_priority(name), turn, name))
        for item in state.engine_names:
            name, sep, turn_text = item.rpartition("@")
            if not sep or name != "Mystic Remora":
                continue
            try:
                turn = int(turn_text)
            except ValueError:
                continue
            if turn == 1 and self._can_keep_remora(state, self.remora_upkeep_payments):
                candidates.append((engine_target_priority(name), turn, name))
        if not candidates:
            return None
        return min(candidates)[2]

    def _best_engine_name(self, state: State) -> str | None:
        names: list[tuple[int, int, str]] = []
        for item in state.engine_names:
            name, sep, turn_text = item.rpartition("@")
            if not sep:
                continue
            try:
                turn = int(turn_text)
            except ValueError:
                turn = 99
            names.append((engine_target_priority(name), turn, name))
        if not names:
            return None
        return min(names)[2]

    def _engine_actions(self, state: State):
        if self.goal != ENGINE_GOAL:
            return
        hand_set = set(state.hand)
        for card, (cost, target_type, perm) in ENGINE_NATIVE.items():
            if card not in hand_set:
                continue
            for mana in pay_options(state.mana, cost):
                ns = self._replace(state, hand=remove_card(state.hand, card), battlefield=(*state.battlefield, perm), mana=mana)
                ns = self._add_engine(self._after_cast(state, ns), target_type, card)
                yield ns, f"cast engine {card}"

        for card, (cost, legal_targets) in ENGINE_COPY_SPELLS.items():
            if card not in hand_set:
                continue
            for target_type in sorted(set(state.engine_targets) & set(legal_targets)):
                for mana in pay_options(state.mana, cost):
                    ns = self._replace(state, hand=remove_card(state.hand, card), battlefield=(*state.battlefield, self._engine_copy_perm(target_type)), mana=mana)
                    ns = self._add_engine(self._after_cast(state, ns), target_type, card)
                    yield ns, f"cast {card} copying engine"

    def _add_engine(self, state: State, target_type: str, engine_name: str | None = None) -> State:
        targets = set(state.engine_targets)
        targets.add(target_type)
        targets.add("PERM")
        names = set(state.engine_names)
        if engine_name is not None:
            names.add(f"{engine_name}@{state.turn}")
        return self._replace(state, engine_count=state.engine_count + 1, engine_targets=tuple(sorted(targets)), engine_names=tuple(sorted(names)))

    def _engine_copy_perm(self, target_type: str) -> Perm:
        if target_type == "ENCH":
            return Perm("ENGINE_ENCH")
        if target_type == "ART":
            return Perm("ESPER", False, "W*")
        return Perm("HEARTWOOD", False, "G*")

    def _resolve_free_engine(self, state: State, target: str) -> State:
        if self.goal != ENGINE_GOAL:
            if target == TARGET:
                return self._replace(state, engine_count=1)
            return state
        if target in ENGINE_NATIVE:
            _cost, target_type, perm = ENGINE_NATIVE[target]
            return self._add_engine(self._replace(state, battlefield=(*state.battlefield, perm)), target_type, target)
        if target in ENGINE_COPY_SPELLS:
            legal_targets = ENGINE_COPY_SPELLS[target][1]
            for target_type in sorted(set(state.engine_targets) & set(legal_targets)):
                return self._add_engine(self._replace(state, battlefield=(*state.battlefield, self._engine_copy_perm(target_type))), target_type, target)
        return state

    def _tutor_targets(self, tutor: str, state: State, top: bool = False) -> tuple[str, ...]:
        if tutor == "Mystical Tutor":
            return tuple(sorted(
                (c for c in MYSTICAL_TUTOR_TARGETS if c in state.library and self._tutor_can_use_target(c, state)),
                key=engine_target_priority,
            ))
        if tutor == "Worldly Tutor":
            candidates = WORLDLY_TUTOR_TARGETS
            if self.goal != ENGINE_GOAL:
                candidates = candidates - {"Heartwood Storyteller"}
            return tuple(sorted(
                (c for c in candidates if c in state.library and self._tutor_can_use_target(c, state)),
                key=engine_target_priority,
            ))
        if self.goal != ENGINE_GOAL:
            return (TARGET,) if TARGET in state.library else ()
        if tutor == "Idyllic Tutor":
            candidates = ENCHANTMENT_TUTOR_TARGETS
        elif tutor == "Enlightened Tutor":
            candidates = ENLIGHTENED_TARGETS
        else:
            candidates = set(ENGINE_NATIVE)
            if state.engine_count:
                candidates |= set(ENGINE_COPY_SPELLS)
        return tuple(sorted(
            (c for c in candidates if c in state.library and self._tutor_can_use_target(c, state)),
            key=engine_target_priority,
        ))

    def _beseech_targets(self, state: State) -> tuple[str, ...]:
        if self.goal != ENGINE_GOAL:
            return (TARGET,) if TARGET in state.library else ()
        candidates = set(ENGINE_NATIVE)
        if state.engine_count:
            candidates |= set(ENGINE_COPY_SPELLS)
        return tuple(sorted(
            (c for c in candidates if c in state.library and self._tutor_can_use_target(c, state)),
            key=engine_target_priority,
        ))

    def _tutor_can_use_target(self, target: str, state: State) -> bool:
        if target in ENGINE_COPY_SPELLS:
            return bool(set(state.engine_targets) & set(ENGINE_COPY_SPELLS[target][1]))
        return True

    def _gamble_targets(self, state: State, mana_after_cost: tuple[int, ...]) -> tuple[str, ...]:
        candidates = self._tutor_targets("Gamble", state)
        if self.gamble_mode != "stochastic" or len(candidates) <= 1:
            return candidates
        return (max(candidates, key=lambda target: self._gamble_target_score(target, mana_after_cost)),)

    def _gamble_target_score(self, target: str, mana_after_cost: tuple[int, ...]) -> tuple[int, int, int, int, str]:
        cost = self._gamble_target_cost(target)
        immediate = int(cost is not None and bool(pay_options(mana_after_cost, cost)))
        if cost is None:
            colored_shortage = 99
            total_shortage = 99
        else:
            colored_shortage = sum(max(0, cost[i + 1] - mana_after_cost[i]) for i in range(5))
            total_available = sum(mana_after_cost)
            total_required = sum(cost)
            total_shortage = max(0, total_required - total_available)
        priority = {
            TARGET: 4,
            "Heartwood Storyteller": 3,
            "Mystic Remora": 2,
            "Smothering Tithe": 1,
        }.get(target, 0)
        return (immediate, -colored_shortage, -total_shortage, priority, target)

    def _gamble_target_cost(self, target: str) -> tuple[int, int, int, int, int, int] | None:
        if target == TARGET:
            return RHYSTIC_COST
        if target in ENGINE_NATIVE:
            return ENGINE_NATIVE[target][0]
        if target in ENGINE_COPY_SPELLS:
            return ENGINE_COPY_SPELLS[target][0]
        return None

    def _gamble_discard(self, state: State, target: str, hand_after_search: tuple[str, ...]) -> str:
        if self.simplified_gamble:
            return self._simplified_gamble_discard(state, target, hand_after_search)
        payload = repr(
            (
                self.gamble_seed,
                state.turn,
                state.hand,
                state.battlefield,
                state.mana,
                target,
            )
        )
        rng = random.Random(payload)
        return hand_after_search[rng.randrange(len(hand_after_search))]

    def _simplified_gamble_discard(self, state: State, target: str, hand_after_search: tuple[str, ...]) -> str:
        hasher = hashlib.blake2b(digest_size=8)
        for part in ("gamble-v1", str(self.gamble_seed), str(state.turn), target):
            hasher.update(part.encode())
            hasher.update(b"\x1e")
        for card in hand_after_search:
            hasher.update(card.encode())
            hasher.update(b"\x1f")
        index = int.from_bytes(hasher.digest(), "big") % len(hand_after_search)
        return hand_after_search[index]

    def _led_target_state(self, state: State, target: str, mana) -> State | None:
        if target not in state.library:
            return None
        if self.goal == ENGINE_GOAL and target in ENGINE_NATIVE:
            cost, target_type, perm = ENGINE_NATIVE[target]
            for remaining in pay_options(mana, cost):
                ns = self._replace(state, hand=remove_card(state.hand, target) if target in state.hand else state.hand, library=obscure_library_top_after_shuffle(remove_card(state.library, target)), battlefield=(*state.battlefield, perm), mana=remaining)
                return self._add_engine(ns, target_type, target)
            return None
        if target == TARGET:
            ns = self._replace(state, hand=(TARGET,), library=obscure_library_top_after_shuffle(remove_card(state.library, TARGET)), mana=mana)
            if self.goal != ENGINE_GOAL:
                return ns
            return ns
        return self._replace(state, hand=(target,), library=obscure_library_top_after_shuffle(remove_card(state.library, target)), mana=mana)

    def _led_tutor_line(self, state: State, tutor: str, cost, creature_index: int | None, target: str):
        if "Lion's Eye Diamond" not in state.hand and not any(p.name == "LED" for p in state.battlefield):
            return
        # Only model LED already on battlefield. Cracking it in response to a tutor
        # discards the current hand; the tutor then resolves into the empty hand.
        for led_i, led in enumerate(state.battlefield):
            if led.name != "LED" or led.tapped:
                continue
            for mana_after_cost in pay_options(state.mana, cost):
                base = self._sac_creature(state, creature_index) if creature_index is not None else state
                bf = list(base.battlefield)
                if led_i >= len(bf) or bf[led_i].name != "LED":
                    continue
                bf.pop(led_i)
                for color in COLORS:
                    floated = add_mana(mana_after_cost, tuple(3 if c == color else 0 for c in COLORS) + (0,))
                    empty = self._replace(base, hand=(), battlefield=bf, mana=floated)
                    ns = self._led_target_state(empty, target, floated)
                    if ns is None:
                        continue
                    yield self._after_cast(base, ns), f"cast {tutor}, crack LED for {color}, tutor {target}"

    def _led_wishclaw_line(self, state: State, wishclaw_index: int, target: str):
        if not any(p.name == "LED" and not p.tapped for p in state.battlefield):
            return
        for mana_after_cost in pay_options(state.mana, (1, 0, 0, 0, 0, 0)):
            for led_i, led in enumerate(state.battlefield):
                if led.name != "LED" or led.tapped:
                    continue
                if wishclaw_index == led_i:
                    continue
                bf = list(state.battlefield)
                for idx in sorted((wishclaw_index, led_i), reverse=True):
                    if idx >= len(bf):
                        break
                    bf.pop(idx)
                else:
                    for color in COLORS:
                        floated = add_mana(mana_after_cost, tuple(3 if c == color else 0 for c in COLORS) + (0,))
                        empty = self._replace(state, hand=(), battlefield=bf, mana=floated)
                        ns = self._led_target_state(empty, target, floated)
                        if ns is None:
                            continue
                        yield ns, f"activate Wishclaw, crack LED for {color}, tutor {target}"

    def _led_beseech_line(self, state: State, target: str):
        if not any(p.name == "LED" and not p.tapped for p in state.battlefield):
            return
        for led_i, led in enumerate(state.battlefield):
            if led.name != "LED" or led.tapped:
                continue
            for mana_after_cost in pay_options(state.mana, (1, 3, 0, 0, 0, 0)):
                bf = list(state.battlefield)
                bf.pop(led_i)
                for color in COLORS:
                    floated = add_mana(mana_after_cost, tuple(3 if c == color else 0 for c in COLORS) + (0,))
                    empty = self._replace(state, hand=(), battlefield=bf, mana=floated)
                    ns = self._led_target_state(empty, target, floated)
                    if ns is None:
                        continue
                    yield self._after_cast(state, ns), f"cast Beseech the Mirror, crack LED for {color}, tutor {target}"

    def _pay_upkeep_pacts(self, state: State) -> Iterable[State]:
        if state.pact_debt <= 0:
            yield state
            return
        cost = (2 * state.pact_debt, 0, 0, 0, 0, 2 * state.pact_debt)
        start = self._normalize_upkeep_payment_state(state, cost)
        queue = deque([start])
        seen = {start: None}
        best_mana: dict[tuple, list[tuple[int, ...]]] = {}
        while queue:
            current = queue.pop()
            paid = False
            for _remaining in pay_options(current.mana, cost):
                yield self._replace(current, mana=(0, 0, 0, 0, 0, 0), pact_debt=0)
                paid = True
                break
            if paid:
                continue
            for next_state, action in self._upkeep_mana_actions(current):
                if action.startswith("attack Ragavan"):
                    continue
                next_state = self._normalize_upkeep_payment_state(next_state, cost)
                if next_state in seen or self._mana_dominated(next_state, best_mana):
                    continue
                if len(seen) >= min(self.state_limit, 4096):
                    continue
                seen[next_state] = None
                queue.append(next_state)

    def _can_survive_next_pact_upkeep(self, state: State) -> bool:
        if state.pact_debt <= 0:
            return True
        next_turn = self._begin_turn(self._end_turn(state))
        cache_key = (
            next_turn.battlefield,
            next_turn.land_grave_count,
            next_turn.mantle_attached,
            next_turn.nature_attached,
            next_turn.pact_debt,
        )
        cached = self._pact_survival_cache.get(cache_key)
        if cached is not None:
            return cached
        if self._can_pay_with_simple_taps(next_turn, (2 * next_turn.pact_debt, 0, 0, 0, 0, 2 * next_turn.pact_debt)):
            self._pact_survival_cache[cache_key] = True
            return True
        result = any(True for _state in self._pay_upkeep_pacts(next_turn))
        self._pact_survival_cache[cache_key] = result
        return result

    def _can_pay_with_simple_taps(self, state: State, cost: tuple[int, int, int, int, int, int]) -> bool:
        mana_options: set[tuple[int, int, int, int, int, int]] = {(0, 0, 0, 0, 0, 0)}
        for perm in state.battlefield:
            if perm.tapped:
                continue
            additions: set[tuple[int, int, int, int, int, int]] = set()
            for opt in self._tap_options(perm, state):
                if opt == "PETAL":
                    continue
                if opt == "VAULT":
                    additions.add((0, 0, 0, 0, 0, 3))
                elif opt in COLORS:
                    additions.add(mana_for_color(opt))
                else:
                    additions.add(mana_for_option(opt))
            if not additions:
                continue
            updated = set(mana_options)
            for current in mana_options:
                for add in additions:
                    candidate = add_mana(current, add)
                    if pay_options(candidate, cost):
                        return True
                    updated.add(candidate)
            mana_options = updated
        return any(pay_options(mana, cost) for mana in mana_options)

    def _can_keep_remora(self, state: State, payments: int) -> bool:
        if payments <= 0:
            return True
        cache_key = (state, payments)
        cached = self._remora_keep_cache.get(cache_key)
        if cached is not None:
            return cached
        states: dict[State, None] = {self._end_turn(state): None}
        for amount in range(1, payments + 1):
            paid_states: dict[State, None] = {}
            for state_at_end in states:
                begun = self._begin_turn(state_at_end)
                begun = self._replace(begun, turn=state_at_end.turn + 1)
                for paid in self._pay_generic_upkeep_options(begun, amount):
                    paid_states[paid] = None
            if not paid_states:
                self._remora_keep_cache[cache_key] = False
                return False
            if amount == payments:
                self._remora_keep_cache[cache_key] = True
                return True
            setup_states: dict[State, None] = {}
            for paid in paid_states:
                for setup in self._future_visible_setup_states(paid):
                    setup_states[self._end_turn(setup)] = None
            states = setup_states
        self._remora_keep_cache[cache_key] = False
        return False

    def _pay_generic_upkeep_options(self, state: State, amount: int) -> Iterable[State]:
        cost = (amount, 0, 0, 0, 0, 0)
        start = self._normalize_upkeep_payment_state(state, cost)
        queue = deque([start])
        seen = {start: None}
        best_mana: dict[tuple, list[tuple[int, ...]]] = {}
        while queue:
            current = queue.pop()
            paid = False
            for _remaining in pay_options(current.mana, cost):
                yield self._replace(current, mana=(0, 0, 0, 0, 0, 0))
                paid = True
                break
            if paid:
                continue
            for next_state, action in self._upkeep_mana_actions(current):
                if action.startswith("attack Ragavan"):
                    continue
                next_state = self._normalize_upkeep_payment_state(next_state, cost)
                if next_state in seen or self._mana_dominated(next_state, best_mana):
                    continue
                if len(seen) >= min(self.state_limit, 2048):
                    continue
                seen[next_state] = None
                queue.append(next_state)

    def _future_visible_setup_states(self, state: State) -> Iterable[State]:
        yield state
        if state.land_played:
            return
        for card in sorted(c for c in set(state.hand) if c in LANDS or c in MDFC_LANDS or is_theoretical_rainbow_land(c)):
            for perm, library, grave_inc, label in self._land_options(card, state.library):
                battlefield = [p for p in state.battlefield if p.name != "CITY"]
                yield self._replace(
                    state,
                    hand=remove_card(state.hand, card),
                    library=library,
                    battlefield=(*battlefield, perm),
                    land_played=True,
                    land_grave_count=state.land_grave_count + grave_inc,
                )

    def _upkeep_mana_actions(self, state: State):
        yield from self._mana_actions(state)
        for i, perm in enumerate(state.battlefield):
            if perm.name != "LED" or perm.tapped:
                continue
            for color in COLORS:
                bf = list(state.battlefield)
                bf.pop(i)
                yield (
                    self._replace(state, battlefield=bf, hand=(), mana=add_mana(state.mana, tuple(3 if c == color else 0 for c in COLORS) + (0,))),
                    f"sac Lion's Eye Diamond for {color}",
                )

    def _normalize_upkeep_payment_state(self, state: State, cost: tuple[int, int, int, int, int, int]) -> State:
        generic, black, red, blue, white, green = cost
        if black or red or blue or white:
            return state
        b, r, u, w, g, c = state.mana
        if green:
            non_green = min(generic, b + r + u + w + c)
            useful_green = min(generic + green, g)
            mana = (non_green, 0, 0, 0, useful_green, 0)
        else:
            mana = (min(generic, b + r + u + w + g + c), 0, 0, 0, 0, 0)
        if mana == state.mana:
            return state
        return self._replace(state, mana=mana)

    def _mana_actions(self, state: State):
        for i, perm in enumerate(state.battlefield):
            if not perm.tapped:
                for opt in self._tap_options(perm, state):
                    bf = list(state.battlefield)
                    if perm.name == "MINE":
                        counters = int(perm.extra or "0")
                        if counters <= 1:
                            bf.pop(i)
                        else:
                            bf[i] = Perm(perm.name, True, str(counters - 1))
                        yield self._replace(state, battlefield=bf, mana=add_mana(state.mana, mana_for_color(opt))), f"tap Gemstone Mine for {opt}"
                        continue
                    if opt == "PETAL":
                        for color in COLORS:
                            bf2 = list(state.battlefield)
                            bf2.pop(i)
                            yield self._replace(state, battlefield=bf2, mana=add_mana(state.mana, mana_for_color(color))), f"sac {perm.name} for {color}"
                        continue
                    if opt == "VAULT":
                        bf[i] = Perm(perm.name, True, perm.extra)
                        yield self._replace(state, battlefield=bf, mana=add_mana(state.mana, (0, 0, 0, 0, 0, 3))), "tap Mana Vault"
                        continue
                    bf[i] = Perm(perm.name, True, perm.extra)
                    if perm.name == "DEATHRITE":
                        yield self._replace(state, battlefield=bf, land_grave_count=max(0, state.land_grave_count - 1), mana=add_mana(state.mana, mana_for_color(opt))), f"tap Deathrite for {opt}"
                    else:
                        yield self._replace(state, battlefield=bf, mana=add_mana(state.mana, mana_for_option(opt))), f"tap {perm.name} for {opt}"

                if perm.name == "TOWER":
                    for creature_i in self._unique_creatures(state):
                        if creature_i == i:
                            continue
                        bf = list(state.battlefield)
                        if creature_i >= len(bf) or i >= len(bf):
                            continue
                        bf[i] = Perm(perm.name, True, perm.extra)
                        creature = bf.pop(creature_i)
                        yield self._replace(state, battlefield=bf, mana=add_mana(state.mana, (2, 0, 0, 0, 0, 0))), f"tap Phyrexian Tower sacrificing {creature.name}"
                if perm.name == "VEIN":
                    bf = list(state.battlefield)
                    bf.pop(i)
                    yield self._replace(state, battlefield=bf, land_grave_count=state.land_grave_count + 1, mana=add_mana(state.mana, (0, 0, 0, 0, 0, 2))), "sac Crystal Vein for CC"

            if perm.name in {"PETAL", "TREASURE"} and not perm.tapped:
                for color in COLORS:
                    bf = list(state.battlefield)
                    bf.pop(i)
                    label = "Lotus Petal" if perm.name == "PETAL" else "Treasure"
                    yield self._replace(state, battlefield=bf, mana=add_mana(state.mana, mana_for_color(color))), f"sac {label} for {color}"

            if perm.name == "TINDER":
                bf = list(state.battlefield)
                bf.pop(i)
                yield self._replace(state, battlefield=bf, mana=add_mana(state.mana, (0, 2, 0, 0, 0, 0))), "sac Tinder Wall"

            if perm.name == "CANTOR":
                for color in COLORS:
                    bf = list(state.battlefield)
                    bf.pop(i)
                    yield self._replace(state, battlefield=bf, mana=add_mana(state.mana, mana_for_color(color))), f"sac Wild Cantor for {color}"

            if perm.name == "RAGAVAN" and not perm.tapped and not perm.extra.endswith("*"):
                bf = list(state.battlefield)
                bf[i] = Perm(perm.name, True, perm.extra)
                yield self._replace(state, battlefield=(*bf, Perm("TREASURE"))), "attack Ragavan, connect, create Treasure"

            if state.rain_active and self._is_land_perm(perm):
                bf = list(state.battlefield)
                bf.pop(i)
                yield self._replace(state, battlefield=bf, land_grave_count=state.land_grave_count + 1, mana=add_mana(state.mana, (1, 0, 0, 0, 0, 0))), f"sac {perm.name} to Rain of Filth"

        if any(p.name == "DRUM" and not p.tapped for p in state.battlefield):
            for drum_i, drum in enumerate(state.battlefield):
                if drum.name != "DRUM" or drum.tapped:
                    continue
                for creature_i in self._unique_creatures(state, untapped=True):
                    for color in COLORS:
                        bf = list(state.battlefield)
                        bf[drum_i] = Perm(drum.name, True, drum.extra)
                        creature = bf[creature_i]
                        bf[creature_i] = Perm(creature.name, True, creature.extra)
                        yield self._replace(state, battlefield=bf, mana=add_mana(state.mana, mana_for_color(color))), f"tap Springleaf Drum for {color}"

        for relic_i, relic in enumerate(state.battlefield):
            if relic.name == "RELIC" and not relic.tapped:
                for color in COLORS:
                    bf = list(state.battlefield)
                    bf[relic_i] = Perm(relic.name, True, relic.extra)
                    yield self._replace(state, battlefield=bf, mana=add_mana(state.mana, mana_for_color(color))), f"tap Relic for {color}"

        if any(p.name == "RELIC" for p in state.battlefield):
            for relic_i, relic in enumerate(state.battlefield):
                if relic.name != "RELIC":
                    continue
                for creature_i in self._unique_creatures(state, untapped=True, legendary=True):
                    for color in COLORS:
                        bf = list(state.battlefield)
                        creature = bf[creature_i]
                        bf[creature_i] = Perm(creature.name, True, creature.extra)
                        yield self._replace(state, battlefield=bf, mana=add_mana(state.mana, mana_for_color(color))), f"tap legendary for Relic {color}"

        if state.nature_attached and any(p.name == "NATURE" for p in state.battlefield):
            if not state.nature_untap_used:
                for creature_i in self._unique_creatures(state):
                    creature = state.battlefield[creature_i]
                    if not creature.tapped or self._mantle_key(creature) != state.nature_attached:
                        continue
                    bf = list(state.battlefield)
                    bf[creature_i] = Perm(creature.name, False, creature.extra)
                    yield self._replace(state, battlefield=bf, nature_untap_used=True), f"activate Nature's Chosen untap {creature.name}"

            if not state.nature_tap_used:
                for creature_i in self._unique_creatures(state, untapped=True):
                    creature = state.battlefield[creature_i]
                    if self._mantle_key(creature) != state.nature_attached:
                        continue
                    if "W" not in creature.extra.removesuffix("*"):
                        continue
                    for target_i, target in enumerate(state.battlefield):
                        if not target.tapped:
                            continue
                        if not (self._is_artifact(target) or self._is_creature(target) or self._is_land_perm(target)):
                            continue
                        bf = list(state.battlefield)
                        bf[creature_i] = Perm(creature.name, True, creature.extra)
                        bf[target_i] = Perm(target.name, False, target.extra)
                        yield self._replace(state, battlefield=bf, nature_tap_used=True), f"tap Nature's Chosen enchanted {creature.name} to untap {target.name}"

        if state.mantle_attached and any(p.name == "MANTLE" for p in state.battlefield):
            for creature_i in self._unique_creatures(state, untapped=True):
                creature = state.battlefield[creature_i]
                if creature.extra.endswith("*") or self._mantle_key(creature) != state.mantle_attached:
                    continue
                for color in COLORS:
                    bf = list(state.battlefield)
                    bf[creature_i] = Perm(creature.name, True, creature.extra)
                    yield self._replace(state, battlefield=bf, mana=add_mana(state.mana, mana_for_color(color))), f"tap Paradise Mantle for {color}"

    def _tap_options(self, perm: Perm, state: State) -> tuple[str, ...]:
        if perm.name == "LAND":
            return tuple(perm.extra)
        if perm.name == "CAVERN":
            return tuple(COLORS)
        if perm.name == "MINE":
            return tuple(COLORS)
        if perm.name == "GLIMMER":
            return tuple(COLORS)
        if perm.name == "CCLAND" or perm.name == "CITY":
            return ("CC",)
        if perm.name == "VEIN":
            return ("C",)
        if perm.name in {"PETAL", "TREASURE"}:
            return ()
        if perm.name == "AMBER":
            colors = set()
            for p in state.battlefield:
                if self._is_legendary(p):
                    colors.update(p.extra.removesuffix("*"))
            return tuple(c for c in COLORS if c in colors)
        if perm.name == "CHROME":
            return tuple(perm.extra)
        if perm.name in {"DIAMOND", "OPAL", "SIGNET"}:
            if perm.name == "OPAL" and self._artifact_count(state) < 3:
                return ()
            return tuple(COLORS if perm.name != "SIGNET" else self.identity)
        if perm.name == "SOL":
            return ("CC",)
        if perm.name == "VAULT":
            return ("VAULT",)
        if perm.name == "BIRD" and not perm.extra.endswith("*"):
            return tuple(COLORS)
        if perm.name == "NOBLE" and not perm.extra.endswith("*"):
            return tuple("UWG")
        if perm.name == "IGNOBLE" and not perm.extra.endswith("*"):
            return tuple("BRG")
        if perm.name == "TINDER":
            return ()
        if perm.name == "CANTOR":
            return ()
        if perm.name == "DEATHRITE" and not perm.extra.endswith("*"):
            return tuple(COLORS)
        return ()

    def _land_options(self, card: str, library: tuple[str, ...], already_removed: bool = False):
        if card in MDFC_LANDS:
            yield Perm("LAND", card not in UNTAPPED_MDFC_LANDS, MDFC_LANDS[card]), library, 0, " as land"
            return
        if card in FETCH_TYPES:
            lib = library if already_removed else library
            for target in dict.fromkeys(lib):
                colors = self._fetch_colors(card, target)
                if not colors:
                    continue
                yield Perm("LAND", False, colors), obscure_library_top_after_shuffle(remove_card(lib, target)), 1, f" fetch {target}"
            return
        if card in CC_LANDS:
            yield Perm("CCLAND" if card == "Ancient Tomb" else "CITY"), library, 0, ""
        elif card == "Crystal Vein":
            yield Perm("VEIN", False, "C"), library, 0, ""
        elif card == "Phyrexian Tower":
            yield Perm("TOWER", False, "C"), library, 0, ""
        elif card == "Glimmervoid":
            yield Perm("GLIMMER", False, COLORS), library, 0, ""
        elif card == "Gemstone Mine":
            yield Perm("MINE", False, "3"), library, 0, ""
        elif card in ANY_COLOR_LANDS or is_theoretical_rainbow_land(card):
            yield Perm("LAND", False, COLORS), library, 0, ""
        elif card in COLOR_LANDS:
            yield Perm("LAND", False, COLOR_LANDS[card]), library, 0, ""
        elif card in COLORLESS_LANDS:
            yield Perm("LAND", False, "C"), library, 0, ""
        elif card in LAND_TYPES:
            colors = "".join(c for c in COLORS if c in {LAND_COLOR_BY_TYPE[t] for t in LAND_TYPES[card]})
            yield Perm("LAND", False, colors), library, 0, ""
        else:
            yield Perm("LAND", False, "C"), library, 0, ""

    def _fetch_colors(self, fetch: str, target: str) -> str:
        if target not in LAND_TYPES:
            return ""
        if not (FETCH_TYPES[fetch] & LAND_TYPES[target]):
            return ""
        return "".join(c for c in COLORS if c in {LAND_COLOR_BY_TYPE[t] for t in LAND_TYPES[target]})

    def _creature_perm(self, card: str) -> Perm:
        mapping = {
            "Birds of Paradise": "BIRD",
            "Deathrite Shaman": "DEATHRITE",
            "Esper Sentinel": "ESPER",
            "Heartwood Storyteller": "HEARTWOOD",
            "Ignoble Hierarch": "IGNOBLE",
            "Noble Hierarch": "NOBLE",
            "Tinder Wall": "TINDER",
            "Wild Cantor": "CANTOR",
            "Tataru Taru": "TATARU",
            "Ragavan, Nimble Pilferer": "RAGAVAN",
            "Lotho, Corrupt Shirriff": "LOTHO",
            "Birgi, God of Storytelling": "BIRGI",
            "Wan Shi Tong, Librarian": "WAN",
        }
        return Perm(mapping.get(card, "CREATURE"), False, CARD_COLORS.get(card, "") + "*")

    def _after_cast(self, before: State, after: State) -> State:
        spells = before.spells_this_turn + 1
        if spells > 5:
            spells = 5
        mana = after.mana
        if any(p.name == "BIRGI" for p in before.battlefield) and any(p.name == "BIRGI" for p in after.battlefield):
            mana = add_mana(mana, (0, 1, 0, 0, 0, 0))
        battlefield = after.battlefield
        if before.spells_this_turn == 1 and any(p.name == "LOTHO" for p in before.battlefield):
            battlefield = norm_battlefield((*battlefield, Perm("TREASURE")))
        return State(
            after.hand,
            after.library,
            battlefield,
            mana,
            after.land_played,
            after.land_grave_count,
            after.mantle_attached,
            after.nature_attached,
            after.nature_untap_used,
            after.nature_tap_used,
            after.rain_active,
            spells,
            after.pact_debt,
            after.turn,
            after.engine_count,
            after.engine_targets,
            after.engine_names,
        )

    def _is_creature(self, perm: Perm) -> bool:
        return perm.name in CREATURE_PERM_NAMES

    def _is_legendary(self, perm: Perm) -> bool:
        return perm.name in LEGENDARY_PERM_NAMES

    def _is_artifact(self, perm: Perm) -> bool:
        return perm.name in ARTIFACT_PERM_NAMES

    def _is_enchantment(self, perm: Perm) -> bool:
        return perm.name in {"ENGINE_ENCH", "NATURE"}

    def _artifact_count(self, state: State) -> int:
        return sum(1 for p in state.battlefield if self._is_artifact(p))

    def _is_land_perm(self, perm: Perm) -> bool:
        return perm.name in LAND_PERM_NAMES

    def _unique_creatures(self, state: State, untapped: bool = False, legendary: bool = False) -> list[int]:
        out = []
        seen = set()
        for i, p in enumerate(state.battlefield):
            if not self._is_creature(p):
                continue
            if untapped and p.tapped:
                continue
            if legendary and not self._is_legendary(p):
                continue
            key = (p.name, p.tapped, p.extra)
            if key in seen:
                continue
            seen.add(key)
            out.append(i)
        return out

    def _mantle_key(self, perm: Perm) -> tuple[str, str]:
        return (perm.name, perm.extra.removesuffix("*"))

    def _sac_creature(self, state: State, idx: int) -> State:
        bf = list(state.battlefield)
        bf.pop(idx)
        return self._replace(state, battlefield=bf)

    def _sac_land(self, state: State, idx: int) -> State:
        bf = list(state.battlefield)
        bf.pop(idx)
        return self._replace(state, battlefield=bf, land_grave_count=state.land_grave_count + 1)

    def _sac_perm(self, state: State, idx: int) -> State:
        bf = list(state.battlefield)
        bf.pop(idx)
        return self._replace(state, battlefield=bf)

    def _structural_key(self, state: State):
        library = state.library
        if state.turn >= self.max_turns and not self._library_order_matters(state):
            library = canonical_library(library)
        return (
            state.hand,
            library,
            state.battlefield,
            state.land_played,
            state.land_grave_count,
            state.mantle_attached,
            state.nature_attached,
            state.nature_untap_used,
            state.nature_tap_used,
            state.rain_active,
            state.spells_this_turn,
            state.pact_debt,
            state.turn,
            state.engine_count,
            state.engine_targets,
            state.engine_names,
        )

    def _library_order_matters(self, state: State) -> bool:
        return library_order_matters_for_hand(state.hand)

    def _mana_dominated(self, state: State, best_mana) -> bool:
        key = self._structural_key(state)
        state_mana = state.mana
        existing = best_mana.get(key)
        if existing is None:
            best_mana[key] = [state_mana]
            return False
        for mana in existing:
            if (
                mana[0] >= state_mana[0]
                and mana[1] >= state_mana[1]
                and mana[2] >= state_mana[2]
                and mana[3] >= state_mana[3]
                and mana[4] >= state_mana[4]
                and mana[5] >= state_mana[5]
            ):
                return True
        existing[:] = [
            mana
            for mana in existing
            if not (
                state_mana[0] >= mana[0]
                and state_mana[1] >= mana[1]
                and state_mana[2] >= mana[2]
                and state_mana[3] >= mana[3]
                and state_mana[4] >= mana[4]
                and state_mana[5] >= mana[5]
            )
        ]
        existing.append(state_mana)
        return False

    def _replace(self, state: State, **kwargs) -> State:
        if len(kwargs) == 1:
            if "mana" in kwargs:
                mana = kwargs["mana"]
                return State(
                    state.hand,
                    state.library,
                    state.battlefield,
                    cap_mana(mana),
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "battlefield" in kwargs:
                return State(
                    state.hand,
                    state.library,
                    norm_battlefield(kwargs["battlefield"]),
                    state.mana,
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "hand" in kwargs:
                return State(
                    norm(kwargs["hand"]),
                    state.library,
                    state.battlefield,
                    state.mana,
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "turn" in kwargs:
                turn = kwargs["turn"]
                if turn < 0:
                    turn = 0
                elif turn > 8:
                    turn = 8
                return State(
                    state.hand,
                    state.library,
                    state.battlefield,
                    state.mana,
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "spells_this_turn" in kwargs:
                spells_this_turn = kwargs["spells_this_turn"]
                if spells_this_turn > 5:
                    spells_this_turn = 5
                return State(
                    state.hand,
                    state.library,
                    state.battlefield,
                    state.mana,
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "engine_count" in kwargs:
                engine_count = kwargs["engine_count"]
                if engine_count < 0:
                    engine_count = 0
                elif engine_count > 3:
                    engine_count = 3
                return State(
                    state.hand,
                    state.library,
                    state.battlefield,
                    state.mana,
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
        elif len(kwargs) == 2:
            if "battlefield" in kwargs and "mana" in kwargs:
                return State(
                    state.hand,
                    state.library,
                    norm_battlefield(kwargs["battlefield"]),
                    cap_mana(kwargs["mana"]),
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "hand" in kwargs and "mana" in kwargs:
                return State(
                    norm(kwargs["hand"]),
                    state.library,
                    state.battlefield,
                    cap_mana(kwargs["mana"]),
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "battlefield" in kwargs and "hand" in kwargs:
                return State(
                    norm(kwargs["hand"]),
                    state.library,
                    norm_battlefield(kwargs["battlefield"]),
                    state.mana,
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "hand" in kwargs and "library" in kwargs:
                return State(
                    norm(kwargs["hand"]),
                    kwargs["library"],
                    state.battlefield,
                    state.mana,
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "battlefield" in kwargs and "land_grave_count" in kwargs:
                land_grave_count = kwargs["land_grave_count"]
                if land_grave_count < 0:
                    land_grave_count = 0
                elif land_grave_count > 4:
                    land_grave_count = 4
                return State(
                    state.hand,
                    state.library,
                    norm_battlefield(kwargs["battlefield"]),
                    state.mana,
                    state.land_played,
                    land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
        elif len(kwargs) == 3:
            if "battlefield" in kwargs and "hand" in kwargs and "mana" in kwargs:
                return State(
                    norm(kwargs["hand"]),
                    state.library,
                    norm_battlefield(kwargs["battlefield"]),
                    cap_mana(kwargs["mana"]),
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "hand" in kwargs and "library" in kwargs and "mana" in kwargs:
                return State(
                    norm(kwargs["hand"]),
                    kwargs["library"],
                    state.battlefield,
                    cap_mana(kwargs["mana"]),
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "battlefield" in kwargs and "land_grave_count" in kwargs and "mana" in kwargs:
                land_grave_count = kwargs["land_grave_count"]
                if land_grave_count < 0:
                    land_grave_count = 0
                elif land_grave_count > 4:
                    land_grave_count = 4
                return State(
                    state.hand,
                    state.library,
                    norm_battlefield(kwargs["battlefield"]),
                    cap_mana(kwargs["mana"]),
                    state.land_played,
                    land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "hand" in kwargs and "library" in kwargs and "pact_debt" in kwargs:
                pact_debt = kwargs["pact_debt"]
                if pact_debt < 0:
                    pact_debt = 0
                elif pact_debt > 2:
                    pact_debt = 2
                return State(
                    norm(kwargs["hand"]),
                    kwargs["library"],
                    state.battlefield,
                    state.mana,
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "battlefield" in kwargs and "hand" in kwargs and "land_grave_count" in kwargs:
                land_grave_count = kwargs["land_grave_count"]
                if land_grave_count < 0:
                    land_grave_count = 0
                elif land_grave_count > 4:
                    land_grave_count = 4
                return State(
                    norm(kwargs["hand"]),
                    state.library,
                    norm_battlefield(kwargs["battlefield"]),
                    state.mana,
                    state.land_played,
                    land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if "hand" in kwargs and "mana" in kwargs and "rain_active" in kwargs:
                return State(
                    norm(kwargs["hand"]),
                    state.library,
                    state.battlefield,
                    cap_mana(kwargs["mana"]),
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    kwargs["rain_active"],
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
        elif len(kwargs) == 4:
            if "battlefield" in kwargs and "hand" in kwargs and "library" in kwargs and "mana" in kwargs:
                return State(
                    norm(kwargs["hand"]),
                    kwargs["library"],
                    norm_battlefield(kwargs["battlefield"]),
                    cap_mana(kwargs["mana"]),
                    state.land_played,
                    state.land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
        elif len(kwargs) == 5:
            if (
                "battlefield" in kwargs
                and "hand" in kwargs
                and "land_grave_count" in kwargs
                and "land_played" in kwargs
                and "library" in kwargs
            ):
                land_grave_count = kwargs["land_grave_count"]
                if land_grave_count < 0:
                    land_grave_count = 0
                elif land_grave_count > 4:
                    land_grave_count = 4
                return State(
                    norm(kwargs["hand"]),
                    kwargs["library"],
                    norm_battlefield(kwargs["battlefield"]),
                    state.mana,
                    kwargs["land_played"],
                    land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
            if (
                "battlefield" in kwargs
                and "hand" in kwargs
                and "land_grave_count" in kwargs
                and "library" in kwargs
                and "mana" in kwargs
            ):
                land_grave_count = kwargs["land_grave_count"]
                if land_grave_count < 0:
                    land_grave_count = 0
                elif land_grave_count > 4:
                    land_grave_count = 4
                return State(
                    norm(kwargs["hand"]),
                    kwargs["library"],
                    norm_battlefield(kwargs["battlefield"]),
                    cap_mana(kwargs["mana"]),
                    state.land_played,
                    land_grave_count,
                    state.mantle_attached,
                    state.nature_attached,
                    state.nature_untap_used,
                    state.nature_tap_used,
                    state.rain_active,
                    state.spells_this_turn,
                    state.pact_debt,
                    state.turn,
                    state.engine_count,
                    state.engine_targets,
                    state.engine_names,
                )
        hand_changed = "hand" in kwargs
        battlefield_changed = "battlefield" in kwargs
        mana_changed = "mana" in kwargs
        engine_targets_changed = "engine_targets" in kwargs
        engine_names_changed = "engine_names" in kwargs

        hand = kwargs["hand"] if hand_changed else state.hand
        library = kwargs.get("library", state.library)
        battlefield = kwargs["battlefield"] if battlefield_changed else state.battlefield
        mana = kwargs["mana"] if mana_changed else state.mana
        land_played = kwargs.get("land_played", state.land_played)
        land_grave_count = kwargs.get("land_grave_count", state.land_grave_count)
        mantle_attached = kwargs.get("mantle_attached", state.mantle_attached)
        nature_attached = kwargs.get("nature_attached", state.nature_attached)
        nature_untap_used = kwargs.get("nature_untap_used", state.nature_untap_used)
        nature_tap_used = kwargs.get("nature_tap_used", state.nature_tap_used)
        rain_active = kwargs.get("rain_active", state.rain_active)
        spells_this_turn = kwargs.get("spells_this_turn", state.spells_this_turn)
        pact_debt = kwargs.get("pact_debt", state.pact_debt)
        turn = kwargs.get("turn", state.turn)
        engine_count = kwargs.get("engine_count", state.engine_count)
        engine_targets = kwargs["engine_targets"] if engine_targets_changed else state.engine_targets
        engine_names = kwargs["engine_names"] if engine_names_changed else state.engine_names

        if hand_changed:
            hand = norm(hand)
        if battlefield_changed:
            battlefield = norm_battlefield(battlefield)
        if mana_changed:
            mana = cap_mana(mana)
        if land_grave_count < 0:
            land_grave_count = 0
        elif land_grave_count > 4:
            land_grave_count = 4
        if spells_this_turn > 5:
            spells_this_turn = 5
        if pact_debt < 0:
            pact_debt = 0
        elif pact_debt > 2:
            pact_debt = 2
        if turn < 0:
            turn = 0
        elif turn > 8:
            turn = 8
        if engine_count < 0:
            engine_count = 0
        elif engine_count > 3:
            engine_count = 3
        if engine_targets_changed:
            engine_targets = tuple(sorted(set(engine_targets)))
        if engine_names_changed:
            engine_names = tuple(sorted(set(engine_names)))
        return State(
            hand,
            library,
            battlefield,
            mana,
            land_played,
            land_grave_count,
            mantle_attached,
            nature_attached,
            nature_untap_used,
            nature_tap_used,
            rain_active,
            spells_this_turn,
            pact_debt,
            turn,
            engine_count,
            engine_targets,
            engine_names,
        )


def wilson(k: int, n: int, z: float = 1.959963984540054) -> tuple[float, float]:
    if n == 0:
        return 0.0, 0.0
    ph = k / n
    den = 1 + z * z / n
    center = (ph + z * z / (2 * n)) / den
    half = z * math.sqrt(ph * (1 - ph) / n + z * z / (4 * n * n)) / den
    return center - half, center + half


def run(
    deck_key: str,
    trials: int,
    seed: int,
    max_turns: int,
    state_limit: int,
    bottom_counts: list[int],
    optimistic_gamble: bool,
    goal: str = "rhystic",
    engine_target_count: int = 1,
    engine_success_policy: str = "count",
    remora_upkeep_payments: int = 2,
    gamble_mode: str = "off",
    rust_action_mode: str | None = None,
    rust_close_mode: str | None = None,
    rust_solver_mode: str | None = None,
    rust_action_bin: str | None = None,
):
    rng = random.Random(seed)
    search = RhysticSearch(
        deck_key,
        max_turns=max_turns,
        state_limit=state_limit,
        optimistic_gamble=optimistic_gamble,
        gamble_mode=gamble_mode,
        goal=goal,
        engine_target_count=engine_target_count,
        engine_success_policy=engine_success_policy,
        remora_upkeep_payments=remora_upkeep_payments,
        rust_action_mode=rust_action_mode,
        rust_close_mode=rust_close_mode,
        rust_solver_mode=rust_solver_mode,
        rust_action_bin=rust_action_bin,
    )
    try:
        results = {}
        for bottom_count in bottom_counts:
            counts = Counter()
            truncated = 0
            for _ in range(trials):
                deck = list(search.mainboard)
                rng.shuffle(deck)
                turn, hit_limit = search.earliest(deck, bottom_count, gamble_seed=rng.randrange(1 << 63))
                counts[str(turn or "miss")] += 1
                truncated += int(hit_limit)
            cumulative = {}
            hits = 0
            ci = {}
            for turn in range(1, max_turns + 1):
                hits += counts[str(turn)]
                cumulative[f"turn_{turn}"] = hits / trials
                ci[f"turn_{turn}"] = wilson(hits, trials)
            results[str(bottom_count)] = {
                "bottom_count": bottom_count,
                "kept_cards": 7 - bottom_count,
                "counts": dict(counts),
                "cumulative": cumulative,
                "ci95": ci,
                "truncated": truncated,
            }
        return {
            "deck_key": deck_key,
            "deck_name": "Experimental 5c Fury Control - COMPY SPLIT SECOND VARIANT" if deck_key == "fury" else "Trigger Farm",
            "commanders": search.commanders,
            "mainboard_count": len(search.mainboard),
            "target": TARGET,
            "trials_per_mulligan": trials,
            "seed": seed,
            "max_turns": max_turns,
            "state_limit": state_limit,
            "optimistic_gamble": optimistic_gamble,
            "gamble_mode": search.gamble_mode,
            "goal": goal,
            "engine_target_count": engine_target_count,
            "engine_success_policy": engine_success_policy,
            "remora_upkeep_payments": remora_upkeep_payments,
            "rust_action_mode": search.rust_action_mode,
            "rust_close_mode": search.rust_close_mode,
            "rust_solver_mode": search.rust_solver_mode,
            "rust_action_bin": str(search._rust_action_accelerator.bin_path)
            if search._rust_action_accelerator
            else str(search._rust_close_accelerator.bin_path)
            if search._rust_close_accelerator
            else str(search._rust_solver_accelerator.bin_path)
            if search._rust_solver_accelerator
            else None,
            "assumptions": [
                "Multiplayer draw on turn 1 is included.",
                "London mulligan is modeled as draw 7 then optimally bottom N cards for each bottom_count.",
                "Gemstone Caverns/Glittering Caves pregame luck is controlled by --gemstone-caverns-live-rate; without a luck counter it taps for colorless.",
                "Exotic Orchard is treated as able to produce all commander colors.",
                "Gamble mode is controlled by --gamble-mode: off for conservative floor, stochastic for sampled legal random discard, optimistic for a ceiling.",
            ],
            "results": results,
        }
    finally:
        search.close()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--deck", choices=sorted(DECKS), required=True)
    parser.add_argument("--trials", type=int, default=10000)
    parser.add_argument("--seed", type=int, default=20260623)
    parser.add_argument("--max-turns", type=int, default=2)
    parser.add_argument("--state-limit", type=int, default=200000)
    parser.add_argument("--bottom-counts", default="0,1,2,3")
    parser.add_argument("--optimistic-gamble", action="store_true")
    parser.add_argument("--gamble-mode", choices=("off", "optimistic", "stochastic"), default="off")
    parser.add_argument("--goal", choices=["rhystic", ENGINE_GOAL], default="rhystic")
    parser.add_argument("--engine-target-count", type=int, default=1)
    parser.add_argument("--engine-success-policy", choices=["count", "resilient"], default="count")
    parser.add_argument("--remora-upkeep-payments", type=int, default=2)
    parser.add_argument("--rust-action-mode", choices=["off", "verify", "require"], default=None)
    parser.add_argument("--rust-close-mode", choices=["off", "verify", "require"], default=None)
    parser.add_argument("--rust-solver-mode", choices=["off", "verify", "require"], default=None)
    parser.add_argument("--rust-action-bin", default=None)
    parser.add_argument("--json-out")
    args = parser.parse_args()
    result = run(
        args.deck,
        args.trials,
        args.seed,
        args.max_turns,
        args.state_limit,
        [int(x) for x in args.bottom_counts.split(",") if x.strip()],
        args.optimistic_gamble,
        args.goal,
        args.engine_target_count,
        args.engine_success_policy,
        args.remora_upkeep_payments,
        args.gamble_mode,
        args.rust_action_mode,
        args.rust_close_mode,
        args.rust_solver_mode,
        args.rust_action_bin,
    )
    text = json.dumps(result, indent=2, sort_keys=True)
    print(text)
    if args.json_out:
        with open(args.json_out, "w") as handle:
            handle.write(text + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
