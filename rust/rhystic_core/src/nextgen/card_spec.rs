use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use super::{CardMask, SlotId, MAX_DECK_SLOTS};
use crate::fast_engine::{
    card_color_mask, is_artifact_card_name, is_fetch_name, is_land_card_name, is_mdfc_land_name,
};

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardFlags(u32);

impl CardFlags {
    pub const LAND: Self = Self(1 << 0);
    pub const MDFC_LAND: Self = Self(1 << 1);
    pub const FETCH: Self = Self(1 << 2);
    pub const ARTIFACT: Self = Self(1 << 3);
    pub const CREATURE: Self = Self(1 << 4);
    pub const MANA: Self = Self(1 << 5);
    pub const TUTOR: Self = Self(1 << 6);
    pub const ENGINE: Self = Self(1 << 7);
    pub const INTERACTION: Self = Self(1 << 8);

    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }

    fn insert(&mut self, flag: Self) {
        self.0 |= flag.0;
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ActionClass {
    Land,
    Mana,
    Tutor,
    Engine,
    Interaction,
    Value,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionTemplateMask(u32);

impl ActionTemplateMask {
    pub const ENGINE: Self = Self(1 << 0);
    pub const LAND: Self = Self(1 << 1);
    pub const ZERO_ARTIFACT: Self = Self(1 << 2);
    pub const CHROME_MOX: Self = Self(1 << 3);
    pub const MOX_DIAMOND: Self = Self(1 << 4);
    pub const ARTIFACT_SPELL: Self = Self(1 << 5);
    pub const CREATURE: Self = Self(1 << 6);
    pub const SPIRIT_GUIDE: Self = Self(1 << 7);
    pub const RITUAL: Self = Self(1 << 8);
    pub const MANAMORPHOSE: Self = Self(1 << 9);
    pub const RAIN: Self = Self(1 << 10);
    pub const SACRIFICE_RITUAL: Self = Self(1 << 11);
    pub const OFFER: Self = Self(1 << 12);
    pub const NOXIOUS: Self = Self(1 << 13);
    pub const SUMMONERS_PACT: Self = Self(1 << 14);
    pub const GREEN_SUN: Self = Self(1 << 15);
    pub const RANGER_CAPTAIN: Self = Self(1 << 16);
    pub const ELDRITCH_EVOLUTION: Self = Self(1 << 17);
    pub const NEOFORM: Self = Self(1 << 18);
    pub const CROP_ROTATION: Self = Self(1 << 19);
    pub const HAND_TUTOR: Self = Self(1 << 20);
    pub const BESEECH: Self = Self(1 << 21);
    pub const TOP_TUTOR: Self = Self(1 << 22);
    pub const GAMBLE: Self = Self(1 << 23);

    pub const fn contains(self, template: Self) -> bool {
        (self.0 & template.0) != 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    fn insert(&mut self, template: Self) {
        self.0 |= template.0;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardMetadata {
    pub name: Box<str>,
    pub color_mask: u8,
    pub flags: CardFlags,
    pub action_class: ActionClass,
    pub action_templates: ActionTemplateMask,
}

impl CardMetadata {
    pub(crate) fn compile(name: &str) -> Self {
        let flags = compile_flags(name);
        Self {
            name: name.into(),
            color_mask: card_color_mask(name),
            flags,
            action_class: action_class(flags),
            action_templates: action_templates(name, flags),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardSpec {
    pub slot: SlotId,
    pub name: Box<str>,
    pub color_mask: u8,
    pub flags: CardFlags,
    pub action_class: ActionClass,
    pub action_templates: ActionTemplateMask,
    pub semantic_class: u8,
}

impl CardSpec {
    pub(crate) fn compile(slot: SlotId, name: &str) -> Self {
        let metadata = CardMetadata::compile(name);
        Self {
            slot,
            name: metadata.name,
            color_mask: metadata.color_mask,
            flags: metadata.flags,
            action_class: metadata.action_class,
            action_templates: metadata.action_templates,
            // Classes remain exact until an equivalence proof supplies a coarser partition.
            semantic_class: slot,
        }
    }
}

fn compile_flags(name: &str) -> CardFlags {
    let mut flags = CardFlags::default();
    if is_land_card_name(name) {
        flags.insert(CardFlags::LAND);
    }
    if is_mdfc_land_name(name) {
        flags.insert(CardFlags::MDFC_LAND);
    }
    if is_fetch_name(name) {
        flags.insert(CardFlags::FETCH);
    }
    if is_artifact_card_name(name) {
        flags.insert(CardFlags::ARTIFACT);
    }
    if is_creature(name) {
        flags.insert(CardFlags::CREATURE);
    }
    if is_mana_card(name) || flags.contains(CardFlags::LAND) {
        flags.insert(CardFlags::MANA);
    }
    if is_tutor(name) {
        flags.insert(CardFlags::TUTOR);
    }
    if matches!(name, "Rhystic Study" | "Heartwood Storyteller") {
        flags.insert(CardFlags::ENGINE);
    }
    if is_interaction(name) {
        flags.insert(CardFlags::INTERACTION);
    }
    flags
}

fn action_class(flags: CardFlags) -> ActionClass {
    if flags.contains(CardFlags::LAND) {
        ActionClass::Land
    } else if flags.contains(CardFlags::ENGINE) {
        ActionClass::Engine
    } else if flags.contains(CardFlags::TUTOR) {
        ActionClass::Tutor
    } else if flags.contains(CardFlags::MANA) {
        ActionClass::Mana
    } else if flags.contains(CardFlags::INTERACTION) {
        ActionClass::Interaction
    } else {
        ActionClass::Value
    }
}

fn action_templates(name: &str, flags: CardFlags) -> ActionTemplateMask {
    let mut templates = ActionTemplateMask::default();
    if flags.contains(CardFlags::LAND) || flags.contains(CardFlags::MDFC_LAND) {
        templates.insert(ActionTemplateMask::LAND);
    }
    match name {
        "Rhystic Study" | "Heartwood Storyteller" => templates.insert(ActionTemplateMask::ENGINE),
        "Lotus Petal" | "Lion's Eye Diamond" | "Mox Amber" | "Mox Opal" | "Paradise Mantle" => {
            templates.insert(ActionTemplateMask::ZERO_ARTIFACT)
        }
        "Chrome Mox" => templates.insert(ActionTemplateMask::CHROME_MOX),
        "Mox Diamond" => templates.insert(ActionTemplateMask::MOX_DIAMOND),
        "Sol Ring" | "Mana Vault" | "Arcane Signet" | "Relic of Legends" | "Wishclaw Talisman"
        | "Springleaf Drum" => templates.insert(ActionTemplateMask::ARTIFACT_SPELL),
        "Birds of Paradise"
        | "Deathrite Shaman"
        | "Esper Sentinel"
        | "Faerie Mastermind"
        | "Ignoble Hierarch"
        | "Lotho, Corrupt Shirriff"
        | "Noble Hierarch"
        | "Orcish Bowmasters"
        | "Ragavan, Nimble Pilferer"
        | "The Cabbage Merchant"
        | "Tinder Wall"
        | "Valley Floodcaller"
        | "Wild Cantor" => templates.insert(ActionTemplateMask::CREATURE),
        "Simian Spirit Guide" | "Elvish Spirit Guide" => {
            templates.insert(ActionTemplateMask::SPIRIT_GUIDE)
        }
        "Dark Ritual" | "Rite of Flame" => templates.insert(ActionTemplateMask::RITUAL),
        "Manamorphose" => templates.insert(ActionTemplateMask::MANAMORPHOSE),
        "Rain of Filth" => templates.insert(ActionTemplateMask::RAIN),
        "Culling the Weak" => templates.insert(ActionTemplateMask::SACRIFICE_RITUAL),
        "An Offer You Can't Refuse" => templates.insert(ActionTemplateMask::OFFER),
        "Noxious Revival" => templates.insert(ActionTemplateMask::NOXIOUS),
        "Summoner's Pact" => templates.insert(ActionTemplateMask::SUMMONERS_PACT),
        "Green Sun's Zenith" => templates.insert(ActionTemplateMask::GREEN_SUN),
        "Ranger-Captain of Eos" => templates.insert(ActionTemplateMask::RANGER_CAPTAIN),
        "Eldritch Evolution" => templates.insert(ActionTemplateMask::ELDRITCH_EVOLUTION),
        "Neoform" => templates.insert(ActionTemplateMask::NEOFORM),
        "Crop Rotation" => templates.insert(ActionTemplateMask::CROP_ROTATION),
        "Demonic Tutor" | "Diabolic Intent" | "Grim Tutor" | "Idyllic Tutor" => {
            templates.insert(ActionTemplateMask::HAND_TUTOR)
        }
        "Beseech the Mirror" => templates.insert(ActionTemplateMask::BESEECH),
        "Enlightened Tutor" | "Imperial Seal" | "Mystical Tutor" | "Scheming Symmetry"
        | "Vampiric Tutor" | "Worldly Tutor" => templates.insert(ActionTemplateMask::TOP_TUTOR),
        "Gamble" => templates.insert(ActionTemplateMask::GAMBLE),
        _ => {}
    }
    templates
}

#[derive(Debug, Clone)]
pub struct DeckSpec {
    cards: Box<[CardSpec]>,
    by_name: FxHashMap<Box<str>, SlotId>,
    card_mask: CardMask,
}

impl DeckSpec {
    pub fn compile(names: &[String]) -> Result<Self, String> {
        if names.len() > MAX_DECK_SLOTS {
            return Err(format!(
                "deck has {} cards; packed engine supports at most {MAX_DECK_SLOTS}",
                names.len()
            ));
        }
        let mut by_name = FxHashMap::default();
        let mut cards = Vec::with_capacity(names.len());
        let mut card_mask = CardMask::EMPTY;
        for (index, name) in names.iter().enumerate() {
            let slot = index as SlotId;
            if by_name
                .insert(name.clone().into_boxed_str(), slot)
                .is_some()
            {
                return Err(format!("duplicate card name is not supported: {name}"));
            }
            cards.push(CardSpec::compile(slot, name));
            card_mask.insert(slot);
        }
        Ok(Self {
            cards: cards.into_boxed_slice(),
            by_name,
            card_mask,
        })
    }

    pub fn cards(&self) -> &[CardSpec] {
        &self.cards
    }

    pub fn card(&self, slot: SlotId) -> &CardSpec {
        &self.cards[slot as usize]
    }

    pub fn slot(&self, name: &str) -> Option<SlotId> {
        self.by_name.get(name).copied()
    }

    pub fn card_mask(&self) -> CardMask {
        self.card_mask
    }

    pub fn semantic_classes(&self) -> [u8; MAX_DECK_SLOTS] {
        let mut classes = [0; MAX_DECK_SLOTS];
        for card in &self.cards {
            classes[card.slot as usize] = card.semantic_class;
        }
        classes
    }
}

fn is_creature(name: &str) -> bool {
    matches!(
        name,
        "Birds of Paradise"
            | "Deathrite Shaman"
            | "Elvish Spirit Guide"
            | "Esper Sentinel"
            | "Faerie Mastermind"
            | "Heartwood Storyteller"
            | "Lotho, Corrupt Shirriff"
            | "Nick Fury, Agent of S.H.I.E.L.D."
            | "Orcish Bowmasters"
            | "Ragavan, Nimble Pilferer"
            | "Ranger-Captain of Eos"
            | "Simian Spirit Guide"
            | "The Cabbage Merchant"
            | "Tinder Wall"
            | "Valley Floodcaller"
    )
}

fn is_mana_card(name: &str) -> bool {
    matches!(
        name,
        "An Offer You Can't Refuse"
            | "Birds of Paradise"
            | "Chrome Mox"
            | "Culling the Weak"
            | "Dark Ritual"
            | "Deathrite Shaman"
            | "Elvish Spirit Guide"
            | "Infernal Plunge"
            | "Lion's Eye Diamond"
            | "Lotus Petal"
            | "Mana Vault"
            | "Manamorphose"
            | "Mox Amber"
            | "Mox Diamond"
            | "Mox Opal"
            | "Rain of Filth"
            | "Rite of Flame"
            | "Simian Spirit Guide"
            | "Sol Ring"
            | "Strike It Rich"
            | "Tinder Wall"
    )
}

fn is_tutor(name: &str) -> bool {
    matches!(
        name,
        "Beseech the Mirror"
            | "Crop Rotation"
            | "Demonic Tutor"
            | "Diabolic Intent"
            | "Eldritch Evolution"
            | "Enlightened Tutor"
            | "Gamble"
            | "Green Sun's Zenith"
            | "Imperial Seal"
            | "Mystical Tutor"
            | "Ranger-Captain of Eos"
            | "Scheming Symmetry"
            | "Summoner's Pact"
            | "Vampiric Tutor"
            | "Wishclaw Talisman"
    )
}

fn is_interaction(name: &str) -> bool {
    matches!(
        name,
        "An Offer You Can't Refuse"
            | "Angel's Grace"
            | "Chain of Vapor"
            | "Commandeer"
            | "Deflecting Swat"
            | "Disrupting Shoal"
            | "Fierce Guardianship"
            | "Firestorm"
            | "Flusterstorm"
            | "Force of Negation"
            | "Force of Will"
            | "Mental Misstep"
            | "Misdirection"
            | "Molten Disaster"
            | "Orim's Chant"
            | "Pact of Negation"
            | "Pyroblast"
            | "Silence"
            | "Snapback"
            | "Subtlety"
            | "Sudden Substitution"
            | "Swan Song"
            | "Wipe Away"
            | "Word of Seizing"
    )
}
