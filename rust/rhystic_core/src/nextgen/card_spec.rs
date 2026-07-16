use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use super::{CardMask, SlotId, MAX_DECK_SLOTS};
use crate::fast_engine::{
    card_color_mask, is_artifact_card_name, is_fetch_name, is_land_card_name, is_mdfc_land_name,
};
use crate::Cost;

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
    pub const ENCHANTMENT: Self = Self(1 << 9);

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
    pub const GITAXIAN_PROBE: Self = Self(1 << 24);
    pub const STREET_WRAITH: Self = Self(1 << 25);
    pub const TURBO_OPENING: Self = Self(1 << 26);
    pub const CREATURE_MANA_ENGINE: Self = Self(1 << 27);
    pub const CREATURE_BATTLEFIELD_TUTOR: Self = Self(1 << 28);

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
    pub payment_gate_costs: [Option<Cost>; 2],
    pub opening_mana: OpeningManaProfile,
    pub opening_artifact: OpeningArtifactKind,
    pub opening_spell: OpeningSpellKind,
    pub opening_creature: OpeningCreatureKind,
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
            payment_gate_costs: payment_gate_costs(name),
            opening_mana: opening_mana_profile(name),
            opening_artifact: opening_artifact_kind(name),
            opening_spell: opening_spell_kind(name),
            opening_creature: opening_creature_kind(name),
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
    pub payment_gate_costs: [Option<Cost>; 2],
    pub opening_mana: OpeningManaProfile,
    pub opening_artifact: OpeningArtifactKind,
    pub opening_spell: OpeningSpellKind,
    pub opening_creature: OpeningCreatureKind,
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
            payment_gate_costs: metadata.payment_gate_costs,
            opening_mana: metadata.opening_mana,
            opening_artifact: metadata.opening_artifact,
            opening_spell: metadata.opening_spell,
            opening_creature: metadata.opening_creature,
            // Classes remain exact until an equivalence proof supplies a coarser partition.
            semantic_class: slot,
        }
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpeningManaProfile {
    pub color_mask: u8,
    pub colorless: u8,
    pub enters_tapped: bool,
    pub kind: OpeningLandKind,
    pub land_types: u8,
    pub fetch_types: u8,
}

impl OpeningManaProfile {
    pub const fn is_supported(self) -> bool {
        !matches!(self.kind, OpeningLandKind::None)
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum OpeningLandKind {
    #[default]
    None = 0,
    Simple = 1,
    Fetch = 2,
    CityOfTraitors = 3,
    CrystalVein = 4,
    Glimmervoid = 5,
    GemstoneMine = 6,
    GemstoneCaverns = 7,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum OpeningArtifactKind {
    #[default]
    None = 0,
    LotusPetal = 1,
    LionsEyeDiamond = 2,
    ChromeMox = 3,
    MoxDiamond = 4,
    MoxOpal = 5,
    MoxAmber = 6,
    ParadiseMantle = 7,
    SolRing = 8,
    ManaVault = 9,
    WishclawTalisman = 10,
}

fn opening_artifact_kind(name: &str) -> OpeningArtifactKind {
    match name {
        "Lotus Petal" => OpeningArtifactKind::LotusPetal,
        "Lion's Eye Diamond" => OpeningArtifactKind::LionsEyeDiamond,
        "Chrome Mox" => OpeningArtifactKind::ChromeMox,
        "Mox Diamond" => OpeningArtifactKind::MoxDiamond,
        "Mox Opal" => OpeningArtifactKind::MoxOpal,
        "Mox Amber" => OpeningArtifactKind::MoxAmber,
        "Paradise Mantle" => OpeningArtifactKind::ParadiseMantle,
        "Sol Ring" => OpeningArtifactKind::SolRing,
        "Mana Vault" => OpeningArtifactKind::ManaVault,
        "Wishclaw Talisman" => OpeningArtifactKind::WishclawTalisman,
        _ => OpeningArtifactKind::None,
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum OpeningSpellKind {
    #[default]
    None = 0,
    ElvishSpiritGuide = 1,
    SimianSpiritGuide = 2,
    DarkRitual = 3,
    RiteOfFlame = 4,
    DemonicTutor = 5,
    ImperialSeal = 6,
    VampiricTutor = 7,
    EnlightenedTutor = 8,
    SchemingSymmetry = 9,
    Manamorphose = 10,
    Gamble = 11,
    NoxiousRevival = 12,
    GreenSunsZenith = 13,
    SummonersPact = 14,
    CropRotation = 15,
    CullingTheWeak = 16,
    DiabolicIntent = 17,
    InfernalPlunge = 18,
    RainOfFilth = 19,
    MysticalTutor = 20,
    EldritchEvolution = 21,
    AnOfferYouCantRefuse = 22,
    GitaxianProbe = 23,
    StreetWraith = 24,
}

fn opening_spell_kind(name: &str) -> OpeningSpellKind {
    match name {
        "Elvish Spirit Guide" => OpeningSpellKind::ElvishSpiritGuide,
        "Simian Spirit Guide" => OpeningSpellKind::SimianSpiritGuide,
        "Dark Ritual" => OpeningSpellKind::DarkRitual,
        "Rite of Flame" => OpeningSpellKind::RiteOfFlame,
        "Demonic Tutor" => OpeningSpellKind::DemonicTutor,
        "Imperial Seal" => OpeningSpellKind::ImperialSeal,
        "Vampiric Tutor" => OpeningSpellKind::VampiricTutor,
        "Enlightened Tutor" => OpeningSpellKind::EnlightenedTutor,
        "Scheming Symmetry" => OpeningSpellKind::SchemingSymmetry,
        "Manamorphose" => OpeningSpellKind::Manamorphose,
        "Gamble" => OpeningSpellKind::Gamble,
        "Noxious Revival" => OpeningSpellKind::NoxiousRevival,
        "Green Sun's Zenith" => OpeningSpellKind::GreenSunsZenith,
        "Summoner's Pact" => OpeningSpellKind::SummonersPact,
        "Crop Rotation" => OpeningSpellKind::CropRotation,
        "Culling the Weak" => OpeningSpellKind::CullingTheWeak,
        "Diabolic Intent" => OpeningSpellKind::DiabolicIntent,
        "Infernal Plunge" => OpeningSpellKind::InfernalPlunge,
        "Rain of Filth" => OpeningSpellKind::RainOfFilth,
        "Mystical Tutor" => OpeningSpellKind::MysticalTutor,
        "Eldritch Evolution" => OpeningSpellKind::EldritchEvolution,
        "An Offer You Can't Refuse" => OpeningSpellKind::AnOfferYouCantRefuse,
        "Gitaxian Probe" => OpeningSpellKind::GitaxianProbe,
        "Street Wraith" => OpeningSpellKind::StreetWraith,
        _ => OpeningSpellKind::None,
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum OpeningCreatureKind {
    #[default]
    None = 0,
    BirdsOfParadise = 1,
    DeathriteShaman = 2,
    Ragavan = 3,
    TinderWall = 4,
    RangerCaptainOfEos = 5,
    EsperSentinel = 6,
}

fn opening_creature_kind(name: &str) -> OpeningCreatureKind {
    match name {
        "Birds of Paradise" => OpeningCreatureKind::BirdsOfParadise,
        "Deathrite Shaman" => OpeningCreatureKind::DeathriteShaman,
        "Ragavan, Nimble Pilferer" => OpeningCreatureKind::Ragavan,
        "Tinder Wall" => OpeningCreatureKind::TinderWall,
        "Ranger-Captain of Eos" => OpeningCreatureKind::RangerCaptainOfEos,
        "Esper Sentinel" => OpeningCreatureKind::EsperSentinel,
        _ => OpeningCreatureKind::None,
    }
}

fn opening_mana_profile(name: &str) -> OpeningManaProfile {
    const B: u8 = 1;
    const R: u8 = 1 << 1;
    const U: u8 = 1 << 2;
    const W: u8 = 1 << 3;
    const G: u8 = 1 << 4;
    const RAINBOW: u8 = B | R | U | W | G;
    const PLAINS: u8 = 1;
    const ISLAND: u8 = 1 << 1;
    const SWAMP: u8 = 1 << 2;
    const MOUNTAIN: u8 = 1 << 3;
    const FOREST: u8 = 1 << 4;

    let (color_mask, colorless, enters_tapped, kind, land_types, fetch_types) = match name {
        "Ancient Tomb" => (0, 2, false, OpeningLandKind::Simple, 0, 0),
        "Badlands" => (
            B | R,
            0,
            false,
            OpeningLandKind::Simple,
            SWAMP | MOUNTAIN,
            0,
        ),
        "Breeding Pool" => (U | G, 0, false, OpeningLandKind::Simple, ISLAND | FOREST, 0),
        "Blood Crypt" => (
            B | R,
            0,
            false,
            OpeningLandKind::Simple,
            SWAMP | MOUNTAIN,
            0,
        ),
        "Bayou" => (B | G, 0, false, OpeningLandKind::Simple, SWAMP | FOREST, 0),
        "Hallowed Fountain" | "Tundra" => {
            (U | W, 0, false, OpeningLandKind::Simple, PLAINS | ISLAND, 0)
        }
        "Scrubland" => (B | W, 0, false, OpeningLandKind::Simple, PLAINS | SWAMP, 0),
        "Tropical Island" => (U | G, 0, false, OpeningLandKind::Simple, ISLAND | FOREST, 0),
        "Taiga" => (
            R | G,
            0,
            false,
            OpeningLandKind::Simple,
            MOUNTAIN | FOREST,
            0,
        ),
        "Underground Sea" => (B | U, 0, false, OpeningLandKind::Simple, ISLAND | SWAMP, 0),
        "Watery Grave" => (B | U, 0, false, OpeningLandKind::Simple, ISLAND | SWAMP, 0),
        "Volcanic Island" => (
            R | U,
            0,
            false,
            OpeningLandKind::Simple,
            ISLAND | MOUNTAIN,
            0,
        ),
        "Emergence Zone" => (0, 1, false, OpeningLandKind::Simple, 0, 0),
        "Forest" | "Dryad Arbor" => (G, 0, false, OpeningLandKind::Simple, FOREST, 0),
        "Gaea's Cradle" => (G, 0, false, OpeningLandKind::Simple, 0, 0),
        "Shifting Woodland" => (G, 0, true, OpeningLandKind::Simple, 0, 0),
        "Gemstone Caverns" => (0, 1, false, OpeningLandKind::GemstoneCaverns, 0, 0),
        "City of Traitors" => (0, 2, false, OpeningLandKind::CityOfTraitors, 0, 0),
        "Crystal Vein" => (0, 1, false, OpeningLandKind::CrystalVein, 0, 0),
        "Glimmervoid" => (RAINBOW, 0, false, OpeningLandKind::Glimmervoid, 0, 0),
        "Gemstone Mine" => (RAINBOW, 0, false, OpeningLandKind::GemstoneMine, 0, 0),
        "Sink into Stupor" => (U, 0, true, OpeningLandKind::Simple, 0, 0),
        "Arid Mesa" => (0, 0, false, OpeningLandKind::Fetch, 0, PLAINS | MOUNTAIN),
        "Bloodstained Mire" => (0, 0, false, OpeningLandKind::Fetch, 0, SWAMP | MOUNTAIN),
        "Flooded Strand" => (0, 0, false, OpeningLandKind::Fetch, 0, PLAINS | ISLAND),
        "Marsh Flats" => (0, 0, false, OpeningLandKind::Fetch, 0, PLAINS | SWAMP),
        "Misty Rainforest" => (0, 0, false, OpeningLandKind::Fetch, 0, ISLAND | FOREST),
        "Polluted Delta" => (0, 0, false, OpeningLandKind::Fetch, 0, ISLAND | SWAMP),
        "Scalding Tarn" => (0, 0, false, OpeningLandKind::Fetch, 0, ISLAND | MOUNTAIN),
        "Verdant Catacombs" => (0, 0, false, OpeningLandKind::Fetch, 0, SWAMP | FOREST),
        "Windswept Heath" => (0, 0, false, OpeningLandKind::Fetch, 0, PLAINS | FOREST),
        "Wooded Foothills" => (0, 0, false, OpeningLandKind::Fetch, 0, MOUNTAIN | FOREST),
        "Hydroelectric Specimen" => (U, 0, false, OpeningLandKind::Simple, 0, 0),
        "City of Brass" | "Command Tower" | "Exotic Orchard" | "Forbidden Orchard"
        | "Mana Confluence" | "Starting Town" | "Tarnished Citadel" => {
            (RAINBOW, 0, false, OpeningLandKind::Simple, 0, 0)
        }
        _ => return OpeningManaProfile::default(),
    };
    OpeningManaProfile {
        color_mask,
        colorless,
        enters_tapped,
        kind,
        land_types,
        fetch_types,
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
    if matches!(
        name,
        "Copy Enchantment"
            | "Cryptolith Rite"
            | "Curse of Opulence"
            | "Earthcraft"
            | "Flash Photography"
            | "Mystic Remora"
            | "Rhystic Study"
            | "Smothering Tithe"
            | "Shimmerwilds Growth"
            | "Underworld Breach"
    ) {
        flags.insert(CardFlags::ENCHANTMENT);
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
        "Sol Ring"
        | "Mana Vault"
        | "Arcane Signet"
        | "Relic of Legends"
        | "Wishclaw Talisman"
        | "Springleaf Drum"
        | "Chromatic Star"
        | "Defense Grid"
        | "Grim Monolith"
        | "Grinding Station"
        | "Jeweled Amulet"
        | "Talisman of Dominance"
        | "Vexing Bauble" => templates.insert(ActionTemplateMask::ARTIFACT_SPELL),
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
        | "Wild Cantor"
        | "Birgi, God of Storytelling"
        | "Badgermole Cub"
        | "Gene Pollinator"
        | "Kinnan, Bonder Prodigy"
        | "Storm-Kiln Artist" => templates.insert(ActionTemplateMask::CREATURE),
        "Hydroelectric Specimen" => templates.insert(ActionTemplateMask::CREATURE),
        "Simian Spirit Guide" | "Elvish Spirit Guide" => {
            templates.insert(ActionTemplateMask::SPIRIT_GUIDE)
        }
        "Dark Ritual" | "Rite of Flame" | "Cabal Ritual" => {
            templates.insert(ActionTemplateMask::RITUAL)
        }
        "Manamorphose" => templates.insert(ActionTemplateMask::MANAMORPHOSE),
        "Rain of Filth" => templates.insert(ActionTemplateMask::RAIN),
        "Culling the Weak" | "Infernal Plunge" => {
            templates.insert(ActionTemplateMask::SACRIFICE_RITUAL)
        }
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
        "Gitaxian Probe" => templates.insert(ActionTemplateMask::GITAXIAN_PROBE),
        "Street Wraith" => templates.insert(ActionTemplateMask::STREET_WRAITH),
        "Ad Nauseam"
        | "Brain Freeze"
        | "Borne Upon a Wind"
        | "Demonic Consultation"
        | "Tainted Pact"
        | "Wheel of Fortune"
        | "Windfall"
        | "Dramatic Reversal"
        | "Curse of Opulence"
        | "Flare of Duplication"
        | "Flashback"
        | "Jeska's Will"
        | "Necropotence" => templates.insert(ActionTemplateMask::TURBO_OPENING),
        "Underworld Breach" => templates.insert(ActionTemplateMask::TURBO_OPENING),
        "Cryptolith Rite" | "Earthcraft" | "Shimmerwilds Growth" | "Mockingbird" => {
            templates.insert(ActionTemplateMask::CREATURE_MANA_ENGINE)
        }
        "Chord of Calling" | "Finale of Devastation" | "Nature's Rhythm" => {
            templates.insert(ActionTemplateMask::CREATURE_BATTLEFIELD_TUTOR)
        }
        _ => {}
    }
    templates
}

fn payment_gate_costs(name: &str) -> [Option<Cost>; 2] {
    let (first, second) = match name {
        "Rhystic Study" => ([2, 0, 0, 1, 0, 0], None),
        "Heartwood Storyteller" => ([1, 0, 0, 0, 0, 2], None),
        "Brain Freeze" => ([1, 0, 0, 1, 0, 0], None),
        "Sol Ring" | "Mana Vault" | "Springleaf Drum" | "Chromatic Star" | "Jeweled Amulet"
        | "Vexing Bauble" => ([1, 0, 0, 0, 0, 0], None),
        "Defense Grid" | "Grim Monolith" | "Grinding Station" | "Talisman of Dominance" => {
            ([2, 0, 0, 0, 0, 0], None)
        }
        "Arcane Signet" => ([2, 0, 0, 0, 0, 0], None),
        "Relic of Legends" => ([3, 0, 0, 0, 0, 0], None),
        "Wishclaw Talisman" => ([1, 1, 0, 0, 0, 0], None),
        "Birds of Paradise" | "Ignoble Hierarch" | "Noble Hierarch" | "Tinder Wall" => {
            ([0, 0, 0, 0, 0, 1], None)
        }
        "Deathrite Shaman" => ([0, 1, 0, 0, 0, 0], Some([0, 0, 0, 0, 0, 1])),
        "Esper Sentinel" => ([0, 0, 0, 0, 1, 0], None),
        "Faerie Mastermind" => ([1, 0, 0, 1, 0, 0], None),
        "Lotho, Corrupt Shirriff" => ([0, 1, 0, 0, 1, 0], None),
        "Orcish Bowmasters" => ([1, 1, 0, 0, 0, 0], None),
        "Ragavan, Nimble Pilferer" => ([0, 0, 1, 0, 0, 0], None),
        "The Cabbage Merchant" => ([2, 0, 0, 0, 0, 1], None),
        "Valley Floodcaller" => ([2, 0, 0, 1, 0, 0], None),
        "Wild Cantor" => ([0, 0, 1, 0, 0, 0], Some([0, 0, 0, 0, 0, 1])),
        "Dark Ritual" => ([0, 1, 0, 0, 0, 0], None),
        "Cabal Ritual" => ([1, 1, 0, 0, 0, 0], None),
        "Rite of Flame" | "Gamble" => ([0, 0, 1, 0, 0, 0], None),
        "Manamorphose" => ([1, 0, 1, 0, 0, 0], Some([1, 0, 0, 0, 0, 1])),
        "Rain of Filth" | "Culling the Weak" => ([0, 1, 0, 0, 0, 0], None),
        // The bait spell adds cost; U is a safe lower bound for pruning.
        "An Offer You Can't Refuse" => ([0, 0, 0, 1, 0, 0], None),
        // One-mana green targets are the cheapest supported GSZ line.
        "Green Sun's Zenith" => ([1, 0, 0, 0, 0, 1], Some([3, 0, 0, 0, 0, 1])),
        "Ranger-Captain of Eos" => ([1, 0, 0, 0, 2, 0], None),
        "Eldritch Evolution" => ([1, 0, 0, 0, 0, 2], None),
        "Neoform" => ([0, 0, 0, 1, 0, 1], None),
        "Crop Rotation" => ([0, 0, 0, 0, 0, 1], None),
        "Demonic Tutor" | "Diabolic Intent" => ([1, 1, 0, 0, 0, 0], None),
        "Grim Tutor" => ([1, 2, 0, 0, 0, 0], None),
        "Idyllic Tutor" => ([2, 0, 0, 0, 1, 0], None),
        "Beseech the Mirror" => ([1, 3, 0, 0, 0, 0], None),
        "Enlightened Tutor" => ([0, 0, 0, 0, 1, 0], None),
        "Imperial Seal" | "Scheming Symmetry" | "Vampiric Tutor" => ([0, 1, 0, 0, 0, 0], None),
        "Mystical Tutor" => ([0, 0, 0, 1, 0, 0], None),
        "Worldly Tutor" => ([0, 0, 0, 0, 0, 1], None),
        "Birgi, God of Storytelling" => ([2, 0, 1, 0, 0, 0], None),
        "Badgermole Cub" | "Kinnan, Bonder Prodigy" => ([1, 0, 0, 0, 0, 1], None),
        "Gene Pollinator" => ([0, 0, 0, 0, 0, 1], None),
        "Storm-Kiln Artist" => ([3, 0, 1, 0, 0, 0], None),
        "Hydroelectric Specimen" => ([2, 0, 0, 1, 0, 0], None),
        "Cryptolith Rite" | "Earthcraft" | "Shimmerwilds Growth" => ([1, 0, 0, 0, 0, 1], None),
        "Mockingbird" => ([0, 0, 0, 1, 0, 0], Some([2, 0, 0, 1, 0, 0])),
        "Chord of Calling" => ([0, 0, 0, 0, 0, 3], None),
        "Finale of Devastation" | "Nature's Rhythm" => ([0, 0, 0, 0, 0, 2], None),
        _ => return [None, None],
    };
    [Some(first), second]
}

#[derive(Debug, Clone)]
pub struct DeckSpec {
    cards: Box<[CardSpec]>,
    by_name: FxHashMap<Box<str>, SlotId>,
    card_mask: CardMask,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct SemanticKey {
    color_mask: u8,
    flags: CardFlags,
    action_templates: ActionTemplateMask,
    payment_gate_costs: [Option<Cost>; 2],
    opening_mana: OpeningManaProfile,
    opening_artifact: OpeningArtifactKind,
    opening_spell: OpeningSpellKind,
    opening_creature: OpeningCreatureKind,
    special: u8,
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
        let mut classes = FxHashMap::default();
        let mut next_class = 0u8;
        for card in &mut cards {
            let key = SemanticKey {
                color_mask: card.color_mask,
                flags: card.flags,
                action_templates: card.action_templates,
                payment_gate_costs: card.payment_gate_costs,
                opening_mana: card.opening_mana,
                opening_artifact: card.opening_artifact,
                opening_spell: card.opening_spell,
                opening_creature: card.opening_creature,
                special: u8::from(card.name.as_ref() == "Angel's Grace"),
            };
            card.semantic_class = *classes.entry(key).or_insert_with(|| {
                let class = next_class;
                next_class = next_class
                    .checked_add(1)
                    .expect("semantic classes fit packed slot ids");
                class
            });
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
            | "Birgi, God of Storytelling"
            | "Badgermole Cub"
            | "Clever Impersonator"
            | "Deathrite Shaman"
            | "Dryad Arbor"
            | "Elvish Spirit Guide"
            | "Esper Sentinel"
            | "Faerie Mastermind"
            | "Heartwood Storyteller"
            | "Hydroelectric Specimen"
            | "Gene Pollinator"
            | "Kinnan, Bonder Prodigy"
            | "Lotho, Corrupt Shirriff"
            | "Mockingbird"
            | "Nick Fury, Agent of S.H.I.E.L.D."
            | "Orcish Bowmasters"
            | "Ragavan, Nimble Pilferer"
            | "Ranger-Captain of Eos"
            | "Simian Spirit Guide"
            | "Street Wraith"
            | "Storm-Kiln Artist"
            | "Subtlety"
            | "The Cabbage Merchant"
            | "Tinder Wall"
            | "Valley Floodcaller"
    )
}

fn is_mana_card(name: &str) -> bool {
    matches!(
        name,
        "An Offer You Can't Refuse"
            | "Arcane Signet"
            | "Birgi, God of Storytelling"
            | "Badgermole Cub"
            | "Cabal Ritual"
            | "Chromatic Star"
            | "Birds of Paradise"
            | "Chrome Mox"
            | "Culling the Weak"
            | "Dark Ritual"
            | "Deathrite Shaman"
            | "Elvish Spirit Guide"
            | "Infernal Plunge"
            | "Lion's Eye Diamond"
            | "Lotus Petal"
            | "Grim Monolith"
            | "Gene Pollinator"
            | "Jeweled Amulet"
            | "Mana Vault"
            | "Manamorphose"
            | "Mox Amber"
            | "Mox Diamond"
            | "Mox Opal"
            | "Rain of Filth"
            | "Rite of Flame"
            | "Simian Spirit Guide"
            | "Sol Ring"
            | "Springleaf Drum"
            | "Talisman of Dominance"
            | "Kinnan, Bonder Prodigy"
            | "Strike It Rich"
            | "Tinder Wall"
    )
}

fn is_tutor(name: &str) -> bool {
    matches!(
        name,
        "Beseech the Mirror"
            | "Chord of Calling"
            | "Crop Rotation"
            | "Demonic Consultation"
            | "Demonic Counsel"
            | "Demonic Tutor"
            | "Diabolic Intent"
            | "Eldritch Evolution"
            | "Enlightened Tutor"
            | "Gamble"
            | "Green Sun's Zenith"
            | "Imperial Seal"
            | "Mystical Tutor"
            | "Nature's Rhythm"
            | "Finale of Devastation"
            | "Ranger-Captain of Eos"
            | "Scheming Symmetry"
            | "Summoner's Pact"
            | "Tainted Pact"
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
