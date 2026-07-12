// PersistentLibrary mutates only memoized digests/canonical views; logical card order is stable.
#![allow(clippy::mutable_key_type)]

use crate::nextgen::{ActionTemplateMask, CardFlags, CardMetadata};
use crate::{
    add_mana, bottom_choices, generate_fixture_action_cores, pay_options, state_signature,
    ActionFixturePayload, CloseTurnRequest, CloseTurnResponse, Cost, EarliestRequest, FixturePerm,
    FixtureState, Mana, PolicyCapReplayRecord, PolicyEvalFastRequest, PolicyEvalFastResponse,
    PolicyGameRecord, PolicyMulliganDecisionRecord, PolicySimFastRequest, PolicySimFastResponse,
    PolicyThresholdRow, PolicyThresholdSweepFastRequest, PolicyThresholdSweepFastResponse,
    PolicyThresholdSweepVariantResponse, PolicyThresholdVariantRequest, PolicyValidationRecord,
    RawDeltaFastRequest, RawDeltaFastResponse, RawDeltaStreamRecord, RawDeltaSwapRequest,
    RawDeltaVariantResponse, RngShuffleAuditRequest, RngShuffleAuditResponse, SolveKeepRequest,
    SolveKeepResponse, VisibleHandBatchRequest, VisibleHandResponse, VisibleHandTaskRequest,
};
use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;
use smallvec::SmallVec;
use std::cell::{Cell, OnceCell};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::time::Instant;

pub type CardId = u16;
pub type InternId = u16;

#[derive(Debug, Clone, Default)]
pub struct PersistentLibrary(Rc<LibraryStorage>);

#[derive(Debug)]
struct LibraryStorage {
    cards: Vec<CardId>,
    hash: Cell<Option<u64>>,
    membership: Cell<Option<[u64; 4]>>,
    canonical: [OnceCell<Rc<LibraryStorage>>; 4],
}

impl LibraryStorage {
    fn new(cards: Vec<CardId>) -> Self {
        Self {
            cards,
            hash: Cell::new(None),
            membership: Cell::new(None),
            canonical: std::array::from_fn(|_| OnceCell::new()),
        }
    }
}

impl Default for LibraryStorage {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Clone for LibraryStorage {
    fn clone(&self) -> Self {
        Self::new(self.cards.clone())
    }
}

impl PersistentLibrary {
    pub fn is_shared_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    fn contains_card(&self, card: CardId) -> bool {
        let index = card as usize;
        if index >= 256 {
            return self.0.cards.contains(&card);
        }
        let membership = self.0.membership.get().unwrap_or_else(|| {
            let mut words = [0u64; 4];
            for item in &self.0.cards {
                let item_index = *item as usize;
                if item_index < 256 {
                    words[item_index / 64] |= 1u64 << (item_index % 64);
                }
            }
            self.0.membership.set(Some(words));
            words
        });
        (membership[index / 64] & (1u64 << (index % 64))) != 0
    }

    fn canonicalized_tail(&self, ordered_prefix: usize) -> Self {
        let prefix = ordered_prefix.min(self.0.cards.len());
        if self.0.cards[prefix..].is_sorted() {
            return self.clone();
        }
        if prefix < self.0.canonical.len() {
            let storage = self.0.canonical[prefix].get_or_init(|| {
                let mut cards = self.0.cards.clone();
                cards[prefix..].sort_unstable();
                Rc::new(LibraryStorage::new(cards))
            });
            return Self(storage.clone());
        }
        let mut cards = self.0.cards.clone();
        cards[prefix..].sort_unstable();
        Self::from(cards)
    }
}

impl From<Vec<CardId>> for PersistentLibrary {
    fn from(cards: Vec<CardId>) -> Self {
        Self(Rc::new(LibraryStorage::new(cards)))
    }
}

impl FromIterator<CardId> for PersistentLibrary {
    fn from_iter<T: IntoIterator<Item = CardId>>(iter: T) -> Self {
        Self::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl Deref for PersistentLibrary {
    type Target = Vec<CardId>;

    fn deref(&self) -> &Self::Target {
        &self.0.cards
    }
}

impl DerefMut for PersistentLibrary {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let storage = Rc::make_mut(&mut self.0);
        storage.hash.set(None);
        storage.membership.set(None);
        for cached in &mut storage.canonical {
            cached.take();
        }
        &mut storage.cards
    }
}

impl PartialEq for PersistentLibrary {
    fn eq(&self, other: &Self) -> bool {
        self.is_shared_with(other) || self.0.cards == other.0.cards
    }
}

impl Eq for PersistentLibrary {}

impl Hash for PersistentLibrary {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let digest = self.0.hash.get().unwrap_or_else(|| {
            let value = hash_value(&self.0.cards);
            self.0.hash.set(Some(value));
            value
        });
        state.write_u64(digest);
    }
}

const COLORS: &[u8] = b"BRUWGC";

fn canonical_card_name(name: &str) -> &str {
    if name.starts_with("Theoretical Birds of Paradise ") {
        return "Birds of Paradise";
    }
    match name {
        "Zidane Tribal" => "Ragavan, Nimble Pilferer",
        _ => name,
    }
}

fn is_theoretical_rainbow_land_name(card: &str) -> bool {
    card.starts_with("Theoretical Rainbow Land ")
}

#[derive(Debug, Clone, Default)]
pub struct FastContext {
    card_to_id: FxHashMap<String, CardId>,
    cards: Vec<String>,
    card_specs: Vec<CardMetadata>,
    intern_to_id: FxHashMap<String, InternId>,
    interned: Vec<String>,
    strict_shuffle_hidden: bool,
    payment_mode: FastPaymentMode,
    funding_cache_limit: usize,
    funding_frontier_limit: usize,
    funding_cache: FxHashMap<FastFundingCacheKey, Rc<[FastFundingNode]>>,
}

impl FastContext {
    pub fn with_card_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut unique: Vec<String> = names
            .into_iter()
            .map(|name| canonical_card_name(name.as_ref()).to_string())
            .collect();
        unique.sort();
        unique.dedup();
        let mut ctx = Self::default();
        ctx.strict_shuffle_hidden = strict_shuffle_hidden_enabled_from_env();
        ctx.payment_mode = payment_mode_from_env();
        ctx.funding_cache_limit = funding_cache_limit_from_env();
        ctx.funding_frontier_limit = funding_frontier_limit_from_env();
        ctx.intern_common_strings();
        for name in unique {
            ctx.intern_card(&name);
        }
        ctx
    }

    fn intern_common_strings(&mut self) {
        for value in [
            "", "B", "R", "U", "W", "G", "C", "BR", "BG", "BU", "BW", "RG", "RU", "RW", "UG", "UW",
            "BRUWG", "B*", "R*", "U*", "W*", "G*", "BG*", "BW*", "RG*", "UG*", "UW*", "1", "2",
            "3",
        ] {
            self.intern_string(value);
        }
    }

    pub fn intern_card(&mut self, name: &str) -> CardId {
        let name = canonical_card_name(name);
        if let Some(id) = self.card_to_id.get(name) {
            return *id;
        }
        let id = self.cards.len() as CardId;
        self.cards.push(name.to_string());
        self.card_specs.push(CardMetadata::compile(name));
        self.card_to_id.insert(name.to_string(), id);
        id
    }

    pub fn intern_string(&mut self, value: &str) -> InternId {
        if let Some(id) = self.intern_to_id.get(value) {
            return *id;
        }
        let id = self.interned.len() as InternId;
        self.interned.push(value.to_string());
        self.intern_to_id.insert(value.to_string(), id);
        id
    }

    pub fn card_name(&self, id: CardId) -> &str {
        &self.cards[id as usize]
    }

    pub fn card_id(&self, name: &str) -> Option<CardId> {
        let name = canonical_card_name(name);
        self.card_to_id.get(name).copied()
    }

    pub fn card_spec(&self, id: CardId) -> &CardMetadata {
        &self.card_specs[id as usize]
    }

    pub fn interned_string(&self, id: InternId) -> &str {
        &self.interned[id as usize]
    }

    pub fn card_count(&self) -> usize {
        self.cards.len()
    }

    pub fn interned_count(&self) -> usize {
        self.interned.len()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum FastPermKind {
    Unknown = 0,
    Land = 1,
    CcLand = 2,
    City = 3,
    Glimmer = 4,
    Cavern = 5,
    Mine = 6,
    Tower = 7,
    Vein = 8,
    Petal = 9,
    Treasure = 10,
    Led = 11,
    Amber = 12,
    Opal = 13,
    Mantle = 14,
    Diamond = 15,
    Chrome = 16,
    Sol = 17,
    Vault = 18,
    Signet = 19,
    Wishclaw = 20,
    Drum = 21,
    Relic = 22,
    Esper = 23,
    EngineEnch = 24,
    Nature = 25,
    Nick = 26,
    Rog = 27,
    Bird = 28,
    Deathrite = 29,
    Tinder = 30,
    Tataru = 31,
    Ragavan = 32,
    Lotho = 33,
    Heartwood = 34,
    Creature = 35,
    Birgi = 36,
    Wan = 37,
    Ishai = 38,
    Artifact = 39,
    Noble = 40,
    Ignoble = 41,
    Cantor = 42,
    Faerie = 43,
    Bowmasters = 44,
    Cabbage = 45,
    Valley = 46,
}

impl FastPermKind {
    fn from_name(name: &str) -> Self {
        match name {
            "LAND" => Self::Land,
            "CCLAND" => Self::CcLand,
            "CITY" => Self::City,
            "GLIMMER" => Self::Glimmer,
            "CAVERN" => Self::Cavern,
            "MINE" => Self::Mine,
            "TOWER" => Self::Tower,
            "VEIN" => Self::Vein,
            "PETAL" => Self::Petal,
            "TREASURE" => Self::Treasure,
            "LED" => Self::Led,
            "AMBER" => Self::Amber,
            "OPAL" => Self::Opal,
            "MANTLE" => Self::Mantle,
            "DIAMOND" => Self::Diamond,
            "CHROME" => Self::Chrome,
            "SOL" => Self::Sol,
            "VAULT" => Self::Vault,
            "SIGNET" => Self::Signet,
            "WISHCLAW" => Self::Wishclaw,
            "DRUM" => Self::Drum,
            "RELIC" => Self::Relic,
            "ESPER" => Self::Esper,
            "ENGINE_ENCH" => Self::EngineEnch,
            "NATURE" => Self::Nature,
            "NICK" => Self::Nick,
            "ROG" => Self::Rog,
            "BIRD" => Self::Bird,
            "DEATHRITE" => Self::Deathrite,
            "TINDER" => Self::Tinder,
            "TATARU" => Self::Tataru,
            "RAGAVAN" => Self::Ragavan,
            "LOTHO" => Self::Lotho,
            "HEARTWOOD" => Self::Heartwood,
            "CREATURE" => Self::Creature,
            "BIRGI" => Self::Birgi,
            "WAN" => Self::Wan,
            "ISHAI" => Self::Ishai,
            "ARTIFACT" => Self::Artifact,
            "NOBLE" => Self::Noble,
            "IGNOBLE" => Self::Ignoble,
            "CANTOR" => Self::Cantor,
            "FAERIE" => Self::Faerie,
            "BOWMASTERS" => Self::Bowmasters,
            "CABBAGE" => Self::Cabbage,
            "VALLEY" => Self::Valley,
            _ => Self::Unknown,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Land,
            2 => Self::CcLand,
            3 => Self::City,
            4 => Self::Glimmer,
            5 => Self::Cavern,
            6 => Self::Mine,
            7 => Self::Tower,
            8 => Self::Vein,
            9 => Self::Petal,
            10 => Self::Treasure,
            11 => Self::Led,
            12 => Self::Amber,
            13 => Self::Opal,
            14 => Self::Mantle,
            15 => Self::Diamond,
            16 => Self::Chrome,
            17 => Self::Sol,
            18 => Self::Vault,
            19 => Self::Signet,
            20 => Self::Wishclaw,
            21 => Self::Drum,
            22 => Self::Relic,
            23 => Self::Esper,
            24 => Self::EngineEnch,
            25 => Self::Nature,
            26 => Self::Nick,
            27 => Self::Rog,
            28 => Self::Bird,
            29 => Self::Deathrite,
            30 => Self::Tinder,
            31 => Self::Tataru,
            32 => Self::Ragavan,
            33 => Self::Lotho,
            34 => Self::Heartwood,
            35 => Self::Creature,
            36 => Self::Birgi,
            37 => Self::Wan,
            38 => Self::Ishai,
            39 => Self::Artifact,
            40 => Self::Noble,
            41 => Self::Ignoble,
            42 => Self::Cantor,
            43 => Self::Faerie,
            44 => Self::Bowmasters,
            45 => Self::Cabbage,
            46 => Self::Valley,
            _ => Self::Unknown,
        }
    }

    fn as_name(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Land => "LAND",
            Self::CcLand => "CCLAND",
            Self::City => "CITY",
            Self::Glimmer => "GLIMMER",
            Self::Cavern => "CAVERN",
            Self::Mine => "MINE",
            Self::Tower => "TOWER",
            Self::Vein => "VEIN",
            Self::Petal => "PETAL",
            Self::Treasure => "TREASURE",
            Self::Led => "LED",
            Self::Amber => "AMBER",
            Self::Opal => "OPAL",
            Self::Mantle => "MANTLE",
            Self::Diamond => "DIAMOND",
            Self::Chrome => "CHROME",
            Self::Sol => "SOL",
            Self::Vault => "VAULT",
            Self::Signet => "SIGNET",
            Self::Wishclaw => "WISHCLAW",
            Self::Drum => "DRUM",
            Self::Relic => "RELIC",
            Self::Esper => "ESPER",
            Self::EngineEnch => "ENGINE_ENCH",
            Self::Nature => "NATURE",
            Self::Nick => "NICK",
            Self::Rog => "ROG",
            Self::Bird => "BIRD",
            Self::Deathrite => "DEATHRITE",
            Self::Tinder => "TINDER",
            Self::Tataru => "TATARU",
            Self::Ragavan => "RAGAVAN",
            Self::Lotho => "LOTHO",
            Self::Heartwood => "HEARTWOOD",
            Self::Creature => "CREATURE",
            Self::Birgi => "BIRGI",
            Self::Wan => "WAN",
            Self::Ishai => "ISHAI",
            Self::Artifact => "ARTIFACT",
            Self::Noble => "NOBLE",
            Self::Ignoble => "IGNOBLE",
            Self::Cantor => "CANTOR",
            Self::Faerie => "FAERIE",
            Self::Bowmasters => "BOWMASTERS",
            Self::Cabbage => "CABBAGE",
            Self::Valley => "VALLEY",
        }
    }

    fn is_land(self) -> bool {
        matches!(
            self,
            Self::Land
                | Self::CcLand
                | Self::City
                | Self::Glimmer
                | Self::Cavern
                | Self::Mine
                | Self::Tower
                | Self::Vein
        )
    }

    fn is_creature(self) -> bool {
        matches!(
            self,
            Self::Nick
                | Self::Rog
                | Self::Bird
                | Self::Deathrite
                | Self::Tinder
                | Self::Tataru
                | Self::Ragavan
                | Self::Lotho
                | Self::Esper
                | Self::Heartwood
                | Self::Creature
                | Self::Noble
                | Self::Ignoble
                | Self::Cantor
                | Self::Faerie
                | Self::Bowmasters
                | Self::Cabbage
                | Self::Valley
        )
    }

    fn is_legendary(self) -> bool {
        matches!(
            self,
            Self::Nick
                | Self::Rog
                | Self::Tataru
                | Self::Ragavan
                | Self::Lotho
                | Self::Ishai
                | Self::Cabbage
        )
    }

    fn is_artifact(self) -> bool {
        matches!(
            self,
            Self::Artifact
                | Self::Petal
                | Self::Treasure
                | Self::Led
                | Self::Amber
                | Self::Opal
                | Self::Mantle
                | Self::Diamond
                | Self::Chrome
                | Self::Sol
                | Self::Vault
                | Self::Signet
                | Self::Wishclaw
                | Self::Drum
                | Self::Relic
                | Self::Esper
        )
    }

    fn is_enchantment(self) -> bool {
        matches!(self, Self::EngineEnch | Self::Nature)
    }

    fn creature_mv(self) -> u8 {
        match self {
            Self::Rog => 0,
            Self::Nick
            | Self::Bird
            | Self::Deathrite
            | Self::Tinder
            | Self::Ragavan
            | Self::Esper
            | Self::Creature
            | Self::Noble
            | Self::Ignoble
            | Self::Cantor => 1,
            Self::Tataru | Self::Lotho | Self::Wan | Self::Faerie | Self::Bowmasters => 2,
            Self::Birgi | Self::Heartwood | Self::Cabbage | Self::Valley => 3,
            Self::Ishai => 4,
            _ => 0,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FastPerm(u32);

impl FastPerm {
    const KIND_BITS: u32 = 0;
    const TAPPED_BIT: u32 = 6;
    const FRESH_BIT: u32 = 7;
    const COLOR_BITS: u32 = 8;
    const COUNTER_BITS: u32 = 14;
    const EXTRA_BITS: u32 = 17;

    pub fn new(
        kind: FastPermKind,
        tapped: bool,
        fresh: bool,
        colors: u8,
        counters: u8,
        extra_id: InternId,
    ) -> Self {
        let mut value = (kind as u32) << Self::KIND_BITS;
        value |= (tapped as u32) << Self::TAPPED_BIT;
        value |= (fresh as u32) << Self::FRESH_BIT;
        value |= ((colors & 0b11_1111) as u32) << Self::COLOR_BITS;
        value |= ((counters & 0b111) as u32) << Self::COUNTER_BITS;
        value |= (extra_id as u32) << Self::EXTRA_BITS;
        Self(value)
    }

    pub fn from_fixture(ctx: &mut FastContext, perm: &FixturePerm) -> Self {
        let extra_id = ctx.intern_string(&perm.extra);
        let fresh = perm.extra.ends_with('*');
        let counters = perm.extra.parse::<u8>().unwrap_or(0).min(7);
        let colors = color_mask(perm.extra.trim_end_matches('*'));
        Self::new(
            FastPermKind::from_name(&perm.name),
            perm.tapped,
            fresh,
            colors,
            counters,
            extra_id,
        )
    }

    pub fn kind(self) -> u8 {
        ((self.0 >> Self::KIND_BITS) & 0b11_1111) as u8
    }

    pub fn kind_enum(self) -> FastPermKind {
        FastPermKind::from_u8(self.kind())
    }

    pub fn tapped(self) -> bool {
        ((self.0 >> Self::TAPPED_BIT) & 1) != 0
    }

    pub fn fresh(self) -> bool {
        ((self.0 >> Self::FRESH_BIT) & 1) != 0
    }

    pub fn colors(self) -> u8 {
        ((self.0 >> Self::COLOR_BITS) & 0b11_1111) as u8
    }

    pub fn counters(self) -> u8 {
        ((self.0 >> Self::COUNTER_BITS) & 0b111) as u8
    }

    pub fn extra_id(self) -> InternId {
        (self.0 >> Self::EXTRA_BITS) as InternId
    }

    pub fn with_tapped(self, tapped: bool) -> Self {
        let mut value = self.0;
        if tapped {
            value |= 1 << Self::TAPPED_BIT;
        } else {
            value &= !(1 << Self::TAPPED_BIT);
        }
        Self(value)
    }

    fn fixture_extra(self, ctx: &FastContext) -> String {
        match self.kind_enum() {
            FastPermKind::Mine => self.counters().to_string(),
            FastPermKind::Land
            | FastPermKind::Glimmer
            | FastPermKind::Cavern
            | FastPermKind::Chrome
            | FastPermKind::Bird
            | FastPermKind::Deathrite
            | FastPermKind::Tinder
            | FastPermKind::Tataru
            | FastPermKind::Ragavan
            | FastPermKind::Lotho
            | FastPermKind::Heartwood
            | FastPermKind::Noble
            | FastPermKind::Ignoble
            | FastPermKind::Cantor
            | FastPermKind::Faerie
            | FastPermKind::Bowmasters
            | FastPermKind::Cabbage
            | FastPermKind::Valley
            | FastPermKind::Creature
            | FastPermKind::Birgi
            | FastPermKind::Wan
            | FastPermKind::Ishai
            | FastPermKind::Nick
            | FastPermKind::Rog
            | FastPermKind::Esper => {
                let mut extra = mask_to_colors(self.colors());
                if self.fresh() {
                    extra.push('*');
                }
                extra
            }
            FastPermKind::Vein | FastPermKind::Tower => "C".to_string(),
            FastPermKind::Unknown => ctx.interned_string(self.extra_id()).to_string(),
            _ => String::new(),
        }
    }

    pub fn to_fixture(self, ctx: &FastContext) -> FixturePerm {
        FixturePerm {
            extra: self.fixture_extra(ctx),
            name: self.kind_enum().as_name().to_string(),
            tapped: self.tapped(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct PackedMana(u32);

impl PackedMana {
    pub fn from_mana(mana: Mana) -> Self {
        let mut value = 0u32;
        for (index, amount) in mana.into_iter().enumerate() {
            value |= ((amount.min(15)) as u32) << (index * 4);
        }
        Self(value)
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn to_mana(self) -> Mana {
        let mut mana = [0u8; 6];
        for (index, value) in mana.iter_mut().enumerate() {
            *value = ((self.0 >> (index * 4)) & 0xF) as u8;
        }
        mana
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FastState {
    pub battlefield: SmallVec<[FastPerm; 16]>,
    pub engine_names: SmallVec<[InternId; 4]>,
    pub engine_targets: u8,
    pub graveyard: SmallVec<[CardId; 16]>,
    pub hand: SmallVec<[CardId; 16]>,
    pub library: PersistentLibrary,
    pub mana: PackedMana,
    pub mantle_attached: SmallVec<[InternId; 2]>,
    pub nature_attached: SmallVec<[InternId; 2]>,
    pub land_grave_count: u8,
    pub counters: u16,
    pub flags: u16,
}

impl FastState {
    const LAND_PLAYED: u16 = 1 << 0;
    const NATURE_TAP_USED: u16 = 1 << 1;
    const NATURE_UNTAP_USED: u16 = 1 << 2;
    const RAIN_ACTIVE: u16 = 1 << 3;

    pub fn from_fixture(ctx: &mut FastContext, state: &FixtureState) -> Self {
        let mut battlefield: SmallVec<[FastPerm; 16]> = state
            .battlefield
            .iter()
            .map(|perm| FastPerm::from_fixture(ctx, perm))
            .collect();
        sort_fast_battlefield(ctx, &mut battlefield);

        let mut hand: SmallVec<[CardId; 16]> = state
            .hand
            .iter()
            .map(|card| ctx.intern_card(card))
            .collect();
        hand.sort_unstable();

        let library = state
            .library
            .iter()
            .map(|card| ctx.intern_card(card))
            .collect();

        let mut engine_names: SmallVec<[InternId; 4]> = state
            .engine_names
            .iter()
            .map(|name| ctx.intern_string(name))
            .collect();
        engine_names
            .sort_by(|left, right| ctx.interned_string(*left).cmp(ctx.interned_string(*right)));

        let engine_targets = state
            .engine_targets
            .iter()
            .fold(0u8, |mask, target| mask | engine_target_mask(target));

        let mut mantle_attached: SmallVec<[InternId; 2]> = state
            .mantle_attached
            .iter()
            .map(|item| ctx.intern_string(item))
            .collect();
        mantle_attached
            .sort_by(|left, right| ctx.interned_string(*left).cmp(ctx.interned_string(*right)));

        let mut nature_attached: SmallVec<[InternId; 2]> = state
            .nature_attached
            .iter()
            .map(|item| ctx.intern_string(item))
            .collect();
        nature_attached
            .sort_by(|left, right| ctx.interned_string(*left).cmp(ctx.interned_string(*right)));

        let mut flags = 0u16;
        flags |= (state.land_played as u16) * Self::LAND_PLAYED;
        flags |= (state.nature_tap_used as u16) * Self::NATURE_TAP_USED;
        flags |= (state.nature_untap_used as u16) * Self::NATURE_UNTAP_USED;
        flags |= (state.rain_active as u16) * Self::RAIN_ACTIVE;

        let counters = (state.engine_count as u16)
            | ((state.land_grave_count as u16) << 3)
            | ((state.pact_debt as u16) << 6)
            | ((state.spells_this_turn as u16) << 9)
            | ((state.turn as u16) << 12);

        Self {
            battlefield,
            engine_names,
            engine_targets,
            graveyard: SmallVec::new(),
            hand,
            library,
            mana: PackedMana::from_mana(state.mana),
            mantle_attached,
            nature_attached,
            land_grave_count: state.land_grave_count,
            counters,
            flags,
        }
    }

    pub fn structural_key(&self, ctx: &FastContext, max_turns: u8) -> Self {
        let mut key = self.clone();
        key.mana = PackedMana::zero();
        key.canonicalize_irrelevant_library_tail(ctx, max_turns);
        key
    }

    pub fn canonicalize_irrelevant_library_tail(&mut self, ctx: &FastContext, max_turns: u8) {
        if library_order_matters(ctx, &self.hand) {
            return;
        }
        let turn = ((self.counters >> 12) & 0b1111) as u8;
        let ordered_draws_remaining = max_turns.saturating_sub(turn) as usize;
        if self.library.len() > ordered_draws_remaining {
            self.library = self.library.canonicalized_tail(ordered_draws_remaining);
        }
    }

    fn engine_count(&self) -> u8 {
        (self.counters & 0b111) as u8
    }

    fn set_engine_count(&mut self, value: u8) {
        self.counters = (self.counters & !0b111) | ((value.min(7) as u16) & 0b111);
    }

    fn pact_debt(&self) -> u8 {
        ((self.counters >> 6) & 0b111) as u8
    }

    fn set_pact_debt(&mut self, value: u8) {
        self.counters = (self.counters & !(0b111 << 6)) | (((value.min(7) as u16) & 0b111) << 6);
    }

    fn spells_this_turn(&self) -> u8 {
        ((self.counters >> 9) & 0b111) as u8
    }

    fn set_spells_this_turn(&mut self, value: u8) {
        self.counters = (self.counters & !(0b111 << 9)) | (((value.min(7) as u16) & 0b111) << 9);
    }

    fn turn(&self) -> u8 {
        ((self.counters >> 12) & 0b1111) as u8
    }

    fn set_turn(&mut self, value: u8) {
        self.counters =
            (self.counters & !(0b1111 << 12)) | (((value.min(15) as u16) & 0b1111) << 12);
    }

    fn flag(&self, flag: u16) -> bool {
        (self.flags & flag) != 0
    }

    fn set_flag(&mut self, flag: u16, value: bool) {
        if value {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
    }

    fn land_played(&self) -> bool {
        self.flag(Self::LAND_PLAYED)
    }

    fn rain_active(&self) -> bool {
        self.flag(Self::RAIN_ACTIVE)
    }

    fn set_land_grave_count(&mut self, value: u8) {
        let capped = value.min(7);
        self.land_grave_count = capped;
        self.counters = (self.counters & !(0b111 << 3)) | (((capped as u16) & 0b111) << 3);
    }

    fn add_land_grave(&mut self, amount: u8) {
        self.set_land_grave_count(self.land_grave_count.saturating_add(amount).min(4));
    }

    fn add_graveyard_card(&mut self, ctx: &FastContext, card: CardId) {
        let spec = ctx.card_spec(card);
        if spec.flags.contains(CardFlags::LAND) || spec.flags.contains(CardFlags::MDFC_LAND) {
            self.add_land_grave(1);
        }
        match self.graveyard.binary_search(&card) {
            Ok(index) | Err(index) => self.graveyard.insert(index, card),
        }
    }

    fn remove_graveyard_card(&mut self, ctx: &FastContext, card: CardId) -> bool {
        let Ok(index) = self.graveyard.binary_search(&card) else {
            return false;
        };
        self.graveyard.remove(index);
        let spec = ctx.card_spec(card);
        if spec.flags.contains(CardFlags::LAND) || spec.flags.contains(CardFlags::MDFC_LAND) {
            self.set_land_grave_count(self.land_grave_count.saturating_sub(1));
        }
        true
    }

    fn remove_hand_to_graveyard(&mut self, ctx: &FastContext, card: CardId) -> bool {
        if !self.remove_hand_card(card) {
            return false;
        }
        self.add_graveyard_card(ctx, card);
        true
    }

    fn move_hand_to_graveyard(&mut self, ctx: &FastContext) {
        let cards: Vec<CardId> = self.hand.iter().copied().collect();
        self.hand.clear();
        for card in cards {
            self.add_graveyard_card(ctx, card);
        }
    }

    fn exile_one_land_from_graveyard(&mut self, ctx: &FastContext) {
        if let Some(card) = self.graveyard.iter().copied().find(|card| {
            let spec = ctx.card_spec(*card);
            spec.flags.contains(CardFlags::LAND) || spec.flags.contains(CardFlags::MDFC_LAND)
        }) {
            self.remove_graveyard_card(ctx, card);
        } else {
            self.set_land_grave_count(self.land_grave_count.saturating_sub(1));
        }
    }

    fn set_mana(&mut self, mana: Mana) {
        self.mana = PackedMana::from_mana(mana);
    }

    fn mana(&self) -> Mana {
        self.mana.to_mana()
    }

    fn has_card(&self, card: Option<CardId>) -> bool {
        card.is_some_and(|id| self.hand.binary_search(&id).is_ok())
    }

    fn has_library_card(&self, card: Option<CardId>) -> bool {
        card.is_some_and(|id| self.library.contains_card(id))
    }

    fn remove_hand_card(&mut self, card: CardId) -> bool {
        let Ok(index) = self.hand.binary_search(&card) else {
            return false;
        };
        self.hand.remove(index);
        true
    }

    fn add_hand_card(&mut self, card: CardId) {
        match self.hand.binary_search(&card) {
            Ok(index) | Err(index) => self.hand.insert(index, card),
        }
    }

    fn remove_library_card(&mut self, card: CardId) -> bool {
        let Some(index) = self.library.iter().position(|candidate| *candidate == card) else {
            return false;
        };
        self.library.remove(index);
        true
    }

    fn push_perm(&mut self, ctx: &FastContext, perm: FastPerm) {
        self.battlefield.push(perm);
        sort_fast_battlefield(ctx, &mut self.battlefield);
    }

    fn remove_perm(&mut self, ctx: &FastContext, index: usize) -> FastPerm {
        let perm = self.battlefield.remove(index);
        sort_fast_battlefield(ctx, &mut self.battlefield);
        perm
    }

    fn to_fixture(&self, ctx: &FastContext) -> FixtureState {
        let mut battlefield: Vec<FixturePerm> = self
            .battlefield
            .iter()
            .map(|perm| perm.to_fixture(ctx))
            .collect();
        battlefield.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.tapped.cmp(&right.tapped))
                .then_with(|| left.extra.cmp(&right.extra))
        });

        let mut engine_names: Vec<String> = self
            .engine_names
            .iter()
            .map(|id| ctx.interned_string(*id).to_string())
            .collect();
        engine_names.sort();

        let mut engine_targets = Vec::new();
        for (bit, name) in [
            (1u8 << 1, "ART"),
            (1u8 << 2, "CREATURE"),
            (1u8 << 0, "ENCH"),
            (1u8 << 3, "PERM"),
        ] {
            if (self.engine_targets & bit) != 0 {
                engine_targets.push(name.to_string());
            }
        }

        let mut mantle_attached: Vec<String> = self
            .mantle_attached
            .iter()
            .map(|id| ctx.interned_string(*id).to_string())
            .collect();
        mantle_attached.sort();
        let mut nature_attached: Vec<String> = self
            .nature_attached
            .iter()
            .map(|id| ctx.interned_string(*id).to_string())
            .collect();
        nature_attached.sort();

        FixtureState {
            battlefield,
            engine_count: self.engine_count(),
            engine_names,
            engine_targets,
            hand: self
                .hand
                .iter()
                .map(|id| ctx.card_name(*id).to_string())
                .collect(),
            land_grave_count: self.land_grave_count,
            land_played: self.land_played(),
            library: self
                .library
                .iter()
                .map(|id| ctx.card_name(*id).to_string())
                .collect(),
            mana: self.mana(),
            mantle_attached,
            nature_attached,
            nature_tap_used: self.flag(Self::NATURE_TAP_USED),
            nature_untap_used: self.flag(Self::NATURE_UNTAP_USED),
            pact_debt: self.pact_debt(),
            rain_active: self.rain_active(),
            spells_this_turn: self.spells_this_turn(),
            turn: self.turn(),
        }
    }
}

const DEFAULT_PRIORITY: i32 = 100;
const LAND_PRIORITY: i32 = 800;
const FAST_MANA_PRIORITY: i32 = 850;
const ENGINE_TUTOR_PRIORITY: i32 = 900;
const MANA_ACTION_PRIORITY: i32 = 1000;

#[derive(Debug, Clone)]
pub struct FastAction {
    pub next_state: FastState,
    pub priority: i32,
    pub is_ragavan_attack: bool,
}

impl FastAction {
    fn new(next_state: FastState, priority: i32) -> Self {
        Self {
            next_state,
            priority,
            is_ragavan_attack: false,
        }
    }

    fn ragavan(next_state: FastState, priority: i32) -> Self {
        Self {
            next_state,
            priority,
            is_ragavan_attack: true,
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum FastTap {
    Color(usize),
    Colorless(u8),
    Vault,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum FastGambleMode {
    Off,
    Optimistic,
    StochasticSimplified,
    StochasticUnsupported,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
enum FastPaymentMode {
    #[default]
    Off,
    Generic,
    Packed,
}

#[derive(Debug, Copy, Clone)]
struct FastSearchConfig {
    gamble_mode: FastGambleMode,
    gamble_seed: u64,
}

impl Default for FastSearchConfig {
    fn default() -> Self {
        Self {
            gamble_mode: FastGambleMode::Off,
            gamble_seed: 0,
        }
    }
}

impl FastSearchConfig {
    fn from_parts(mode: Option<&str>, seed: Option<u64>, simplified: bool) -> Self {
        let gamble_mode = match mode.unwrap_or("off") {
            "optimistic" => FastGambleMode::Optimistic,
            "stochastic" if simplified => FastGambleMode::StochasticSimplified,
            "stochastic" => FastGambleMode::StochasticUnsupported,
            _ => FastGambleMode::Off,
        };
        Self {
            gamble_mode,
            gamble_seed: seed.unwrap_or(0),
        }
    }

    fn from_solve_request(request: &SolveKeepRequest) -> Self {
        Self::from_parts(
            request.gamble_mode.as_deref(),
            request.gamble_seed,
            request.simplified_gamble,
        )
    }

    fn from_earliest_request(request: &EarliestRequest) -> Self {
        Self::from_parts(
            request.gamble_mode.as_deref(),
            request.gamble_seed,
            request.simplified_gamble,
        )
    }
}

pub fn generate_fast_actions(ctx: &mut FastContext, state: &FastState) -> Vec<FastAction> {
    generate_fast_actions_with_config(ctx, state, &FastSearchConfig::default())
}

fn generate_fast_actions_with_config(
    ctx: &mut FastContext,
    state: &FastState,
    config: &FastSearchConfig,
) -> Vec<FastAction> {
    match ctx.payment_mode {
        FastPaymentMode::Generic => {
            return generate_fast_generic_payment_directed_actions(ctx, state, config);
        }
        FastPaymentMode::Packed => {
            return generate_fast_packed_payment_directed_actions(ctx, state, config);
        }
        FastPaymentMode::Off => {}
    }
    generate_fast_legacy_actions_with_config(ctx, state, config)
}

fn generate_fast_legacy_actions_with_config(
    ctx: &mut FastContext,
    state: &FastState,
    config: &FastSearchConfig,
) -> Vec<FastAction> {
    let mut actions = Vec::new();
    generate_fast_mana_actions(ctx, &mut actions, state);
    generate_fast_strategic_actions_with_config(ctx, &mut actions, state, config);
    actions
}

fn generate_fast_trace_actions_with_config(
    ctx: &mut FastContext,
    state: &FastState,
    config: &FastSearchConfig,
) -> Vec<FastAction> {
    if ctx.payment_mode == FastPaymentMode::Packed {
        generate_fast_legacy_actions_with_config(ctx, state, config)
    } else {
        generate_fast_actions_with_config(ctx, state, config)
    }
}

fn generate_fast_generic_payment_directed_actions(
    ctx: &mut FastContext,
    state: &FastState,
    config: &FastSearchConfig,
) -> Vec<FastAction> {
    let mut closure = vec![state.clone()];
    let mut best_mana = FxHashMap::default();
    retain_payment_frontier_state(state, &mut best_mana);
    let mut actions = Vec::new();
    generate_fast_strategic_actions_with_config(ctx, &mut actions, state, config);
    let mut cursor = 0;

    while cursor < closure.len() {
        let current = closure[cursor].clone();
        cursor += 1;

        if cursor > 1 && has_payable_costed_action(ctx, &current) {
            generate_fast_costed_strategic_actions(ctx, &mut actions, &current, config);
        }

        let mut mana_actions = Vec::new();
        generate_fast_mana_actions(ctx, &mut mana_actions, &current);
        for action in mana_actions {
            // Ragavan creates a durable resource and must remain a legal standalone action.
            if action.is_ragavan_attack {
                actions.push(action.clone());
            }
            if retain_payment_frontier_state(&action.next_state, &mut best_mana) {
                closure.push(action.next_state);
            }
        }
    }
    actions
}

type FastResourceMask = u32;

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash)]
struct FastManaActivation {
    requires_untapped: FastResourceMask,
    requires_present: FastResourceMask,
    taps: FastResourceMask,
    sacrifices: FastResourceMask,
    mine_spent: FastResourceMask,
    grave_exiles: u8,
    mana: Mana,
}

#[derive(Debug, Clone, Default)]
struct FastManaSourceGroup {
    options: SmallVec<[FastManaActivation; 8]>,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash)]
struct FastFundingNode {
    mana: Mana,
    tapped: FastResourceMask,
    sacrificed: FastResourceMask,
    mine_spent: FastResourceMask,
    grave_exiles: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FastFundingCacheKey {
    battlefield: SmallVec<[FastPerm; 16]>,
    mantle_attached: SmallVec<[InternId; 2]>,
    mana: Mana,
    cost: Cost,
}

impl FastFundingNode {
    fn dominates(self, other: Self) -> bool {
        self.sacrificed == other.sacrificed
            && self.mine_spent == other.mine_spent
            && self.grave_exiles == other.grave_exiles
            && self.tapped & other.tapped == self.tapped
            && self
                .mana
                .iter()
                .zip(other.mana.iter())
                .all(|(have, need)| have >= need)
    }

    fn can_apply(self, activation: FastManaActivation) -> bool {
        activation.requires_present & self.sacrificed == 0
            && activation.requires_untapped & (self.tapped | self.sacrificed) == 0
    }

    fn apply(self, activation: FastManaActivation) -> Self {
        let sacrificed = self.sacrificed | activation.sacrifices;
        Self {
            mana: add_mana(self.mana, activation.mana),
            tapped: (self.tapped | activation.taps) & !sacrificed,
            sacrificed,
            mine_spent: (self.mine_spent | activation.mine_spent) & !sacrificed,
            grave_exiles: self.grave_exiles.saturating_add(activation.grave_exiles),
        }
    }
}

fn generate_fast_packed_payment_directed_actions(
    ctx: &mut FastContext,
    state: &FastState,
    config: &FastSearchConfig,
) -> Vec<FastAction> {
    if !fast_packed_payment_supported(ctx, state) {
        return generate_fast_legacy_actions_with_config(ctx, state, config);
    }

    let mut actions = Vec::new();
    generate_fast_strategic_actions_with_config(ctx, &mut actions, state, config);

    let initial = FastFundingNode {
        mana: state.mana(),
        tapped: fast_initial_tapped_mask(state),
        ..FastFundingNode::default()
    };
    let mut sources = None;
    for cost in fast_relevant_payment_costs(ctx, state) {
        let plans = if ctx.funding_cache_limit == 0 {
            let source_groups = sources.get_or_insert_with(|| fast_mana_source_groups(ctx, state));
            let Some(computed) = compute_fast_funding_plans(
                initial,
                source_groups,
                cost,
                ctx.funding_frontier_limit,
            ) else {
                return generate_fast_legacy_actions_with_config(ctx, state, config);
            };
            computed.into()
        } else {
            let cache_key = FastFundingCacheKey {
                battlefield: state.battlefield.clone(),
                mantle_attached: state.mantle_attached.clone(),
                mana: initial.mana,
                cost,
            };
            if let Some(cached) = ctx.funding_cache.get(&cache_key).cloned() {
                cached
            } else {
                let source_groups =
                    sources.get_or_insert_with(|| fast_mana_source_groups(ctx, state));
                let Some(computed) = compute_fast_funding_plans(
                    initial,
                    source_groups,
                    cost,
                    ctx.funding_frontier_limit,
                ) else {
                    return generate_fast_legacy_actions_with_config(ctx, state, config);
                };
                let computed: Rc<[FastFundingNode]> = computed.into();
                if ctx.funding_cache.len() < ctx.funding_cache_limit {
                    ctx.funding_cache.insert(cache_key, computed.clone());
                }
                computed
            }
        };
        for plan in plans.iter().copied() {
            if plan == initial {
                continue;
            }
            let funded = apply_fast_funding_plan(ctx, state, plan);
            generate_fast_costed_strategic_actions_for_cost(
                ctx,
                &mut actions,
                &funded,
                config,
                cost,
            );
        }
    }
    actions
}

fn fast_packed_payment_supported(ctx: &FastContext, state: &FastState) -> bool {
    if state.battlefield.len() > FastResourceMask::BITS as usize || state.rain_active() {
        return false;
    }
    if state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Ragavan && !perm.tapped() && !perm.fresh())
    {
        return false;
    }
    let templates = state
        .hand
        .iter()
        .fold(ActionTemplateMask::default(), |mask, card| {
            mask.union(ctx.card_spec(*card).action_templates)
        });
    if templates.contains(ActionTemplateMask::OFFER)
        || templates.contains(ActionTemplateMask::SACRIFICE_RITUAL)
        || templates.contains(ActionTemplateMask::ELDRITCH_EVOLUTION)
        || templates.contains(ActionTemplateMask::NEOFORM)
        || templates.contains(ActionTemplateMask::CROP_ROTATION)
        || templates.contains(ActionTemplateMask::BESEECH)
    {
        return false;
    }
    let has_manamorphose = templates.contains(ActionTemplateMask::MANAMORPHOSE);
    let has_noxious = templates.contains(ActionTemplateMask::NOXIOUS);
    if has_manamorphose && has_noxious {
        return false;
    }
    if templates.contains(ActionTemplateMask::LAND)
        && state
            .battlefield
            .iter()
            .any(|perm| perm.kind_enum() == FastPermKind::City)
    {
        return false;
    }
    let has_led = state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Led && !perm.tapped())
        || ctx
            .card_id("Lion's Eye Diamond")
            .is_some_and(|card| state.has_card(Some(card)));
    if has_led
        && (templates.contains(ActionTemplateMask::HAND_TUTOR)
            || state
                .battlefield
                .iter()
                .any(|perm| perm.kind_enum() == FastPermKind::Wishclaw))
    {
        return false;
    }
    !ctx.card_id("Diabolic Intent")
        .is_some_and(|card| state.has_card(Some(card)))
}

fn fast_relevant_payment_costs(ctx: &FastContext, state: &FastState) -> SmallVec<[Cost; 12]> {
    let mut costs = SmallVec::new();
    if !state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Nick)
    {
        push_unique_cost(&mut costs, [0, 0, 0, 0, 1, 0]);
    }
    if state.battlefield.iter().any(|perm| {
        matches!(
            perm.kind_enum(),
            FastPermKind::Mantle | FastPermKind::Wishclaw
        )
    }) {
        push_unique_cost(&mut costs, [1, 0, 0, 0, 0, 0]);
    }
    for card in &state.hand {
        for cost in ctx.card_spec(*card).payment_gate_costs.iter().flatten() {
            push_unique_cost(&mut costs, *cost);
        }
    }
    costs
}

fn push_unique_cost(costs: &mut SmallVec<[Cost; 12]>, cost: Cost) {
    if !costs.contains(&cost) {
        costs.push(cost);
    }
}

fn fast_initial_tapped_mask(state: &FastState) -> FastResourceMask {
    state
        .battlefield
        .iter()
        .enumerate()
        .fold(0, |mask, (index, perm)| {
            mask | if perm.tapped() { 1 << index } else { 0 }
        })
}

fn compute_fast_funding_plans(
    initial: FastFundingNode,
    sources: &[FastManaSourceGroup],
    cost: Cost,
    frontier_limit: usize,
) -> Option<Vec<FastFundingNode>> {
    if can_pay_fast(initial.mana, cost) {
        return Some(vec![initial]);
    }
    let mut frontier = vec![initial];
    let mut funded = Vec::new();
    for source in sources {
        let current = frontier;
        let mut candidates = Vec::with_capacity(current.len() * (source.options.len() + 1));
        for node in current {
            candidates.push(node);
            for option in &source.options {
                if !node.can_apply(*option) {
                    continue;
                }
                let next = node.apply(*option);
                if can_pay_fast(next.mana, cost) {
                    funded.push(next);
                } else {
                    candidates.push(next);
                }
            }
        }
        frontier = pareto_prune_fast_funding(candidates);
        if frontier.len().saturating_add(funded.len()) > frontier_limit {
            return None;
        }
    }
    funded.extend(
        frontier
            .into_iter()
            .filter(|node| can_pay_fast(node.mana, cost)),
    );
    Some(pareto_prune_fast_funding(funded))
}

fn pareto_prune_fast_funding(candidates: Vec<FastFundingNode>) -> Vec<FastFundingNode> {
    let mut unique = FxHashSet::default();
    let unique_candidates: Vec<_> = candidates
        .into_iter()
        .filter(|candidate| unique.insert(*candidate))
        .collect();
    unique_candidates
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, candidate)| {
            (!unique_candidates
                .iter()
                .copied()
                .enumerate()
                .any(|(other_index, other)| index != other_index && other.dominates(candidate)))
            .then_some(candidate)
        })
        .collect()
}

fn fast_mana_source_groups(
    ctx: &mut FastContext,
    state: &FastState,
) -> SmallVec<[FastManaSourceGroup; 16]> {
    let mut groups = SmallVec::new();
    for (index, perm) in state.battlefield.iter().copied().enumerate() {
        let kind = perm.kind_enum();
        if kind == FastPermKind::Tower {
            continue;
        }
        let bit = 1u32 << index;
        let mut group = FastManaSourceGroup::default();
        if !perm.tapped() {
            for option in fast_tap_options(state, perm) {
                let mana = mana_for_fast_tap(option);
                let mut activation = FastManaActivation {
                    requires_untapped: bit,
                    requires_present: bit,
                    mana,
                    ..FastManaActivation::default()
                };
                match kind {
                    FastPermKind::Mine if perm.counters() <= 1 => {
                        activation.sacrifices = bit;
                    }
                    FastPermKind::Mine => {
                        activation.taps = bit;
                        activation.mine_spent = bit;
                    }
                    FastPermKind::Deathrite => {
                        activation.taps = bit;
                        activation.grave_exiles = 1;
                    }
                    _ => activation.taps = bit,
                }
                group.options.push(activation);
            }
        }
        if kind == FastPermKind::Vein {
            group.options.push(FastManaActivation {
                requires_present: bit,
                sacrifices: bit,
                mana: [0, 0, 0, 0, 0, 2],
                ..FastManaActivation::default()
            });
            if !perm.tapped() {
                // Preserve the legacy engine's tap-then-sacrifice Crystal Vein line.
                group.options.push(FastManaActivation {
                    requires_untapped: bit,
                    requires_present: bit,
                    sacrifices: bit,
                    mana: [0, 0, 0, 0, 0, 3],
                    ..FastManaActivation::default()
                });
            }
        }
        if matches!(kind, FastPermKind::Petal | FastPermKind::Treasure) && !perm.tapped() {
            for color in 0..5 {
                group.options.push(FastManaActivation {
                    requires_untapped: bit,
                    requires_present: bit,
                    sacrifices: bit,
                    mana: mana_for_color_index(color),
                    ..FastManaActivation::default()
                });
            }
        }
        if kind == FastPermKind::Tinder {
            group.options.push(FastManaActivation {
                requires_present: bit,
                sacrifices: bit,
                mana: [0, 2, 0, 0, 0, 0],
                ..FastManaActivation::default()
            });
        }
        if kind == FastPermKind::Cantor {
            for color in 0..5 {
                group.options.push(FastManaActivation {
                    requires_present: bit,
                    sacrifices: bit,
                    mana: mana_for_color_index(color),
                    ..FastManaActivation::default()
                });
            }
        }
        if !group.options.is_empty() {
            groups.push(group);
        }
    }

    if state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Mantle)
        && !state.mantle_attached.is_empty()
    {
        for (index, perm) in state.battlefield.iter().copied().enumerate() {
            if perm.tapped()
                || perm.fresh()
                || !perm.kind_enum().is_creature()
                || mantle_key_fast(ctx, perm) != state.mantle_attached
            {
                continue;
            }
            let bit = 1u32 << index;
            groups.push(rainbow_tap_group(bit, bit));
        }
    }

    let creature_indices = unique_creature_indices(state);
    for (relic_index, relic) in state.battlefield.iter().copied().enumerate() {
        if relic.kind_enum() != FastPermKind::Relic {
            continue;
        }
        let _relic_bit = 1u32 << relic_index;
        for creature_index in &creature_indices {
            let creature = state.battlefield[*creature_index];
            if creature.tapped() || !creature.kind_enum().is_legendary() {
                continue;
            }
            let creature_bit = 1u32 << creature_index;
            groups.push(rainbow_tap_group(creature_bit, creature_bit));
        }
    }

    for (drum_index, drum) in state.battlefield.iter().copied().enumerate() {
        if drum.kind_enum() != FastPermKind::Drum || drum.tapped() {
            continue;
        }
        let drum_bit = 1u32 << drum_index;
        let mut group = FastManaSourceGroup::default();
        for creature_index in &creature_indices {
            let creature = state.battlefield[*creature_index];
            if creature.tapped() {
                continue;
            }
            let creature_bit = 1u32 << creature_index;
            for color in 0..5 {
                group.options.push(FastManaActivation {
                    requires_untapped: drum_bit | creature_bit,
                    requires_present: drum_bit | creature_bit,
                    taps: drum_bit | creature_bit,
                    mana: mana_for_color_index(color),
                    ..FastManaActivation::default()
                });
            }
        }
        if !group.options.is_empty() {
            groups.push(group);
        }
    }

    for (tower_index, tower) in state.battlefield.iter().copied().enumerate() {
        if tower.kind_enum() != FastPermKind::Tower || tower.tapped() {
            continue;
        }
        let tower_bit = 1u32 << tower_index;
        let mut group = FastManaSourceGroup::default();
        group.options.push(FastManaActivation {
            requires_untapped: tower_bit,
            requires_present: tower_bit,
            taps: tower_bit,
            mana: [0, 0, 0, 0, 0, 1],
            ..FastManaActivation::default()
        });
        for creature_index in &creature_indices {
            if *creature_index == tower_index {
                continue;
            }
            let creature_bit = 1u32 << creature_index;
            group.options.push(FastManaActivation {
                requires_untapped: tower_bit,
                requires_present: tower_bit | creature_bit,
                taps: tower_bit,
                sacrifices: creature_bit,
                mana: [2, 0, 0, 0, 0, 0],
                ..FastManaActivation::default()
            });
        }
        groups.push(group);
    }
    groups
}

fn rainbow_tap_group(
    requires_untapped: FastResourceMask,
    taps: FastResourceMask,
) -> FastManaSourceGroup {
    let mut group = FastManaSourceGroup::default();
    for color in 0..5 {
        group.options.push(FastManaActivation {
            requires_untapped,
            requires_present: requires_untapped,
            taps,
            mana: mana_for_color_index(color),
            ..FastManaActivation::default()
        });
    }
    group
}

fn mana_for_fast_tap(option: FastTap) -> Mana {
    match option {
        FastTap::Color(color) => mana_for_color_index(color),
        FastTap::Colorless(amount) => [0, 0, 0, 0, 0, amount],
        FastTap::Vault => [0, 0, 0, 0, 0, 3],
    }
}

fn apply_fast_funding_plan(
    ctx: &mut FastContext,
    state: &FastState,
    plan: FastFundingNode,
) -> FastState {
    let mut next = state.clone();
    next.battlefield.clear();
    for (index, perm) in state.battlefield.iter().copied().enumerate() {
        let bit = 1u32 << index;
        if plan.sacrificed & bit != 0 {
            if let Some(card) = graveyard_card_for_perm(ctx, perm) {
                next.add_graveyard_card(ctx, card);
            } else if perm.kind_enum().is_land() {
                next.add_land_grave(1);
            }
            continue;
        }
        let updated = if plan.mine_spent & bit != 0 {
            make_perm(
                ctx,
                FastPermKind::Mine,
                true,
                0,
                false,
                perm.counters().saturating_sub(1),
            )
        } else if plan.tapped & bit != 0 {
            perm.with_tapped(true)
        } else {
            perm
        };
        next.battlefield.push(updated);
    }
    sort_fast_battlefield(ctx, &mut next.battlefield);
    for _ in 0..plan.grave_exiles {
        next.exile_one_land_from_graveyard(ctx);
    }
    next.set_mana(plan.mana);
    next
}

fn can_pay_fast(mana: Mana, cost: Cost) -> bool {
    let mut remaining = 0u16;
    for index in 0..5 {
        if mana[index] < cost[index + 1] {
            return false;
        }
        remaining += u16::from(mana[index] - cost[index + 1]);
    }
    remaining + u16::from(mana[5]) >= u16::from(cost[0])
}

fn payment_cost_matches(filter: Option<Cost>, cost: Cost) -> bool {
    filter.is_none_or(|expected| expected == cost)
}

fn has_payable_costed_action(ctx: &FastContext, state: &FastState) -> bool {
    let mana = state.mana();
    if !state.land_played()
        && state
            .battlefield
            .iter()
            .any(|perm| perm.kind_enum() == FastPermKind::City)
        && state.hand.iter().any(|card| {
            let flags = ctx.card_spec(*card).flags;
            flags.contains(CardFlags::LAND) || flags.contains(CardFlags::MDFC_LAND)
        })
    {
        return true;
    }
    if !state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Nick)
        && can_pay_fast(mana, [0, 0, 0, 0, 1, 0])
    {
        return true;
    }
    if state.battlefield.iter().any(|perm| {
        matches!(
            perm.kind_enum(),
            FastPermKind::Mantle | FastPermKind::Wishclaw
        )
    }) && can_pay_fast(mana, [1, 0, 0, 0, 0, 0])
    {
        return true;
    }
    state.hand.iter().any(|card| {
        ctx.card_spec(*card)
            .payment_gate_costs
            .iter()
            .flatten()
            .any(|cost| can_pay_fast(mana, *cost))
    })
}

fn retain_payment_frontier_state(
    state: &FastState,
    best_mana: &mut FxHashMap<FastState, SmallVec<[Mana; 4]>>,
) -> bool {
    let mut key = state.clone();
    key.mana = PackedMana::zero();
    let candidate = state.mana();
    let Some(existing) = best_mana.get_mut(&key) else {
        let mut values = SmallVec::new();
        values.push(candidate);
        best_mana.insert(key, values);
        return true;
    };
    if existing.iter().any(|mana| {
        mana.iter()
            .zip(candidate.iter())
            .all(|(have, need)| have >= need)
    }) {
        return false;
    }
    existing.retain(|mana| {
        !candidate
            .iter()
            .zip(mana.iter())
            .all(|(have, old)| have >= old)
    });
    existing.push(candidate);
    true
}

fn generate_fast_costed_strategic_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    config: &FastSearchConfig,
) {
    let hand_templates = state
        .hand
        .iter()
        .fold(ActionTemplateMask::default(), |templates, card| {
            templates.union(ctx.card_spec(*card).action_templates)
        });
    if hand_templates.contains(ActionTemplateMask::LAND)
        && state
            .battlefield
            .iter()
            .any(|perm| perm.kind_enum() == FastPermKind::City)
    {
        generate_fast_land_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::ENGINE) {
        generate_fast_engine_actions(ctx, actions, state);
    }
    generate_fast_commander_actions(ctx, actions, state);
    if hand_templates.contains(ActionTemplateMask::ARTIFACT_SPELL) {
        generate_fast_artifact_spell_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::CREATURE) {
        generate_fast_creature_actions(ctx, actions, state);
    }
    generate_fast_mantle_equip_actions(ctx, actions, state);
    if hand_templates.contains(ActionTemplateMask::RITUAL) {
        generate_fast_ritual_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::MANAMORPHOSE) {
        generate_fast_manamorphose_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::RAIN) {
        generate_fast_rain_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::SACRIFICE_RITUAL) {
        generate_fast_sac_spell_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::OFFER) {
        generate_fast_offer_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::GREEN_SUN) {
        generate_fast_green_sun_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::RANGER_CAPTAIN) {
        generate_fast_ranger_captain_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::ELDRITCH_EVOLUTION) {
        generate_fast_eldritch_evolution_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::NEOFORM) {
        generate_fast_neoform_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::CROP_ROTATION) {
        generate_fast_crop_rotation_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::HAND_TUTOR) {
        generate_fast_hand_tutor_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::BESEECH) {
        generate_fast_beseech_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::TOP_TUTOR) {
        generate_fast_top_tutor_actions(ctx, actions, state);
    }
    generate_fast_wishclaw_actions(ctx, actions, state);
    if hand_templates.contains(ActionTemplateMask::GAMBLE) {
        generate_fast_gamble_actions(ctx, actions, state, config);
    }
}

fn generate_fast_costed_strategic_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    config: &FastSearchConfig,
    cost: Cost,
) {
    let matching_templates =
        state
            .hand
            .iter()
            .fold(ActionTemplateMask::default(), |templates, card| {
                let spec = ctx.card_spec(*card);
                if spec
                    .payment_gate_costs
                    .iter()
                    .flatten()
                    .any(|item| *item == cost)
                {
                    templates.union(spec.action_templates)
                } else {
                    templates
                }
            });
    if matching_templates.contains(ActionTemplateMask::ENGINE) {
        generate_fast_engine_actions_for_cost(ctx, actions, state, Some(cost));
    }
    if cost == [0, 0, 0, 0, 1, 0] {
        generate_fast_commander_actions(ctx, actions, state);
    }
    if matching_templates.contains(ActionTemplateMask::ARTIFACT_SPELL) {
        generate_fast_artifact_spell_actions_for_cost(ctx, actions, state, Some(cost));
    }
    if matching_templates.contains(ActionTemplateMask::CREATURE) {
        generate_fast_creature_actions_for_cost(ctx, actions, state, Some(cost));
    }
    if cost == [1, 0, 0, 0, 0, 0] {
        generate_fast_mantle_equip_actions(ctx, actions, state);
    }
    if matching_templates.contains(ActionTemplateMask::RITUAL) {
        generate_fast_ritual_actions_for_cost(ctx, actions, state, Some(cost));
    }
    if matching_templates.contains(ActionTemplateMask::MANAMORPHOSE) {
        generate_fast_manamorphose_actions_for_cost(ctx, actions, state, Some(cost));
    }
    if matching_templates.contains(ActionTemplateMask::RAIN) {
        generate_fast_rain_actions_for_cost(ctx, actions, state, Some(cost));
    }
    if matching_templates.contains(ActionTemplateMask::GREEN_SUN) {
        generate_fast_green_sun_actions_for_cost(ctx, actions, state, Some(cost));
    }
    if matching_templates.contains(ActionTemplateMask::RANGER_CAPTAIN) {
        generate_fast_ranger_captain_actions_for_cost(ctx, actions, state, Some(cost));
    }
    if matching_templates.contains(ActionTemplateMask::HAND_TUTOR) {
        generate_fast_hand_tutor_actions_for_cost(ctx, actions, state, Some(cost));
    }
    if matching_templates.contains(ActionTemplateMask::TOP_TUTOR) {
        generate_fast_top_tutor_actions_for_cost(ctx, actions, state, Some(cost));
    }
    if cost == [1, 0, 0, 0, 0, 0] {
        generate_fast_wishclaw_actions(ctx, actions, state);
    }
    if matching_templates.contains(ActionTemplateMask::GAMBLE) {
        generate_fast_gamble_actions_for_cost(ctx, actions, state, config, Some(cost));
    }
}

fn generate_fast_strategic_actions_with_config(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    config: &FastSearchConfig,
) {
    let hand_templates = state
        .hand
        .iter()
        .fold(ActionTemplateMask::default(), |templates, card| {
            templates.union(ctx.card_spec(*card).action_templates)
        });
    if hand_templates.contains(ActionTemplateMask::ENGINE) {
        generate_fast_engine_actions(ctx, actions, state);
    }
    generate_fast_commander_actions(ctx, actions, state);
    if hand_templates.contains(ActionTemplateMask::LAND) {
        generate_fast_land_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::ZERO_ARTIFACT) {
        generate_fast_zero_artifact_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::CHROME_MOX) {
        generate_fast_chrome_mox_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::MOX_DIAMOND) {
        generate_fast_mox_diamond_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::ARTIFACT_SPELL) {
        generate_fast_artifact_spell_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::CREATURE) {
        generate_fast_creature_actions(ctx, actions, state);
    }
    generate_fast_mantle_equip_actions(ctx, actions, state);
    if hand_templates.contains(ActionTemplateMask::SPIRIT_GUIDE) {
        generate_fast_spirit_guide_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::RITUAL) {
        generate_fast_ritual_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::MANAMORPHOSE) {
        generate_fast_manamorphose_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::RAIN) {
        generate_fast_rain_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::SACRIFICE_RITUAL) {
        generate_fast_sac_spell_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::OFFER) {
        generate_fast_offer_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::NOXIOUS) {
        generate_fast_noxious_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::SUMMONERS_PACT) {
        generate_fast_summoners_pact_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::GREEN_SUN) {
        generate_fast_green_sun_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::RANGER_CAPTAIN) {
        generate_fast_ranger_captain_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::ELDRITCH_EVOLUTION) {
        generate_fast_eldritch_evolution_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::NEOFORM) {
        generate_fast_neoform_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::CROP_ROTATION) {
        generate_fast_crop_rotation_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::HAND_TUTOR) {
        generate_fast_hand_tutor_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::BESEECH) {
        generate_fast_beseech_actions(ctx, actions, state);
    }
    if hand_templates.contains(ActionTemplateMask::TOP_TUTOR) {
        generate_fast_top_tutor_actions(ctx, actions, state);
    }
    generate_fast_wishclaw_actions(ctx, actions, state);
    if hand_templates.contains(ActionTemplateMask::GAMBLE) {
        generate_fast_gamble_actions(ctx, actions, state, config);
    }
}

fn generate_fast_mana_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    for (index, perm) in state.battlefield.iter().copied().enumerate() {
        let kind = perm.kind_enum();
        if !perm.tapped() {
            for opt in fast_tap_options(state, perm) {
                let mut next = state.clone();
                match (kind, opt) {
                    (FastPermKind::Mine, FastTap::Color(color)) => {
                        let counters = perm.counters();
                        if counters <= 1 {
                            remove_perm_to_graveyard(ctx, &mut next, index);
                        } else {
                            next.battlefield[index] =
                                make_perm(ctx, FastPermKind::Mine, true, 0, false, counters - 1);
                        }
                        next.set_mana(add_mana(state.mana(), mana_for_color_index(color)));
                        sort_fast_battlefield(ctx, &mut next.battlefield);
                        actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
                    }
                    (_, FastTap::Vault) => {
                        next.battlefield[index] = perm.with_tapped(true);
                        next.set_mana(add_mana(state.mana(), [0, 0, 0, 0, 0, 3]));
                        sort_fast_battlefield(ctx, &mut next.battlefield);
                        actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
                    }
                    (FastPermKind::Deathrite, FastTap::Color(color)) => {
                        next.battlefield[index] = perm.with_tapped(true);
                        next.exile_one_land_from_graveyard(ctx);
                        next.set_mana(add_mana(state.mana(), mana_for_color_index(color)));
                        sort_fast_battlefield(ctx, &mut next.battlefield);
                        actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
                    }
                    (_, FastTap::Color(color)) => {
                        next.battlefield[index] = perm.with_tapped(true);
                        next.set_mana(add_mana(state.mana(), mana_for_color_index(color)));
                        sort_fast_battlefield(ctx, &mut next.battlefield);
                        actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
                    }
                    (_, FastTap::Colorless(amount)) => {
                        next.battlefield[index] = perm.with_tapped(true);
                        next.set_mana(add_mana(state.mana(), [0, 0, 0, 0, 0, amount]));
                        sort_fast_battlefield(ctx, &mut next.battlefield);
                        actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
                    }
                }
            }
            if kind == FastPermKind::Vein {
                let mut next = state.clone();
                remove_land_perm_to_graveyard(ctx, &mut next, index);
                next.set_mana(add_mana(state.mana(), [0, 0, 0, 0, 0, 2]));
                actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
            }
            if kind == FastPermKind::Tower {
                for creature_index in unique_creature_indices(state) {
                    if creature_index == index || creature_index >= state.battlefield.len() {
                        continue;
                    }
                    let mut next = state.clone();
                    next.battlefield[index] = perm.with_tapped(true);
                    remove_perm_to_graveyard(ctx, &mut next, creature_index);
                    next.set_mana(add_mana(state.mana(), [2, 0, 0, 0, 0, 0]));
                    actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
                }
            }
        }
        if matches!(kind, FastPermKind::Petal | FastPermKind::Treasure) && !perm.tapped() {
            for color in 0..5 {
                let mut next = state.clone();
                remove_perm_to_graveyard(ctx, &mut next, index);
                next.set_mana(add_mana(state.mana(), mana_for_color_index(color)));
                actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
            }
        }
        if kind == FastPermKind::Tinder {
            let mut next = state.clone();
            remove_perm_to_graveyard(ctx, &mut next, index);
            next.set_mana(add_mana(state.mana(), [0, 2, 0, 0, 0, 0]));
            actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
        }
        if kind == FastPermKind::Cantor {
            for color in 0..5 {
                let mut next = state.clone();
                remove_perm_to_graveyard(ctx, &mut next, index);
                next.set_mana(add_mana(state.mana(), mana_for_color_index(color)));
                actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
            }
        }
        if kind == FastPermKind::Drum && !perm.tapped() {
            for creature_index in unique_creature_indices(state) {
                if creature_index == index || creature_index >= state.battlefield.len() {
                    continue;
                }
                let creature = state.battlefield[creature_index];
                if creature.tapped() {
                    continue;
                }
                for color in 0..5 {
                    let mut next = state.clone();
                    next.battlefield[index] = perm.with_tapped(true);
                    next.battlefield[creature_index] = creature.with_tapped(true);
                    next.set_mana(add_mana(state.mana(), mana_for_color_index(color)));
                    sort_fast_battlefield(ctx, &mut next.battlefield);
                    actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
                }
            }
        }
        if kind == FastPermKind::Relic {
            for creature_index in unique_creature_indices(state) {
                let creature = state.battlefield[creature_index];
                if creature.tapped() || !creature.kind_enum().is_legendary() {
                    continue;
                }
                for color in 0..5 {
                    let mut next = state.clone();
                    next.battlefield[creature_index] = creature.with_tapped(true);
                    next.set_mana(add_mana(state.mana(), mana_for_color_index(color)));
                    sort_fast_battlefield(ctx, &mut next.battlefield);
                    actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
                }
            }
        }
        if !state.mantle_attached.is_empty()
            && !perm.tapped()
            && !perm.fresh()
            && perm.kind_enum().is_creature()
            && state
                .battlefield
                .iter()
                .any(|candidate| candidate.kind_enum() == FastPermKind::Mantle)
            && mantle_key_fast(ctx, perm) == state.mantle_attached
        {
            for color in 0..5 {
                let mut next = state.clone();
                next.battlefield[index] = perm.with_tapped(true);
                next.set_mana(add_mana(state.mana(), mana_for_color_index(color)));
                sort_fast_battlefield(ctx, &mut next.battlefield);
                actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
            }
        }
        if kind == FastPermKind::Ragavan && !perm.tapped() && !perm.fresh() {
            let mut next = state.clone();
            next.battlefield[index] = perm.with_tapped(true);
            let treasure = make_perm(ctx, FastPermKind::Treasure, false, 0, false, 0);
            next.push_perm(ctx, treasure);
            actions.push(FastAction::ragavan(next, DEFAULT_PRIORITY));
        }
        if state.rain_active() && kind.is_land() {
            let mut next = state.clone();
            remove_land_perm_to_graveyard(ctx, &mut next, index);
            next.set_mana(add_mana(state.mana(), [1, 0, 0, 0, 0, 0]));
            actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
        }
    }
}

fn generate_fast_engine_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    generate_fast_engine_actions_for_cost(ctx, actions, state, None);
}

fn generate_fast_engine_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    cost_filter: Option<Cost>,
) {
    for name in ["Rhystic Study", "Heartwood Storyteller"] {
        let Some(card) = ctx.card_id(name) else {
            continue;
        };
        if !state.has_card(Some(card)) {
            continue;
        }
        let Some((cost, target_mask, perm)) = engine_native_fast(ctx, card) else {
            continue;
        };
        if !payment_cost_matches(cost_filter, cost) {
            continue;
        }
        for mana in pay_options(state.mana(), cost) {
            let mut next = state.clone();
            next.remove_hand_card(card);
            next.push_perm(ctx, perm);
            next.set_mana(mana);
            next = after_cast_fast(ctx, state, next);
            next = add_engine_fast(ctx, next, target_mask, name);
            actions.push(FastAction::new(next, label_priority_name(name)));
        }
    }
}

fn generate_fast_commander_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    if state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Nick)
    {
        return;
    }
    for mana in pay_options(state.mana(), [0, 0, 0, 0, 1, 0]) {
        let mut next = state.clone();
        let nick = make_perm(ctx, FastPermKind::Nick, false, color_mask("WUBRG"), true, 0);
        next.push_perm(ctx, nick);
        next.set_mana(mana);
        actions.push(FastAction::new(
            after_cast_fast(ctx, state, next),
            DEFAULT_PRIORITY,
        ));
    }
}

fn generate_fast_land_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    if state.land_played() {
        return;
    }
    for card in state.hand.iter().copied().collect::<Vec<_>>() {
        let flags = ctx.card_spec(card).flags;
        if !flags.contains(CardFlags::LAND) && !flags.contains(CardFlags::MDFC_LAND) {
            continue;
        }
        for (perm, library, grave_inc) in land_options_fast(ctx, card, &state.library) {
            let mut next = state.clone();
            let city_indices: Vec<usize> = next
                .battlefield
                .iter()
                .enumerate()
                .filter_map(|(index, candidate)| {
                    (candidate.kind_enum() == FastPermKind::City).then_some(index)
                })
                .collect();
            for index in city_indices.into_iter().rev() {
                remove_land_perm_to_graveyard(ctx, &mut next, index);
            }
            next.battlefield.push(perm);
            sort_fast_battlefield(ctx, &mut next.battlefield);
            next.remove_hand_card(card);
            next.library = library.into();
            next.set_flag(FastState::LAND_PLAYED, true);
            if grave_inc > 0 {
                next.add_graveyard_card(ctx, card);
            }
            actions.push(FastAction::new(next, LAND_PRIORITY));
        }
    }
}

fn generate_fast_zero_artifact_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    for (name, kind) in [
        ("Lotus Petal", FastPermKind::Petal),
        ("Lion's Eye Diamond", FastPermKind::Led),
        ("Mox Amber", FastPermKind::Amber),
        ("Mox Opal", FastPermKind::Opal),
        ("Paradise Mantle", FastPermKind::Mantle),
    ] {
        let Some(card) = ctx.card_id(name) else {
            continue;
        };
        if !state.has_card(Some(card)) {
            continue;
        }
        let mut next = state.clone();
        next.remove_hand_card(card);
        let perm = make_perm(ctx, kind, false, 0, false, 0);
        next.push_perm(ctx, perm);
        actions.push(FastAction::new(
            after_cast_fast(ctx, state, next),
            FAST_MANA_PRIORITY,
        ));
    }
}

fn generate_fast_chrome_mox_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    let Some(chrome) = ctx.card_id("Chrome Mox") else {
        return;
    };
    if !state.has_card(Some(chrome)) {
        return;
    }
    for imprint in state.hand.iter().copied().collect::<Vec<_>>() {
        let imprint_name = ctx.card_name(imprint).to_string();
        let imprint_spec = ctx.card_spec(imprint);
        let imprint_colors = imprint_spec.color_mask;
        if imprint == chrome
            || imprint_spec.flags.contains(CardFlags::LAND)
            || imprint_spec.flags.contains(CardFlags::ARTIFACT)
            || imprint_colors == 0
        {
            continue;
        }
        let mut next = state.clone();
        next.remove_hand_card(chrome);
        next.remove_hand_card(imprint);
        let chrome_perm = make_perm(ctx, FastPermKind::Chrome, false, imprint_colors, false, 0);
        next.push_perm(ctx, chrome_perm);
        let priority = FAST_MANA_PRIORITY.max(label_priority_name(&imprint_name));
        actions.push(FastAction::new(after_cast_fast(ctx, state, next), priority));
    }
}

fn generate_fast_mox_diamond_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    let Some(diamond) = ctx.card_id("Mox Diamond") else {
        return;
    };
    if !state.has_card(Some(diamond)) {
        return;
    }
    for land in state.hand.iter().copied().collect::<Vec<_>>() {
        if !ctx.card_spec(land).flags.contains(CardFlags::LAND) {
            continue;
        }
        let mut next = state.clone();
        next.remove_hand_card(diamond);
        next.remove_hand_to_graveyard(ctx, land);
        let diamond_perm = make_perm(ctx, FastPermKind::Diamond, false, 0, false, 0);
        next.push_perm(ctx, diamond_perm);
        actions.push(FastAction::new(
            after_cast_fast(ctx, state, next),
            FAST_MANA_PRIORITY,
        ));
    }
}

fn generate_fast_artifact_spell_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    generate_fast_artifact_spell_actions_for_cost(ctx, actions, state, None);
}

fn generate_fast_artifact_spell_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    cost_filter: Option<Cost>,
) {
    for (name, kind, cost) in [
        ("Sol Ring", FastPermKind::Sol, [1, 0, 0, 0, 0, 0]),
        ("Mana Vault", FastPermKind::Vault, [1, 0, 0, 0, 0, 0]),
        ("Arcane Signet", FastPermKind::Signet, [2, 0, 0, 0, 0, 0]),
        ("Relic of Legends", FastPermKind::Relic, [3, 0, 0, 0, 0, 0]),
        (
            "Wishclaw Talisman",
            FastPermKind::Wishclaw,
            [1, 1, 0, 0, 0, 0],
        ),
        ("Springleaf Drum", FastPermKind::Drum, [1, 0, 0, 0, 0, 0]),
    ] {
        if !payment_cost_matches(cost_filter, cost) {
            continue;
        }
        let perm = make_perm(ctx, kind, false, 0, false, 0);
        cast_fast_cost_action(ctx, actions, state, name, perm, cost, FAST_MANA_PRIORITY);
    }
}

fn generate_fast_creature_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    generate_fast_creature_actions_for_cost(ctx, actions, state, None);
}

fn generate_fast_creature_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    cost_filter: Option<Cost>,
) {
    for (name, costs) in [
        ("Birds of Paradise", &[[0, 0, 0, 0, 0, 1]][..]),
        (
            "Deathrite Shaman",
            &[[0, 1, 0, 0, 0, 0], [0, 0, 0, 0, 0, 1]][..],
        ),
        ("Esper Sentinel", &[[0, 0, 0, 0, 1, 0]][..]),
        ("Faerie Mastermind", &[[1, 0, 0, 1, 0, 0]][..]),
        ("Ignoble Hierarch", &[[0, 0, 0, 0, 0, 1]][..]),
        ("Lotho, Corrupt Shirriff", &[[0, 1, 0, 0, 1, 0]][..]),
        ("Noble Hierarch", &[[0, 0, 0, 0, 0, 1]][..]),
        ("Orcish Bowmasters", &[[1, 1, 0, 0, 0, 0]][..]),
        ("Ragavan, Nimble Pilferer", &[[0, 0, 1, 0, 0, 0]][..]),
        ("The Cabbage Merchant", &[[2, 0, 0, 0, 0, 1]][..]),
        ("Tinder Wall", &[[0, 0, 0, 0, 0, 1]][..]),
        ("Valley Floodcaller", &[[2, 0, 0, 1, 0, 0]][..]),
        ("Wild Cantor", &[[0, 0, 1, 0, 0, 0], [0, 0, 0, 0, 0, 1]][..]),
    ] {
        if !costs
            .iter()
            .any(|cost| payment_cost_matches(cost_filter, *cost))
        {
            continue;
        }
        let Some(card) = ctx.card_id(name) else {
            continue;
        };
        if !state.has_card(Some(card)) {
            continue;
        }
        for cost in costs {
            if !payment_cost_matches(cost_filter, *cost) {
                continue;
            }
            for mana in pay_options(state.mana(), *cost) {
                let mut next = state.clone();
                next.remove_hand_card(card);
                let perm = creature_perm_fast(ctx, name);
                next.push_perm(ctx, perm);
                next.set_mana(mana);
                actions.push(FastAction::new(
                    after_cast_fast(ctx, state, next),
                    label_priority_name(name),
                ));
            }
        }
    }
}

fn generate_fast_mantle_equip_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    if !state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Mantle)
    {
        return;
    }
    for creature_index in unique_creature_indices(state) {
        let creature = state.battlefield[creature_index];
        let key = mantle_key_fast(ctx, creature);
        if key == state.mantle_attached {
            continue;
        }
        for mana in pay_options(state.mana(), [1, 0, 0, 0, 0, 0]) {
            let mut next = state.clone();
            next.mantle_attached = key.clone();
            next.set_mana(mana);
            actions.push(FastAction::new(next, FAST_MANA_PRIORITY));
        }
    }
}

fn generate_fast_spirit_guide_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    for (name, add) in [
        ("Simian Spirit Guide", [0, 1, 0, 0, 0, 0]),
        ("Elvish Spirit Guide", [0, 0, 0, 0, 1, 0]),
    ] {
        let Some(card) = ctx.card_id(name) else {
            continue;
        };
        if !state.has_card(Some(card)) {
            continue;
        }
        let mut next = state.clone();
        next.remove_hand_card(card);
        next.set_mana(add_mana(state.mana(), add));
        actions.push(FastAction::new(next, MANA_ACTION_PRIORITY));
    }
}

fn generate_fast_ritual_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    generate_fast_ritual_actions_for_cost(ctx, actions, state, None);
}

fn generate_fast_ritual_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    cost_filter: Option<Cost>,
) {
    for (name, cost, add) in [
        ("Dark Ritual", [0, 1, 0, 0, 0, 0], [3, 0, 0, 0, 0, 0]),
        ("Rite of Flame", [0, 0, 1, 0, 0, 0], [0, 2, 0, 0, 0, 0]),
    ] {
        if !payment_cost_matches(cost_filter, cost) {
            continue;
        }
        let Some(card) = ctx.card_id(name) else {
            continue;
        };
        if !state.has_card(Some(card)) {
            continue;
        }
        for mana in pay_options(state.mana(), cost) {
            let mut next = state.clone();
            next.remove_hand_to_graveyard(ctx, card);
            next.set_mana(add_mana(mana, add));
            actions.push(FastAction::new(
                after_cast_fast(ctx, state, next),
                DEFAULT_PRIORITY,
            ));
        }
    }
}

fn generate_fast_manamorphose_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    generate_fast_manamorphose_actions_for_cost(ctx, actions, state, None);
}

fn generate_fast_manamorphose_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    cost_filter: Option<Cost>,
) {
    let Some(card) = ctx.card_id("Manamorphose") else {
        return;
    };
    if !state.has_card(Some(card)) {
        return;
    }
    for cost in [[1, 0, 1, 0, 0, 0], [1, 0, 0, 0, 0, 1]] {
        if !payment_cost_matches(cost_filter, cost) {
            continue;
        }
        for mana in pay_options(state.mana(), cost) {
            for first_color in 0..5 {
                for second_color in 0..5 {
                    let mut next = state.clone();
                    next.remove_hand_to_graveyard(ctx, card);
                    let next_mana = add_mana(
                        add_mana(mana, mana_for_color_index(first_color)),
                        mana_for_color_index(second_color),
                    );
                    next.set_mana(next_mana);
                    next = draw_card_fast(next);
                    actions.push(FastAction::new(
                        after_cast_fast(ctx, state, next),
                        label_priority_name("Manamorphose"),
                    ));
                }
            }
        }
    }
}

fn generate_fast_dark_ritual_action(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    let Some(card) = ctx.card_id("Dark Ritual") else {
        return;
    };
    if !state.has_card(Some(card)) {
        return;
    }
    for mana in pay_options(state.mana(), [0, 1, 0, 0, 0, 0]) {
        let mut next = state.clone();
        next.remove_hand_to_graveyard(ctx, card);
        next.set_mana(add_mana(mana, [3, 0, 0, 0, 0, 0]));
        actions.push(FastAction::new(
            after_cast_fast(ctx, state, next),
            DEFAULT_PRIORITY,
        ));
    }
}

fn generate_fast_rain_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    generate_fast_rain_actions_for_cost(ctx, actions, state, None);
}

fn generate_fast_rain_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    cost_filter: Option<Cost>,
) {
    let cost = [0, 1, 0, 0, 0, 0];
    if !payment_cost_matches(cost_filter, cost) {
        return;
    }
    let Some(rain) = ctx.card_id("Rain of Filth") else {
        return;
    };
    if !state.has_card(Some(rain)) {
        return;
    }
    for mana in pay_options(state.mana(), cost) {
        let mut next = state.clone();
        next.remove_hand_to_graveyard(ctx, rain);
        next.set_mana(mana);
        next.set_flag(FastState::RAIN_ACTIVE, true);
        actions.push(FastAction::new(
            after_cast_fast(ctx, state, next),
            DEFAULT_PRIORITY,
        ));
    }
}

fn generate_fast_sac_spell_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    let Some(culling) = ctx.card_id("Culling the Weak") else {
        return;
    };
    if !state.has_card(Some(culling)) {
        return;
    }
    for creature_index in unique_creature_indices(state) {
        for mana in pay_options(state.mana(), [0, 1, 0, 0, 0, 0]) {
            let base = sac_creature_fast(ctx, state, creature_index);
            let mut next = base.clone();
            next.remove_hand_to_graveyard(ctx, culling);
            next.set_mana(add_mana(mana, [4, 0, 0, 0, 0, 0]));
            actions.push(FastAction::new(
                after_cast_fast(ctx, &base, next),
                DEFAULT_PRIORITY,
            ));
        }
    }
}

fn generate_fast_offer_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    let Some(offer) = ctx.card_id("An Offer You Can't Refuse") else {
        return;
    };
    if !state.has_card(Some(offer)) {
        return;
    }
    for bait in state.hand.iter().copied().collect::<Vec<_>>() {
        if bait == offer || !offer_bait_can_help(ctx, state, bait) {
            continue;
        }
        let bait_name = ctx.card_name(bait).to_string();
        for bait_cost in offer_counterable_costs(&bait_name) {
            for after_bait_mana in pay_options(state.mana(), bait_cost) {
                let mut bait_cast = state.clone();
                bait_cast.remove_hand_to_graveyard(ctx, bait);
                bait_cast.set_mana(after_bait_mana);
                bait_cast = after_cast_fast(ctx, state, bait_cast);
                for after_offer_mana in pay_options(bait_cast.mana(), [0, 0, 0, 1, 0, 0]) {
                    let mut next = bait_cast.clone();
                    next.remove_hand_to_graveyard(ctx, offer);
                    next.set_mana(after_offer_mana);
                    let treasure = make_perm(ctx, FastPermKind::Treasure, false, 0, false, 0);
                    next.push_perm(ctx, treasure);
                    next.push_perm(ctx, treasure);
                    actions.push(FastAction::new(
                        after_cast_fast(ctx, &bait_cast, next),
                        label_priority_name(&bait_name),
                    ));
                }
            }
        }
    }
}

fn generate_fast_noxious_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    let Some(noxious) = ctx.card_id("Noxious Revival") else {
        return;
    };
    if !state.has_card(Some(noxious)) || state.graveyard.is_empty() {
        return;
    }
    let targets: Vec<CardId> = state
        .graveyard
        .iter()
        .copied()
        .filter(|target| *target != noxious && noxious_target_can_help(ctx, *target))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    for target in targets {
        let mut next = state.clone();
        next.remove_hand_to_graveyard(ctx, noxious);
        next.remove_graveyard_card(ctx, target);
        next.library.insert(0, target);
        actions.push(FastAction::new(
            after_cast_fast(ctx, state, next),
            tutor_target_priority_fast(ctx, "Noxious Revival", target),
        ));
    }
}

fn noxious_target_can_help(ctx: &FastContext, target: CardId) -> bool {
    ctx.card_name(target) != "Blank"
}

fn generate_fast_summoners_pact_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    let Some(pact) = ctx.card_id("Summoner's Pact") else {
        return;
    };
    if !state.has_card(Some(pact)) {
        return;
    }
    for target_name in [
        "Elvish Spirit Guide",
        "Tinder Wall",
        "Birds of Paradise",
        "Deathrite Shaman",
        "Wild Cantor",
        "Noble Hierarch",
        "Ignoble Hierarch",
        "Heartwood Storyteller",
    ] {
        let Some(target) = ctx.card_id(target_name) else {
            continue;
        };
        if !state.has_library_card(Some(target)) {
            continue;
        }
        let mut next = state.clone();
        next.remove_hand_to_graveyard(ctx, pact);
        next.add_hand_card(target);
        next.remove_library_card(target);
        obscure_library_top_after_shuffle(ctx, &mut next.library);
        next.set_pact_debt(next.pact_debt().saturating_add(1).min(2));
        actions.push(FastAction::new(
            after_cast_fast(ctx, state, next),
            ENGINE_TUTOR_PRIORITY,
        ));
    }
}

fn generate_fast_green_sun_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    generate_fast_green_sun_actions_for_cost(ctx, actions, state, None);
}

fn generate_fast_green_sun_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    cost_filter: Option<Cost>,
) {
    let Some(gsz) = ctx.card_id("Green Sun's Zenith") else {
        return;
    };
    if !state.has_card(Some(gsz)) {
        return;
    }
    let small_cost = [1, 0, 0, 0, 0, 1];
    if payment_cost_matches(cost_filter, small_cost) {
        for target_name in [
            "Tinder Wall",
            "Birds of Paradise",
            "Deathrite Shaman",
            "Wild Cantor",
            "Noble Hierarch",
            "Ignoble Hierarch",
        ] {
            let Some(target) = ctx.card_id(target_name) else {
                continue;
            };
            if !state.has_library_card(Some(target)) {
                continue;
            }
            for mana in pay_options(state.mana(), small_cost) {
                let mut next = state.clone();
                next.remove_hand_card(gsz);
                next.remove_library_card(target);
                obscure_library_top_after_shuffle(ctx, &mut next.library);
                let perm = creature_perm_fast(ctx, target_name);
                next.push_perm(ctx, perm);
                next.set_mana(mana);
                actions.push(FastAction::new(
                    after_cast_fast(ctx, state, next),
                    ENGINE_TUTOR_PRIORITY,
                ));
            }
        }
    }
    let heartwood_cost = [3, 0, 0, 0, 0, 1];
    if !payment_cost_matches(cost_filter, heartwood_cost) {
        return;
    }
    let Some(heartwood) = ctx.card_id("Heartwood Storyteller") else {
        return;
    };
    if !state.has_library_card(Some(heartwood)) {
        return;
    }
    for mana in pay_options(state.mana(), heartwood_cost) {
        let mut next = state.clone();
        next.remove_hand_card(gsz);
        next.remove_library_card(heartwood);
        obscure_library_top_after_shuffle(ctx, &mut next.library);
        let perm = make_perm(
            ctx,
            FastPermKind::Heartwood,
            false,
            color_mask("G"),
            true,
            0,
        );
        next.push_perm(ctx, perm);
        next.set_mana(mana);
        let casted = after_cast_fast(ctx, state, next);
        let next = add_engine_fast(
            ctx,
            casted,
            engine_target_mask("CREATURE"),
            "Heartwood Storyteller",
        );
        actions.push(FastAction::new(next, ENGINE_TUTOR_PRIORITY));
    }
}

fn generate_fast_ranger_captain_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    generate_fast_ranger_captain_actions_for_cost(ctx, actions, state, None);
}

fn generate_fast_ranger_captain_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    cost_filter: Option<Cost>,
) {
    let cost = [1, 0, 0, 0, 2, 0];
    if !payment_cost_matches(cost_filter, cost) {
        return;
    }
    let Some(ranger) = ctx.card_id("Ranger-Captain of Eos") else {
        return;
    };
    let Some(esper) = ctx.card_id("Esper Sentinel") else {
        return;
    };
    if !state.has_card(Some(ranger)) || !state.has_library_card(Some(esper)) {
        return;
    }
    for mana in pay_options(state.mana(), cost) {
        let mut next = state.clone();
        next.remove_hand_card(ranger);
        next.add_hand_card(esper);
        next.remove_library_card(esper);
        obscure_library_top_after_shuffle(ctx, &mut next.library);
        let perm = make_perm(ctx, FastPermKind::Creature, false, color_mask("W"), true, 0);
        next.push_perm(ctx, perm);
        next.set_mana(mana);
        actions.push(FastAction::new(
            after_cast_fast(ctx, state, next),
            ENGINE_TUTOR_PRIORITY,
        ));
    }
}

fn generate_fast_eldritch_evolution_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    let Some(evolution) = ctx.card_id("Eldritch Evolution") else {
        return;
    };
    let Some(heartwood) = ctx.card_id("Heartwood Storyteller") else {
        return;
    };
    if !state.has_card(Some(evolution)) || !state.has_library_card(Some(heartwood)) {
        return;
    }
    for creature_index in unique_creature_indices(state) {
        let creature = state.battlefield[creature_index];
        if creature.kind_enum().creature_mv().saturating_add(2) < 3 {
            continue;
        }
        for mana in pay_options(state.mana(), [1, 0, 0, 0, 0, 2]) {
            let base = sac_creature_fast(ctx, state, creature_index);
            let mut next = base.clone();
            next.remove_hand_card(evolution);
            next.remove_library_card(heartwood);
            obscure_library_top_after_shuffle(ctx, &mut next.library);
            let perm = make_perm(
                ctx,
                FastPermKind::Heartwood,
                false,
                color_mask("G"),
                true,
                0,
            );
            next.push_perm(ctx, perm);
            next.set_mana(mana);
            let casted = after_cast_fast(ctx, &base, next);
            let next = add_engine_fast(
                ctx,
                casted,
                engine_target_mask("CREATURE"),
                "Heartwood Storyteller",
            );
            actions.push(FastAction::new(next, ENGINE_TUTOR_PRIORITY));
        }
    }
}

fn generate_fast_neoform_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    let Some(neoform) = ctx.card_id("Neoform") else {
        return;
    };
    let Some(heartwood) = ctx.card_id("Heartwood Storyteller") else {
        return;
    };
    if !state.has_card(Some(neoform)) || !state.has_library_card(Some(heartwood)) {
        return;
    }
    for creature_index in unique_creature_indices(state) {
        let creature = state.battlefield[creature_index];
        if creature.kind_enum().creature_mv().saturating_add(1) != 3 {
            continue;
        }
        for mana in pay_options(state.mana(), [0, 0, 0, 1, 0, 1]) {
            let base = sac_creature_fast(ctx, state, creature_index);
            let mut next = base.clone();
            next.remove_hand_to_graveyard(ctx, neoform);
            next.remove_library_card(heartwood);
            obscure_library_top_after_shuffle(ctx, &mut next.library);
            let perm = make_perm(
                ctx,
                FastPermKind::Heartwood,
                false,
                color_mask("G"),
                true,
                0,
            );
            next.push_perm(ctx, perm);
            next.set_mana(mana);
            let casted = after_cast_fast(ctx, &base, next);
            let next = add_engine_fast(
                ctx,
                casted,
                engine_target_mask("CREATURE"),
                "Heartwood Storyteller",
            );
            actions.push(FastAction::new(next, ENGINE_TUTOR_PRIORITY));
        }
    }
}

fn generate_fast_crop_rotation_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    let Some(crop) = ctx.card_id("Crop Rotation") else {
        return;
    };
    if !state.has_card(Some(crop))
        || !state
            .battlefield
            .iter()
            .any(|perm| perm.kind_enum().is_land())
    {
        return;
    }
    for mana in pay_options(state.mana(), [0, 0, 0, 0, 0, 1]) {
        for (land_index, perm) in state.battlefield.iter().enumerate() {
            if !perm.kind_enum().is_land() {
                continue;
            }
            for target_name in [
                "Ancient Tomb",
                "City of Brass",
                "City of Traitors",
                "Command Tower",
                "Crystal Vein",
                "Mana Confluence",
                "Phyrexian Tower",
                "Tropical Island",
                "Tundra",
                "Underground Sea",
                "Volcanic Island",
            ] {
                let Some(target) = ctx.card_id(target_name) else {
                    continue;
                };
                if !state.library.contains_card(target) {
                    continue;
                }
                let target_removed_library = remove_first_card_vec(&state.library, target);
                for (target_perm, library, grave_inc) in
                    land_options_fast(ctx, target, &target_removed_library)
                {
                    let base = sac_land_fast(ctx, state, land_index);
                    let mut next = base.clone();
                    next.remove_hand_to_graveyard(ctx, crop);
                    next.library = library.into();
                    obscure_library_top_after_shuffle(ctx, &mut next.library);
                    next.push_perm(ctx, target_perm);
                    next.set_mana(mana);
                    next.set_land_grave_count(
                        base.land_grave_count.saturating_add(grave_inc).min(4),
                    );
                    actions.push(FastAction::new(
                        after_cast_fast(ctx, state, next),
                        DEFAULT_PRIORITY,
                    ));
                }
            }
        }
    }
}

fn generate_fast_hand_tutor_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    generate_fast_hand_tutor_actions_for_cost(ctx, actions, state, None);
}

fn generate_fast_hand_tutor_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    cost_filter: Option<Cost>,
) {
    for (tutor_name, cost) in [
        ("Demonic Tutor", [1, 1, 0, 0, 0, 0]),
        ("Diabolic Intent", [1, 1, 0, 0, 0, 0]),
        ("Grim Tutor", [1, 2, 0, 0, 0, 0]),
        ("Idyllic Tutor", [2, 0, 0, 0, 1, 0]),
    ] {
        if !payment_cost_matches(cost_filter, cost) {
            continue;
        }
        let Some(tutor) = ctx.card_id(tutor_name) else {
            continue;
        };
        if !state.has_card(Some(tutor)) {
            continue;
        }
        let creature_indices: Vec<Option<usize>> = if tutor_name == "Diabolic Intent" {
            unique_creature_indices(state)
                .into_iter()
                .map(Some)
                .collect()
        } else {
            vec![None]
        };
        for creature_index in creature_indices {
            for target in tutor_targets_fast(ctx, tutor_name, state) {
                for mana in pay_options(state.mana(), cost) {
                    let base = if let Some(index) = creature_index {
                        sac_creature_fast(ctx, state, index)
                    } else {
                        state.clone()
                    };
                    let mut next = base.clone();
                    next.remove_hand_to_graveyard(ctx, tutor);
                    next.add_hand_card(target);
                    next.remove_library_card(target);
                    obscure_library_top_after_shuffle(ctx, &mut next.library);
                    next.set_mana(mana);
                    actions.push(FastAction::new(
                        after_cast_fast(ctx, &base, next),
                        tutor_target_priority_fast(ctx, tutor_name, target),
                    ));
                    generate_fast_led_tutor_line(
                        ctx,
                        actions,
                        state,
                        tutor,
                        cost,
                        creature_index,
                        target,
                    );
                }
            }
        }
    }
}

fn generate_fast_beseech_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    let Some(beseech) = ctx.card_id("Beseech the Mirror") else {
        return;
    };
    if !state.has_card(Some(beseech)) {
        return;
    }
    let cost = [1, 3, 0, 0, 0, 0];
    let targets = tutor_targets_fast(ctx, "Beseech the Mirror", state);
    if targets.is_empty() {
        return;
    }
    let payments = pay_options(state.mana(), cost);
    for target in targets.iter().copied() {
        for mana in payments.iter().copied() {
            let mut next = state.clone();
            next.remove_hand_to_graveyard(ctx, beseech);
            next.add_hand_card(target);
            next.remove_library_card(target);
            obscure_library_top_after_shuffle(ctx, &mut next.library);
            next.set_mana(mana);
            actions.push(FastAction::new(
                after_cast_fast(ctx, state, next),
                tutor_target_priority_fast(ctx, "Beseech the Mirror", target),
            ));
        }
        generate_fast_led_beseech_line(ctx, actions, state, target);
    }
    if payments.is_empty() {
        return;
    }

    for bargain_index in 0..state.battlefield.len() {
        let permanent = state.battlefield[bargain_index];
        if !permanent.kind_enum().is_artifact() && !permanent.kind_enum().is_enchantment() {
            continue;
        }
        let base = sac_perm_fast(ctx, state, bargain_index);
        for target in targets.iter().copied() {
            for mana in payments.iter().copied() {
                let mut next = base.clone();
                next.remove_hand_to_graveyard(ctx, beseech);
                next.remove_library_card(target);
                obscure_library_top_after_shuffle(ctx, &mut next.library);
                next.set_mana(mana);
                next = after_cast_fast(ctx, state, next);
                next = resolve_free_engine_fast(ctx, next, target);
                actions.push(FastAction::new(
                    next,
                    tutor_target_priority_fast(ctx, "Beseech the Mirror", target),
                ));
            }
        }
    }
}

fn generate_fast_top_tutor_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    generate_fast_top_tutor_actions_for_cost(ctx, actions, state, None);
}

fn generate_fast_top_tutor_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    cost_filter: Option<Cost>,
) {
    for (tutor_name, cost) in [
        ("Enlightened Tutor", [0, 0, 0, 0, 1, 0]),
        ("Imperial Seal", [0, 1, 0, 0, 0, 0]),
        ("Mystical Tutor", [0, 0, 0, 1, 0, 0]),
        ("Scheming Symmetry", [0, 1, 0, 0, 0, 0]),
        ("Vampiric Tutor", [0, 1, 0, 0, 0, 0]),
        ("Worldly Tutor", [0, 0, 0, 0, 0, 1]),
    ] {
        if !payment_cost_matches(cost_filter, cost) {
            continue;
        }
        let Some(tutor) = ctx.card_id(tutor_name) else {
            continue;
        };
        if !state.has_card(Some(tutor)) {
            continue;
        }
        for target in tutor_targets_fast(ctx, tutor_name, state) {
            for mana in pay_options(state.mana(), cost) {
                let mut next = state.clone();
                next.remove_hand_to_graveyard(ctx, tutor);
                next.library = known_top_library_after_shuffle(ctx, target, &state.library);
                next.set_mana(mana);
                actions.push(FastAction::new(
                    after_cast_fast(ctx, state, next),
                    tutor_target_priority_fast(ctx, tutor_name, target),
                ));
            }
        }
    }
}

fn generate_fast_wishclaw_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
) {
    if !state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Wishclaw && !perm.tapped())
    {
        return;
    }
    for target in tutor_targets_fast(ctx, "Wishclaw Talisman", state) {
        for (wishclaw_index, perm) in state.battlefield.iter().enumerate() {
            if perm.kind_enum() != FastPermKind::Wishclaw || perm.tapped() {
                continue;
            }
            for mana in pay_options(state.mana(), [1, 0, 0, 0, 0, 0]) {
                let mut next = state.clone();
                next.battlefield.remove(wishclaw_index);
                next.add_hand_card(target);
                next.remove_library_card(target);
                obscure_library_top_after_shuffle(ctx, &mut next.library);
                next.set_mana(mana);
                sort_fast_battlefield(ctx, &mut next.battlefield);
                actions.push(FastAction::new(
                    next,
                    tutor_target_priority_fast(ctx, "Wishclaw Talisman", target),
                ));
            }
            generate_fast_led_wishclaw_line(ctx, actions, state, wishclaw_index, target);
        }
    }
}

fn generate_fast_gamble_actions(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    config: &FastSearchConfig,
) {
    generate_fast_gamble_actions_for_cost(ctx, actions, state, config, None);
}

fn generate_fast_gamble_actions_for_cost(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    config: &FastSearchConfig,
    cost_filter: Option<Cost>,
) {
    let cost = [0, 0, 1, 0, 0, 0];
    if !payment_cost_matches(cost_filter, cost) {
        return;
    }
    let Some(gamble) = ctx.card_id("Gamble") else {
        return;
    };
    if !state.has_card(Some(gamble))
        || matches!(
            config.gamble_mode,
            FastGambleMode::Off | FastGambleMode::StochasticUnsupported
        )
    {
        return;
    }
    for mana_after_cost in pay_options(state.mana(), cost) {
        let targets = gamble_targets_fast(ctx, state, mana_after_cost, config);
        for target in targets {
            let mut next = state.clone();
            next.remove_hand_to_graveyard(ctx, gamble);
            next.add_hand_card(target);
            next.remove_library_card(target);
            obscure_library_top_after_shuffle(ctx, &mut next.library);
            next.set_mana(mana_after_cost);
            if matches!(config.gamble_mode, FastGambleMode::StochasticSimplified) {
                let discard =
                    simplified_gamble_discard_fast(ctx, config, state, target, &next.hand);
                next.remove_hand_to_graveyard(ctx, discard);
            }
            actions.push(FastAction::new(
                after_cast_fast(ctx, state, next),
                tutor_target_priority_fast(ctx, "Gamble", target),
            ));
        }
    }
}

fn gamble_targets_fast(
    ctx: &FastContext,
    state: &FastState,
    mana_after_cost: Mana,
    config: &FastSearchConfig,
) -> Vec<CardId> {
    let candidates = tutor_targets_fast(ctx, "Gamble", state);
    if !matches!(config.gamble_mode, FastGambleMode::StochasticSimplified) || candidates.len() <= 1
    {
        return candidates;
    }
    candidates
        .into_iter()
        .max_by_key(|target| gamble_target_score_fast(ctx, *target, mana_after_cost))
        .into_iter()
        .collect()
}

fn gamble_target_score_fast(
    ctx: &FastContext,
    target: CardId,
    mana_after_cost: Mana,
) -> (u8, i16, i16, u8, String) {
    let Some(cost) = gamble_target_cost_fast(ctx.card_name(target)) else {
        return (
            0,
            -99,
            -99,
            gamble_target_priority_fast(ctx.card_name(target)),
            ctx.card_name(target).to_string(),
        );
    };
    let immediate = (!pay_options(mana_after_cost, cost).is_empty()) as u8;
    let colored_shortage: i16 = (0..5)
        .map(|index| cost[index + 1].saturating_sub(mana_after_cost[index]) as i16)
        .sum();
    let total_available: i16 = mana_after_cost.iter().map(|value| *value as i16).sum();
    let total_required: i16 = cost.iter().map(|value| *value as i16).sum();
    let total_shortage = (total_required - total_available).max(0);
    (
        immediate,
        -colored_shortage,
        -total_shortage,
        gamble_target_priority_fast(ctx.card_name(target)),
        ctx.card_name(target).to_string(),
    )
}

fn gamble_target_cost_fast(target: &str) -> Option<Cost> {
    match target {
        "Rhystic Study" => Some([2, 0, 0, 1, 0, 0]),
        "Heartwood Storyteller" => Some([3, 0, 0, 0, 0, 1]),
        "Mystic Remora" => Some([0, 0, 0, 1, 0, 0]),
        "Smothering Tithe" => Some([3, 0, 0, 0, 1, 0]),
        _ => None,
    }
}

fn gamble_target_priority_fast(target: &str) -> u8 {
    match target {
        "Rhystic Study" => 4,
        "Heartwood Storyteller" => 3,
        "Mystic Remora" => 2,
        "Smothering Tithe" => 1,
        _ => 0,
    }
}

fn simplified_gamble_discard_fast(
    ctx: &FastContext,
    config: &FastSearchConfig,
    state: &FastState,
    target: CardId,
    hand_after_search: &[CardId],
) -> CardId {
    let mut hasher = Blake2bVar::new(8).expect("valid digest size");
    for part in [
        "gamble-v1".to_string(),
        config.gamble_seed.to_string(),
        state.turn().to_string(),
        ctx.card_name(target).to_string(),
    ] {
        hasher.update(part.as_bytes());
        hasher.update(&[0x1e]);
    }
    for card in hand_after_search {
        hasher.update(ctx.card_name(*card).as_bytes());
        hasher.update(&[0x1f]);
    }
    let mut digest = [0u8; 8];
    hasher
        .finalize_variable(&mut digest)
        .expect("digest output size");
    let index = (u64::from_be_bytes(digest) as usize) % hand_after_search.len().max(1);
    hand_after_search[index]
}

fn generate_fast_led_tutor_line(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    tutor: CardId,
    cost: Cost,
    creature_index: Option<usize>,
    target: CardId,
) {
    let led_in_hand = ctx
        .card_id("Lion's Eye Diamond")
        .is_some_and(|card| state.has_card(Some(card)));
    let led_in_play = state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Led && !perm.tapped());
    if !led_in_hand && !led_in_play {
        return;
    }
    let tutor_name = ctx.card_name(tutor).to_string();
    for (_led_index, led) in state.battlefield.iter().enumerate() {
        if led.kind_enum() != FastPermKind::Led || led.tapped() {
            continue;
        }
        for mana_after_cost in pay_options(state.mana(), cost) {
            let base = if let Some(index) = creature_index {
                sac_creature_fast(ctx, state, index)
            } else {
                state.clone()
            };
            let Some(base_led_index) = base
                .battlefield
                .iter()
                .position(|perm| perm.kind_enum() == FastPermKind::Led && !perm.tapped())
            else {
                continue;
            };
            for color in 0..5 {
                let floated = add_mana(mana_after_cost, led_mana_for_color(color));
                let mut empty = base.clone();
                empty.remove_hand_to_graveyard(ctx, tutor);
                empty.move_hand_to_graveyard(ctx);
                remove_perm_to_graveyard(ctx, &mut empty, base_led_index);
                empty.set_mana(floated);
                if let Some(next) = led_target_state_fast(ctx, &empty, target, floated) {
                    actions.push(FastAction::new(
                        after_cast_fast(ctx, &base, next),
                        tutor_target_priority_fast(ctx, &tutor_name, target),
                    ));
                }
            }
        }
    }
}

fn generate_fast_led_beseech_line(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    target: CardId,
) {
    if !state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Led && !perm.tapped())
    {
        return;
    }
    let cost = [1, 3, 0, 0, 0, 0];
    for (led_index, led) in state.battlefield.iter().enumerate() {
        if led.kind_enum() != FastPermKind::Led || led.tapped() {
            continue;
        }
        for mana_after_cost in pay_options(state.mana(), cost) {
            for color in 0..5 {
                let floated = add_mana(mana_after_cost, led_mana_for_color(color));
                let mut empty = state.clone();
                empty.remove_hand_to_graveyard(
                    ctx,
                    ctx.card_id("Beseech the Mirror")
                        .expect("Beseech checked before LED line"),
                );
                empty.move_hand_to_graveyard(ctx);
                remove_perm_to_graveyard(ctx, &mut empty, led_index);
                empty.set_mana(floated);
                if let Some(next) = led_target_state_fast(ctx, &empty, target, floated) {
                    actions.push(FastAction::new(
                        after_cast_fast(ctx, state, next),
                        tutor_target_priority_fast(ctx, "Beseech the Mirror", target),
                    ));
                }
            }
        }
    }
}

fn generate_fast_led_wishclaw_line(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    wishclaw_index: usize,
    target: CardId,
) {
    if !state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Led && !perm.tapped())
    {
        return;
    }
    for mana_after_cost in pay_options(state.mana(), [1, 0, 0, 0, 0, 0]) {
        for (led_index, led) in state.battlefield.iter().enumerate() {
            if led.kind_enum() != FastPermKind::Led || led.tapped() || led_index == wishclaw_index {
                continue;
            }
            let mut battlefield = state.battlefield.clone();
            let mut indexes = [wishclaw_index, led_index];
            indexes.sort_unstable_by(|left, right| right.cmp(left));
            for index in indexes {
                if index < battlefield.len() {
                    battlefield.remove(index);
                }
            }
            for color in 0..5 {
                let floated = add_mana(mana_after_cost, led_mana_for_color(color));
                let mut empty = state.clone();
                empty.move_hand_to_graveyard(ctx);
                empty.battlefield = battlefield.clone();
                if let Some(led_card) = ctx.card_id("Lion's Eye Diamond") {
                    empty.add_graveyard_card(ctx, led_card);
                }
                empty.set_mana(floated);
                sort_fast_battlefield(ctx, &mut empty.battlefield);
                if let Some(next) = led_target_state_fast(ctx, &empty, target, floated) {
                    actions.push(FastAction::new(
                        next,
                        tutor_target_priority_fast(ctx, "Wishclaw Talisman", target),
                    ));
                }
            }
        }
    }
}

fn cast_fast_cost_action(
    ctx: &mut FastContext,
    actions: &mut Vec<FastAction>,
    state: &FastState,
    name: &str,
    perm: FastPerm,
    cost: Cost,
    priority: i32,
) {
    let Some(card) = ctx.card_id(name) else {
        return;
    };
    if !state.has_card(Some(card)) {
        return;
    }
    for mana in pay_options(state.mana(), cost) {
        let mut next = state.clone();
        next.remove_hand_card(card);
        next.push_perm(ctx, perm);
        next.set_mana(mana);
        actions.push(FastAction::new(after_cast_fast(ctx, state, next), priority));
    }
}

fn after_cast_fast(ctx: &mut FastContext, before: &FastState, mut after: FastState) -> FastState {
    after.set_spells_this_turn(before.spells_this_turn().saturating_add(1).min(5));
    let before_has_birgi = before
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Birgi);
    let after_has_birgi = after
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Birgi);
    if before_has_birgi && after_has_birgi {
        after.set_mana(add_mana(after.mana(), [0, 1, 0, 0, 0, 0]));
    }
    if before.spells_this_turn() == 1
        && before
            .battlefield
            .iter()
            .any(|perm| perm.kind_enum() == FastPermKind::Lotho)
    {
        let treasure = make_perm(ctx, FastPermKind::Treasure, false, 0, false, 0);
        after.push_perm(ctx, treasure);
    }
    after
}

fn add_engine_fast(
    ctx: &mut FastContext,
    mut state: FastState,
    target_mask: u8,
    engine_name: &str,
) -> FastState {
    state.engine_targets |= target_mask | engine_target_mask("PERM");
    let name = ctx.intern_string(&format!("{engine_name}@{}", state.turn()));
    if !state.engine_names.contains(&name) {
        state.engine_names.push(name);
        state
            .engine_names
            .sort_by(|left, right| ctx.interned_string(*left).cmp(ctx.interned_string(*right)));
    }
    state.set_engine_count(state.engine_count().saturating_add(1).min(3));
    state
}

fn resolve_free_engine_fast(
    ctx: &mut FastContext,
    mut state: FastState,
    target: CardId,
) -> FastState {
    if let Some((_cost, target_mask, perm)) = engine_native_fast(ctx, target) {
        let name = ctx.card_name(target).to_string();
        state.push_perm(ctx, perm);
        return add_engine_fast(ctx, state, target_mask, &name);
    }
    state
}

fn led_target_state_fast(
    ctx: &mut FastContext,
    state: &FastState,
    target: CardId,
    mana: Mana,
) -> Option<FastState> {
    if !state.library.contains_card(target) {
        return None;
    }
    if let Some((cost, target_mask, perm)) = engine_native_fast(ctx, target) {
        for remaining in pay_options(mana, cost) {
            let mut next = state.clone();
            next.remove_hand_card(target);
            next.remove_library_card(target);
            obscure_library_top_after_shuffle(ctx, &mut next.library);
            next.push_perm(ctx, perm);
            next.set_mana(remaining);
            let name = ctx.card_name(target).to_string();
            return Some(add_engine_fast(ctx, next, target_mask, &name));
        }
        return None;
    }
    let mut next = state.clone();
    next.move_hand_to_graveyard(ctx);
    next.add_hand_card(target);
    next.remove_library_card(target);
    obscure_library_top_after_shuffle(ctx, &mut next.library);
    next.set_mana(mana);
    Some(next)
}

fn engine_native_fast(ctx: &mut FastContext, target: CardId) -> Option<(Cost, u8, FastPerm)> {
    let target_name = ctx.card_name(target).to_string();
    match target_name.as_str() {
        "Rhystic Study" => Some((
            [2, 0, 0, 1, 0, 0],
            engine_target_mask("ENCH"),
            make_perm(ctx, FastPermKind::EngineEnch, false, 0, false, 0),
        )),
        "Heartwood Storyteller" => Some((
            [1, 0, 0, 0, 0, 2],
            engine_target_mask("CREATURE"),
            make_perm(
                ctx,
                FastPermKind::Heartwood,
                false,
                color_mask("G"),
                true,
                0,
            ),
        )),
        _ => None,
    }
}

fn unique_creature_indices(state: &FastState) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (index, perm) in state.battlefield.iter().enumerate() {
        if !perm.kind_enum().is_creature() {
            continue;
        }
        let key = (
            perm.kind(),
            perm.tapped(),
            perm.colors(),
            perm.fresh(),
            perm.counters(),
        );
        if seen.insert(key) {
            out.push(index);
        }
    }
    out
}

fn mantle_key_fast(ctx: &mut FastContext, perm: FastPerm) -> SmallVec<[InternId; 2]> {
    let mut extra = perm.fixture_extra(ctx);
    if extra.ends_with('*') {
        extra.pop();
    }
    let mut parts = [perm.kind_enum().as_name().to_string(), extra];
    parts.sort();
    let mut out = SmallVec::new();
    for part in parts {
        out.push(ctx.intern_string(&part));
    }
    out
}

fn sac_creature_fast(ctx: &FastContext, state: &FastState, index: usize) -> FastState {
    let mut next = state.clone();
    remove_perm_to_graveyard(ctx, &mut next, index);
    next
}

fn sac_land_fast(ctx: &FastContext, state: &FastState, index: usize) -> FastState {
    let mut next = state.clone();
    remove_land_perm_to_graveyard(ctx, &mut next, index);
    next
}

fn sac_perm_fast(ctx: &FastContext, state: &FastState, index: usize) -> FastState {
    let mut next = state.clone();
    remove_perm_to_graveyard(ctx, &mut next, index);
    next
}

fn remove_perm_to_graveyard(
    ctx: &FastContext,
    state: &mut FastState,
    index: usize,
) -> Option<FastPerm> {
    if index >= state.battlefield.len() {
        return None;
    }
    let perm = state.remove_perm(ctx, index);
    if let Some(card) = graveyard_card_for_perm(ctx, perm) {
        state.add_graveyard_card(ctx, card);
    }
    Some(perm)
}

fn remove_land_perm_to_graveyard(
    ctx: &FastContext,
    state: &mut FastState,
    index: usize,
) -> Option<FastPerm> {
    if index >= state.battlefield.len() {
        return None;
    }
    let perm = state.remove_perm(ctx, index);
    if let Some(card) = graveyard_card_for_perm(ctx, perm) {
        state.add_graveyard_card(ctx, card);
    } else {
        state.add_land_grave(1);
    }
    Some(perm)
}

fn graveyard_card_for_perm(ctx: &FastContext, perm: FastPerm) -> Option<CardId> {
    let name = trace_perm_name(ctx, perm);
    if matches!(
        name.as_str(),
        "land" | "Treasure" | "creature" | "artifact" | "permanent"
    ) {
        return None;
    }
    ctx.card_id(&name)
}

fn fast_tap_options(state: &FastState, perm: FastPerm) -> SmallVec<[FastTap; 5]> {
    let mut out = SmallVec::new();
    match perm.kind_enum() {
        FastPermKind::Land | FastPermKind::Chrome => {
            push_color_mask_options(&mut out, perm.colors())
        }
        FastPermKind::Cavern
        | FastPermKind::Mine
        | FastPermKind::Glimmer
        | FastPermKind::Diamond => {
            for color in 0..5 {
                out.push(FastTap::Color(color));
            }
        }
        FastPermKind::CcLand | FastPermKind::City | FastPermKind::Sol => {
            out.push(FastTap::Colorless(2))
        }
        FastPermKind::Vein => out.push(FastTap::Colorless(1)),
        FastPermKind::Vault => out.push(FastTap::Vault),
        FastPermKind::Signet | FastPermKind::Relic => {
            for color in 0..5 {
                out.push(FastTap::Color(color));
            }
        }
        FastPermKind::Opal if artifact_count_fast(state) >= 3 => {
            for color in 0..5 {
                out.push(FastTap::Color(color));
            }
        }
        FastPermKind::Amber => {
            let mut available = 0u8;
            for permanent in &state.battlefield {
                if permanent.kind_enum().is_legendary() {
                    available |= permanent.colors();
                }
            }
            push_color_mask_options(&mut out, available);
        }
        FastPermKind::Bird if !perm.fresh() => {
            for color in 0..5 {
                out.push(FastTap::Color(color));
            }
        }
        FastPermKind::Deathrite if !perm.fresh() => {
            for color in 0..5 {
                out.push(FastTap::Color(color));
            }
        }
        FastPermKind::Noble if !perm.fresh() => {
            push_color_mask_options(&mut out, color_mask("UWG"))
        }
        FastPermKind::Ignoble if !perm.fresh() => {
            push_color_mask_options(&mut out, color_mask("BRG"))
        }
        _ => {}
    }
    out
}

fn push_color_mask_options(out: &mut SmallVec<[FastTap; 5]>, mask: u8) {
    for color in 0..5 {
        if (mask & (1 << color)) != 0 {
            out.push(FastTap::Color(color));
        }
    }
    if (mask & (1 << 5)) != 0 {
        out.push(FastTap::Colorless(1));
    }
}

fn artifact_count_fast(state: &FastState) -> usize {
    state
        .battlefield
        .iter()
        .filter(|perm| perm.kind_enum().is_artifact())
        .count()
}

fn land_options_fast(
    ctx: &mut FastContext,
    card: CardId,
    library: &[CardId],
) -> Vec<(FastPerm, Vec<CardId>, u8)> {
    let card_name = ctx.card_name(card).to_string();
    let flags = ctx.card_spec(card).flags;
    if flags.contains(CardFlags::MDFC_LAND) {
        return vec![(
            make_perm(ctx, FastPermKind::Land, false, color_mask("U"), false, 0),
            library.to_vec(),
            0,
        )];
    }
    if flags.contains(CardFlags::FETCH) {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for target in library {
            let target_name = ctx.card_name(*target).to_string();
            if !seen.insert(*target) || !fetch_can_get_name(&card_name, &target_name) {
                continue;
            }
            if let Some(colors) = land_type_color_mask(&target_name) {
                let mut next_library = remove_first_card_vec(library, *target);
                obscure_library_top_after_shuffle(ctx, &mut next_library);
                out.push((
                    make_perm(ctx, FastPermKind::Land, false, colors, false, 0),
                    next_library,
                    1,
                ));
            }
        }
        return out;
    }
    let perm = match card_name.as_str() {
        "Ancient Tomb" => make_perm(ctx, FastPermKind::CcLand, false, 0, false, 0),
        "City of Traitors" => make_perm(ctx, FastPermKind::City, false, 0, false, 0),
        "Crystal Vein" => make_perm(ctx, FastPermKind::Vein, false, color_mask("C"), false, 0),
        "Phyrexian Tower" => make_perm(ctx, FastPermKind::Tower, false, color_mask("C"), false, 0),
        "Glimmervoid" => make_perm(
            ctx,
            FastPermKind::Glimmer,
            false,
            color_mask("BRUWG"),
            false,
            0,
        ),
        "Gemstone Mine" => make_perm(ctx, FastPermKind::Mine, false, 0, false, 3),
        _ if is_theoretical_rainbow_land_name(&card_name) => make_perm(
            ctx,
            FastPermKind::Land,
            false,
            color_mask("BRUWG"),
            false,
            0,
        ),
        "City of Brass" | "Command Tower" | "Exotic Orchard" | "Forbidden Orchard"
        | "Mana Confluence" | "Starting Town" | "Tarnished Citadel" => make_perm(
            ctx,
            FastPermKind::Land,
            false,
            color_mask("BRUWG"),
            false,
            0,
        ),
        "Boseiju, Who Endures" => {
            make_perm(ctx, FastPermKind::Land, false, color_mask("G"), false, 0)
        }
        "Otawara, Soaring City" => {
            make_perm(ctx, FastPermKind::Land, false, color_mask("U"), false, 0)
        }
        "Sea of Clouds" => make_perm(ctx, FastPermKind::Land, false, color_mask("UW"), false, 0),
        _ => {
            if let Some(colors) = land_type_color_mask(&card_name) {
                make_perm(ctx, FastPermKind::Land, false, colors, false, 0)
            } else {
                make_perm(ctx, FastPermKind::Land, false, color_mask("C"), false, 0)
            }
        }
    };
    vec![(perm, library.to_vec(), 0)]
}

fn tutor_targets_fast(ctx: &FastContext, tutor: &str, state: &FastState) -> Vec<CardId> {
    if matches!(
        tutor,
        "Imperial Seal" | "Scheming Symmetry" | "Vampiric Tutor"
    ) {
        return [
            "Rhystic Study",
            "Heartwood Storyteller",
            "Demonic Tutor",
            "Beseech the Mirror",
            "Wishclaw Talisman",
            "Diabolic Intent",
            "Grim Tutor",
            "Gamble",
            "Enlightened Tutor",
            "Mystical Tutor",
            "Worldly Tutor",
            "Idyllic Tutor",
            "Green Sun's Zenith",
            "Eldritch Evolution",
            "Neoform",
            "Summoner's Pact",
            "Crop Rotation",
            "Lion's Eye Diamond",
            "Lotus Petal",
            "Mana Vault",
            "Sol Ring",
            "Dark Ritual",
            "Culling the Weak",
            "Rain of Filth",
            "Rite of Flame",
            "Elvish Spirit Guide",
            "Simian Spirit Guide",
            "Tinder Wall",
            "Mox Amber",
            "Mox Diamond",
            "Chrome Mox",
            "Mox Opal",
            "Springleaf Drum",
            "Paradise Mantle",
            "Manamorphose",
        ]
        .iter()
        .filter_map(|name| ctx.card_id(name))
        .filter(|target| state.library.contains_card(*target))
        .collect();
    }
    if tutor == "Mystical Tutor" {
        return [
            "Demonic Tutor",
            "Beseech the Mirror",
            "Diabolic Intent",
            "Grim Tutor",
            "Enlightened Tutor",
            "Worldly Tutor",
            "Idyllic Tutor",
            "Green Sun's Zenith",
            "Eldritch Evolution",
            "Neoform",
            "Summoner's Pact",
            "Crop Rotation",
            "Dark Ritual",
            "Culling the Weak",
            "Rain of Filth",
            "Rite of Flame",
            "Manamorphose",
            "An Offer You Can't Refuse",
            "Imperial Seal",
            "Infernal Plunge",
            "Scheming Symmetry",
            "Vampiric Tutor",
        ]
        .iter()
        .filter_map(|name| ctx.card_id(name))
        .filter(|target| state.library.contains_card(*target))
        .collect();
    }
    if tutor == "Enlightened Tutor" || tutor == "Idyllic Tutor" {
        return ctx
            .card_id("Rhystic Study")
            .filter(|target| state.library.contains_card(*target))
            .into_iter()
            .collect();
    }
    if tutor == "Worldly Tutor" {
        return [
            "Heartwood Storyteller",
            "Tinder Wall",
            "Birds of Paradise",
            "Deathrite Shaman",
            "Ignoble Hierarch",
            "Noble Hierarch",
            "Wild Cantor",
        ]
        .iter()
        .filter_map(|name| ctx.card_id(name))
        .filter(|target| state.library.contains_card(*target))
        .collect();
    }
    ["Rhystic Study", "Heartwood Storyteller"]
        .iter()
        .filter_map(|name| ctx.card_id(name))
        .filter(|target| state.library.contains_card(*target))
        .collect()
}

fn offer_bait_can_help(ctx: &FastContext, state: &FastState, bait: CardId) -> bool {
    let bait_name = ctx.card_name(bait);
    if matches!(
        bait_name,
        "Rhystic Study"
            | "Mystic Remora"
            | "Esper Sentinel"
            | "Heartwood Storyteller"
            | "Copy Enchantment"
            | "Mirrormade"
            | "Flash Photography"
            | "Clever Impersonator"
            | "Necropotence"
            | "Smothering Tithe"
    ) {
        return false;
    }
    if bait_name == "Noxious Revival" && state.graveyard.is_empty() && state.land_grave_count == 0 {
        return false;
    }
    let costs = offer_counterable_costs(bait_name);
    if costs.is_empty() {
        return false;
    }
    if costs
        .iter()
        .any(|cost| cost.iter().copied().sum::<u8>() <= 1)
    {
        return true;
    }
    state
        .battlefield
        .iter()
        .any(|perm| matches!(perm.kind_enum(), FastPermKind::Birgi | FastPermKind::Lotho))
}

fn offer_counterable_costs(card: &str) -> Vec<Cost> {
    match card {
        "Summoner's Pact" | "Lotus Petal" | "Chaos Emerald" | "Chrome Mox"
        | "Lion's Eye Diamond" | "Mox Amber" | "Mox Diamond" | "Mox Opal" | "Paradise Mantle"
        | "Noxious Revival" => vec![[0, 0, 0, 0, 0, 0]],
        "Sol Ring" | "Mana Vault" | "Springleaf Drum" => vec![[1, 0, 0, 0, 0, 0]],
        "Arcane Signet" => vec![[2, 0, 0, 0, 0, 0]],
        "Relic of Legends" => vec![[3, 0, 0, 0, 0, 0]],
        "Wishclaw Talisman" | "Demonic Tutor" => vec![[1, 1, 0, 0, 0, 0]],
        "Grim Tutor" => vec![[1, 2, 0, 0, 0, 0]],
        "Idyllic Tutor" => vec![[2, 0, 0, 0, 1, 0]],
        "Dark Ritual" | "Imperial Seal" | "Scheming Symmetry" | "Vampiric Tutor" => {
            vec![[0, 1, 0, 0, 0, 0]]
        }
        "Rite of Flame" | "Strike It Rich" => vec![[0, 0, 1, 0, 0, 0]],
        "Enlightened Tutor" => vec![[0, 0, 0, 0, 1, 0]],
        "Mystical Tutor" => vec![[0, 0, 0, 1, 0, 0]],
        "Worldly Tutor" | "Nature's Chosen" | "Green Sun's Zenith" => vec![[0, 0, 0, 0, 0, 1]],
        "Beseech the Mirror" => vec![[1, 3, 0, 0, 0, 0]],
        "Rhystic Study" | "Copy Enchantment" => vec![[2, 0, 0, 1, 0, 0]],
        "Mystic Remora" => vec![[0, 0, 0, 1, 0, 0]],
        "Mirrormade" | "Flash Photography" => vec![[1, 0, 0, 2, 0, 0]],
        "Necropotence" => vec![[0, 3, 0, 0, 0, 0]],
        "Smothering Tithe" => vec![[3, 0, 0, 0, 1, 0]],
        "Manamorphose" => vec![[1, 0, 1, 0, 0, 0], [1, 0, 0, 0, 0, 1]],
        _ => Vec::new(),
    }
}

fn engine_target_priority_rank(target: &str) -> u8 {
    match target {
        "Rhystic Study" => 0,
        "Heartwood Storyteller" => 1,
        "Mystic Remora" => 2,
        "Smothering Tithe" => 3,
        "Demonic Tutor" => 4,
        "Beseech the Mirror" => 5,
        "Wishclaw Talisman" => 6,
        "Diabolic Intent" => 7,
        "Grim Tutor" => 8,
        "Gamble" => 9,
        "Enlightened Tutor" => 10,
        "Mystical Tutor" => 11,
        "Worldly Tutor" => 12,
        "Idyllic Tutor" => 13,
        "Green Sun's Zenith" => 14,
        "Eldritch Evolution" => 15,
        "Neoform" => 16,
        "Summoner's Pact" => 17,
        "Crop Rotation" => 18,
        "Lion's Eye Diamond" => 19,
        "Lotus Petal" => 20,
        "Mana Vault" => 21,
        "Sol Ring" => 22,
        "Dark Ritual" => 23,
        "Culling the Weak" => 24,
        "Rain of Filth" => 25,
        "Rite of Flame" => 26,
        "Elvish Spirit Guide" => 27,
        "Simian Spirit Guide" => 28,
        "Tinder Wall" => 29,
        "Mox Amber" => 30,
        "Mox Diamond" => 31,
        "Chrome Mox" => 32,
        "Mox Opal" => 33,
        "Springleaf Drum" => 34,
        "Paradise Mantle" => 35,
        "Manamorphose" => 36,
        _ => 50,
    }
}

fn label_priority_name(label: &str) -> i32 {
    if label.contains("Rhystic Study")
        || label.contains("Tutor")
        || label.contains("Wishclaw")
        || label.contains("Beseech")
        || label.contains("Gamble")
        || label.contains("Noxious Revival")
    {
        return ENGINE_TUTOR_PRIORITY;
    }
    if [
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
    ]
    .iter()
    .any(|needle| label.contains(needle))
    {
        return ENGINE_TUTOR_PRIORITY;
    }
    if [
        "Arcane Signet",
        "Drum",
        "Lotus",
        "Manamorphose",
        "Mantle",
        "Mox",
        "Relic",
        "Sol Ring",
        "Mana Vault",
        "Chrome",
        "Diamond",
        "Opal",
    ]
    .iter()
    .any(|needle| label.contains(needle))
    {
        return FAST_MANA_PRIORITY;
    }
    DEFAULT_PRIORITY
}

fn tutor_target_priority_fast(ctx: &FastContext, tutor: &str, target: CardId) -> i32 {
    if matches!(tutor, "Imperial Seal" | "Scheming Symmetry") {
        label_priority_name(ctx.card_name(target))
    } else {
        ENGINE_TUTOR_PRIORITY
    }
}

fn creature_perm_fast(ctx: &mut FastContext, card: &str) -> FastPerm {
    let kind = match card {
        "Birds of Paradise" => FastPermKind::Bird,
        "Deathrite Shaman" => FastPermKind::Deathrite,
        "Esper Sentinel" => FastPermKind::Esper,
        "Faerie Mastermind" => FastPermKind::Faerie,
        "Ignoble Hierarch" => FastPermKind::Ignoble,
        "Noble Hierarch" => FastPermKind::Noble,
        "Orcish Bowmasters" => FastPermKind::Bowmasters,
        "Ragavan, Nimble Pilferer" => FastPermKind::Ragavan,
        "Lotho, Corrupt Shirriff" => FastPermKind::Lotho,
        "The Cabbage Merchant" => FastPermKind::Cabbage,
        "Tinder Wall" => FastPermKind::Tinder,
        "Valley Floodcaller" => FastPermKind::Valley,
        "Wild Cantor" => FastPermKind::Cantor,
        _ => FastPermKind::Creature,
    };
    make_perm(ctx, kind, false, card_color_mask(card), true, 0)
}

fn make_perm(
    ctx: &mut FastContext,
    kind: FastPermKind,
    tapped: bool,
    colors: u8,
    fresh: bool,
    counters: u8,
) -> FastPerm {
    let extra = match kind {
        FastPermKind::Mine => counters.min(7).to_string(),
        FastPermKind::Land
        | FastPermKind::Glimmer
        | FastPermKind::Cavern
        | FastPermKind::Chrome
        | FastPermKind::Bird
        | FastPermKind::Deathrite
        | FastPermKind::Tinder
        | FastPermKind::Tataru
        | FastPermKind::Ragavan
        | FastPermKind::Lotho
        | FastPermKind::Heartwood
        | FastPermKind::Noble
        | FastPermKind::Ignoble
        | FastPermKind::Cantor
        | FastPermKind::Faerie
        | FastPermKind::Bowmasters
        | FastPermKind::Cabbage
        | FastPermKind::Valley
        | FastPermKind::Creature
        | FastPermKind::Birgi
        | FastPermKind::Wan
        | FastPermKind::Ishai
        | FastPermKind::Nick
        | FastPermKind::Rog
        | FastPermKind::Esper => {
            let mut extra = mask_to_colors(colors);
            if fresh {
                extra.push('*');
            }
            extra
        }
        FastPermKind::Vein | FastPermKind::Tower => "C".to_string(),
        _ => String::new(),
    };
    let extra_id = ctx.intern_string(&extra);
    FastPerm::new(kind, tapped, fresh, colors, counters, extra_id)
}

fn sort_fast_battlefield(ctx: &FastContext, battlefield: &mut SmallVec<[FastPerm; 16]>) {
    battlefield.sort_by(|left, right| {
        left.kind_enum()
            .as_name()
            .cmp(right.kind_enum().as_name())
            .then_with(|| left.tapped().cmp(&right.tapped()))
            .then_with(|| compare_fast_perm_extra(ctx, *left, *right))
    });
}

fn compare_fast_perm_extra(ctx: &FastContext, left: FastPerm, right: FastPerm) -> Ordering {
    debug_assert_eq!(left.kind_enum(), right.kind_enum());
    match left.kind_enum() {
        FastPermKind::Mine => left.counters().cmp(&right.counters()),
        FastPermKind::Land
        | FastPermKind::Glimmer
        | FastPermKind::Cavern
        | FastPermKind::Chrome
        | FastPermKind::Bird
        | FastPermKind::Deathrite
        | FastPermKind::Tinder
        | FastPermKind::Tataru
        | FastPermKind::Ragavan
        | FastPermKind::Lotho
        | FastPermKind::Heartwood
        | FastPermKind::Noble
        | FastPermKind::Ignoble
        | FastPermKind::Cantor
        | FastPermKind::Faerie
        | FastPermKind::Bowmasters
        | FastPermKind::Cabbage
        | FastPermKind::Valley
        | FastPermKind::Creature
        | FastPermKind::Birgi
        | FastPermKind::Wan
        | FastPermKind::Ishai
        | FastPermKind::Nick
        | FastPermKind::Rog
        | FastPermKind::Esper => compare_color_extra(left, right),
        FastPermKind::Unknown => ctx
            .interned_string(left.extra_id())
            .cmp(ctx.interned_string(right.extra_id())),
        _ => Ordering::Equal,
    }
}

fn compare_color_extra(left: FastPerm, right: FastPerm) -> Ordering {
    let mut left_bytes = [0; 7];
    let mut right_bytes = [0; 7];
    let left_len = write_color_extra(left, &mut left_bytes);
    let right_len = write_color_extra(right, &mut right_bytes);
    left_bytes[..left_len].cmp(&right_bytes[..right_len])
}

fn write_color_extra(perm: FastPerm, out: &mut [u8; 7]) -> usize {
    let mut len = 0;
    for (index, color) in COLORS.iter().copied().enumerate() {
        if perm.colors() & (1 << index) != 0 {
            out[len] = color;
            len += 1;
        }
    }
    if perm.fresh() {
        out[len] = b'*';
        len += 1;
    }
    len
}

fn remove_first_card_vec(values: &[CardId], card: CardId) -> Vec<CardId> {
    let mut removed = false;
    let mut out = Vec::with_capacity(values.len().saturating_sub(1));
    for value in values {
        if !removed && *value == card {
            removed = true;
            continue;
        }
        out.push(*value);
    }
    out
}

const UNKNOWN_SHUFFLE_DRAW_BARRIER: usize = 20;

fn strict_shuffle_hidden_enabled_from_env() -> bool {
    std::env::var("RHYSTIC_STRICT_SHUFFLE_HIDDEN")
        .map(|value| {
            matches!(
                value.as_str(),
                "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
            )
        })
        .unwrap_or(false)
}

fn payment_mode_from_env() -> FastPaymentMode {
    match std::env::var("RALGORITHM_PAYMENT_DIRECTED")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "generic" => FastPaymentMode::Generic,
        "1" | "true" | "yes" | "on" | "packed" => FastPaymentMode::Packed,
        _ => FastPaymentMode::Off,
    }
}

fn funding_cache_limit_from_env() -> usize {
    std::env::var("RALGORITHM_FUNDING_CACHE_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(100_000)
}

fn funding_frontier_limit_from_env() -> usize {
    std::env::var("RALGORITHM_FUNDING_FRONTIER_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(usize::MAX)
}

fn obscure_library_top_after_shuffle(ctx: &mut FastContext, library: &mut Vec<CardId>) {
    if !ctx.strict_shuffle_hidden {
        return;
    }
    if library.is_empty() {
        return;
    }
    let blank = ctx.intern_card("Blank");
    let leading_blanks = library.iter().take_while(|card| **card == blank).count();
    if leading_blanks >= UNKNOWN_SHUFFLE_DRAW_BARRIER {
        let ordered_prefix = UNKNOWN_SHUFFLE_DRAW_BARRIER.min(library.len());
        library[ordered_prefix..].sort_unstable();
        return;
    }
    let missing = UNKNOWN_SHUFFLE_DRAW_BARRIER - leading_blanks;
    library.splice(0..0, std::iter::repeat_n(blank, missing));
    let ordered_prefix = UNKNOWN_SHUFFLE_DRAW_BARRIER.min(library.len());
    library[ordered_prefix..].sort_unstable();
}

fn known_top_library_after_shuffle(
    ctx: &mut FastContext,
    top: CardId,
    library: &PersistentLibrary,
) -> PersistentLibrary {
    let mut out = Vec::with_capacity(library.len());
    out.push(top);
    out.extend(library.iter().copied().filter(|card| *card != top));
    if ctx.strict_shuffle_hidden {
        let mut tail = out.split_off(1);
        obscure_library_top_after_shuffle(ctx, &mut tail);
        out.extend(tail);
    }
    out.into()
}

fn mana_for_color_index(index: usize) -> Mana {
    let mut out = [0, 0, 0, 0, 0, 0];
    if index < 5 {
        out[index] = 1;
    }
    out
}

fn led_mana_for_color(index: usize) -> Mana {
    let mut out = [0, 0, 0, 0, 0, 0];
    if index < 5 {
        out[index] = 3;
    }
    out
}

fn mask_to_colors(mask: u8) -> String {
    let mut out = String::new();
    for (index, byte) in COLORS.iter().enumerate() {
        if (mask & (1 << index)) != 0 {
            out.push(*byte as char);
        }
    }
    out
}

pub(crate) fn is_land_card_name(card: &str) -> bool {
    if is_theoretical_rainbow_land_name(card) {
        return true;
    }
    matches!(
        card,
        "Ancient Tomb"
            | "Arid Mesa"
            | "Bayou"
            | "Boseiju, Who Endures"
            | "Bloodstained Mire"
            | "City of Brass"
            | "City of Traitors"
            | "Command Tower"
            | "Crystal Vein"
            | "Emergence Zone"
            | "Exotic Orchard"
            | "Flooded Strand"
            | "Forbidden Orchard"
            | "Gemstone Caverns"
            | "Gemstone Mine"
            | "Glimmervoid"
            | "Glittering Caves of Aglarond"
            | "Hallowed Fountain"
            | "Mana Confluence"
            | "Marsh Flats"
            | "Misty Rainforest"
            | "Otawara, Soaring City"
            | "Plateau"
            | "Polluted Delta"
            | "Phyrexian Tower"
            | "Savannah"
            | "Scalding Tarn"
            | "Scrubland"
            | "Sea of Clouds"
            | "Starting Town"
            | "Steam Vents"
            | "Taiga"
            | "Tarnished Citadel"
            | "Tropical Island"
            | "Tundra"
            | "Underground Sea"
            | "Verdant Catacombs"
            | "Volcanic Island"
            | "Windswept Heath"
            | "Wooded Foothills"
    )
}

pub(crate) fn is_mdfc_land_name(card: &str) -> bool {
    matches!(
        card,
        "Sink into Stupor" | "Sink into Stupor // Soporific Springs"
    )
}

pub(crate) fn is_artifact_card_name(card: &str) -> bool {
    matches!(
        card,
        "Arcane Signet"
            | "Chrome Mox"
            | "Lion's Eye Diamond"
            | "Lotus Petal"
            | "Mana Vault"
            | "Mox Amber"
            | "Mox Diamond"
            | "Mox Opal"
            | "Paradise Mantle"
            | "Relic of Legends"
            | "Sol Ring"
            | "Springleaf Drum"
            | "Wishclaw Talisman"
    )
}

pub(crate) fn is_fetch_name(card: &str) -> bool {
    matches!(
        card,
        "Arid Mesa"
            | "Bloodstained Mire"
            | "Flooded Strand"
            | "Marsh Flats"
            | "Misty Rainforest"
            | "Polluted Delta"
            | "Scalding Tarn"
            | "Verdant Catacombs"
            | "Windswept Heath"
            | "Wooded Foothills"
    )
}

fn land_type_color_mask(card: &str) -> Option<u8> {
    match card {
        "Badlands" => Some(color_mask("BR")),
        "Bayou" => Some(color_mask("BG")),
        "Blood Crypt" => Some(color_mask("BR")),
        "Hallowed Fountain" => Some(color_mask("UW")),
        "Plateau" => Some(color_mask("RW")),
        "Savannah" => Some(color_mask("GW")),
        "Scrubland" => Some(color_mask("BW")),
        "Steam Vents" => Some(color_mask("RU")),
        "Taiga" => Some(color_mask("RG")),
        "Tropical Island" => Some(color_mask("UG")),
        "Tundra" => Some(color_mask("UW")),
        "Underground Sea" => Some(color_mask("BU")),
        "Volcanic Island" => Some(color_mask("RU")),
        "Watery Grave" => Some(color_mask("BU")),
        _ => None,
    }
}

fn fetch_can_get_name(fetch: &str, target: &str) -> bool {
    matches!(
        (fetch, target),
        (
            "Arid Mesa",
            "Badlands"
                | "Blood Crypt"
                | "Hallowed Fountain"
                | "Plateau"
                | "Savannah"
                | "Scrubland"
                | "Steam Vents"
                | "Taiga"
                | "Tundra"
                | "Volcanic Island"
        ) | (
            "Bloodstained Mire",
            "Badlands"
                | "Bayou"
                | "Blood Crypt"
                | "Plateau"
                | "Scrubland"
                | "Steam Vents"
                | "Taiga"
                | "Underground Sea"
                | "Volcanic Island"
                | "Watery Grave"
        ) | (
            "Flooded Strand",
            "Hallowed Fountain"
                | "Plateau"
                | "Savannah"
                | "Scrubland"
                | "Steam Vents"
                | "Tropical Island"
                | "Tundra"
                | "Underground Sea"
                | "Volcanic Island"
                | "Watery Grave"
        ) | (
            "Marsh Flats",
            "Badlands"
                | "Bayou"
                | "Blood Crypt"
                | "Hallowed Fountain"
                | "Plateau"
                | "Savannah"
                | "Scrubland"
                | "Tundra"
                | "Underground Sea"
                | "Watery Grave"
        ) | (
            "Misty Rainforest",
            "Bayou"
                | "Hallowed Fountain"
                | "Savannah"
                | "Steam Vents"
                | "Taiga"
                | "Tropical Island"
                | "Tundra"
                | "Underground Sea"
                | "Volcanic Island"
                | "Watery Grave"
        ) | (
            "Polluted Delta",
            "Badlands"
                | "Bayou"
                | "Blood Crypt"
                | "Hallowed Fountain"
                | "Scrubland"
                | "Steam Vents"
                | "Tropical Island"
                | "Tundra"
                | "Underground Sea"
                | "Volcanic Island"
                | "Watery Grave"
        ) | (
            "Scalding Tarn",
            "Badlands"
                | "Blood Crypt"
                | "Hallowed Fountain"
                | "Plateau"
                | "Steam Vents"
                | "Taiga"
                | "Tropical Island"
                | "Tundra"
                | "Underground Sea"
                | "Volcanic Island"
                | "Watery Grave"
        ) | (
            "Verdant Catacombs",
            "Badlands"
                | "Bayou"
                | "Blood Crypt"
                | "Savannah"
                | "Scrubland"
                | "Taiga"
                | "Tropical Island"
                | "Underground Sea"
                | "Watery Grave"
        ) | (
            "Windswept Heath",
            "Bayou"
                | "Hallowed Fountain"
                | "Plateau"
                | "Savannah"
                | "Scrubland"
                | "Taiga"
                | "Tropical Island"
                | "Tundra"
        ) | (
            "Wooded Foothills",
            "Badlands"
                | "Bayou"
                | "Blood Crypt"
                | "Plateau"
                | "Savannah"
                | "Steam Vents"
                | "Taiga"
                | "Tropical Island"
                | "Volcanic Island"
        )
    )
}

pub(crate) fn card_color_mask(card: &str) -> u8 {
    color_mask(match card {
        "Angel's Grace" => "W",
        "An Offer You Can't Refuse" => "U",
        "Beseech the Mirror" => "B",
        "Birgi, God of Storytelling" => "R",
        "Birds of Paradise" => "G",
        "Borne Upon a Wind" => "U",
        "Brain Freeze" => "U",
        "Chain of Vapor" => "U",
        "Clever Impersonator" => "U",
        "Commandeer" => "U",
        "Copy Enchantment" => "U",
        "Crop Rotation" => "G",
        "Culling the Weak" => "B",
        "Curse of Opulence" => "R",
        "Dark Ritual" => "B",
        "Deathrite Shaman" => "BG",
        "Deflecting Swat" => "R",
        "Demonic Tutor" => "B",
        "Diabolic Intent" => "B",
        "Dispel" => "U",
        "Disrupting Shoal" => "U",
        "Eldritch Evolution" => "G",
        "Elvish Spirit Guide" => "G",
        "Enlightened Tutor" => "W",
        "Esper Sentinel" => "W",
        "Faerie Mastermind" => "U",
        "Fierce Guardianship" => "U",
        "Firestorm" => "R",
        "Flash Photography" => "U",
        "Flesh Duplicate" => "U",
        "Flusterstorm" => "U",
        "Force of Negation" => "U",
        "Force of Will" => "U",
        "Flashback" => "R",
        "Gamble" => "R",
        "Gifts Ungiven" => "U",
        "Green Sun's Zenith" => "G",
        "Grim Tutor" => "B",
        "Heartwood Storyteller" => "G",
        "Hullbreaker Horror" => "U",
        "Idyllic Tutor" => "W",
        "Infernal Plunge" => "R",
        "Ignoble Hierarch" => "G",
        "Imperial Seal" => "B",
        "Intuition" => "U",
        "Into the Flood Maw" => "U",
        "Ishai, Ojutai Dragonspeaker" => "UW",
        "Jeska's Will" => "R",
        "Lotho, Corrupt Shirriff" => "BW",
        "Manamorphose" => "RG",
        "Mental Misstep" => "U",
        "Mindbreak Trap" => "U",
        "Mirrormade" => "U",
        "Misdirection" => "U",
        "Mockingbird" => "U",
        "Molten Disaster" => "R",
        "Mystic Remora" => "U",
        "Mystical Tutor" => "U",
        "Nature's Chosen" => "G",
        "Necropotence" => "B",
        "Neoform" => "UG",
        "Nick Fury, Agent of S.H.I.E.L.D." => "W",
        "Noble Hierarch" => "G",
        "Noxious Revival" => "G",
        "Orcish Bowmasters" => "B",
        "Orim's Chant" => "W",
        "Pact of Negation" => "U",
        "Phyrexian Metamorph" => "U",
        "Pyroblast" => "R",
        "Ragavan, Nimble Pilferer" => "R",
        "Rain of Filth" => "B",
        "Ranger-Captain of Eos" => "W",
        "Red Elemental Blast" => "R",
        "Redirect Lightning" => "R",
        "Rhystic Study" => "U",
        "Rite of Flame" => "R",
        "Rograkh, Son of Rohgahh" => "R",
        "Scheming Symmetry" => "B",
        "Sevinne's Reclamation" => "W",
        "Silence" => "W",
        "Simian Spirit Guide" => "R",
        "Sink into Stupor" => "U",
        "Sink into Stupor // Soporific Springs" => "U",
        "Smothering Tithe" => "W",
        "Snapback" => "U",
        "Storm-Kiln Artist" => "R",
        "Strike It Rich" => "R",
        "Subtlety" => "U",
        "Sudden Substitution" => "U",
        "Summoner's Pact" => "G",
        "Swan Song" => "U",
        "Tataru Taru" => "W",
        "The Cabbage Merchant" => "G",
        "Tinder Wall" => "G",
        "Underworld Breach" => "R",
        "Valley Floodcaller" => "U",
        "Vampiric Tutor" => "B",
        "Wan Shi Tong, Librarian" => "U",
        "Wild Cantor" => "RG",
        "Worldly Tutor" => "G",
        _ => "",
    })
}

pub fn close_turn_fast(request: &CloseTurnRequest) -> CloseTurnResponse {
    let mut card_names = Vec::new();
    for state in &request.states {
        card_names.extend(state.hand.iter().cloned());
        card_names.extend(state.library.iter().cloned());
    }
    let mut ctx = FastContext::with_card_names(card_names);
    let start_states: Vec<FastState> = request
        .states
        .iter()
        .map(|state| FastState::from_fixture(&mut ctx, state))
        .collect();
    let result = close_turn_fast_inner(
        &mut ctx,
        request,
        start_states,
        &FastSearchConfig::default(),
    );
    close_response_from_fast(&ctx, result)
}

#[derive(Debug)]
struct FastCloseTurnResult {
    closed: Vec<FastState>,
    success: bool,
    hit_limit: bool,
    label: Option<String>,
}

#[derive(Debug)]
struct FastTraceCloseTurnResult {
    closed: Vec<(FastState, Vec<String>)>,
    success: bool,
    hit_limit: bool,
    label: Option<String>,
    path: Vec<String>,
}

#[derive(Debug, Clone)]
struct FastTraceSolveResult {
    response: SolveKeepResponse,
    trace_turn: Option<u8>,
    trace_capped: bool,
    trace_path: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SolveKeepTraceFastResponse {
    pub response: SolveKeepResponse,
    pub trace_turn: Option<u8>,
    pub trace_capped: bool,
    pub trace_path: Vec<String>,
}

struct FastStateIndex {
    buckets: FxHashMap<u64, SmallVec<[usize; 1]>>,
}

impl FastStateIndex {
    fn from_states(states: &[FastState]) -> Self {
        let mut buckets: FxHashMap<u64, SmallVec<[usize; 1]>> = FxHashMap::default();
        buckets.reserve(states.len());
        for (index, state) in states.iter().enumerate() {
            buckets.entry(hash_value(state)).or_default().push(index);
        }
        Self { buckets }
    }

    fn contains(&self, states: &[FastState], state: &FastState, digest: u64) -> bool {
        self.buckets
            .get(&digest)
            .is_some_and(|indices| indices.iter().any(|index| states[*index] == *state))
    }

    fn insert(&mut self, digest: u64, index: usize) {
        self.buckets.entry(digest).or_default().push(index);
    }
}

fn close_turn_fast_inner(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    start_states: Vec<FastState>,
    config: &FastSearchConfig,
) -> FastCloseTurnResult {
    let mut queue = start_states.clone();
    let mut seen_index = FastStateIndex::from_states(&start_states);
    let mut seen_order = start_states;
    let mut best_mana: FxHashMap<FastState, Vec<Mana>> = FxHashMap::default();
    let mut hit_limit = false;
    while let Some(state) = queue.pop() {
        if let Some(label) = success_label_fast(ctx, request, &state) {
            return FastCloseTurnResult {
                closed: seen_order,
                success: true,
                hit_limit,
                label: Some(label),
            };
        }
        let mut actions = generate_fast_actions_with_config(ctx, &state, config);
        if request.action_sort {
            actions.sort_by_key(|action| action.priority);
        }
        if let Some((_index, label)) = best_immediate_success_fast(ctx, request, &actions) {
            return FastCloseTurnResult {
                closed: seen_order,
                success: true,
                hit_limit,
                label: Some(label),
            };
        }
        for action in actions {
            let next_state = action.next_state;
            let state_digest = hash_value(&next_state);
            if seen_index.contains(&seen_order, &next_state, state_digest) {
                continue;
            }
            if mana_dominated_fast(ctx, request, &next_state, &mut best_mana) {
                continue;
            }
            if seen_order.len() >= request.state_limit {
                hit_limit = true;
                continue;
            }
            seen_index.insert(state_digest, seen_order.len());
            seen_order.push(next_state.clone());
            queue.push(next_state);
        }
    }
    FastCloseTurnResult {
        closed: seen_order,
        success: false,
        hit_limit,
        label: None,
    }
}

fn close_turn_trace_fast_inner(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    start_states: Vec<(FastState, Vec<String>)>,
    config: &FastSearchConfig,
) -> FastTraceCloseTurnResult {
    let mut queue: VecDeque<FastState> = VecDeque::new();
    let mut seen_set: FxHashSet<FastState> = FxHashSet::default();
    let mut seen_order = Vec::new();
    let mut start_paths: FxHashMap<FastState, Vec<String>> = FxHashMap::default();
    let mut parents: FxHashMap<FastState, (FastState, String)> = FxHashMap::default();
    let mut best_mana: FxHashMap<FastState, Vec<Mana>> = FxHashMap::default();
    let mut hit_limit = false;

    for (state, path) in start_states {
        if !seen_set.insert(state.clone()) {
            continue;
        }
        start_paths.insert(state.clone(), path);
        seen_order.push(state.clone());
        queue.push_back(state);
    }

    while let Some(state) = queue.pop_back() {
        if let Some(label) = success_label_fast(ctx, request, &state) {
            return FastTraceCloseTurnResult {
                closed: seen_order
                    .iter()
                    .map(|closed| {
                        (
                            closed.clone(),
                            build_trace_path(closed, &start_paths, &parents),
                        )
                    })
                    .collect(),
                success: true,
                hit_limit,
                label: Some(label),
                path: build_trace_path(&state, &start_paths, &parents),
            };
        }
        let mut actions = generate_fast_trace_actions_with_config(ctx, &state, config);
        if request.action_sort {
            actions.sort_by_key(|action| action.priority);
        }
        if let Some((success_index, label)) = best_immediate_success_fast(ctx, request, &actions) {
            let action = &actions[success_index];
            let mut path = build_trace_path(&state, &start_paths, &parents);
            path.push(trace_label_for_transition(
                ctx,
                &state,
                &action.next_state,
                action,
            ));
            return FastTraceCloseTurnResult {
                closed: seen_order
                    .iter()
                    .map(|closed| {
                        (
                            closed.clone(),
                            build_trace_path(closed, &start_paths, &parents),
                        )
                    })
                    .collect(),
                success: true,
                hit_limit,
                label: Some(label),
                path,
            };
        }
        for action in actions {
            let next_state = action.next_state.clone();
            if seen_set.contains(&next_state)
                || mana_dominated_fast(ctx, request, &next_state, &mut best_mana)
            {
                continue;
            }
            let transition_label = trace_label_for_transition(ctx, &state, &next_state, &action);
            if seen_order.len() >= request.state_limit {
                hit_limit = true;
                continue;
            }
            seen_set.insert(next_state.clone());
            parents.insert(next_state.clone(), (state.clone(), transition_label));
            seen_order.push(next_state.clone());
            queue.push_back(next_state);
        }
    }

    FastTraceCloseTurnResult {
        closed: seen_order
            .iter()
            .map(|closed| {
                (
                    closed.clone(),
                    build_trace_path(closed, &start_paths, &parents),
                )
            })
            .collect(),
        success: false,
        hit_limit,
        label: None,
        path: Vec::new(),
    }
}

fn best_immediate_success_fast(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    actions: &[FastAction],
) -> Option<(usize, String)> {
    actions
        .iter()
        .enumerate()
        .filter_map(|(index, action)| {
            success_label_fast(ctx, request, &action.next_state).map(|label| (index, label))
        })
        .min_by_key(|(index, label)| (engine_target_priority_rank(label), *index))
}

fn build_trace_path(
    state: &FastState,
    start_paths: &FxHashMap<FastState, Vec<String>>,
    parents: &FxHashMap<FastState, (FastState, String)>,
) -> Vec<String> {
    let mut current = state.clone();
    let mut tail = Vec::new();
    while let Some((parent, label)) = parents.get(&current) {
        tail.push(label.clone());
        current = parent.clone();
    }
    let mut path = start_paths.get(&current).cloned().unwrap_or_default();
    tail.reverse();
    path.extend(tail);
    path
}

fn trace_label_for_transition(
    ctx: &FastContext,
    before: &FastState,
    after: &FastState,
    action: &FastAction,
) -> String {
    if action.is_ragavan_attack {
        return "attack Ragavan, Nimble Pilferer".to_string();
    }

    let removed_hand = card_multiset_delta(&before.hand, &after.hand);
    let added_hand = card_multiset_delta(&after.hand, &before.hand);
    let removed_library = card_multiset_delta(&before.library, &after.library);
    let removed_graveyard = card_multiset_delta(&before.graveyard, &after.graveyard);
    let removed_perms = perm_multiset_delta(&before.battlefield, &after.battlefield);
    let added_perms = perm_multiset_delta(&after.battlefield, &before.battlefield);
    let removed_names = card_names_for_trace(ctx, &removed_hand);
    let added_hand_names = card_names_for_trace(ctx, &added_hand);
    let engine_added = trace_added_engine_name(ctx, before, after);
    let mana_increased = mana_total(after.mana()) > mana_total(before.mana());
    let led_used = removed_perms
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Led);

    if has_trace_card(&removed_names, "An Offer You Can't Refuse") {
        let bait = removed_names
            .iter()
            .find(|name| name.as_str() != "An Offer You Can't Refuse")
            .map(String::as_str)
            .unwrap_or("own spell");
        return format!("cast An Offer You Can't Refuse countering {bait}");
    }

    if has_trace_card(&removed_names, "Noxious Revival") {
        let target_names = card_names_for_trace(ctx, &removed_graveyard);
        let target = target_names
            .iter()
            .find(|name| name.as_str() != "Noxious Revival")
            .map(String::as_str)
            .unwrap_or("graveyard card");
        return format!("cast Noxious Revival for {target}");
    }

    if has_trace_card(&removed_names, "Chrome Mox") {
        let imprint = removed_names
            .iter()
            .find(|name| name.as_str() != "Chrome Mox")
            .map(String::as_str)
            .unwrap_or("unknown card");
        return format!("cast Chrome Mox imprinting {imprint}");
    }

    if has_trace_card(&removed_names, "Mox Diamond") {
        let discard = removed_names
            .iter()
            .find(|name| name.as_str() != "Mox Diamond")
            .map(String::as_str)
            .unwrap_or("land");
        return format!("cast Mox Diamond discarding {discard}");
    }

    if let Some(engine_name) = engine_added {
        if let Some(tutor_name) = selected_trace_tutor_name(&removed_names) {
            if led_used {
                return format!("cast {tutor_name} with Lion's Eye Diamond for {engine_name}");
            }
            return format!("cast {tutor_name} for {engine_name}");
        }
        if removed_perms
            .iter()
            .any(|perm| perm.kind_enum() == FastPermKind::Wishclaw)
        {
            if led_used {
                return format!("activate Wishclaw with Lion's Eye Diamond for {engine_name}");
            }
            return format!("activate Wishclaw for {engine_name}");
        }
        if has_trace_card(&removed_names, &engine_name) {
            return format!("cast {engine_name}");
        }
        if let Some(card_name) = removed_names.first() {
            return format!("cast {card_name} for {engine_name}");
        }
        return format!("resolve {engine_name}");
    }

    if before.spells_this_turn() != after.spells_this_turn() {
        if let Some(tutor_name) = selected_trace_tutor_name(&removed_names) {
            if let Some(target_name) =
                selected_trace_target_name(&added_hand_names, &removed_library, ctx)
            {
                if led_used {
                    return format!("cast {tutor_name} with Lion's Eye Diamond for {target_name}");
                }
                return format!("cast {tutor_name} for {target_name}");
            }
            return format!("cast {tutor_name}");
        }
    }

    if removed_perms
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Wishclaw)
    {
        if let Some(target_name) =
            selected_trace_target_name(&added_hand_names, &removed_library, ctx)
        {
            if led_used {
                return format!("activate Wishclaw with Lion's Eye Diamond for {target_name}");
            }
            return format!("activate Wishclaw for {target_name}");
        }
        return "activate Wishclaw".to_string();
    }

    if !before.land_played() && after.land_played() {
        if let Some(card_name) = removed_names.first() {
            return format!("play {card_name}");
        }
    }

    if let Some(card_name) = removed_names
        .iter()
        .find(|name| matches!(name.as_str(), "Elvish Spirit Guide" | "Simian Spirit Guide"))
    {
        return format!("exile {card_name} for mana");
    }

    if mana_increased {
        if let Some(name) = trace_tapped_perm_name(ctx, before, after) {
            return format!("tap {name} for mana");
        }
        if let Some(name) = removed_perms
            .first()
            .map(|perm| trace_perm_name(ctx, *perm))
        {
            return format!("sacrifice {name} for mana");
        }
    }

    if before.spells_this_turn() != after.spells_this_turn() {
        if let Some(card_name) = removed_names.first() {
            return format!("cast {card_name}");
        }
        if let Some(name) = added_perms.first().map(|perm| trace_perm_name(ctx, *perm)) {
            return format!("cast {name}");
        }
    }

    if mana_increased {
        if let Some(card_name) = removed_names.first() {
            return format!("use {card_name} for mana");
        }
    }

    "advance line".to_string()
}

fn card_multiset_delta(left: &[CardId], right: &[CardId]) -> Vec<CardId> {
    let mut counts = BTreeMap::<CardId, isize>::new();
    for card in left {
        *counts.entry(*card).or_insert(0) += 1;
    }
    for card in right {
        *counts.entry(*card).or_insert(0) -= 1;
    }
    let mut out = Vec::new();
    for (card, count) in counts {
        for _ in 0..count.max(0) {
            out.push(card);
        }
    }
    out
}

fn perm_multiset_delta(left: &[FastPerm], right: &[FastPerm]) -> Vec<FastPerm> {
    let mut counts = BTreeMap::<FastPerm, isize>::new();
    for perm in left {
        *counts.entry(*perm).or_insert(0) += 1;
    }
    for perm in right {
        *counts.entry(*perm).or_insert(0) -= 1;
    }
    let mut out = Vec::new();
    for (perm, count) in counts {
        for _ in 0..count.max(0) {
            out.push(perm);
        }
    }
    out
}

fn card_names_for_trace(ctx: &FastContext, cards: &[CardId]) -> Vec<String> {
    cards
        .iter()
        .map(|card| ctx.card_name(*card).to_string())
        .collect()
}

fn has_trace_card(names: &[String], needle: &str) -> bool {
    names.iter().any(|name| name == needle)
}

fn selected_trace_tutor_name(removed_names: &[String]) -> Option<String> {
    for candidate in [
        "Beseech the Mirror",
        "Demonic Tutor",
        "Diabolic Intent",
        "Grim Tutor",
        "Idyllic Tutor",
        "Eldritch Evolution",
        "Neoform",
        "Green Sun's Zenith",
        "Summoner's Pact",
        "Enlightened Tutor",
        "Worldly Tutor",
        "Imperial Seal",
        "Mystical Tutor",
        "Scheming Symmetry",
        "Vampiric Tutor",
        "Gamble",
        "Crop Rotation",
    ] {
        if has_trace_card(removed_names, candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn selected_trace_target_name(
    added_hand_names: &[String],
    removed_library: &[CardId],
    ctx: &FastContext,
) -> Option<String> {
    if !added_hand_names.is_empty() {
        let mut candidates = added_hand_names.to_vec();
        candidates.sort_unstable_by(|left, right| {
            engine_target_priority_rank(left)
                .cmp(&engine_target_priority_rank(right))
                .then_with(|| left.cmp(right))
        });
        return candidates.into_iter().next();
    }
    let mut library_names = card_names_for_trace(ctx, removed_library);
    library_names.sort_unstable_by(|left, right| {
        engine_target_priority_rank(left)
            .cmp(&engine_target_priority_rank(right))
            .then_with(|| left.cmp(right))
    });
    library_names.into_iter().next()
}

fn trace_added_engine_name(
    ctx: &FastContext,
    before: &FastState,
    after: &FastState,
) -> Option<String> {
    let before_names: BTreeSet<InternId> = before.engine_names.iter().copied().collect();
    for item in &after.engine_names {
        if before_names.contains(item) {
            continue;
        }
        let item_text = ctx.interned_string(*item);
        if let Some((name, _turn)) = parse_engine_name_turn(item_text) {
            return Some(name.to_string());
        }
        if let Some((name, _turn_text)) = item_text.rsplit_once('@') {
            return Some(name.to_string());
        }
    }
    None
}

fn mana_total(mana: Mana) -> u8 {
    mana.iter().copied().sum()
}

fn trace_tapped_perm_name(
    ctx: &FastContext,
    before: &FastState,
    after: &FastState,
) -> Option<String> {
    for before_perm in before.battlefield.iter().copied() {
        if before_perm.tapped() {
            continue;
        }
        if after.battlefield.iter().any(|after_perm| {
            after_perm.kind_enum() == before_perm.kind_enum()
                && after_perm.tapped()
                && after_perm.colors() == before_perm.colors()
                && after_perm.fresh() == before_perm.fresh()
        }) {
            return Some(trace_perm_name(ctx, before_perm));
        }
    }
    None
}

fn trace_perm_name(_ctx: &FastContext, perm: FastPerm) -> String {
    match perm.kind_enum() {
        FastPermKind::Land => "land".to_string(),
        FastPermKind::CcLand => "Ancient Tomb".to_string(),
        FastPermKind::City => "City of Traitors".to_string(),
        FastPermKind::Glimmer => "Glimmervoid".to_string(),
        FastPermKind::Cavern => "Gemstone Caverns".to_string(),
        FastPermKind::Mine => "Gemstone Mine".to_string(),
        FastPermKind::Tower => "Phyrexian Tower".to_string(),
        FastPermKind::Vein => "Crystal Vein".to_string(),
        FastPermKind::Petal => "Lotus Petal".to_string(),
        FastPermKind::Treasure => "Treasure".to_string(),
        FastPermKind::Led => "Lion's Eye Diamond".to_string(),
        FastPermKind::Amber => "Mox Amber".to_string(),
        FastPermKind::Opal => "Mox Opal".to_string(),
        FastPermKind::Mantle => "Paradise Mantle".to_string(),
        FastPermKind::Diamond => "Mox Diamond".to_string(),
        FastPermKind::Chrome => "Chrome Mox".to_string(),
        FastPermKind::Sol => "Sol Ring".to_string(),
        FastPermKind::Vault => "Mana Vault".to_string(),
        FastPermKind::Signet => "Arcane Signet".to_string(),
        FastPermKind::Wishclaw => "Wishclaw Talisman".to_string(),
        FastPermKind::Drum => "Springleaf Drum".to_string(),
        FastPermKind::Relic => "Relic of Legends".to_string(),
        FastPermKind::Esper => "Esper Sentinel".to_string(),
        FastPermKind::EngineEnch => "Rhystic Study".to_string(),
        FastPermKind::Nature => "Nature's Chosen".to_string(),
        FastPermKind::Nick => "Nick Fury, Agent of S.H.I.E.L.D.".to_string(),
        FastPermKind::Rog => "Rograkh, Son of Rohgahh".to_string(),
        FastPermKind::Bird => "Birds of Paradise".to_string(),
        FastPermKind::Deathrite => "Deathrite Shaman".to_string(),
        FastPermKind::Tinder => "Tinder Wall".to_string(),
        FastPermKind::Noble => "Noble Hierarch".to_string(),
        FastPermKind::Ignoble => "Ignoble Hierarch".to_string(),
        FastPermKind::Cantor => "Wild Cantor".to_string(),
        FastPermKind::Faerie => "Faerie Mastermind".to_string(),
        FastPermKind::Bowmasters => "Orcish Bowmasters".to_string(),
        FastPermKind::Cabbage => "The Cabbage Merchant".to_string(),
        FastPermKind::Valley => "Valley Floodcaller".to_string(),
        FastPermKind::Tataru => "Tataru Taru".to_string(),
        FastPermKind::Ragavan => "Ragavan, Nimble Pilferer".to_string(),
        FastPermKind::Lotho => "Lotho, Corrupt Shirriff".to_string(),
        FastPermKind::Heartwood => "Heartwood Storyteller".to_string(),
        FastPermKind::Creature => "creature".to_string(),
        FastPermKind::Birgi => "Birgi, God of Storytelling".to_string(),
        FastPermKind::Wan => "Wan Shi Tong, Librarian".to_string(),
        FastPermKind::Ishai => "Ishai, Ojutai Dragonspeaker".to_string(),
        FastPermKind::Artifact => "artifact".to_string(),
        FastPermKind::Unknown => "permanent".to_string(),
    }
}

fn trace_line_cards_from_actions(deck: &[String], actions: &[String]) -> Vec<String> {
    let mut candidates = deck.to_vec();
    candidates.push("Nick Fury, Agent of S.H.I.E.L.D.".to_string());
    candidates.sort_by_key(|name| std::cmp::Reverse(name.len()));
    candidates.dedup();
    let mut found = BTreeSet::new();
    for action in actions {
        let excluded_cost_card = trace_excluded_cost_card(action);
        for candidate in &candidates {
            if excluded_cost_card.as_deref() == Some(candidate.as_str()) {
                continue;
            }
            if action.contains(candidate) {
                found.insert(candidate.clone());
            }
        }
    }
    found.into_iter().collect()
}

fn trace_excluded_cost_card(action: &str) -> Option<String> {
    for marker in [
        "begin with Gemstone Caverns exiling ",
        "begin with Glittering Caves of Aglarond exiling ",
        "cast Chrome Mox imprinting ",
        "cast Mox Diamond discarding ",
    ] {
        if let Some(card) = action.strip_prefix(marker) {
            return Some(card.to_string());
        }
    }
    None
}

fn close_response_from_fast(ctx: &FastContext, result: FastCloseTurnResult) -> CloseTurnResponse {
    let seen_count = result.closed.len();
    CloseTurnResponse {
        closed: result
            .closed
            .iter()
            .map(|state| state.to_fixture(ctx))
            .collect(),
        success: result.success,
        hit_limit: result.hit_limit,
        label: result.label,
        seen_count,
    }
}

fn success_label_fast(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    state: &FastState,
) -> Option<String> {
    let label = if request.goal == "engine" {
        engine_success_label_fast(ctx, request, state)
    } else if state.engine_count() > 0 {
        Some("Rhystic Study".to_string())
    } else {
        let rhystic = ctx.card_id("Rhystic Study");
        if state.has_card(rhystic) && !pay_options(state.mana(), [2, 0, 0, 1, 0, 0]).is_empty() {
            Some("Rhystic Study".to_string())
        } else {
            None
        }
    }?;
    if can_survive_next_pact_upkeep_fast(ctx, request, state) {
        Some(label)
    } else {
        None
    }
}

fn engine_success_label_fast(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    state: &FastState,
) -> Option<String> {
    if request.engine_success_policy == "count" {
        if state.engine_count() < request.engine_target_count {
            return None;
        }
        return best_engine_name_fast(ctx, state).or_else(|| Some("engine".to_string()));
    }
    if request.engine_success_policy != "resilient" {
        return None;
    }
    let mut candidates: Vec<(u8, u8, String)> = Vec::new();
    for item in &state.engine_names {
        let Some((name, turn)) = parse_engine_name_turn(ctx.interned_string(*item)) else {
            continue;
        };
        if matches!(
            name,
            "Rhystic Study" | "Heartwood Storyteller" | "Smothering Tithe"
        ) && turn <= 2
        {
            candidates.push((engine_target_priority_rank(name), turn, name.to_string()));
        }
    }
    for item in &state.engine_names {
        let item_text = ctx.interned_string(*item).to_string();
        let Some((name, turn)) = parse_engine_name_turn(&item_text) else {
            continue;
        };
        if name == "Mystic Remora"
            && turn == 1
            && can_keep_remora_fast(ctx, request, state, request.remora_upkeep_payments)
        {
            candidates.push((engine_target_priority_rank(name), turn, name.to_string()));
        }
    }
    candidates.into_iter().min().map(|(_, _, name)| name)
}

fn best_engine_name_fast(ctx: &FastContext, state: &FastState) -> Option<String> {
    let mut candidates: Vec<(u8, u8, String)> = Vec::new();
    for item in &state.engine_names {
        let item_text = ctx.interned_string(*item);
        if let Some((name, turn)) = parse_engine_name_turn(item_text) {
            candidates.push((engine_target_priority_rank(name), turn, name.to_string()));
        } else if let Some((name, _turn_text)) = item_text.rsplit_once('@') {
            candidates.push((engine_target_priority_rank(name), 99, name.to_string()));
        }
    }
    candidates.into_iter().min().map(|(_, _, name)| name)
}

fn parse_engine_name_turn(item: &str) -> Option<(&str, u8)> {
    let (name, turn_text) = item.rsplit_once('@')?;
    let turn = turn_text.parse::<u8>().ok()?;
    Some((name, turn))
}

fn can_survive_next_pact_upkeep_fast(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    state: &FastState,
) -> bool {
    if state.pact_debt() == 0 {
        return true;
    }
    let ended = end_turn_fast(ctx, state);
    let next_turn = begin_turn_fast(ctx, &ended);
    let cost = [
        2 * next_turn.pact_debt(),
        0,
        0,
        0,
        0,
        2 * next_turn.pact_debt(),
    ];
    can_pay_with_simple_taps_fast(&next_turn, cost)
        || pay_upkeep_pacts_fast(ctx, request, &next_turn)
        || can_cast_angels_grace_upkeep_fast(ctx, request, &next_turn)
}

fn can_pay_with_simple_taps_fast(state: &FastState, cost: Cost) -> bool {
    let mut mana_options: FxHashSet<Mana> = FxHashSet::default();
    mana_options.insert([0, 0, 0, 0, 0, 0]);
    for perm in &state.battlefield {
        if perm.tapped() {
            continue;
        }
        let mut additions: FxHashSet<Mana> = FxHashSet::default();
        for opt in fast_tap_options(state, *perm) {
            match opt {
                FastTap::Color(color) => {
                    additions.insert(mana_for_color_index(color));
                }
                FastTap::Colorless(amount) => {
                    additions.insert([0, 0, 0, 0, 0, amount]);
                }
                FastTap::Vault => {
                    additions.insert([0, 0, 0, 0, 0, 3]);
                }
            }
        }
        if additions.is_empty() {
            continue;
        }
        let mut updated = mana_options.clone();
        for current in &mana_options {
            for add in &additions {
                let candidate = add_mana(*current, *add);
                if !pay_options(candidate, cost).is_empty() {
                    return true;
                }
                updated.insert(candidate);
            }
        }
        mana_options = updated;
    }
    mana_options
        .into_iter()
        .any(|mana| !pay_options(mana, cost).is_empty())
}

fn pay_upkeep_pacts_fast(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    state: &FastState,
) -> bool {
    if state.pact_debt() == 0 {
        return true;
    }
    let cost = [2 * state.pact_debt(), 0, 0, 0, 0, 2 * state.pact_debt()];
    !pay_upkeep_options_fast(ctx, request, state, cost, true, 4096).is_empty()
}

fn can_cast_angels_grace_upkeep_fast(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    state: &FastState,
) -> bool {
    let Some(grace) = ctx.card_id("Angel's Grace") else {
        return false;
    };
    if !state.has_card(Some(grace)) {
        return false;
    }
    let cost = [0, 0, 0, 0, 1, 0];
    let mut queue = Vec::new();
    let mut seen = FxHashSet::default();
    let mut best_mana: FxHashMap<FastState, Vec<Mana>> = FxHashMap::default();
    queue.push(state.clone());
    seen.insert(state.clone());
    while let Some(current) = queue.pop() {
        if current.has_card(Some(grace)) && !pay_options(current.mana(), cost).is_empty() {
            return true;
        }
        let mut actions = Vec::new();
        generate_fast_mana_actions(ctx, &mut actions, &current);
        for action in actions {
            if action.is_ragavan_attack {
                continue;
            }
            let next_state = action.next_state;
            if !next_state.has_card(Some(grace))
                || seen.contains(&next_state)
                || mana_dominated_fast(ctx, request, &next_state, &mut best_mana)
            {
                continue;
            }
            if seen.len() >= request.state_limit.min(2048) {
                continue;
            }
            seen.insert(next_state.clone());
            queue.push(next_state);
        }
    }
    false
}

fn pact_upkeep_states_fast(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    state: &FastState,
) -> Vec<FastState> {
    if state.pact_debt() == 0 {
        return vec![state.clone()];
    }
    let cost = [2 * state.pact_debt(), 0, 0, 0, 0, 2 * state.pact_debt()];
    pay_upkeep_options_fast(ctx, request, state, cost, true, 4096)
}

fn pay_generic_upkeep_options_fast(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    state: &FastState,
    amount: u8,
) -> Vec<FastState> {
    pay_upkeep_options_fast(ctx, request, state, [amount, 0, 0, 0, 0, 0], false, 2048)
}

fn pay_upkeep_options_fast(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    state: &FastState,
    cost: Cost,
    clear_pact: bool,
    limit: usize,
) -> Vec<FastState> {
    let start = normalize_upkeep_payment_state_fast(state, cost);
    let mut queue = Vec::new();
    let mut seen = FxHashSet::default();
    let mut best_mana: FxHashMap<FastState, Vec<Mana>> = FxHashMap::default();
    let mut paid_states = Vec::new();
    queue.push(start.clone());
    seen.insert(start);
    while let Some(current) = queue.pop() {
        if !pay_options(current.mana(), cost).is_empty() {
            let mut paid = current.clone();
            paid.mana = PackedMana::zero();
            if clear_pact {
                paid.set_pact_debt(0);
            }
            paid_states.push(paid);
            continue;
        }
        for action in upkeep_mana_actions_fast(ctx, &current) {
            if action.is_ragavan_attack {
                continue;
            }
            let next_state = normalize_upkeep_payment_state_fast(&action.next_state, cost);
            if seen.contains(&next_state)
                || mana_dominated_fast(ctx, request, &next_state, &mut best_mana)
            {
                continue;
            }
            if seen.len() >= request.state_limit.min(limit) {
                continue;
            }
            seen.insert(next_state.clone());
            queue.push(next_state);
        }
    }
    paid_states
}

fn upkeep_mana_actions_fast(ctx: &mut FastContext, state: &FastState) -> Vec<FastAction> {
    let mut actions = Vec::new();
    generate_fast_mana_actions(ctx, &mut actions, state);
    generate_fast_spirit_guide_actions(ctx, &mut actions, state);
    generate_fast_dark_ritual_action(ctx, &mut actions, state);
    generate_fast_rain_actions(ctx, &mut actions, state);
    generate_fast_sac_spell_actions(ctx, &mut actions, state);
    for (index, perm) in state.battlefield.iter().enumerate() {
        if perm.kind_enum() != FastPermKind::Led || perm.tapped() {
            continue;
        }
        for color in 0..5 {
            let mut next = state.clone();
            remove_perm_to_graveyard(ctx, &mut next, index);
            next.move_hand_to_graveyard(ctx);
            next.set_mana(add_mana(state.mana(), led_mana_for_color(color)));
            actions.push(FastAction::new(next, FAST_MANA_PRIORITY));
        }
    }
    actions
}

fn normalize_upkeep_payment_state_fast(state: &FastState, cost: Cost) -> FastState {
    let generic = cost[0];
    let black = cost[1];
    let red = cost[2];
    let blue = cost[3];
    let white = cost[4];
    let green = cost[5];
    if black != 0 || red != 0 || blue != 0 || white != 0 {
        return state.clone();
    }
    let [b, r, u, w, g, c] = state.mana();
    let mana = if green != 0 {
        let non_green = generic.min(
            b.saturating_add(r)
                .saturating_add(u)
                .saturating_add(w)
                .saturating_add(c),
        );
        let useful_green = generic.saturating_add(green).min(g);
        [non_green, 0, 0, 0, useful_green, 0]
    } else {
        [
            generic.min(
                b.saturating_add(r)
                    .saturating_add(u)
                    .saturating_add(w)
                    .saturating_add(g)
                    .saturating_add(c),
            ),
            0,
            0,
            0,
            0,
            0,
        ]
    };
    if mana == state.mana() {
        return state.clone();
    }
    let mut next = state.clone();
    next.set_mana(mana);
    next
}

fn can_keep_remora_fast(
    ctx: &mut FastContext,
    request: &CloseTurnRequest,
    state: &FastState,
    payments: u8,
) -> bool {
    if payments == 0 {
        return true;
    }
    let mut states = vec![end_turn_fast(ctx, state)];
    for amount in 1..=payments {
        let mut paid_set = FxHashSet::default();
        let mut paid_states = Vec::new();
        for state_at_end in &states {
            let mut begun = begin_turn_fast(ctx, state_at_end);
            begun.set_turn(state_at_end.turn().saturating_add(1).min(8));
            for paid in pay_generic_upkeep_options_fast(ctx, request, &begun, amount) {
                if paid_set.insert(paid.clone()) {
                    paid_states.push(paid);
                }
            }
        }
        if paid_states.is_empty() {
            return false;
        }
        if amount == payments {
            return true;
        }
        let mut setup_set = FxHashSet::default();
        let mut setup_states = Vec::new();
        for paid in &paid_states {
            for setup in future_visible_setup_states_fast(ctx, paid) {
                let ended = end_turn_fast(ctx, &setup);
                if setup_set.insert(ended.clone()) {
                    setup_states.push(ended);
                }
            }
        }
        states = setup_states;
    }
    false
}

fn future_visible_setup_states_fast(ctx: &mut FastContext, state: &FastState) -> Vec<FastState> {
    let mut out = vec![state.clone()];
    if state.land_played() {
        return out;
    }
    let mut cards: Vec<CardId> = state
        .hand
        .iter()
        .copied()
        .filter(|card| {
            let flags = ctx.card_spec(*card).flags;
            flags.contains(CardFlags::LAND) || flags.contains(CardFlags::MDFC_LAND)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    cards.sort_unstable();
    for card in cards {
        for (perm, library, grave_inc) in land_options_fast(ctx, card, &state.library) {
            let mut next = state.clone();
            next.remove_hand_card(card);
            next.library = library.into();
            let city_indices: Vec<usize> = next
                .battlefield
                .iter()
                .enumerate()
                .filter_map(|(index, permanent)| {
                    (permanent.kind_enum() == FastPermKind::City).then_some(index)
                })
                .collect();
            for index in city_indices.into_iter().rev() {
                remove_land_perm_to_graveyard(ctx, &mut next, index);
            }
            next.push_perm(ctx, perm);
            next.set_flag(FastState::LAND_PLAYED, true);
            if grave_inc > 0 {
                next.add_graveyard_card(ctx, card);
            }
            out.push(next);
        }
    }
    out
}

fn begin_turn_fast(ctx: &mut FastContext, state: &FastState) -> FastState {
    let mut next = state.clone();
    for index in 0..next.battlefield.len() {
        let perm = next.battlefield[index];
        let tapped = if matches!(perm.kind_enum(), FastPermKind::Vault) {
            perm.tapped()
        } else {
            false
        };
        next.battlefield[index] = make_perm(
            ctx,
            perm.kind_enum(),
            tapped,
            perm.colors(),
            false,
            perm.counters(),
        );
    }
    sort_fast_battlefield(ctx, &mut next.battlefield);
    next.mana = PackedMana::zero();
    next.set_flag(FastState::LAND_PLAYED, false);
    next.set_flag(FastState::NATURE_UNTAP_USED, false);
    next.set_flag(FastState::NATURE_TAP_USED, false);
    next.set_flag(FastState::RAIN_ACTIVE, false);
    next.set_spells_this_turn(0);
    next
}

fn end_turn_fast(ctx: &mut FastContext, state: &FastState) -> FastState {
    let mut next = state.clone();
    if next
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Glimmer)
        && !next
            .battlefield
            .iter()
            .any(|perm| perm.kind_enum().is_artifact())
    {
        next.battlefield
            .retain(|perm| perm.kind_enum() != FastPermKind::Glimmer);
    }
    next.mana = PackedMana::zero();
    next.set_flag(FastState::LAND_PLAYED, false);
    next.set_flag(FastState::RAIN_ACTIVE, false);
    next.set_spells_this_turn(0);
    sort_fast_battlefield(ctx, &mut next.battlefield);
    next
}

fn draw_card_fast(mut state: FastState) -> FastState {
    if !state.library.is_empty() {
        let card = state.library.remove(0);
        state.add_hand_card(card);
    }
    state
}

pub fn solve_keep_fast(request: &SolveKeepRequest) -> SolveKeepResponse {
    let mut card_names = request.hand.clone();
    card_names.extend(request.library.iter().cloned());
    let mut ctx = FastContext::with_card_names(card_names);
    let config = FastSearchConfig::from_solve_request(request);
    solve_keep_fast_with_ctx(
        &mut ctx,
        &request.hand,
        &request.library,
        request.gemstone_live,
        request.state_limit,
        request.max_turns,
        &request.goal,
        request.engine_target_count,
        &request.engine_success_policy,
        request.remora_upkeep_payments,
        request.action_sort,
        &config,
    )
}

pub fn solve_keep_trace_fast(request: &SolveKeepRequest) -> SolveKeepTraceFastResponse {
    let mut card_names = request.hand.clone();
    card_names.extend(request.library.iter().cloned());
    let mut ctx = FastContext::with_card_names(card_names);
    let config = FastSearchConfig::from_solve_request(request);
    let traced = solve_keep_trace_fast_with_ctx(
        &mut ctx,
        &request.hand,
        &request.library,
        request.gemstone_live,
        request.state_limit,
        request.max_turns,
        &request.goal,
        request.engine_target_count,
        &request.engine_success_policy,
        request.remora_upkeep_payments,
        request.action_sort,
        &config,
    );
    SolveKeepTraceFastResponse {
        response: traced.response,
        trace_turn: traced.trace_turn,
        trace_capped: traced.trace_capped,
        trace_path: traced.trace_path,
    }
}

pub fn solve_keep_batch_fast(requests: &[SolveKeepRequest]) -> Vec<SolveKeepResponse> {
    let mut card_names = Vec::new();
    for request in requests {
        card_names.extend(request.hand.iter().cloned());
        card_names.extend(request.library.iter().cloned());
    }
    let mut ctx = FastContext::with_card_names(card_names);
    requests
        .iter()
        .map(|request| {
            let config = FastSearchConfig::from_solve_request(request);
            solve_keep_fast_with_ctx(
                &mut ctx,
                &request.hand,
                &request.library,
                request.gemstone_live,
                request.state_limit,
                request.max_turns,
                &request.goal,
                request.engine_target_count,
                &request.engine_success_policy,
                request.remora_upkeep_payments,
                request.action_sort,
                &config,
            )
        })
        .collect()
}

pub fn earliest_fast(request: &EarliestRequest) -> SolveKeepResponse {
    if request.deck_order.len() < 7 {
        return SolveKeepResponse {
            turn: None,
            capped: false,
            label: None,
            unsupported: true,
            unsupported_reason: Some("deck_order must contain at least 7 cards".to_string()),
        };
    }
    if request.bottom_count > 7 {
        return SolveKeepResponse {
            turn: None,
            capped: false,
            label: None,
            unsupported: true,
            unsupported_reason: Some("bottom_count must be <= 7".to_string()),
        };
    }

    let mut ctx = FastContext::with_card_names(request.deck_order.iter().cloned());
    let config = FastSearchConfig::from_earliest_request(request);
    let hand7 = &request.deck_order[..7];
    let rest = &request.deck_order[7..];
    let mut best_turn: Option<u8> = None;
    let mut best_label: Option<String> = None;
    let mut capped = false;

    for choice in bottom_choices(7, request.bottom_count) {
        let hand: Vec<String> = choice
            .kept_indices
            .iter()
            .map(|index| hand7[*index].clone())
            .collect();
        let mut library: Vec<String> = rest.to_vec();
        library.extend(
            choice
                .removed_indices
                .iter()
                .map(|index| hand7[*index].clone()),
        );
        let result = solve_keep_fast_with_ctx(
            &mut ctx,
            &hand,
            &library,
            request.gemstone_live,
            request.state_limit,
            request.max_turns,
            &request.goal,
            request.engine_target_count,
            &request.engine_success_policy,
            request.remora_upkeep_payments,
            request.action_sort,
            &config,
        );
        if result.unsupported {
            return result;
        }
        capped |= result.capped;
        if let Some(turn) = result.turn {
            if best_turn.map_or(true, |best| turn < best) {
                best_turn = Some(turn);
                best_label = result.label;
                if turn == 1 {
                    break;
                }
            }
        }
    }

    SolveKeepResponse {
        turn: best_turn,
        capped,
        label: best_label,
        unsupported: false,
        unsupported_reason: None,
    }
}

fn rust_stable_seed_bytes(
    seed: u64,
    tag: &str,
    parts: &[String],
    sample_index: Option<usize>,
) -> [u8; 32] {
    let mut hasher = Blake2bVar::new(32).expect("valid digest size");
    hasher.update(seed.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(tag.as_bytes());
    for card in parts {
        hasher.update(b"|");
        hasher.update(card.as_bytes());
    }
    if let Some(index) = sample_index {
        hasher.update(b"|");
        hasher.update(index.to_string().as_bytes());
    }
    let mut out = [0u8; 32];
    hasher
        .finalize_variable(&mut out)
        .expect("valid digest output");
    out
}

fn rust_stable_seed(seed: u64, tag: &str, parts: &[String], sample_index: Option<usize>) -> u64 {
    let bytes = rust_stable_seed_bytes(seed, tag, parts, sample_index);
    u64::from_be_bytes(
        bytes[..8]
            .try_into()
            .expect("seed digest has at least 8 bytes"),
    )
}

fn chacha_rng(seed: u64, tag: &str, parts: &[String], sample_index: Option<usize>) -> ChaCha20Rng {
    ChaCha20Rng::from_seed(rust_stable_seed_bytes(seed, tag, parts, sample_index))
}

fn shuffled_copy(
    cards: &[String],
    seed: u64,
    tag: &str,
    parts: &[String],
    sample_index: Option<usize>,
) -> Vec<String> {
    let mut out = cards.to_vec();
    out.shuffle(&mut chacha_rng(seed, tag, parts, sample_index));
    out
}

fn selection_not_better(
    score: f64,
    hits: usize,
    bottom_len: usize,
    best_score: f64,
    best_hits: usize,
    best_bottom_len: usize,
) -> bool {
    if score < best_score {
        return true;
    }
    if score > best_score {
        return false;
    }
    if hits < best_hits {
        return true;
    }
    if hits > best_hits {
        return false;
    }
    bottom_len >= best_bottom_len
}

fn selection_better(
    score: f64,
    hits: usize,
    bottom_len: usize,
    best_score: f64,
    best_hits: usize,
    best_bottom_len: usize,
) -> bool {
    !selection_not_better(
        score,
        hits,
        bottom_len,
        best_score,
        best_hits,
        best_bottom_len,
    ) && (score != best_score || hits != best_hits || bottom_len != best_bottom_len)
}

fn selection_equal(
    score: f64,
    hits: usize,
    bottom_len: usize,
    best_score: f64,
    best_hits: usize,
    best_bottom_len: usize,
) -> bool {
    score == best_score && hits == best_hits && bottom_len == best_bottom_len
}

fn bottom_preference_score_fast(bottom: &[String]) -> i32 {
    bottom
        .iter()
        .map(|card| match card.as_str() {
            // These cards are often post-engine protection/payoff. In an
            // otherwise tied pre-engine keep, bottom them before mana/tutors.
            "Commandeer"
            | "Deflecting Swat"
            | "Disrupting Shoal"
            | "Fierce Guardianship"
            | "Force of Negation"
            | "Force of Will"
            | "Misdirection"
            | "Mindbreak Trap"
            | "Pact of Negation"
            | "Subtlety" => 100,
            "Ranger-Captain of Eos"
            | "The Cabbage Merchant"
            | "Valley Floodcaller"
            | "Faerie Mastermind"
            | "Orcish Bowmasters"
            | "Sudden Substitution"
            | "Word of Seizing"
            | "Molten Disaster" => 80,
            "Copy Enchantment" | "Flash Photography" | "Borne Upon a Wind" | "Angel's Grace" => 60,
            "Chain of Vapor"
            | "Dispel"
            | "Flusterstorm"
            | "Mental Misstep"
            | "Orim's Chant"
            | "Pyroblast"
            | "Silence"
            | "Snapback"
            | "Swan Song"
            | "Wipe Away"
            | "An Offer You Can't Refuse" => 40,
            _ => 0,
        })
        .sum()
}

struct VisibleSolveOutcome {
    hit: bool,
    capped: bool,
    turn: Option<u8>,
    label: Option<String>,
    unsupported_reason: Option<String>,
}

fn policy_max_sample_score(request: &VisibleHandBatchRequest) -> f64 {
    if !request.weighted_policy_ev {
        return 1.0;
    }
    request
        .cap_weight
        .max(request.rhystic_t1_weight)
        .max(request.rhystic_t2_weight)
        .max(request.heartwood_t1_weight)
        .max(request.heartwood_t2_weight)
}

fn policy_outcome_hit_score(
    request: &VisibleHandBatchRequest,
    label: Option<&str>,
    turn: Option<u8>,
) -> f64 {
    if !request.weighted_policy_ev {
        return if turn.is_some_and(|value| value <= request.max_turns) {
            1.0
        } else {
            0.0
        };
    }
    match (label, turn) {
        (Some("Rhystic Study"), Some(1)) => request.rhystic_t1_weight,
        (Some("Rhystic Study"), Some(2)) => request.rhystic_t2_weight,
        (Some("Heartwood Storyteller"), Some(1)) => request.heartwood_t1_weight,
        (Some("Heartwood Storyteller"), Some(2)) => request.heartwood_t2_weight,
        _ => 0.0,
    }
}

fn policy_outcome_score(request: &VisibleHandBatchRequest, outcome: &VisibleSolveOutcome) -> f64 {
    if outcome.hit {
        policy_outcome_hit_score(request, outcome.label.as_deref(), outcome.turn)
    } else if outcome.capped {
        request.cap_weight
    } else {
        0.0
    }
}

fn visible_solve_outcome(
    ctx: &mut FastContext,
    request: &VisibleHandBatchRequest,
    keep: &[String],
    library: &[String],
    gemstone_live: bool,
    gamble_seed: u64,
    base_config: &FastSearchConfig,
    force_gamble_off: bool,
) -> VisibleSolveOutcome {
    let config = FastSearchConfig {
        gamble_mode: if force_gamble_off {
            FastGambleMode::Off
        } else {
            base_config.gamble_mode
        },
        gamble_seed,
    };
    let response = solve_keep_fast_with_ctx(
        ctx,
        keep,
        library,
        gemstone_live,
        request.state_limit,
        request.max_turns,
        &request.goal,
        request.engine_target_count,
        &request.engine_success_policy,
        request.remora_upkeep_payments,
        request.action_sort,
        &config,
    );
    VisibleSolveOutcome {
        hit: response.turn.is_some_and(|turn| turn <= request.max_turns),
        capped: response.capped,
        turn: response.turn,
        label: response.label,
        unsupported_reason: if response.unsupported {
            response.unsupported_reason
        } else {
            None
        },
    }
}

fn unsupported_visible_response(
    task: &VisibleHandTaskRequest,
    hand: &[String],
    reason: String,
    solver_calls: usize,
    deterministic_checked: usize,
    bottom_candidates_checked: usize,
) -> VisibleHandResponse {
    VisibleHandResponse {
        key: task.key.clone(),
        hand: hand.to_vec(),
        bottom_count: task.bottom_count,
        gemstone_caverns_live: task.gemstone_live,
        ev: 0.0,
        upper_ev: 0.0,
        score_ev: 0.0,
        hits: 0,
        cap_misses: 0,
        samples: 0,
        best_bottom: Vec::new(),
        deterministic: false,
        selection_hits: 0,
        selection_cap_misses: 0,
        selection_samples: 0,
        selection_score: 0.0,
        solver_calls,
        deterministic_checked,
        bottom_candidates_checked,
        selection_early_stopped: false,
        selection_pruned_candidates: 0,
        selection_pruned_samples: 0,
        selection_skipped_single_candidate_samples: 0,
        validation_samples_used: 0,
        adaptive_threshold_resolved: false,
        adaptive_threshold_resolution: "unsupported".to_string(),
        adaptive_validation_samples_saved: 0,
        unsupported: true,
        unsupported_reason: Some(reason),
    }
}

fn deterministic_hit_response(
    task: &VisibleHandTaskRequest,
    hand: &[String],
    bottom: &[String],
    request: &VisibleHandBatchRequest,
    score: f64,
    solver_calls: usize,
    deterministic_checked: usize,
    bottom_candidates_checked: usize,
    selection_pruned_candidates: usize,
    selection_pruned_samples: usize,
    selection_skipped_single_candidate_samples: usize,
) -> VisibleHandResponse {
    VisibleHandResponse {
        key: task.key.clone(),
        hand: hand.to_vec(),
        bottom_count: task.bottom_count,
        gemstone_caverns_live: task.gemstone_live,
        ev: 1.0,
        upper_ev: 1.0,
        score_ev: score,
        hits: request.validation_samples,
        cap_misses: 0,
        samples: request.validation_samples,
        best_bottom: bottom.to_vec(),
        deterministic: true,
        selection_hits: request.samples_per_bottom,
        selection_cap_misses: 0,
        selection_samples: request.samples_per_bottom,
        selection_score: score,
        solver_calls,
        deterministic_checked,
        bottom_candidates_checked,
        selection_early_stopped: true,
        selection_pruned_candidates,
        selection_pruned_samples,
        selection_skipped_single_candidate_samples,
        validation_samples_used: 0,
        adaptive_threshold_resolved: task.adaptive_threshold_sampling,
        adaptive_threshold_resolution: if task.adaptive_threshold_sampling {
            "deterministic_hit".to_string()
        } else {
            "full".to_string()
        },
        adaptive_validation_samples_saved: if task.adaptive_threshold_sampling {
            request.validation_samples
        } else {
            0
        },
        unsupported: false,
        unsupported_reason: None,
    }
}

fn evaluate_visible_hand_fast_with_ctx(
    ctx: &mut FastContext,
    request: &VisibleHandBatchRequest,
    task: &VisibleHandTaskRequest,
    config: &FastSearchConfig,
) -> VisibleHandResponse {
    let mut hand = task.hand.clone();
    hand.sort();
    let hand_set: FxHashSet<&str> = hand.iter().map(String::as_str).collect();
    let base_remaining: Vec<String> = request
        .deck
        .iter()
        .filter(|card| !hand_set.contains(card.as_str()))
        .cloned()
        .collect();
    let choices = bottom_choices(hand.len(), task.bottom_count);
    let single_bottom_choice = choices.len() == 1;
    let mut best_bottom: Vec<String> = Vec::new();
    let mut best_selection_hits = 0usize;
    let mut best_selection_cap_misses = 0usize;
    let mut best_selection_samples = 0usize;
    let mut best_selection_score = f64::NEG_INFINITY;
    let mut best_bottom_preference_score = i32::MIN;
    let mut best_selection_deterministic = false;
    let mut have_best_selection = false;

    let mut total_solver_calls = 0usize;
    let mut deterministic_checked = 0usize;
    let mut bottom_candidates_checked = 0usize;
    let selection_early_stopped = false;
    let mut selection_pruned_candidates = 0usize;
    let mut selection_pruned_samples = 0usize;
    let mut selection_skipped_single_candidate_samples = 0usize;

    for choice in &choices {
        bottom_candidates_checked += 1;
        let bottom: Vec<String> = choice
            .removed_indices
            .iter()
            .map(|index| hand[*index].clone())
            .collect();
        let keep: Vec<String> = choice
            .kept_indices
            .iter()
            .map(|index| hand[*index].clone())
            .collect();
        let mut blank_library = vec!["Blank".to_string(); 20];
        blank_library.extend(base_remaining.iter().cloned());
        blank_library.extend(bottom.iter().cloned());
        deterministic_checked += 1;
        total_solver_calls += 1;
        let deterministic = visible_solve_outcome(
            ctx,
            request,
            &keep,
            &blank_library,
            task.gemstone_live,
            rust_stable_seed(task.seed, "deterministic", &bottom, None),
            config,
            matches!(
                config.gamble_mode,
                FastGambleMode::StochasticSimplified | FastGambleMode::StochasticUnsupported
            ),
        );
        if let Some(reason) = deterministic.unsupported_reason {
            return unsupported_visible_response(
                task,
                &hand,
                reason,
                total_solver_calls,
                deterministic_checked,
                bottom_candidates_checked,
            );
        }
        if deterministic.hit {
            let bottom_preference_score = bottom_preference_score_fast(&bottom);
            let selection_score = policy_outcome_score(request, &deterministic);
            let selection_hits = request.samples_per_bottom;
            if !have_best_selection
                || selection_better(
                    selection_score,
                    selection_hits,
                    bottom.len(),
                    best_selection_score,
                    best_selection_hits,
                    best_bottom.len(),
                )
                || (selection_equal(
                    selection_score,
                    selection_hits,
                    bottom.len(),
                    best_selection_score,
                    best_selection_hits,
                    best_bottom.len(),
                ) && bottom_preference_score > best_bottom_preference_score)
            {
                best_bottom = bottom;
                best_selection_hits = selection_hits;
                best_selection_cap_misses = 0;
                best_selection_samples = request.samples_per_bottom;
                best_selection_score = selection_score;
                best_bottom_preference_score = bottom_preference_score;
                best_selection_deterministic = true;
                have_best_selection = true;
            }
            continue;
        }

        let mut selection_hits = 0usize;
        let mut selection_cap_misses = usize::from(deterministic.capped);
        let mut selection_score_units = if deterministic.capped {
            request.cap_weight
        } else {
            0.0
        };
        if single_bottom_choice {
            selection_skipped_single_candidate_samples += request.samples_per_bottom;
            best_bottom = bottom;
            best_selection_hits = 0;
            best_selection_cap_misses = selection_cap_misses;
            best_selection_samples = 0;
            best_selection_score = 0.0;
            best_selection_deterministic = false;
            have_best_selection = true;
            break;
        }

        if have_best_selection {
            let max_remaining_value = policy_max_sample_score(request);
            let max_possible_score = ((selection_score_units
                + request.samples_per_bottom as f64 * max_remaining_value)
                / request.samples_per_bottom as f64)
                .min(max_remaining_value);
            if selection_not_better(
                max_possible_score,
                request.samples_per_bottom,
                bottom.len(),
                best_selection_score,
                best_selection_hits,
                best_bottom.len(),
            ) {
                selection_pruned_candidates += 1;
                selection_pruned_samples += request.samples_per_bottom;
                continue;
            }
        }

        let mut candidate_pruned = false;
        for sample_index in 0..request.samples_per_bottom {
            let mut library = shuffled_copy(
                &base_remaining,
                task.seed,
                "selection_shuffle",
                &bottom,
                Some(sample_index),
            );
            library.extend(bottom.iter().cloned());
            total_solver_calls += 1;
            let sample = visible_solve_outcome(
                ctx,
                request,
                &keep,
                &library,
                task.gemstone_live,
                rust_stable_seed(task.seed, "selection", &bottom, Some(sample_index)),
                config,
                false,
            );
            if let Some(reason) = sample.unsupported_reason {
                return unsupported_visible_response(
                    task,
                    &hand,
                    reason,
                    total_solver_calls,
                    deterministic_checked,
                    bottom_candidates_checked,
                );
            }
            selection_hits += usize::from(sample.hit);
            selection_cap_misses += usize::from(sample.capped && !sample.hit);
            selection_score_units += policy_outcome_score(request, &sample);
            let remaining_samples = request.samples_per_bottom - sample_index - 1;
            if have_best_selection && remaining_samples > 0 {
                let max_remaining_value = policy_max_sample_score(request);
                let max_possible_score = ((selection_score_units
                    + remaining_samples as f64 * max_remaining_value)
                    / request.samples_per_bottom as f64)
                    .min(max_remaining_value);
                let max_possible_hits = selection_hits + remaining_samples;
                if selection_not_better(
                    max_possible_score,
                    max_possible_hits,
                    bottom.len(),
                    best_selection_score,
                    best_selection_hits,
                    best_bottom.len(),
                ) {
                    selection_pruned_candidates += 1;
                    selection_pruned_samples += remaining_samples;
                    candidate_pruned = true;
                    break;
                }
            }
        }
        if candidate_pruned {
            continue;
        }

        let max_remaining_value = policy_max_sample_score(request);
        let selection_score =
            (selection_score_units / request.samples_per_bottom as f64).min(max_remaining_value);
        let bottom_preference_score = bottom_preference_score_fast(&bottom);
        if !have_best_selection
            || selection_better(
                selection_score,
                selection_hits,
                bottom.len(),
                best_selection_score,
                best_selection_hits,
                best_bottom.len(),
            )
            || (selection_equal(
                selection_score,
                selection_hits,
                bottom.len(),
                best_selection_score,
                best_selection_hits,
                best_bottom.len(),
            ) && bottom_preference_score > best_bottom_preference_score)
        {
            best_bottom = bottom;
            best_selection_hits = selection_hits;
            best_selection_cap_misses = selection_cap_misses;
            best_selection_samples = request.samples_per_bottom;
            best_selection_score = selection_score;
            best_bottom_preference_score = bottom_preference_score;
            best_selection_deterministic = false;
            have_best_selection = true;
        }
    }

    if !have_best_selection {
        return unsupported_visible_response(
            task,
            &hand,
            "no legal bottom choices".to_string(),
            total_solver_calls,
            deterministic_checked,
            bottom_candidates_checked,
        );
    }

    let bottom_set: FxHashSet<&str> = best_bottom.iter().map(String::as_str).collect();
    let keep: Vec<String> = hand
        .iter()
        .filter(|card| !bottom_set.contains(card.as_str()))
        .cloned()
        .collect();

    if best_selection_deterministic {
        return deterministic_hit_response(
            task,
            &hand,
            &best_bottom,
            request,
            best_selection_score,
            total_solver_calls,
            deterministic_checked,
            bottom_candidates_checked,
            selection_pruned_candidates,
            selection_pruned_samples,
            selection_skipped_single_candidate_samples,
        );
    }

    let mut validation_hits = 0usize;
    let mut validation_cap_misses = 0usize;
    let mut validation_score_units = 0.0f64;
    let mut validation_samples_used = 0usize;
    let mut adaptive_threshold_resolved = false;
    let mut adaptive_threshold_resolution = "full".to_string();
    let (ev, upper_ev, score_ev) = if task.adaptive_threshold_sampling && task.force_keep {
        adaptive_threshold_resolved = true;
        adaptive_threshold_resolution = "force_keep".to_string();
        (0.0, 1.0, policy_max_sample_score(request))
    } else {
        for sample_index in 0..request.validation_samples {
            let mut library = shuffled_copy(
                &base_remaining,
                task.seed,
                "validation_shuffle",
                &best_bottom,
                Some(sample_index),
            );
            library.extend(best_bottom.iter().cloned());
            total_solver_calls += 1;
            let sample = visible_solve_outcome(
                ctx,
                request,
                &keep,
                &library,
                task.gemstone_live,
                rust_stable_seed(task.seed, "validation", &best_bottom, Some(sample_index)),
                config,
                false,
            );
            if let Some(reason) = sample.unsupported_reason {
                return unsupported_visible_response(
                    task,
                    &hand,
                    reason,
                    total_solver_calls,
                    deterministic_checked,
                    bottom_candidates_checked,
                );
            }
            validation_hits += usize::from(sample.hit);
            validation_cap_misses += usize::from(sample.capped && !sample.hit);
            validation_score_units += policy_outcome_score(request, &sample);
            validation_samples_used += 1;
            if task.adaptive_threshold_sampling {
                if let Some(keep_threshold) = task.keep_threshold {
                    let remaining_samples = request.validation_samples - validation_samples_used;
                    let max_remaining_value = policy_max_sample_score(request);
                    let current_score_units = validation_score_units;
                    let min_score = (current_score_units / request.validation_samples as f64)
                        .min(max_remaining_value);
                    let max_score = ((current_score_units
                        + remaining_samples as f64 * max_remaining_value)
                        / request.validation_samples as f64)
                        .min(max_remaining_value);
                    if min_score >= keep_threshold {
                        adaptive_threshold_resolved = true;
                        adaptive_threshold_resolution = "keep".to_string();
                        break;
                    }
                    if max_score < keep_threshold {
                        adaptive_threshold_resolved = true;
                        adaptive_threshold_resolution = "mulligan".to_string();
                        break;
                    }
                }
            }
        }

        let ev = validation_hits as f64 / request.validation_samples as f64;
        let mut upper_ev = ((validation_hits + validation_cap_misses) as f64
            / request.validation_samples as f64)
            .min(1.0);
        let max_remaining_value = policy_max_sample_score(request);
        let mut score_ev =
            (validation_score_units / request.validation_samples as f64).min(max_remaining_value);
        if adaptive_threshold_resolution == "mulligan" {
            let remaining_samples = request.validation_samples - validation_samples_used;
            score_ev = ((validation_score_units + remaining_samples as f64 * max_remaining_value)
                / request.validation_samples as f64)
                .min(max_remaining_value);
            upper_ev = ((validation_hits + validation_cap_misses + remaining_samples) as f64
                / request.validation_samples as f64)
                .min(1.0);
        }
        (ev, upper_ev, score_ev)
    };

    VisibleHandResponse {
        key: task.key.clone(),
        hand,
        bottom_count: task.bottom_count,
        gemstone_caverns_live: task.gemstone_live,
        ev,
        upper_ev,
        score_ev,
        hits: validation_hits,
        cap_misses: validation_cap_misses,
        samples: request.validation_samples,
        best_bottom,
        deterministic: false,
        selection_hits: best_selection_hits,
        selection_cap_misses: best_selection_cap_misses,
        selection_samples: best_selection_samples,
        selection_score: best_selection_score,
        solver_calls: total_solver_calls,
        deterministic_checked,
        bottom_candidates_checked,
        selection_early_stopped,
        selection_pruned_candidates,
        selection_pruned_samples,
        selection_skipped_single_candidate_samples,
        validation_samples_used,
        adaptive_threshold_resolved,
        adaptive_threshold_resolution,
        adaptive_validation_samples_saved: request.validation_samples - validation_samples_used,
        unsupported: false,
        unsupported_reason: None,
    }
}

pub fn evaluate_visible_hand_batch_fast(
    request: &VisibleHandBatchRequest,
) -> Vec<VisibleHandResponse> {
    let mut card_names = request.deck.clone();
    card_names.push("Blank".to_string());
    for task in &request.tasks {
        card_names.extend(task.hand.iter().cloned());
    }
    let mut ctx = FastContext::with_card_names(card_names);
    let config = FastSearchConfig::from_parts(
        request.gamble_mode.as_deref(),
        None,
        request.simplified_gamble,
    );
    request
        .tasks
        .iter()
        .map(|task| evaluate_visible_hand_fast_with_ctx(&mut ctx, request, task, &config))
        .collect()
}

const COMMANDER_MULLIGAN_BOTTOMS_FAST: [usize; 6] = [0, 0, 1, 2, 3, 4];

fn rng_metadata() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("rng_algorithm".to_string(), "ChaCha20Rng".to_string()),
        (
            "seed_derivation".to_string(),
            "BLAKE2b-256 over root seed, domain tag, ordered domain parts, and optional sample index"
                .to_string(),
        ),
        (
            "shuffle".to_string(),
            "rand SliceRandom Fisher-Yates using unbiased bounded sampling".to_string(),
        ),
        (
            "float_draw".to_string(),
            "rand Rng::gen::<f64>() on domain-separated ChaCha20 stream".to_string(),
        ),
        (
            "domains".to_string(),
            "threshold_order, threshold_visible, policy_stage_order, gemstone_caverns_live, policy_visible, policy_actual, selection_shuffle, selection, validation_shuffle, validation"
                .to_string(),
        ),
    ])
}

fn unsupported_policy_response(
    request: &PolicyEvalFastRequest,
    reason: String,
) -> PolicyEvalFastResponse {
    PolicyEvalFastResponse {
        games: request.games,
        successes: 0,
        success_rate: 0.0,
        cap_misses: 0,
        initial_cap_misses_before_actual_rerun: 0,
        actual_cap_rerun_state_limit: request.actual_rerun_state_limit,
        actual_cap_rerun_attempts: 0,
        actual_cap_rerun_successes: 0,
        actual_cap_rerun_remaining_caps: 0,
        upper_success_rate_if_caps_hit: 0.0,
        wilson95: vec![0.0, 0.0],
        turn_counts: BTreeMap::new(),
        keep_counts_by_stage: BTreeMap::new(),
        keep_counts_by_bottom: BTreeMap::new(),
        gemstone_caverns_live_counts: BTreeMap::new(),
        gemstone_caverns_live_successes: BTreeMap::new(),
        success_rate_by_gemstone_caverns_live: BTreeMap::new(),
        visible_ev_cache_size: 0,
        internal_shards: request.internal_shards.max(1),
        internal_shard_workers: request.internal_shard_workers.max(1),
        rng_metadata: rng_metadata(),
        unsupported: true,
        unsupported_reason: Some(reason),
        game_records: if request.include_game_records {
            Some(Vec::new())
        } else {
            None
        },
        cap_replay_records: if request.include_cap_replay_records {
            Some(Vec::new())
        } else {
            None
        },
        validation_records: if request.include_validation_records {
            Some(Vec::new())
        } else {
            None
        },
    }
}

fn increment_count(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_insert(0) += 1;
}

fn wilson_interval(k: usize, n: usize) -> Vec<f64> {
    if n == 0 {
        return vec![0.0, 0.0];
    }
    let z = 1.959963984540054_f64;
    let p = k as f64 / n as f64;
    let den = 1.0 + z * z / n as f64;
    let center = (p + z * z / (2.0 * n as f64)) / den;
    let half = z * (p * (1.0 - p) / n as f64 + z * z / (4.0 * n as f64 * n as f64)).sqrt() / den;
    vec![center - half, center + half]
}

fn policy_shuffle_order(
    deck: &[String],
    seed: u64,
    game_index: usize,
    stage: usize,
) -> Vec<String> {
    let seed_parts = vec![game_index.to_string(), stage.to_string()];
    shuffled_copy(deck, seed, "policy_stage_order", &seed_parts, None)
}

fn policy_gemstone_live(seed: u64, rate: f64, game_index: usize) -> bool {
    if rate <= 0.0 {
        return false;
    }
    if rate >= 1.0 {
        return true;
    }
    let seed_parts = vec![game_index.to_string()];
    let mut rng = chacha_rng(seed, "gemstone_caverns_live", &seed_parts, None);
    rng.gen::<f64>() < rate
}

fn visible_cache_key(
    stage: usize,
    bottom_count: usize,
    gemstone_live: bool,
    hand: &[String],
) -> String {
    let mut key = format!("{stage}|{bottom_count}|{}", usize::from(gemstone_live));
    for card in hand {
        key.push('\x1f');
        key.push_str(card);
    }
    key
}

fn split_even_counts(total: usize, shards: usize) -> Vec<usize> {
    let shards = shards.max(1);
    let base = total / shards;
    let extra = total % shards;
    (0..shards)
        .map(|index| base + usize::from(index < extra))
        .filter(|count| *count > 0)
        .collect()
}

fn python_compat_stable_seed(parts: &[String]) -> u64 {
    let mut hasher = Blake2bVar::new(8).expect("valid digest size");
    hasher.update(parts.join("|").as_bytes());
    let mut digest = [0u8; 8];
    hasher
        .finalize_variable(&mut digest)
        .expect("digest output size");
    u64::from_be_bytes(digest)
}

fn add_count_maps(target: &mut BTreeMap<String, usize>, source: &BTreeMap<String, usize>) {
    for (key, value) in source {
        *target.entry(key.clone()).or_insert(0) += *value;
    }
}

fn merge_policy_eval_fast_shards(
    request: &PolicyEvalFastRequest,
    mut shard_results: Vec<(usize, usize, PolicyEvalFastResponse)>,
    shard_workers: usize,
) -> PolicyEvalFastResponse {
    shard_results.sort_by_key(|(shard_index, _, _)| *shard_index);
    for (shard_index, _, response) in &shard_results {
        if response.unsupported {
            return unsupported_policy_response(
                request,
                format!(
                    "internal policy-eval shard {shard_index} unsupported: {}",
                    response
                        .unsupported_reason
                        .clone()
                        .unwrap_or_else(|| "unknown reason".to_string())
                ),
            );
        }
    }

    let mut games = 0usize;
    let mut successes = 0usize;
    let mut cap_misses = 0usize;
    let mut initial_cap_misses_before_actual_rerun = 0usize;
    let mut actual_cap_rerun_attempts = 0usize;
    let mut actual_cap_rerun_successes = 0usize;
    let mut actual_cap_rerun_remaining_caps = 0usize;
    let mut turn_counts = BTreeMap::new();
    let mut keep_counts_by_stage = BTreeMap::new();
    let mut keep_counts_by_bottom = BTreeMap::new();
    let mut gemstone_caverns_live_counts = BTreeMap::new();
    let mut gemstone_caverns_live_successes = BTreeMap::new();
    let mut visible_ev_cache_size = 0usize;
    let mut game_records = if request.include_game_records {
        Some(Vec::with_capacity(request.games))
    } else {
        None
    };
    let mut cap_replay_records = if request.include_cap_replay_records {
        Some(Vec::new())
    } else {
        None
    };
    let mut validation_records = if request.include_validation_records {
        Some(Vec::with_capacity(request.games))
    } else {
        None
    };

    for (_, game_offset, response) in &mut shard_results {
        games += response.games;
        successes += response.successes;
        cap_misses += response.cap_misses;
        initial_cap_misses_before_actual_rerun += response.initial_cap_misses_before_actual_rerun;
        actual_cap_rerun_attempts += response.actual_cap_rerun_attempts;
        actual_cap_rerun_successes += response.actual_cap_rerun_successes;
        actual_cap_rerun_remaining_caps += response.actual_cap_rerun_remaining_caps;
        add_count_maps(&mut turn_counts, &response.turn_counts);
        add_count_maps(&mut keep_counts_by_stage, &response.keep_counts_by_stage);
        add_count_maps(&mut keep_counts_by_bottom, &response.keep_counts_by_bottom);
        add_count_maps(
            &mut gemstone_caverns_live_counts,
            &response.gemstone_caverns_live_counts,
        );
        add_count_maps(
            &mut gemstone_caverns_live_successes,
            &response.gemstone_caverns_live_successes,
        );
        visible_ev_cache_size += response.visible_ev_cache_size;
        if let (Some(dest), Some(records)) = (game_records.as_mut(), response.game_records.take()) {
            for mut record in records {
                record.game_index += *game_offset;
                dest.push(record);
            }
        }
        if let (Some(dest), Some(records)) = (
            cap_replay_records.as_mut(),
            response.cap_replay_records.take(),
        ) {
            for mut record in records {
                record.game_index += *game_offset;
                dest.push(record);
            }
        }
        if let (Some(dest), Some(records)) = (
            validation_records.as_mut(),
            response.validation_records.take(),
        ) {
            for mut record in records {
                record.game_index += *game_offset;
                dest.push(record);
            }
        }
    }

    if let Some(records) = game_records.as_mut() {
        records.sort_by_key(|record| record.game_index);
    }
    if let Some(records) = cap_replay_records.as_mut() {
        records.sort_by_key(|record| record.game_index);
    }
    if let Some(records) = validation_records.as_mut() {
        records.sort_by_key(|record| record.game_index);
    }
    let success_rate_by_gemstone_caverns_live = gemstone_caverns_live_counts
        .iter()
        .map(|(key, count)| {
            (
                key.clone(),
                if *count == 0 {
                    0.0
                } else {
                    *gemstone_caverns_live_successes.get(key).unwrap_or(&0) as f64 / *count as f64
                },
            )
        })
        .collect();
    let mut metadata = rng_metadata();
    metadata.insert(
        "internal_policy_eval_sharding".to_string(),
        "BLAKE2b-64 over eval seed, rust-full-sim-shard domain, and shard index; each shard starts game_index at zero"
            .to_string(),
    );

    PolicyEvalFastResponse {
        games,
        successes,
        success_rate: if games == 0 {
            0.0
        } else {
            successes as f64 / games as f64
        },
        cap_misses,
        initial_cap_misses_before_actual_rerun,
        actual_cap_rerun_state_limit: request.actual_rerun_state_limit,
        actual_cap_rerun_attempts,
        actual_cap_rerun_successes,
        actual_cap_rerun_remaining_caps,
        internal_shards: shard_results.len().max(1),
        internal_shard_workers: shard_workers.max(1),
        upper_success_rate_if_caps_hit: if games == 0 {
            0.0
        } else {
            (successes + cap_misses) as f64 / games as f64
        },
        wilson95: wilson_interval(successes, games),
        turn_counts,
        keep_counts_by_stage,
        keep_counts_by_bottom,
        gemstone_caverns_live_counts,
        gemstone_caverns_live_successes,
        success_rate_by_gemstone_caverns_live,
        visible_ev_cache_size,
        rng_metadata: metadata,
        unsupported: false,
        unsupported_reason: None,
        game_records,
        cap_replay_records,
        validation_records,
    }
}

fn evaluate_policy_fast_sharded(request: &PolicyEvalFastRequest) -> PolicyEvalFastResponse {
    let shard_counts = split_even_counts(
        request.games,
        request.internal_shards.min(request.games).max(1),
    );
    let shard_count = shard_counts.len();
    let shard_workers = if request.internal_shard_workers == 0 {
        std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1)
    } else {
        request.internal_shard_workers
    }
    .max(1)
    .min(shard_count.max(1));
    let mut specs = Vec::with_capacity(shard_count);
    let mut game_offset = 0usize;
    for (shard_index, shard_games) in shard_counts.into_iter().enumerate() {
        let seed_parts = vec![
            request.seed.to_string(),
            "rust-full-sim-shard".to_string(),
            shard_index.to_string(),
        ];
        let shard_seed = python_compat_stable_seed(&seed_parts);
        specs.push((shard_index, game_offset, shard_games, shard_seed));
        game_offset += shard_games;
    }

    let specs = std::sync::Arc::new(std::sync::Mutex::new(VecDeque::from(specs)));
    let shard_results = std::sync::Arc::new(std::sync::Mutex::new(Vec::with_capacity(shard_count)));
    let mut handles = Vec::with_capacity(shard_workers);
    for _ in 0..shard_workers {
        let specs = std::sync::Arc::clone(&specs);
        let shard_results = std::sync::Arc::clone(&shard_results);
        let request = request.clone();
        handles.push(std::thread::spawn(move || loop {
            let spec = specs.lock().expect("internal shard queue lock").pop_front();
            let Some((shard_index, shard_offset, shard_games, shard_seed)) = spec else {
                break;
            };
            let mut shard_request = request.clone();
            shard_request.games = shard_games;
            shard_request.seed = shard_seed;
            shard_request.internal_shards = 1;
            shard_request.internal_shard_workers = 1;
            let response = evaluate_policy_fast(&shard_request);
            shard_results
                .lock()
                .expect("internal shard result lock")
                .push((shard_index, shard_offset, response));
        }));
    }
    for handle in handles {
        if handle.join().is_err() {
            return unsupported_policy_response(
                request,
                "internal policy-eval shard thread panicked".to_string(),
            );
        }
    }
    let shard_results = std::sync::Arc::try_unwrap(shard_results)
        .expect("internal shard result handles should be joined")
        .into_inner()
        .expect("internal shard results should not be poisoned");
    merge_policy_eval_fast_shards(request, shard_results, shard_workers)
}

pub fn evaluate_policy_fast(request: &PolicyEvalFastRequest) -> PolicyEvalFastResponse {
    if request.internal_shards > 1 && request.games > 0 {
        return evaluate_policy_fast_sharded(request);
    }
    if request.thresholds_dead.len() != COMMANDER_MULLIGAN_BOTTOMS_FAST.len()
        || request.thresholds_live.len() != COMMANDER_MULLIGAN_BOTTOMS_FAST.len()
    {
        return unsupported_policy_response(
            request,
            format!(
                "threshold vectors must both have length {}",
                COMMANDER_MULLIGAN_BOTTOMS_FAST.len()
            ),
        );
    }
    if request.deck.len() < 7 {
        return unsupported_policy_response(
            request,
            "deck must contain at least 7 cards".to_string(),
        );
    }

    let mut card_names = request.deck.clone();
    card_names.push("Blank".to_string());
    let mut ctx = FastContext::with_card_names(card_names);
    let config = FastSearchConfig::from_parts(
        request.gamble_mode.as_deref(),
        None,
        request.simplified_gamble,
    );
    let visible_request = VisibleHandBatchRequest {
        deck: request.deck.clone(),
        tasks: Vec::new(),
        state_limit: request.state_limit,
        samples_per_bottom: request.samples_per_bottom,
        validation_samples: request.validation_samples,
        cap_weight: request.cap_weight,
        max_turns: request.max_turns,
        goal: request.goal.clone(),
        engine_target_count: request.engine_target_count,
        engine_success_policy: request.engine_success_policy.clone(),
        remora_upkeep_payments: request.remora_upkeep_payments,
        action_sort: request.action_sort,
        gamble_mode: request.gamble_mode.clone(),
        simplified_gamble: request.simplified_gamble,
        weighted_policy_ev: request.weighted_policy_ev,
        rhystic_t1_weight: request.rhystic_t1_weight,
        rhystic_t2_weight: request.rhystic_t2_weight,
        heartwood_t1_weight: request.heartwood_t1_weight,
        heartwood_t2_weight: request.heartwood_t2_weight,
    };

    let gemstone_live_by_game: Vec<bool> = (0..request.games)
        .map(|game_index| {
            policy_gemstone_live(request.seed, request.gemstone_caverns_live_rate, game_index)
        })
        .collect();
    let mut active: Vec<usize> = (0..request.games).collect();
    let mut visible_cache: FxHashMap<String, VisibleHandResponse> = FxHashMap::default();
    let mut resolved = FxHashSet::default();
    let mut successes = 0usize;
    let mut cap_misses = 0usize;
    let mut initial_cap_misses_before_actual_rerun = 0usize;
    let mut actual_cap_rerun_attempts = 0usize;
    let mut actual_cap_rerun_successes = 0usize;
    let mut actual_cap_rerun_remaining_caps = 0usize;
    let mut turn_counts = BTreeMap::new();
    let mut keep_counts_by_stage = BTreeMap::new();
    let mut keep_counts_by_bottom = BTreeMap::new();
    let mut gemstone_caverns_live_counts = BTreeMap::new();
    let mut gemstone_caverns_live_successes = BTreeMap::new();
    let mut game_records = if request.include_game_records {
        Some(Vec::with_capacity(request.games))
    } else {
        None
    };
    let mut cap_replay_records = if request.include_cap_replay_records {
        Some(Vec::new())
    } else {
        None
    };
    let mut validation_records = if request.include_validation_records {
        Some(Vec::with_capacity(request.games))
    } else {
        None
    };
    let mut mulligan_decisions_by_game = if request.include_validation_records {
        Some(vec![
            Vec::<PolicyMulliganDecisionRecord>::new();
            request.games
        ])
    } else {
        None
    };
    let use_trace = request.trace_lines && request.include_game_records;

    for gemstone_live in &gemstone_live_by_game {
        increment_count(
            &mut gemstone_caverns_live_counts,
            if *gemstone_live { "live" } else { "dead" },
        );
    }

    for (stage, bottom_count) in COMMANDER_MULLIGAN_BOTTOMS_FAST.iter().copied().enumerate() {
        if active.is_empty() {
            break;
        }
        let mut next_active = Vec::new();
        for game_index in active {
            let gemstone_live = gemstone_live_by_game[game_index];
            let order = policy_shuffle_order(&request.deck, request.seed, game_index, stage);
            let mut hand = order[..7].to_vec();
            hand.sort();
            let cache_key = visible_cache_key(stage, bottom_count, gemstone_live, &hand);
            if !visible_cache.contains_key(&cache_key) {
                let seed_parts = vec![game_index.to_string(), stage.to_string(), cache_key.clone()];
                let task = VisibleHandTaskRequest {
                    key: cache_key.clone(),
                    hand: hand.clone(),
                    bottom_count,
                    seed: rust_stable_seed(request.seed, "policy_visible", &seed_parts, None),
                    gemstone_live,
                    keep_threshold: Some(if gemstone_live {
                        request.thresholds_live[stage]
                    } else {
                        request.thresholds_dead[stage]
                    }),
                    force_keep: stage == COMMANDER_MULLIGAN_BOTTOMS_FAST.len() - 1,
                    adaptive_threshold_sampling: request.adaptive_threshold_sampling,
                };
                let row =
                    evaluate_visible_hand_fast_with_ctx(&mut ctx, &visible_request, &task, &config);
                if row.unsupported {
                    return unsupported_policy_response(
                        request,
                        row.unsupported_reason.unwrap_or_else(|| {
                            "visible-hand evaluator returned unsupported".to_string()
                        }),
                    );
                }
                visible_cache.insert(cache_key.clone(), row);
            }
            let ev_row = visible_cache
                .get(&cache_key)
                .expect("visible cache row should be present");
            let keep_threshold = if gemstone_live {
                request.thresholds_live[stage]
            } else {
                request.thresholds_dead[stage]
            };
            let keep_now = stage == COMMANDER_MULLIGAN_BOTTOMS_FAST.len() - 1
                || ev_row.score_ev >= keep_threshold;
            if let Some(decisions_by_game) = mulligan_decisions_by_game.as_mut() {
                decisions_by_game[game_index].push(PolicyMulliganDecisionRecord {
                    stage,
                    bottom_count,
                    gemstone_caverns_live: gemstone_live,
                    visible_hand: hand.clone(),
                    best_bottom: ev_row.best_bottom.clone(),
                    keep: keep_now,
                    force_keep: stage == COMMANDER_MULLIGAN_BOTTOMS_FAST.len() - 1,
                    score_ev: ev_row.score_ev,
                    upper_ev: ev_row.upper_ev,
                    keep_threshold,
                });
            }
            if !keep_now {
                next_active.push(game_index);
                continue;
            }

            let bottom_set: FxHashSet<&str> =
                ev_row.best_bottom.iter().map(String::as_str).collect();
            let keep: Vec<String> = hand
                .iter()
                .filter(|card| !bottom_set.contains(card.as_str()))
                .cloned()
                .collect();
            let mut library = order[7..].to_vec();
            library.extend(ev_row.best_bottom.iter().cloned());
            let actual_seed_parts =
                vec![game_index.to_string(), stage.to_string(), cache_key.clone()];
            let actual_seed =
                rust_stable_seed(request.seed, "policy_actual", &actual_seed_parts, None);
            let actual_config = FastSearchConfig {
                gamble_mode: config.gamble_mode,
                gamble_seed: actual_seed,
            };
            let mut actual_trace = None;
            let mut actual = if use_trace {
                let traced = solve_keep_trace_fast_with_ctx(
                    &mut ctx,
                    &keep,
                    &library,
                    gemstone_live,
                    request.state_limit,
                    request.max_turns,
                    &request.goal,
                    request.engine_target_count,
                    &request.engine_success_policy,
                    request.remora_upkeep_payments,
                    request.action_sort,
                    &actual_config,
                );
                let response = traced.response.clone();
                actual_trace = Some(traced);
                response
            } else {
                solve_keep_fast_with_ctx(
                    &mut ctx,
                    &keep,
                    &library,
                    gemstone_live,
                    request.state_limit,
                    request.max_turns,
                    &request.goal,
                    request.engine_target_count,
                    &request.engine_success_policy,
                    request.remora_upkeep_payments,
                    request.action_sort,
                    &actual_config,
                )
            };
            if actual.unsupported {
                return unsupported_policy_response(
                    request,
                    actual
                        .unsupported_reason
                        .unwrap_or_else(|| "actual solve returned unsupported".to_string()),
                );
            }
            let initial_hit = actual.turn.is_some_and(|turn| turn <= request.max_turns);
            if !initial_hit && actual.capped {
                initial_cap_misses_before_actual_rerun += 1;
            }
            if !initial_hit
                && actual.capped
                && request.actual_rerun_state_limit > request.state_limit
            {
                actual_cap_rerun_attempts += 1;
                actual = if use_trace {
                    let traced = solve_keep_trace_fast_with_ctx(
                        &mut ctx,
                        &keep,
                        &library,
                        gemstone_live,
                        request.actual_rerun_state_limit,
                        request.max_turns,
                        &request.goal,
                        request.engine_target_count,
                        &request.engine_success_policy,
                        request.remora_upkeep_payments,
                        request.action_sort,
                        &actual_config,
                    );
                    let response = traced.response.clone();
                    actual_trace = Some(traced);
                    response
                } else {
                    solve_keep_fast_with_ctx(
                        &mut ctx,
                        &keep,
                        &library,
                        gemstone_live,
                        request.actual_rerun_state_limit,
                        request.max_turns,
                        &request.goal,
                        request.engine_target_count,
                        &request.engine_success_policy,
                        request.remora_upkeep_payments,
                        request.action_sort,
                        &actual_config,
                    )
                };
                if actual.unsupported {
                    return unsupported_policy_response(
                        request,
                        actual.unsupported_reason.unwrap_or_else(|| {
                            "actual rerun solve returned unsupported".to_string()
                        }),
                    );
                }
            }

            let hit = actual.turn.is_some_and(|turn| turn <= request.max_turns);
            if hit {
                successes += 1;
                increment_count(
                    &mut gemstone_caverns_live_successes,
                    if gemstone_live { "live" } else { "dead" },
                );
                if !initial_hit {
                    actual_cap_rerun_successes += 1;
                }
                increment_count(
                    &mut turn_counts,
                    actual
                        .turn
                        .map_or_else(|| "miss".to_string(), |turn| turn.to_string()),
                );
            } else {
                increment_count(&mut turn_counts, "miss");
                if actual.capped {
                    cap_misses += 1;
                    if request.actual_rerun_state_limit > request.state_limit {
                        actual_cap_rerun_remaining_caps += 1;
                    }
                    if let Some(records) = cap_replay_records.as_mut() {
                        records.push(PolicyCapReplayRecord {
                            game_index,
                            stage,
                            bottom_count,
                            gemstone_caverns_live: gemstone_live,
                            visible_hand: hand.clone(),
                            bottomed: ev_row.best_bottom.clone(),
                            keep: keep.clone(),
                            library: library.clone(),
                            gamble_seed: actual_seed,
                            state_limit: request.state_limit,
                            actual_cap_rerun_state_limit: request.actual_rerun_state_limit,
                            max_turns: request.max_turns,
                            goal: request.goal.clone(),
                            engine_target_count: request.engine_target_count,
                            engine_success_policy: request.engine_success_policy.clone(),
                            remora_upkeep_payments: request.remora_upkeep_payments,
                            action_sort: request.action_sort,
                            gamble_mode: request.gamble_mode.clone(),
                            simplified_gamble: request.simplified_gamble,
                        });
                    }
                }
            }
            if let Some(records) = validation_records.as_mut() {
                records.push(PolicyValidationRecord {
                    game_index,
                    stage,
                    bottom_count,
                    gemstone_caverns_live: gemstone_live,
                    visible_hand: hand.clone(),
                    bottomed: ev_row.best_bottom.clone(),
                    keep: keep.clone(),
                    library: library.clone(),
                    mulligan_decisions: mulligan_decisions_by_game
                        .as_ref()
                        .map(|decisions| decisions[game_index].clone())
                        .unwrap_or_default(),
                    gamble_seed: actual_seed,
                    state_limit: request.state_limit,
                    actual_cap_rerun_state_limit: request.actual_rerun_state_limit,
                    max_turns: request.max_turns,
                    goal: request.goal.clone(),
                    engine_target_count: request.engine_target_count,
                    engine_success_policy: request.engine_success_policy.clone(),
                    remora_upkeep_payments: request.remora_upkeep_payments,
                    action_sort: request.action_sort,
                    gamble_mode: request.gamble_mode.clone(),
                    simplified_gamble: request.simplified_gamble,
                    hit,
                    capped: actual.capped,
                    turn: actual.turn,
                    engine_label: if hit { actual.label.clone() } else { None },
                });
            }
            increment_count(&mut keep_counts_by_stage, stage.to_string());
            increment_count(&mut keep_counts_by_bottom, bottom_count.to_string());
            resolved.insert(game_index);
            if let Some(records) = game_records.as_mut() {
                let trace_path = actual_trace
                    .as_ref()
                    .filter(|_| hit)
                    .map(|trace| trace.trace_path.clone());
                let line_cards = trace_path
                    .as_ref()
                    .map(|actions| trace_line_cards_from_actions(&request.deck, actions));
                let line_casts_nick_fury = trace_path.as_ref().map(|actions| {
                    actions
                        .iter()
                        .any(|action| action.contains("Nick Fury, Agent of S.H.I.E.L.D."))
                });
                let trace_turn = actual_trace.as_ref().and_then(|trace| trace.trace_turn);
                let trace_capped = actual_trace
                    .as_ref()
                    .is_some_and(|trace| trace.trace_capped);
                let trace_found = if use_trace {
                    Some(
                        hit && trace_path
                            .as_ref()
                            .is_some_and(|actions| !actions.is_empty() || trace_turn.is_some()),
                    )
                } else {
                    None
                };
                records.push(PolicyGameRecord {
                    game_index,
                    stage,
                    bottom_count,
                    gemstone_caverns_live: gemstone_live,
                    hit,
                    capped: actual.capped,
                    turn: actual.turn,
                    engine_label: if hit { actual.label.clone() } else { None },
                    trace_turn,
                    trace_capped,
                    trace_found,
                    line_action_count: trace_path.as_ref().map(Vec::len),
                    line_actions: trace_path,
                    line_cards,
                    line_casts_nick_fury,
                });
            }
        }
        active = next_active;
    }

    if resolved.len() != request.games {
        return unsupported_policy_response(
            request,
            format!(
                "only resolved {} of {} games",
                resolved.len(),
                request.games
            ),
        );
    }

    let success_rate_by_gemstone_caverns_live = gemstone_caverns_live_counts
        .iter()
        .map(|(key, count)| {
            (
                key.clone(),
                if *count == 0 {
                    0.0
                } else {
                    *gemstone_caverns_live_successes.get(key).unwrap_or(&0) as f64 / *count as f64
                },
            )
        })
        .collect();

    PolicyEvalFastResponse {
        games: request.games,
        successes,
        success_rate: if request.games == 0 {
            0.0
        } else {
            successes as f64 / request.games as f64
        },
        cap_misses,
        initial_cap_misses_before_actual_rerun,
        actual_cap_rerun_state_limit: request.actual_rerun_state_limit,
        actual_cap_rerun_attempts,
        actual_cap_rerun_successes,
        actual_cap_rerun_remaining_caps,
        upper_success_rate_if_caps_hit: if request.games == 0 {
            0.0
        } else {
            (successes + cap_misses) as f64 / request.games as f64
        },
        wilson95: wilson_interval(successes, request.games),
        turn_counts,
        keep_counts_by_stage,
        keep_counts_by_bottom,
        gemstone_caverns_live_counts,
        gemstone_caverns_live_successes,
        success_rate_by_gemstone_caverns_live,
        visible_ev_cache_size: visible_cache.len(),
        internal_shards: request.internal_shards.max(1),
        internal_shard_workers: request.internal_shard_workers.max(1),
        rng_metadata: rng_metadata(),
        unsupported: false,
        unsupported_reason: None,
        game_records,
        cap_replay_records,
        validation_records,
    }
}

fn unsupported_threshold_sweep_response(reason: String) -> PolicyThresholdSweepFastResponse {
    PolicyThresholdSweepFastResponse {
        variants: Vec::new(),
        rng_metadata: rng_metadata(),
        unsupported: true,
        unsupported_reason: Some(reason),
    }
}

struct PolicyThresholdSweepRuntime {
    spec: PolicyThresholdVariantRequest,
    active: Vec<usize>,
    successes: usize,
    cap_misses: usize,
    initial_cap_misses_before_actual_rerun: usize,
    actual_cap_rerun_attempts: usize,
    actual_cap_rerun_successes: usize,
    actual_cap_rerun_remaining_caps: usize,
    turn_counts: BTreeMap<String, usize>,
    keep_counts_by_stage: BTreeMap<String, usize>,
    keep_counts_by_bottom: BTreeMap<String, usize>,
    gemstone_caverns_live_successes: BTreeMap<String, usize>,
}

#[derive(Clone)]
struct PolicyThresholdSweepActualOutcome {
    response: SolveKeepResponse,
    initial_hit: bool,
    initial_capped: bool,
}

impl PolicyThresholdSweepRuntime {
    fn new(spec: PolicyThresholdVariantRequest, games: usize) -> Self {
        Self {
            spec,
            active: (0..games).collect(),
            successes: 0,
            cap_misses: 0,
            initial_cap_misses_before_actual_rerun: 0,
            actual_cap_rerun_attempts: 0,
            actual_cap_rerun_successes: 0,
            actual_cap_rerun_remaining_caps: 0,
            turn_counts: BTreeMap::new(),
            keep_counts_by_stage: BTreeMap::new(),
            keep_counts_by_bottom: BTreeMap::new(),
            gemstone_caverns_live_successes: BTreeMap::new(),
        }
    }

    fn evaluation(
        self,
        request: &PolicyThresholdSweepFastRequest,
        gemstone_caverns_live_counts: &BTreeMap<String, usize>,
        visible_ev_cache_size: usize,
    ) -> PolicyThresholdSweepVariantResponse {
        let success_rate_by_gemstone_caverns_live = gemstone_caverns_live_counts
            .iter()
            .map(|(key, count)| {
                (
                    key.clone(),
                    if *count == 0 {
                        0.0
                    } else {
                        *self.gemstone_caverns_live_successes.get(key).unwrap_or(&0) as f64
                            / *count as f64
                    },
                )
            })
            .collect();
        let evaluation = PolicyEvalFastResponse {
            games: request.games,
            successes: self.successes,
            success_rate: if request.games == 0 {
                0.0
            } else {
                self.successes as f64 / request.games as f64
            },
            cap_misses: self.cap_misses,
            initial_cap_misses_before_actual_rerun: self.initial_cap_misses_before_actual_rerun,
            actual_cap_rerun_state_limit: request.actual_rerun_state_limit,
            actual_cap_rerun_attempts: self.actual_cap_rerun_attempts,
            actual_cap_rerun_successes: self.actual_cap_rerun_successes,
            actual_cap_rerun_remaining_caps: self.actual_cap_rerun_remaining_caps,
            internal_shards: 1,
            internal_shard_workers: 1,
            upper_success_rate_if_caps_hit: if request.games == 0 {
                0.0
            } else {
                (self.successes + self.cap_misses) as f64 / request.games as f64
            },
            wilson95: wilson_interval(self.successes, request.games),
            turn_counts: self.turn_counts,
            keep_counts_by_stage: self.keep_counts_by_stage,
            keep_counts_by_bottom: self.keep_counts_by_bottom,
            gemstone_caverns_live_counts: gemstone_caverns_live_counts.clone(),
            gemstone_caverns_live_successes: self.gemstone_caverns_live_successes,
            success_rate_by_gemstone_caverns_live,
            visible_ev_cache_size,
            rng_metadata: rng_metadata(),
            unsupported: false,
            unsupported_reason: None,
            game_records: None,
            cap_replay_records: None,
            validation_records: None,
        };
        PolicyThresholdSweepVariantResponse {
            name: self.spec.name,
            thresholds_dead: self.spec.thresholds_dead,
            thresholds_live: self.spec.thresholds_live,
            evaluation,
        }
    }
}

pub fn evaluate_policy_threshold_sweep_fast(
    request: &PolicyThresholdSweepFastRequest,
) -> PolicyThresholdSweepFastResponse {
    if request.variants.is_empty() {
        return unsupported_threshold_sweep_response("variants must not be empty".to_string());
    }
    if request.deck.len() < 7 {
        return unsupported_threshold_sweep_response(
            "deck must contain at least 7 cards".to_string(),
        );
    }
    if request.samples_per_bottom == 0 || request.validation_samples == 0 {
        return unsupported_threshold_sweep_response(
            "samples_per_bottom and validation_samples must be positive".to_string(),
        );
    }
    for variant in &request.variants {
        if variant.thresholds_dead.len() != COMMANDER_MULLIGAN_BOTTOMS_FAST.len()
            || variant.thresholds_live.len() != COMMANDER_MULLIGAN_BOTTOMS_FAST.len()
        {
            return unsupported_threshold_sweep_response(format!(
                "variant {} threshold vectors must both have length {}",
                variant.name,
                COMMANDER_MULLIGAN_BOTTOMS_FAST.len()
            ));
        }
    }

    let mut card_names = request.deck.clone();
    card_names.push("Blank".to_string());
    let mut ctx = FastContext::with_card_names(card_names);
    let config = FastSearchConfig::from_parts(
        request.gamble_mode.as_deref(),
        None,
        request.simplified_gamble,
    );
    let visible_request = VisibleHandBatchRequest {
        deck: request.deck.clone(),
        tasks: Vec::new(),
        state_limit: request.state_limit,
        samples_per_bottom: request.samples_per_bottom,
        validation_samples: request.validation_samples,
        cap_weight: request.cap_weight,
        max_turns: request.max_turns,
        goal: request.goal.clone(),
        engine_target_count: request.engine_target_count,
        engine_success_policy: request.engine_success_policy.clone(),
        remora_upkeep_payments: request.remora_upkeep_payments,
        action_sort: request.action_sort,
        gamble_mode: request.gamble_mode.clone(),
        simplified_gamble: request.simplified_gamble,
        weighted_policy_ev: request.weighted_policy_ev,
        rhystic_t1_weight: request.rhystic_t1_weight,
        rhystic_t2_weight: request.rhystic_t2_weight,
        heartwood_t1_weight: request.heartwood_t1_weight,
        heartwood_t2_weight: request.heartwood_t2_weight,
    };

    let gemstone_live_by_game: Vec<bool> = (0..request.games)
        .map(|game_index| {
            policy_gemstone_live(request.seed, request.gemstone_caverns_live_rate, game_index)
        })
        .collect();
    let mut gemstone_caverns_live_counts = BTreeMap::new();
    for gemstone_live in &gemstone_live_by_game {
        increment_count(
            &mut gemstone_caverns_live_counts,
            if *gemstone_live { "live" } else { "dead" },
        );
    }

    let mut visible_cache: FxHashMap<String, VisibleHandResponse> = FxHashMap::default();
    let mut actual_cache: FxHashMap<String, PolicyThresholdSweepActualOutcome> =
        FxHashMap::default();
    let mut variants: Vec<PolicyThresholdSweepRuntime> = request
        .variants
        .iter()
        .cloned()
        .map(|variant| PolicyThresholdSweepRuntime::new(variant, request.games))
        .collect();

    for (stage, bottom_count) in COMMANDER_MULLIGAN_BOTTOMS_FAST.iter().copied().enumerate() {
        let force_keep = stage == COMMANDER_MULLIGAN_BOTTOMS_FAST.len() - 1;
        for variant in &mut variants {
            if variant.active.is_empty() {
                continue;
            }
            let mut next_active = Vec::new();
            for game_index in variant.active.drain(..) {
                let gemstone_live = gemstone_live_by_game[game_index];
                let order = policy_shuffle_order(&request.deck, request.seed, game_index, stage);
                let mut hand = order[..7].to_vec();
                hand.sort();
                let cache_key = visible_cache_key(stage, bottom_count, gemstone_live, &hand);
                if !visible_cache.contains_key(&cache_key) {
                    let seed_parts =
                        vec![game_index.to_string(), stage.to_string(), cache_key.clone()];
                    let task = VisibleHandTaskRequest {
                        key: cache_key.clone(),
                        hand: hand.clone(),
                        bottom_count,
                        seed: rust_stable_seed(request.seed, "policy_visible", &seed_parts, None),
                        gemstone_live,
                        keep_threshold: None,
                        force_keep,
                        adaptive_threshold_sampling: false,
                    };
                    let row = evaluate_visible_hand_fast_with_ctx(
                        &mut ctx,
                        &visible_request,
                        &task,
                        &config,
                    );
                    if row.unsupported {
                        return unsupported_threshold_sweep_response(
                            row.unsupported_reason.unwrap_or_else(|| {
                                "visible-hand evaluator returned unsupported".to_string()
                            }),
                        );
                    }
                    visible_cache.insert(cache_key.clone(), row);
                }
                let ev_row = visible_cache
                    .get(&cache_key)
                    .expect("visible cache row should be present");
                let keep_threshold = if gemstone_live {
                    variant.spec.thresholds_live[stage]
                } else {
                    variant.spec.thresholds_dead[stage]
                };
                let keep_now = force_keep || ev_row.score_ev >= keep_threshold;
                if !keep_now {
                    next_active.push(game_index);
                    continue;
                }

                let bottom_set: FxHashSet<&str> =
                    ev_row.best_bottom.iter().map(String::as_str).collect();
                let keep: Vec<String> = hand
                    .iter()
                    .filter(|card| !bottom_set.contains(card.as_str()))
                    .cloned()
                    .collect();
                let mut library = order[7..].to_vec();
                library.extend(ev_row.best_bottom.iter().cloned());
                let actual_cache_key = format!("{game_index}|{cache_key}");
                if !actual_cache.contains_key(&actual_cache_key) {
                    let actual_seed_parts =
                        vec![game_index.to_string(), stage.to_string(), cache_key.clone()];
                    let actual_seed =
                        rust_stable_seed(request.seed, "policy_actual", &actual_seed_parts, None);
                    let actual_config = FastSearchConfig {
                        gamble_mode: config.gamble_mode,
                        gamble_seed: actual_seed,
                    };
                    let mut actual = solve_keep_fast_with_ctx(
                        &mut ctx,
                        &keep,
                        &library,
                        gemstone_live,
                        request.state_limit,
                        request.max_turns,
                        &request.goal,
                        request.engine_target_count,
                        &request.engine_success_policy,
                        request.remora_upkeep_payments,
                        request.action_sort,
                        &actual_config,
                    );
                    if actual.unsupported {
                        return unsupported_threshold_sweep_response(
                            actual
                                .unsupported_reason
                                .unwrap_or_else(|| "actual solve returned unsupported".to_string()),
                        );
                    }
                    let initial_hit = actual.turn.is_some_and(|turn| turn <= request.max_turns);
                    let initial_capped = actual.capped;
                    if !initial_hit
                        && actual.capped
                        && request.actual_rerun_state_limit > request.state_limit
                    {
                        actual = solve_keep_fast_with_ctx(
                            &mut ctx,
                            &keep,
                            &library,
                            gemstone_live,
                            request.actual_rerun_state_limit,
                            request.max_turns,
                            &request.goal,
                            request.engine_target_count,
                            &request.engine_success_policy,
                            request.remora_upkeep_payments,
                            request.action_sort,
                            &actual_config,
                        );
                        if actual.unsupported {
                            return unsupported_threshold_sweep_response(
                                actual.unsupported_reason.unwrap_or_else(|| {
                                    "actual rerun solve returned unsupported".to_string()
                                }),
                            );
                        }
                    }
                    actual_cache.insert(
                        actual_cache_key.clone(),
                        PolicyThresholdSweepActualOutcome {
                            response: actual,
                            initial_hit,
                            initial_capped,
                        },
                    );
                }
                let actual_outcome = actual_cache
                    .get(&actual_cache_key)
                    .expect("actual cache row should be present");
                let actual = &actual_outcome.response;
                if !actual_outcome.initial_hit && actual_outcome.initial_capped {
                    variant.initial_cap_misses_before_actual_rerun += 1;
                }
                if !actual_outcome.initial_hit
                    && actual_outcome.initial_capped
                    && request.actual_rerun_state_limit > request.state_limit
                {
                    variant.actual_cap_rerun_attempts += 1;
                }

                let hit = actual.turn.is_some_and(|turn| turn <= request.max_turns);
                if hit {
                    variant.successes += 1;
                    increment_count(
                        &mut variant.gemstone_caverns_live_successes,
                        if gemstone_live { "live" } else { "dead" },
                    );
                    if !actual_outcome.initial_hit {
                        variant.actual_cap_rerun_successes += 1;
                    }
                    increment_count(
                        &mut variant.turn_counts,
                        actual
                            .turn
                            .map_or_else(|| "miss".to_string(), |turn| turn.to_string()),
                    );
                } else {
                    increment_count(&mut variant.turn_counts, "miss");
                    if actual.capped {
                        variant.cap_misses += 1;
                        if request.actual_rerun_state_limit > request.state_limit {
                            variant.actual_cap_rerun_remaining_caps += 1;
                        }
                    }
                }
                increment_count(&mut variant.keep_counts_by_stage, stage.to_string());
                increment_count(&mut variant.keep_counts_by_bottom, bottom_count.to_string());
            }
            variant.active = next_active;
        }
    }

    if variants.iter().any(|variant| !variant.active.is_empty()) {
        return unsupported_threshold_sweep_response(
            "at least one variant did not force-keep by final stage".to_string(),
        );
    }
    let visible_ev_cache_size = visible_cache.len();
    let responses = variants
        .into_iter()
        .map(|variant| {
            variant.evaluation(
                request,
                &gemstone_caverns_live_counts,
                visible_ev_cache_size,
            )
        })
        .collect();
    PolicyThresholdSweepFastResponse {
        variants: responses,
        rng_metadata: rng_metadata(),
        unsupported: false,
        unsupported_reason: None,
    }
}

fn compute_thresholds_from_values(
    stage_values_input: &[Vec<f64>],
) -> (Vec<f64>, Vec<PolicyThresholdRow>) {
    let mut thresholds = vec![0.0; COMMANDER_MULLIGAN_BOTTOMS_FAST.len()];
    let mut rows_reversed = Vec::with_capacity(COMMANDER_MULLIGAN_BOTTOMS_FAST.len());
    let mut future_value = 0.0;
    for stage in (0..COMMANDER_MULLIGAN_BOTTOMS_FAST.len()).rev() {
        let values = &stage_values_input[stage];
        let raw_mean = if values.is_empty() {
            0.0
        } else {
            values.iter().copied().sum::<f64>() / values.len() as f64
        };
        thresholds[stage] = future_value;
        let stage_value = if values.is_empty() {
            future_value
        } else if stage == COMMANDER_MULLIGAN_BOTTOMS_FAST.len() - 1 {
            raw_mean
        } else {
            values
                .iter()
                .map(|value| value.max(future_value))
                .sum::<f64>()
                / values.len() as f64
        };
        rows_reversed.push(PolicyThresholdRow {
            stage,
            bottom_count: COMMANDER_MULLIGAN_BOTTOMS_FAST[stage],
            future_keep_threshold: future_value,
            raw_mean_visible_ev: raw_mean,
            stage_value,
            hands: values.len(),
        });
        future_value = stage_value;
    }
    rows_reversed.reverse();
    (thresholds, rows_reversed)
}

fn unsupported_sim_response(
    request: &PolicySimFastRequest,
    reason: String,
) -> PolicySimFastResponse {
    PolicySimFastResponse {
        thresholds_dead: Vec::new(),
        thresholds_live: Vec::new(),
        threshold_rows_dead: Vec::new(),
        threshold_rows_live: Vec::new(),
        evaluation: unsupported_policy_response(
            &PolicyEvalFastRequest {
                deck: request.deck.clone(),
                thresholds_dead: vec![0.0; COMMANDER_MULLIGAN_BOTTOMS_FAST.len()],
                thresholds_live: vec![0.0; COMMANDER_MULLIGAN_BOTTOMS_FAST.len()],
                games: request.eval_games,
                seed: request.eval_seed.unwrap_or(request.seed),
                gemstone_caverns_live_rate: request.gemstone_caverns_live_rate,
                state_limit: request.state_limit,
                actual_rerun_state_limit: request.actual_rerun_state_limit,
                samples_per_bottom: request.samples_per_bottom,
                validation_samples: request.validation_samples,
                cap_weight: request.cap_weight,
                max_turns: request.max_turns,
                goal: request.goal.clone(),
                engine_target_count: request.engine_target_count,
                engine_success_policy: request.engine_success_policy.clone(),
                remora_upkeep_payments: request.remora_upkeep_payments,
                action_sort: request.action_sort,
                adaptive_threshold_sampling: request.adaptive_threshold_sampling,
                include_game_records: request.include_game_records,
                include_cap_replay_records: request.include_cap_replay_records,
                include_validation_records: request.include_validation_records,
                trace_lines: request.trace_lines,
                gamble_mode: request.gamble_mode.clone(),
                simplified_gamble: request.simplified_gamble,
                internal_shards: request.internal_shards,
                internal_shard_workers: request.internal_shard_workers,
                weighted_policy_ev: request.weighted_policy_ev,
                rhystic_t1_weight: request.rhystic_t1_weight,
                rhystic_t2_weight: request.rhystic_t2_weight,
                heartwood_t1_weight: request.heartwood_t1_weight,
                heartwood_t2_weight: request.heartwood_t2_weight,
            },
            reason.clone(),
        ),
        rng_metadata: rng_metadata(),
        unsupported: true,
        unsupported_reason: Some(reason),
    }
}

pub fn simulate_policy_fast(request: &PolicySimFastRequest) -> PolicySimFastResponse {
    if request.threshold_hands == 0 {
        return unsupported_sim_response(request, "threshold_hands must be positive".to_string());
    }
    if request.deck.len() < 7 {
        return unsupported_sim_response(request, "deck must contain at least 7 cards".to_string());
    }

    let mut card_names = request.deck.clone();
    card_names.push("Blank".to_string());
    let mut ctx = FastContext::with_card_names(card_names);
    let config = FastSearchConfig::from_parts(
        request.gamble_mode.as_deref(),
        None,
        request.simplified_gamble,
    );
    let visible_request = VisibleHandBatchRequest {
        deck: request.deck.clone(),
        tasks: Vec::new(),
        state_limit: request.state_limit,
        samples_per_bottom: request.samples_per_bottom,
        validation_samples: request.validation_samples,
        cap_weight: request.cap_weight,
        max_turns: request.max_turns,
        goal: request.goal.clone(),
        engine_target_count: request.engine_target_count,
        engine_success_policy: request.engine_success_policy.clone(),
        remora_upkeep_payments: request.remora_upkeep_payments,
        action_sort: request.action_sort,
        gamble_mode: request.gamble_mode.clone(),
        simplified_gamble: request.simplified_gamble,
        weighted_policy_ev: request.weighted_policy_ev,
        rhystic_t1_weight: request.rhystic_t1_weight,
        rhystic_t2_weight: request.rhystic_t2_weight,
        heartwood_t1_weight: request.heartwood_t1_weight,
        heartwood_t2_weight: request.heartwood_t2_weight,
    };

    let mut visible_cache: FxHashMap<String, VisibleHandResponse> = FxHashMap::default();
    let mut dead_values = vec![Vec::new(); COMMANDER_MULLIGAN_BOTTOMS_FAST.len()];
    let mut live_values = vec![Vec::new(); COMMANDER_MULLIGAN_BOTTOMS_FAST.len()];

    for (stage, bottom_count) in COMMANDER_MULLIGAN_BOTTOMS_FAST.iter().copied().enumerate() {
        for hand_index in 0..request.threshold_hands {
            let seed_parts = vec![stage.to_string(), hand_index.to_string()];
            let mut order = shuffled_copy(
                &request.deck,
                request.seed,
                "threshold_order",
                &seed_parts,
                None,
            );
            let mut hand = order.drain(..7).collect::<Vec<_>>();
            hand.sort();
            for gemstone_live in [false, true] {
                let cache_key = visible_cache_key(stage, bottom_count, gemstone_live, &hand);
                if !visible_cache.contains_key(&cache_key) {
                    let task_seed_parts =
                        vec![stage.to_string(), hand_index.to_string(), cache_key.clone()];
                    let task = VisibleHandTaskRequest {
                        key: cache_key.clone(),
                        hand: hand.clone(),
                        bottom_count,
                        seed: rust_stable_seed(
                            request.seed,
                            "threshold_visible",
                            &task_seed_parts,
                            None,
                        ),
                        gemstone_live,
                        keep_threshold: None,
                        force_keep: false,
                        adaptive_threshold_sampling: false,
                    };
                    let row = evaluate_visible_hand_fast_with_ctx(
                        &mut ctx,
                        &visible_request,
                        &task,
                        &config,
                    );
                    if row.unsupported {
                        return unsupported_sim_response(
                            request,
                            row.unsupported_reason.unwrap_or_else(|| {
                                "threshold visible-hand evaluator returned unsupported".to_string()
                            }),
                        );
                    }
                    visible_cache.insert(cache_key.clone(), row);
                }
                let score = visible_cache
                    .get(&cache_key)
                    .expect("threshold visible cache row should exist")
                    .score_ev;
                if gemstone_live {
                    live_values[stage].push(score);
                } else {
                    dead_values[stage].push(score);
                }
            }
        }
    }

    let (thresholds_dead, threshold_rows_dead) = compute_thresholds_from_values(&dead_values);
    let (thresholds_live, threshold_rows_live) = compute_thresholds_from_values(&live_values);
    let evaluation_request = PolicyEvalFastRequest {
        deck: request.deck.clone(),
        thresholds_dead: thresholds_dead.clone(),
        thresholds_live: thresholds_live.clone(),
        games: request.eval_games,
        seed: request.eval_seed.unwrap_or(request.seed),
        gemstone_caverns_live_rate: request.gemstone_caverns_live_rate,
        state_limit: request.state_limit,
        actual_rerun_state_limit: request.actual_rerun_state_limit,
        samples_per_bottom: request.samples_per_bottom,
        validation_samples: request.validation_samples,
        cap_weight: request.cap_weight,
        max_turns: request.max_turns,
        goal: request.goal.clone(),
        engine_target_count: request.engine_target_count,
        engine_success_policy: request.engine_success_policy.clone(),
        remora_upkeep_payments: request.remora_upkeep_payments,
        action_sort: request.action_sort,
        adaptive_threshold_sampling: request.adaptive_threshold_sampling,
        include_game_records: request.include_game_records,
        include_cap_replay_records: request.include_cap_replay_records,
        include_validation_records: request.include_validation_records,
        trace_lines: request.trace_lines,
        gamble_mode: request.gamble_mode.clone(),
        simplified_gamble: request.simplified_gamble,
        internal_shards: request.internal_shards,
        internal_shard_workers: request.internal_shard_workers,
        weighted_policy_ev: request.weighted_policy_ev,
        rhystic_t1_weight: request.rhystic_t1_weight,
        rhystic_t2_weight: request.rhystic_t2_weight,
        heartwood_t1_weight: request.heartwood_t1_weight,
        heartwood_t2_weight: request.heartwood_t2_weight,
    };
    let evaluation = evaluate_policy_fast(&evaluation_request);
    let unsupported = evaluation.unsupported;
    let unsupported_reason = evaluation.unsupported_reason.clone();
    PolicySimFastResponse {
        thresholds_dead,
        thresholds_live,
        threshold_rows_dead,
        threshold_rows_live,
        evaluation,
        rng_metadata: rng_metadata(),
        unsupported,
        unsupported_reason,
    }
}

#[derive(Debug, Clone)]
struct RawDeltaBaselineSample {
    stage: usize,
    bottom_count: usize,
    sample_index: usize,
    gemstone_live: bool,
    indices: Vec<usize>,
    score: f64,
    hit: bool,
}

#[derive(Debug, Clone)]
struct RawDeltaSolveResult {
    score: f64,
    hit: bool,
    solver_calls: usize,
    unsupported_reason: Option<String>,
}

#[derive(Debug, Default)]
struct RawDeltaMoments {
    sum: f64,
    sum_sq: f64,
    n: usize,
}

impl RawDeltaMoments {
    fn push(&mut self, value: f64) {
        self.sum += value;
        self.sum_sq += value * value;
        self.n += 1;
    }

    fn mean(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.sum / self.n as f64
        }
    }

    fn standard_error(&self) -> f64 {
        if self.n < 2 {
            return 0.0;
        }
        let n = self.n as f64;
        let variance = ((self.sum_sq - (self.sum * self.sum / n)) / (n - 1.0)).max(0.0);
        (variance / n).sqrt()
    }
}

fn unsupported_raw_delta_response(
    request: &RawDeltaFastRequest,
    reason: String,
) -> RawDeltaFastResponse {
    RawDeltaFastResponse {
        stages: Vec::new(),
        bottom_counts: Vec::new(),
        samples_per_stage: request.samples_per_stage,
        total_samples: 0,
        draw_window: request.draw_window,
        relevance_mode: request.relevance_mode.clone(),
        baseline_solver_calls: 0,
        baseline_elapsed_ms: 0.0,
        variants: Vec::new(),
        rng_metadata: rng_metadata(),
        unsupported: true,
        unsupported_reason: Some(reason),
    }
}

fn raw_delta_variant_unsupported(
    swap: &RawDeltaSwapRequest,
    slot_index: Option<usize>,
    total_samples: usize,
    baseline_score_mean: f64,
    baseline_hits: usize,
    reason: String,
) -> RawDeltaVariantResponse {
    RawDeltaVariantResponse {
        name: swap.name.clone(),
        cut: swap.cut.clone(),
        add: swap.add.clone(),
        slot_index,
        samples: total_samples,
        relevant_samples: 0,
        irrelevant_samples: total_samples,
        relevance_rate: 0.0,
        baseline_score_mean,
        candidate_score_mean_delta: baseline_score_mean,
        candidate_score_mean_naive: None,
        mean_delta_delta_method: 0.0,
        mean_delta_naive: None,
        delta_method_se: 0.0,
        naive_delta_se: None,
        delta_abs_error_vs_naive: None,
        baseline_hits,
        candidate_hits_delta_method: baseline_hits,
        candidate_hits_naive: None,
        positive_delta_samples: 0,
        negative_delta_samples: 0,
        zero_delta_samples: total_samples,
        candidate_delta_solver_calls: 0,
        candidate_naive_solver_calls: None,
        delta_elapsed_ms: 0.0,
        naive_elapsed_ms: None,
        speedup_vs_naive: None,
        unsupported: true,
        unsupported_reason: Some(reason),
    }
}

fn raw_delta_score(request: &RawDeltaFastRequest, response: &SolveKeepResponse) -> f64 {
    if let Some(turn) = response.turn.filter(|turn| *turn <= request.max_turns) {
        match (response.label.as_deref(), turn) {
            (Some("Rhystic Study"), 1) => request.rhystic_t1_weight,
            (Some("Rhystic Study"), 2) => request.rhystic_t2_weight,
            (Some("Heartwood Storyteller"), 1) => request.heartwood_t1_weight,
            (Some("Heartwood Storyteller"), 2) => request.heartwood_t2_weight,
            _ => 0.0,
        }
    } else if response.capped {
        request.cap_weight
    } else {
        0.0
    }
}

fn raw_delta_max_score(request: &RawDeltaFastRequest) -> f64 {
    request
        .cap_weight
        .max(request.rhystic_t1_weight)
        .max(request.rhystic_t2_weight)
        .max(request.heartwood_t1_weight)
        .max(request.heartwood_t2_weight)
}

fn shuffled_indices(
    len: usize,
    seed: u64,
    tag: &str,
    parts: &[String],
    sample_index: Option<usize>,
) -> Vec<usize> {
    let mut out: Vec<usize> = (0..len).collect();
    out.shuffle(&mut chacha_rng(seed, tag, parts, sample_index));
    out
}

fn raw_order_from_indices(
    deck: &[String],
    indices: &[usize],
    replacement: Option<(usize, &str)>,
) -> Vec<String> {
    match replacement {
        Some((slot_index, add)) => {
            raw_order_from_indices_multi(deck, indices, &[(slot_index, add)])
        }
        None => raw_order_from_indices_multi(deck, indices, &[]),
    }
}

fn raw_order_from_indices_multi(
    deck: &[String],
    indices: &[usize],
    replacements: &[(usize, &str)],
) -> Vec<String> {
    indices
        .iter()
        .map(|index| {
            if let Some((_, add)) = replacements
                .iter()
                .find(|(slot_index, _)| *slot_index == *index)
            {
                (*add).to_string()
            } else {
                deck[*index].clone()
            }
        })
        .collect()
}

fn raw_best_score_with_ctx(
    ctx: &mut FastContext,
    request: &RawDeltaFastRequest,
    order: &[String],
    bottom_count: usize,
    gemstone_live: bool,
    config: &FastSearchConfig,
    sample_key: &str,
    choices: &[crate::BottomChoice],
) -> RawDeltaSolveResult {
    if order.len() < 7 {
        return RawDeltaSolveResult {
            score: 0.0,
            hit: false,
            solver_calls: 0,
            unsupported_reason: Some("deck order must contain at least 7 cards".to_string()),
        };
    }

    let hand7 = &order[..7];
    let rest = &order[7..];
    let mut best_score = f64::NEG_INFINITY;
    let mut best_hit = false;
    let mut best_turn: Option<u8> = None;
    let mut solver_calls = 0usize;
    let max_score = raw_delta_max_score(request);

    for (choice_index, choice) in choices.iter().enumerate() {
        let hand: Vec<String> = choice
            .kept_indices
            .iter()
            .map(|index| hand7[*index].clone())
            .collect();
        let mut library: Vec<String> = rest.to_vec();
        library.extend(
            choice
                .removed_indices
                .iter()
                .map(|index| hand7[*index].clone()),
        );
        let gamble_parts = vec![
            sample_key.to_string(),
            bottom_count.to_string(),
            choice_index.to_string(),
        ];
        let per_choice_config = FastSearchConfig {
            gamble_mode: config.gamble_mode,
            gamble_seed: rust_stable_seed(request.seed, "raw_delta_gamble", &gamble_parts, None),
        };
        solver_calls += 1;
        let response = solve_keep_fast_with_ctx(
            ctx,
            &hand,
            &library,
            gemstone_live,
            request.state_limit,
            request.max_turns,
            &request.goal,
            request.engine_target_count,
            &request.engine_success_policy,
            request.remora_upkeep_payments,
            request.action_sort,
            &per_choice_config,
        );
        if response.unsupported {
            return RawDeltaSolveResult {
                score: 0.0,
                hit: false,
                solver_calls,
                unsupported_reason: response.unsupported_reason,
            };
        }
        let score = raw_delta_score(request, &response);
        let hit = response
            .turn
            .is_some_and(|turn| turn <= request.max_turns && score > 0.0);
        let turn = response.turn;
        let better = score > best_score
            || (score == best_score && hit && !best_hit)
            || (score == best_score
                && hit == best_hit
                && turn.is_some()
                && best_turn.map_or(true, |best| turn.expect("turn checked") < best));
        if better {
            best_score = score;
            best_hit = hit;
            best_turn = turn;
            if best_score >= max_score {
                break;
            }
        }
    }

    RawDeltaSolveResult {
        score: if best_score.is_finite() {
            best_score
        } else {
            0.0
        },
        hit: best_hit,
        solver_calls,
        unsupported_reason: None,
    }
}

fn raw_delta_is_any_search(card: &str) -> bool {
    matches!(
        card,
        "Arid Mesa"
            | "Bloodstained Mire"
            | "Flooded Strand"
            | "Marsh Flats"
            | "Misty Rainforest"
            | "Polluted Delta"
            | "Scalding Tarn"
            | "Verdant Catacombs"
            | "Windswept Heath"
            | "Wooded Foothills"
            | "Beseech the Mirror"
            | "Crop Rotation"
            | "Demonic Tutor"
            | "Diabolic Intent"
            | "Eldritch Evolution"
            | "Enlightened Tutor"
            | "Gamble"
            | "Green Sun's Zenith"
            | "Grim Tutor"
            | "Idyllic Tutor"
            | "Imperial Seal"
            | "Mystical Tutor"
            | "Neoform"
            | "Ranger-Captain of Eos"
            | "Scheming Symmetry"
            | "Summoner's Pact"
            | "Vampiric Tutor"
            | "Wishclaw Talisman"
            | "Worldly Tutor"
    )
}

fn raw_delta_is_broad_tutor(card: &str) -> bool {
    matches!(
        card,
        "Beseech the Mirror"
            | "Demonic Tutor"
            | "Diabolic Intent"
            | "Gamble"
            | "Grim Tutor"
            | "Imperial Seal"
            | "Scheming Symmetry"
            | "Vampiric Tutor"
            | "Wishclaw Talisman"
    )
}

fn raw_delta_is_creature(card: &str) -> bool {
    matches!(
        card,
        "Birds of Paradise"
            | "Deathrite Shaman"
            | "Esper Sentinel"
            | "Faerie Mastermind"
            | "Heartwood Storyteller"
            | "Ignoble Hierarch"
            | "Lotho, Corrupt Shirriff"
            | "Nick Fury, Agent of S.H.I.E.L.D."
            | "Noble Hierarch"
            | "Orcish Bowmasters"
            | "Ragavan, Nimble Pilferer"
            | "Ranger-Captain of Eos"
            | "The Cabbage Merchant"
            | "Tinder Wall"
            | "Valley Floodcaller"
            | "Wild Cantor"
    )
}

fn raw_delta_is_enchantment(card: &str) -> bool {
    matches!(
        card,
        "Copy Enchantment"
            | "Flash Photography"
            | "Hunting Grounds"
            | "Mirrormade"
            | "Mystic Remora"
            | "Nature's Chosen"
            | "Rhystic Study"
            | "Smothering Tithe"
    )
}

fn raw_delta_is_land_like(card: &str) -> bool {
    is_land_card_name(card) || is_theoretical_rainbow_land_name(card) || is_mdfc_land_name(card)
}

fn raw_delta_is_instant_or_sorcery(card: &str) -> bool {
    !raw_delta_is_land_like(card)
        && !is_artifact_card_name(card)
        && !raw_delta_is_creature(card)
        && !raw_delta_is_enchantment(card)
}

fn raw_delta_typed_search_can_reach(search: &str, target: &str) -> bool {
    if raw_delta_is_broad_tutor(search) {
        return true;
    }
    if is_fetch_name(search) {
        return land_type_color_mask(target).is_some() && fetch_can_get_name(search, target);
    }
    match search {
        "Crop Rotation" => raw_delta_is_land_like(target),
        "Enlightened Tutor" | "Idyllic Tutor" => {
            is_artifact_card_name(target) || raw_delta_is_enchantment(target)
        }
        "Mystical Tutor" => raw_delta_is_instant_or_sorcery(target),
        "Worldly Tutor" | "Summoner's Pact" | "Green Sun's Zenith" | "Eldritch Evolution"
        | "Neoform" => raw_delta_is_creature(target),
        "Ranger-Captain of Eos" => target == "Esper Sentinel",
        _ => false,
    }
}

fn raw_delta_replacement_pairs(swap: &RawDeltaSwapRequest) -> Vec<(&str, &str)> {
    if swap.replacements.is_empty() {
        vec![(swap.cut.as_str(), swap.add.as_str())]
    } else {
        swap.replacements
            .iter()
            .map(|replacement| (replacement.cut.as_str(), replacement.add.as_str()))
            .collect()
    }
}

fn raw_delta_sample_relevant_multi(
    sample: &RawDeltaBaselineSample,
    deck: &[String],
    slot_replacements: &[(usize, &str)],
    draw_window: usize,
    relevance_mode: &str,
    replacements: &[(&str, &str)],
) -> bool {
    let accessible_len = (7 + draw_window).min(sample.indices.len());
    if slot_replacements.iter().any(|(slot_index, _)| {
        sample
            .indices
            .iter()
            .position(|index| *index == *slot_index)
            .is_some_and(|position| position < accessible_len)
    }) {
        return true;
    }
    match relevance_mode {
        "slot_only" => false,
        "conservative_search" => sample.indices[..accessible_len]
            .iter()
            .any(|index| raw_delta_is_any_search(&deck[*index])),
        _ => sample.indices[..accessible_len].iter().any(|index| {
            let search = &deck[*index];
            replacements.iter().any(|(cut, add)| {
                raw_delta_typed_search_can_reach(search, cut)
                    || raw_delta_typed_search_can_reach(search, add)
            })
        }),
    }
}

fn emit_raw_delta_variant<F>(
    emit: &mut F,
    variant_index: usize,
    variants_total: usize,
    variant: &RawDeltaVariantResponse,
) where
    F: FnMut(RawDeltaStreamRecord),
{
    emit(RawDeltaStreamRecord::Variant {
        variant_index,
        variants_total,
        variant: variant.clone(),
    });
}

pub fn evaluate_raw_delta_fast(request: &RawDeltaFastRequest) -> RawDeltaFastResponse {
    evaluate_raw_delta_fast_with_emit(request, |_record| {})
}

pub fn evaluate_raw_delta_fast_streaming<F>(
    request: &RawDeltaFastRequest,
    emit: F,
) -> RawDeltaFastResponse
where
    F: FnMut(RawDeltaStreamRecord),
{
    evaluate_raw_delta_fast_with_emit(request, emit)
}

fn evaluate_raw_delta_fast_with_emit<F>(
    request: &RawDeltaFastRequest,
    mut emit: F,
) -> RawDeltaFastResponse
where
    F: FnMut(RawDeltaStreamRecord),
{
    if request.deck.len() < 7 {
        return unsupported_raw_delta_response(
            request,
            "deck must contain at least 7 cards".to_string(),
        );
    }
    if request.samples_per_stage == 0 {
        return unsupported_raw_delta_response(
            request,
            "samples_per_stage must be positive".to_string(),
        );
    }

    let stages = request
        .stages
        .clone()
        .unwrap_or_else(|| (0..COMMANDER_MULLIGAN_BOTTOMS_FAST.len()).collect());
    if stages.is_empty() {
        return unsupported_raw_delta_response(request, "stages must not be empty".to_string());
    }
    for stage in &stages {
        if *stage >= COMMANDER_MULLIGAN_BOTTOMS_FAST.len() {
            return unsupported_raw_delta_response(
                request,
                format!(
                    "stage {stage} is out of range for {} commander mulligan stages",
                    COMMANDER_MULLIGAN_BOTTOMS_FAST.len()
                ),
            );
        }
    }
    let bottom_counts: Vec<usize> = stages
        .iter()
        .map(|stage| COMMANDER_MULLIGAN_BOTTOMS_FAST[*stage])
        .collect();
    let total_samples = stages.len() * request.samples_per_stage;
    let mut choices_by_bottom: BTreeMap<usize, Vec<crate::BottomChoice>> = BTreeMap::new();
    for bottom_count in bottom_counts.iter().copied() {
        choices_by_bottom
            .entry(bottom_count)
            .or_insert_with(|| bottom_choices(7, bottom_count));
    }

    let mut card_names = request.deck.clone();
    for swap in &request.swaps {
        for (cut, add) in raw_delta_replacement_pairs(swap) {
            card_names.push(cut.to_string());
            card_names.push(add.to_string());
        }
    }
    card_names.push("Blank".to_string());
    let mut ctx = FastContext::with_card_names(card_names);
    let config = FastSearchConfig::from_parts(
        request.gamble_mode.as_deref(),
        None,
        request.simplified_gamble,
    );

    let baseline_started = Instant::now();
    let mut baseline_samples = Vec::with_capacity(total_samples);
    let mut baseline_score_sum = 0.0;
    let mut baseline_hits = 0usize;
    let mut baseline_solver_calls = 0usize;

    for stage in &stages {
        let bottom_count = COMMANDER_MULLIGAN_BOTTOMS_FAST[*stage];
        let choices = choices_by_bottom
            .get(&bottom_count)
            .expect("bottom choices should have been precomputed");
        for sample_index in 0..request.samples_per_stage {
            let shuffle_parts = vec![stage.to_string(), sample_index.to_string()];
            let indices = shuffled_indices(
                request.deck.len(),
                request.seed,
                "raw_delta_stage_order",
                &shuffle_parts,
                None,
            );
            let order = raw_order_from_indices(&request.deck, &indices, None);
            let gemstone_live = policy_gemstone_live(
                request.seed,
                request.gemstone_caverns_live_rate,
                sample_index,
            );
            let sample_key = format!("baseline|stage={stage}|sample={sample_index}");
            let result = raw_best_score_with_ctx(
                &mut ctx,
                request,
                &order,
                bottom_count,
                gemstone_live,
                &config,
                &sample_key,
                choices,
            );
            if let Some(reason) = result.unsupported_reason {
                return unsupported_raw_delta_response(request, reason);
            }
            baseline_score_sum += result.score;
            baseline_hits += usize::from(result.hit);
            baseline_solver_calls += result.solver_calls;
            baseline_samples.push(RawDeltaBaselineSample {
                stage: *stage,
                bottom_count,
                sample_index,
                gemstone_live,
                indices,
                score: result.score,
                hit: result.hit,
            });
        }
    }
    let baseline_elapsed_ms = baseline_started.elapsed().as_secs_f64() * 1000.0;
    let baseline_score_mean = baseline_score_sum / total_samples as f64;
    emit(RawDeltaStreamRecord::Start {
        stages: stages.clone(),
        bottom_counts: bottom_counts.clone(),
        samples_per_stage: request.samples_per_stage,
        total_samples,
        draw_window: request.draw_window,
        relevance_mode: request.relevance_mode.clone(),
        baseline_solver_calls,
        baseline_elapsed_ms,
        baseline_score_mean,
        baseline_hits,
        variants_total: request.swaps.len(),
        rng_metadata: rng_metadata(),
    });

    let mut variants = Vec::with_capacity(request.swaps.len());
    let mut unsupported = false;
    let mut unsupported_reason = None;

    for (variant_index, swap) in request.swaps.iter().enumerate() {
        let replacements = raw_delta_replacement_pairs(swap);
        let mut slot_replacements: Vec<(usize, &str)> = Vec::with_capacity(replacements.len());
        let mut seen_slots = FxHashSet::default();
        let mut missing_cut = None;
        for (cut, add) in &replacements {
            let Some(slot_index) = request.deck.iter().position(|card| card == cut) else {
                missing_cut = Some((*cut).to_string());
                break;
            };
            if !seen_slots.insert(slot_index) {
                missing_cut = Some(format!("duplicate cut slot for {cut}"));
                break;
            }
            slot_replacements.push((slot_index, *add));
        }
        if let Some(cut) = missing_cut {
            unsupported = true;
            let reason = if cut.starts_with("duplicate cut slot") {
                cut
            } else {
                format!("cut card not found in deck: {cut}")
            };
            unsupported_reason.get_or_insert_with(|| reason.clone());
            let variant = raw_delta_variant_unsupported(
                swap,
                slot_replacements.first().map(|(slot_index, _)| *slot_index),
                total_samples,
                baseline_score_mean,
                baseline_hits,
                reason,
            );
            emit_raw_delta_variant(&mut emit, variant_index, request.swaps.len(), &variant);
            variants.push(variant);
            continue;
        }
        let slot_index = slot_replacements.first().map(|(slot_index, _)| *slot_index);
        let choices_lookup = &choices_by_bottom;
        let delta_started = Instant::now();
        let mut relevant_samples = 0usize;
        let mut candidate_score_sum_delta = 0.0;
        let mut candidate_hits_delta = 0usize;
        let mut candidate_delta_solver_calls = 0usize;
        let mut delta_moments = RawDeltaMoments::default();
        let mut positive_delta_samples = 0usize;
        let mut negative_delta_samples = 0usize;
        let mut zero_delta_samples = 0usize;
        let mut variant_unsupported_reason = None;

        for sample in &baseline_samples {
            let relevant = raw_delta_sample_relevant_multi(
                sample,
                &request.deck,
                &slot_replacements,
                request.draw_window,
                &request.relevance_mode,
                &replacements,
            );
            let (candidate_score, candidate_hit, solver_calls) = if relevant {
                relevant_samples += 1;
                let order = raw_order_from_indices_multi(
                    &request.deck,
                    &sample.indices,
                    &slot_replacements,
                );
                let choices = choices_lookup
                    .get(&sample.bottom_count)
                    .expect("bottom choices should be available for sample");
                let sample_key = format!(
                    "delta|variant={variant_index}|stage={}|sample={}",
                    sample.stage, sample.sample_index
                );
                let result = raw_best_score_with_ctx(
                    &mut ctx,
                    request,
                    &order,
                    sample.bottom_count,
                    sample.gemstone_live,
                    &config,
                    &sample_key,
                    choices,
                );
                if let Some(reason) = result.unsupported_reason {
                    variant_unsupported_reason = Some(reason);
                    (sample.score, sample.hit, result.solver_calls)
                } else {
                    (result.score, result.hit, result.solver_calls)
                }
            } else {
                (sample.score, sample.hit, 0)
            };
            candidate_delta_solver_calls += solver_calls;
            let delta = candidate_score - sample.score;
            if delta > 0.0 {
                positive_delta_samples += 1;
            } else if delta < 0.0 {
                negative_delta_samples += 1;
            } else {
                zero_delta_samples += 1;
            }
            delta_moments.push(delta);
            candidate_score_sum_delta += candidate_score;
            candidate_hits_delta += usize::from(candidate_hit);
            if variant_unsupported_reason.is_some() {
                break;
            }
        }
        let delta_elapsed_ms = delta_started.elapsed().as_secs_f64() * 1000.0;

        if let Some(reason) = variant_unsupported_reason {
            unsupported = true;
            unsupported_reason.get_or_insert_with(|| reason.clone());
            let variant = RawDeltaVariantResponse {
                name: swap.name.clone(),
                cut: swap.cut.clone(),
                add: swap.add.clone(),
                slot_index,
                samples: total_samples,
                relevant_samples,
                irrelevant_samples: total_samples.saturating_sub(relevant_samples),
                relevance_rate: relevant_samples as f64 / total_samples as f64,
                baseline_score_mean,
                candidate_score_mean_delta: candidate_score_sum_delta / total_samples as f64,
                candidate_score_mean_naive: None,
                mean_delta_delta_method: delta_moments.mean(),
                mean_delta_naive: None,
                delta_method_se: delta_moments.standard_error(),
                naive_delta_se: None,
                delta_abs_error_vs_naive: None,
                baseline_hits,
                candidate_hits_delta_method: candidate_hits_delta,
                candidate_hits_naive: None,
                positive_delta_samples,
                negative_delta_samples,
                zero_delta_samples,
                candidate_delta_solver_calls,
                candidate_naive_solver_calls: None,
                delta_elapsed_ms,
                naive_elapsed_ms: None,
                speedup_vs_naive: None,
                unsupported: true,
                unsupported_reason: Some(reason),
            };
            emit_raw_delta_variant(&mut emit, variant_index, request.swaps.len(), &variant);
            variants.push(variant);
            continue;
        }

        let run_naive = request.run_naive && variant_index < request.naive_variant_limit;
        let mut candidate_score_mean_naive = None;
        let mut mean_delta_naive = None;
        let mut naive_delta_se = None;
        let mut delta_abs_error_vs_naive = None;
        let mut candidate_hits_naive = None;
        let mut candidate_naive_solver_calls = None;
        let mut naive_elapsed_ms = None;
        let mut speedup_vs_naive = None;

        if run_naive {
            let naive_started = Instant::now();
            let mut naive_candidate_score_sum = 0.0;
            let mut naive_candidate_hits = 0usize;
            let mut naive_solver_calls = 0usize;
            let mut naive_moments = RawDeltaMoments::default();
            let mut naive_unsupported_reason = None;
            for sample in &baseline_samples {
                let order = raw_order_from_indices_multi(
                    &request.deck,
                    &sample.indices,
                    &slot_replacements,
                );
                let choices = choices_lookup
                    .get(&sample.bottom_count)
                    .expect("bottom choices should be available for sample");
                let sample_key = format!(
                    "naive|variant={variant_index}|stage={}|sample={}",
                    sample.stage, sample.sample_index
                );
                let result = raw_best_score_with_ctx(
                    &mut ctx,
                    request,
                    &order,
                    sample.bottom_count,
                    sample.gemstone_live,
                    &config,
                    &sample_key,
                    choices,
                );
                naive_solver_calls += result.solver_calls;
                if let Some(reason) = result.unsupported_reason {
                    naive_unsupported_reason = Some(reason);
                    break;
                }
                naive_candidate_score_sum += result.score;
                naive_candidate_hits += usize::from(result.hit);
                naive_moments.push(result.score - sample.score);
            }
            let elapsed = naive_started.elapsed().as_secs_f64() * 1000.0;
            if let Some(reason) = naive_unsupported_reason {
                unsupported = true;
                unsupported_reason.get_or_insert_with(|| reason.clone());
                let variant = RawDeltaVariantResponse {
                    name: swap.name.clone(),
                    cut: swap.cut.clone(),
                    add: swap.add.clone(),
                    slot_index,
                    samples: total_samples,
                    relevant_samples,
                    irrelevant_samples: total_samples.saturating_sub(relevant_samples),
                    relevance_rate: relevant_samples as f64 / total_samples as f64,
                    baseline_score_mean,
                    candidate_score_mean_delta: candidate_score_sum_delta / total_samples as f64,
                    candidate_score_mean_naive: None,
                    mean_delta_delta_method: delta_moments.mean(),
                    mean_delta_naive: None,
                    delta_method_se: delta_moments.standard_error(),
                    naive_delta_se: None,
                    delta_abs_error_vs_naive: None,
                    baseline_hits,
                    candidate_hits_delta_method: candidate_hits_delta,
                    candidate_hits_naive: None,
                    positive_delta_samples,
                    negative_delta_samples,
                    zero_delta_samples,
                    candidate_delta_solver_calls,
                    candidate_naive_solver_calls: Some(naive_solver_calls),
                    delta_elapsed_ms,
                    naive_elapsed_ms: Some(elapsed),
                    speedup_vs_naive: None,
                    unsupported: true,
                    unsupported_reason: Some(reason),
                };
                emit_raw_delta_variant(&mut emit, variant_index, request.swaps.len(), &variant);
                variants.push(variant);
                continue;
            }
            let naive_candidate_mean = naive_candidate_score_sum / total_samples as f64;
            let naive_delta_mean = naive_moments.mean();
            candidate_score_mean_naive = Some(naive_candidate_mean);
            mean_delta_naive = Some(naive_delta_mean);
            naive_delta_se = Some(naive_moments.standard_error());
            delta_abs_error_vs_naive = Some((delta_moments.mean() - naive_delta_mean).abs());
            candidate_hits_naive = Some(naive_candidate_hits);
            candidate_naive_solver_calls = Some(naive_solver_calls);
            naive_elapsed_ms = Some(elapsed);
            speedup_vs_naive = if delta_elapsed_ms > 0.0 {
                Some(elapsed / delta_elapsed_ms)
            } else {
                None
            };
        }

        let variant = RawDeltaVariantResponse {
            name: swap.name.clone(),
            cut: swap.cut.clone(),
            add: swap.add.clone(),
            slot_index,
            samples: total_samples,
            relevant_samples,
            irrelevant_samples: total_samples.saturating_sub(relevant_samples),
            relevance_rate: relevant_samples as f64 / total_samples as f64,
            baseline_score_mean,
            candidate_score_mean_delta: candidate_score_sum_delta / total_samples as f64,
            candidate_score_mean_naive,
            mean_delta_delta_method: delta_moments.mean(),
            mean_delta_naive,
            delta_method_se: delta_moments.standard_error(),
            naive_delta_se,
            delta_abs_error_vs_naive,
            baseline_hits,
            candidate_hits_delta_method: candidate_hits_delta,
            candidate_hits_naive,
            positive_delta_samples,
            negative_delta_samples,
            zero_delta_samples,
            candidate_delta_solver_calls,
            candidate_naive_solver_calls,
            delta_elapsed_ms,
            naive_elapsed_ms,
            speedup_vs_naive,
            unsupported: false,
            unsupported_reason: None,
        };
        emit_raw_delta_variant(&mut emit, variant_index, request.swaps.len(), &variant);
        variants.push(variant);
    }

    variants.sort_by(|left, right| {
        right
            .mean_delta_delta_method
            .partial_cmp(&left.mean_delta_delta_method)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.name.cmp(&right.name))
    });
    emit(RawDeltaStreamRecord::Complete {
        variants_total: request.swaps.len(),
        unsupported,
        unsupported_reason: unsupported_reason.clone(),
    });

    RawDeltaFastResponse {
        stages,
        bottom_counts,
        samples_per_stage: request.samples_per_stage,
        total_samples,
        draw_window: request.draw_window,
        relevance_mode: request.relevance_mode.clone(),
        baseline_solver_calls,
        baseline_elapsed_ms,
        variants,
        rng_metadata: rng_metadata(),
        unsupported,
        unsupported_reason,
    }
}

pub fn audit_rng_shuffle(request: &RngShuffleAuditRequest) -> RngShuffleAuditResponse {
    if request.deck.is_empty() {
        return RngShuffleAuditResponse {
            samples: request.samples,
            deck_size: 0,
            domain: request.domain.clone(),
            rng_metadata: rng_metadata(),
            position_counts_by_card: BTreeMap::new(),
            first_card_counts: BTreeMap::new(),
            unsupported: true,
            unsupported_reason: Some("deck must not be empty".to_string()),
        };
    }
    let mut position_counts_by_card: BTreeMap<String, Vec<usize>> = request
        .deck
        .iter()
        .map(|card| (card.clone(), vec![0; request.deck.len()]))
        .collect();
    let mut first_card_counts = BTreeMap::new();
    for sample_index in 0..request.samples {
        let parts = vec![sample_index.to_string()];
        let order = shuffled_copy(&request.deck, request.seed, &request.domain, &parts, None);
        if let Some(first) = order.first() {
            increment_count(&mut first_card_counts, first.clone());
        }
        for (position, card) in order.iter().enumerate() {
            if let Some(counts) = position_counts_by_card.get_mut(card) {
                counts[position] += 1;
            }
        }
    }
    RngShuffleAuditResponse {
        samples: request.samples,
        deck_size: request.deck.len(),
        domain: request.domain.clone(),
        rng_metadata: rng_metadata(),
        position_counts_by_card,
        first_card_counts,
        unsupported: false,
        unsupported_reason: None,
    }
}

fn solve_keep_fast_with_ctx(
    ctx: &mut FastContext,
    hand: &[String],
    library: &[String],
    gemstone_live: bool,
    state_limit: usize,
    max_turns: u8,
    goal: &str,
    engine_target_count: u8,
    engine_success_policy: &str,
    remora_upkeep_payments: u8,
    action_sort: bool,
    config: &FastSearchConfig,
) -> SolveKeepResponse {
    let close_request = CloseTurnRequest {
        states: Vec::new(),
        state_limit,
        max_turns,
        goal: goal.to_string(),
        engine_target_count,
        engine_success_policy: engine_success_policy.to_string(),
        remora_upkeep_payments,
        action_sort,
    };
    let chancellor = hand.iter().any(|card| card == "Chancellor of the Tangle");
    let mut states = starting_state_options_fast(ctx, hand, library, gemstone_live);
    let mut capped = false;

    for turn in 1..=max_turns {
        let mut turn_state_set: FxHashSet<FastState> = FxHashSet::default();
        let mut turn_states = Vec::new();
        for state in &states {
            let mut begun = begin_turn_fast(ctx, state);
            begun.set_turn(turn);
            let upkeep_states = if begun.pact_debt() == 0 {
                vec![begun]
            } else {
                pact_upkeep_states_fast(ctx, &close_request, &begun)
            };
            for upkeep_paid in upkeep_states {
                let mut drawn = draw_card_fast(upkeep_paid);
                if turn == 1 && chancellor {
                    drawn.set_mana(add_mana(drawn.mana(), [0, 0, 0, 0, 1, 0]));
                }
                if state_has_unsupported_gamble_fast(ctx, &drawn, config) {
                    return SolveKeepResponse {
                        turn: None,
                        capped,
                        label: None,
                        unsupported: true,
                        unsupported_reason: Some(
                            "Gamble reached in Rust solve_keep frontier".to_string(),
                        ),
                    };
                }
                if turn_state_set.insert(drawn.clone()) {
                    turn_states.push(drawn);
                }
            }
        }

        let closed = close_turn_fast_inner(ctx, &close_request, turn_states, config);
        capped |= closed.hit_limit;
        if closed.success {
            return SolveKeepResponse {
                turn: Some(turn),
                capped,
                label: closed.label,
                unsupported: false,
                unsupported_reason: None,
            };
        }

        let mut next_state_set: FxHashSet<FastState> = FxHashSet::default();
        states.clear();
        for state in closed.closed {
            let ended = end_turn_fast(ctx, &state);
            if next_state_set.insert(ended.clone()) {
                states.push(ended);
            }
        }
    }

    SolveKeepResponse {
        turn: None,
        capped,
        label: None,
        unsupported: false,
        unsupported_reason: None,
    }
}

fn solve_keep_trace_fast_with_ctx(
    ctx: &mut FastContext,
    hand: &[String],
    library: &[String],
    gemstone_live: bool,
    state_limit: usize,
    max_turns: u8,
    goal: &str,
    engine_target_count: u8,
    engine_success_policy: &str,
    remora_upkeep_payments: u8,
    action_sort: bool,
    config: &FastSearchConfig,
) -> FastTraceSolveResult {
    let close_request = CloseTurnRequest {
        states: Vec::new(),
        state_limit,
        max_turns,
        goal: goal.to_string(),
        engine_target_count,
        engine_success_policy: engine_success_policy.to_string(),
        remora_upkeep_payments,
        action_sort,
    };
    let chancellor = hand.iter().any(|card| card == "Chancellor of the Tangle");
    let mut states = starting_state_options_fast_with_trace(ctx, hand, library, gemstone_live);
    let mut capped = false;

    for turn in 1..=max_turns {
        let mut turn_state_set: FxHashSet<FastState> = FxHashSet::default();
        let mut turn_states = Vec::new();
        for (state, path) in &states {
            let mut begun = begin_turn_fast(ctx, state);
            begun.set_turn(turn);
            let upkeep_states = if begun.pact_debt() == 0 {
                vec![begun]
            } else {
                pact_upkeep_states_fast(ctx, &close_request, &begun)
            };
            for upkeep_paid in upkeep_states {
                let mut drawn = draw_card_fast(upkeep_paid);
                let mut next_path = path.clone();
                if turn == 1 && chancellor {
                    drawn.set_mana(add_mana(drawn.mana(), [0, 0, 0, 0, 1, 0]));
                    next_path.push("reveal Chancellor of the Tangle".to_string());
                }
                if state.pact_debt() > 0 {
                    next_path.push("pay Summoner's Pact upkeep".to_string());
                }
                if state_has_unsupported_gamble_fast(ctx, &drawn, config) {
                    return FastTraceSolveResult {
                        response: SolveKeepResponse {
                            turn: None,
                            capped,
                            label: None,
                            unsupported: true,
                            unsupported_reason: Some(
                                "Gamble reached in Rust solve_keep frontier".to_string(),
                            ),
                        },
                        trace_turn: None,
                        trace_capped: capped,
                        trace_path: Vec::new(),
                    };
                }
                if turn_state_set.insert(drawn.clone()) {
                    turn_states.push((drawn, next_path));
                }
            }
        }

        let closed = close_turn_trace_fast_inner(ctx, &close_request, turn_states, config);
        capped |= closed.hit_limit;
        if closed.success {
            return FastTraceSolveResult {
                response: SolveKeepResponse {
                    turn: Some(turn),
                    capped,
                    label: closed.label,
                    unsupported: false,
                    unsupported_reason: None,
                },
                trace_turn: Some(turn),
                trace_capped: capped,
                trace_path: closed.path,
            };
        }

        let mut next_state_set: FxHashSet<FastState> = FxHashSet::default();
        states.clear();
        for (state, path) in closed.closed {
            let ended = end_turn_fast(ctx, &state);
            if next_state_set.insert(ended.clone()) {
                states.push((ended, path));
            }
        }
    }

    FastTraceSolveResult {
        response: SolveKeepResponse {
            turn: None,
            capped,
            label: None,
            unsupported: false,
            unsupported_reason: None,
        },
        trace_turn: None,
        trace_capped: capped,
        trace_path: Vec::new(),
    }
}

fn gemstone_caverns_alias_in_hand(ctx: &FastContext, state: &FastState) -> Option<CardId> {
    ["Gemstone Caverns", "Glittering Caves of Aglarond"]
        .iter()
        .filter_map(|name| ctx.card_id(name))
        .find(|card| state.has_card(Some(*card)))
}

fn preturn_caverns_top_tutor_states_fast(
    ctx: &mut FastContext,
    state: &FastState,
) -> Vec<(FastState, String)> {
    if !state
        .battlefield
        .iter()
        .any(|perm| perm.kind_enum() == FastPermKind::Cavern)
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    for tutor_name in [
        "Enlightened Tutor",
        "Mystical Tutor",
        "Vampiric Tutor",
        "Worldly Tutor",
    ] {
        let Some(tutor) = ctx.card_id(tutor_name) else {
            continue;
        };
        if !state.has_card(Some(tutor)) {
            continue;
        }
        for target in tutor_targets_fast(ctx, tutor_name, state) {
            let target_name = ctx.card_name(target).to_string();
            let mut next = state.clone();
            next.remove_hand_to_graveyard(ctx, tutor);
            next.library = known_top_library_after_shuffle(ctx, target, &state.library);
            next.set_mana([0, 0, 0, 0, 0, 0]);
            next.set_spells_this_turn(0);
            out.push((next, format!("preturn cast {tutor_name} for {target_name}")));
        }
    }
    out
}

fn starting_state_options_fast(
    ctx: &mut FastContext,
    hand_names: &[String],
    library_names: &[String],
    gemstone_live: bool,
) -> Vec<FastState> {
    let mut hand: SmallVec<[CardId; 16]> = hand_names
        .iter()
        .map(|card| ctx.intern_card(card))
        .collect();
    hand.sort_unstable();
    let library = library_names
        .iter()
        .map(|card| ctx.intern_card(card))
        .collect();
    let base = FastState {
        battlefield: SmallVec::new(),
        engine_names: SmallVec::new(),
        engine_targets: 0,
        graveyard: SmallVec::new(),
        hand,
        library,
        mana: PackedMana::zero(),
        mantle_attached: SmallVec::new(),
        nature_attached: SmallVec::new(),
        land_grave_count: 0,
        counters: 0,
        flags: 0,
    };
    let mut out = vec![base.clone()];
    let Some(gemstone) = gemstone_caverns_alias_in_hand(ctx, &base) else {
        return out;
    };
    if !gemstone_live {
        return out;
    }
    let exiles: Vec<CardId> = base
        .hand
        .iter()
        .copied()
        .filter(|card| *card != gemstone)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    for exile in exiles {
        let mut state = base.clone();
        state.remove_hand_card(gemstone);
        state.remove_hand_card(exile);
        let cavern = make_perm(
            ctx,
            FastPermKind::Cavern,
            false,
            color_mask("BRUWG"),
            false,
            0,
        );
        state.push_perm(ctx, cavern);
        out.push(state.clone());
        out.extend(
            preturn_caverns_top_tutor_states_fast(ctx, &state)
                .into_iter()
                .map(|(next, _label)| next),
        );
    }
    out
}

fn starting_state_options_fast_with_trace(
    ctx: &mut FastContext,
    hand_names: &[String],
    library_names: &[String],
    gemstone_live: bool,
) -> Vec<(FastState, Vec<String>)> {
    let mut hand: SmallVec<[CardId; 16]> = hand_names
        .iter()
        .map(|card| ctx.intern_card(card))
        .collect();
    hand.sort_unstable();
    let library = library_names
        .iter()
        .map(|card| ctx.intern_card(card))
        .collect();
    let base = FastState {
        battlefield: SmallVec::new(),
        engine_names: SmallVec::new(),
        engine_targets: 0,
        graveyard: SmallVec::new(),
        hand,
        library,
        mana: PackedMana::zero(),
        mantle_attached: SmallVec::new(),
        nature_attached: SmallVec::new(),
        land_grave_count: 0,
        counters: 0,
        flags: 0,
    };
    let mut out = vec![(base.clone(), Vec::new())];
    let Some(gemstone) = gemstone_caverns_alias_in_hand(ctx, &base) else {
        return out;
    };
    if !gemstone_live {
        return out;
    }
    let gemstone_name = ctx.card_name(gemstone).to_string();
    let exiles: Vec<CardId> = base
        .hand
        .iter()
        .copied()
        .filter(|card| *card != gemstone)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    for exile in exiles {
        let exile_name = ctx.card_name(exile).to_string();
        let mut state = base.clone();
        state.remove_hand_card(gemstone);
        state.remove_hand_card(exile);
        let cavern = make_perm(
            ctx,
            FastPermKind::Cavern,
            false,
            color_mask("BRUWG"),
            false,
            0,
        );
        state.push_perm(ctx, cavern);
        let start_path = vec![format!("begin with {gemstone_name} exiling {exile_name}")];
        out.push((state.clone(), start_path.clone()));
        for (next, label) in preturn_caverns_top_tutor_states_fast(ctx, &state) {
            let mut path = start_path.clone();
            path.push(label);
            out.push((next, path));
        }
    }
    out
}

fn state_has_unsupported_gamble_fast(
    ctx: &FastContext,
    state: &FastState,
    config: &FastSearchConfig,
) -> bool {
    matches!(config.gamble_mode, FastGambleMode::StochasticUnsupported)
        && state.has_card(ctx.card_id("Gamble"))
}

fn mana_dominated_fast(
    ctx: &FastContext,
    request: &CloseTurnRequest,
    state: &FastState,
    best_mana: &mut FxHashMap<FastState, Vec<Mana>>,
) -> bool {
    use std::collections::hash_map::Entry;

    let key = state.structural_key(ctx, request.max_turns);
    let state_mana = state.mana();
    let existing = match best_mana.entry(key) {
        Entry::Occupied(entry) => entry.into_mut(),
        Entry::Vacant(entry) => {
            entry.insert(vec![state_mana]);
            return false;
        }
    };
    if existing.iter().any(|mana| {
        mana.iter()
            .zip(state_mana.iter())
            .all(|(have, need)| have >= need)
    }) {
        return true;
    }
    existing.retain(|mana| {
        !state_mana
            .iter()
            .zip(mana.iter())
            .all(|(have, old)| have >= old)
    });
    existing.push(state_mana);
    false
}

#[derive(Debug, Clone, Serialize)]
pub struct FastStateBenchReport {
    pub fixture_count: usize,
    pub iterations: u64,
    pub card_count: usize,
    pub interned_string_count: usize,
    pub fixture_state_count: usize,
    pub fixture_clone_hash_seconds: f64,
    pub fast_convert_seconds: f64,
    pub fast_clone_hash_seconds: f64,
    pub fixture_insert_count: usize,
    pub fast_insert_count: usize,
    pub fixture_hash_checksum: u64,
    pub fast_hash_checksum: u64,
    pub clone_hash_speedup: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FastActionMismatch {
    pub fixture_index: usize,
    pub source: Option<String>,
    pub state_signature: String,
    pub expected_count: usize,
    pub generated_count: usize,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FastActionBenchReport {
    pub fixture_count: usize,
    pub iterations: u64,
    pub compared_fixtures: usize,
    pub exact_fixtures: usize,
    pub expected_actions: usize,
    pub generated_actions: usize,
    pub missing_actions: usize,
    pub extra_actions: usize,
    pub mismatches: Vec<FastActionMismatch>,
    pub fixture_action_seconds: f64,
    pub fast_action_seconds: f64,
    pub fixture_action_count_checksum: u64,
    pub fast_action_count_checksum: u64,
    pub action_generation_speedup: f64,
}

pub fn bench_fast_state_fixtures(
    input: &str,
    iterations: u64,
) -> Result<FastStateBenchReport, String> {
    let payload: ActionFixturePayload =
        serde_json::from_str(input).map_err(|err| err.to_string())?;
    let mut fixture_states = Vec::new();
    for fixture in &payload.fixtures {
        fixture_states.push(fixture.state.clone());
    }
    let mut card_names = Vec::new();
    for state in &fixture_states {
        card_names.extend(state.hand.iter().cloned());
        card_names.extend(state.library.iter().cloned());
    }
    let mut ctx = FastContext::with_card_names(card_names);
    let convert_started = Instant::now();
    let fast_states: Vec<FastState> = fixture_states
        .iter()
        .map(|state| FastState::from_fixture(&mut ctx, state))
        .collect();
    let fast_convert_seconds = convert_started.elapsed().as_secs_f64();

    let fixture_started = Instant::now();
    let mut fixture_set: FxHashSet<FixtureState> = FxHashSet::default();
    let mut fixture_checksum = 0u64;
    for _ in 0..iterations {
        fixture_set.clear();
        for state in &fixture_states {
            let cloned = state.clone();
            fixture_checksum = fixture_checksum.wrapping_add(hash_value(&cloned));
            fixture_set.insert(cloned);
        }
    }
    let fixture_clone_hash_seconds = fixture_started.elapsed().as_secs_f64();

    let fast_started = Instant::now();
    let mut fast_set: FxHashSet<FastState> = FxHashSet::default();
    let mut fast_checksum = 0u64;
    for _ in 0..iterations {
        fast_set.clear();
        for state in &fast_states {
            let cloned = state.clone();
            fast_checksum = fast_checksum.wrapping_add(hash_value(&cloned));
            fast_set.insert(cloned);
        }
    }
    let fast_clone_hash_seconds = fast_started.elapsed().as_secs_f64();

    Ok(FastStateBenchReport {
        fixture_count: payload.fixture_count,
        iterations,
        card_count: ctx.card_count(),
        interned_string_count: ctx.interned_count(),
        fixture_state_count: fixture_states.len(),
        fixture_clone_hash_seconds,
        fast_convert_seconds,
        fast_clone_hash_seconds,
        fixture_insert_count: fixture_set.len(),
        fast_insert_count: fast_set.len(),
        fixture_hash_checksum: fixture_checksum,
        fast_hash_checksum: fast_checksum,
        clone_hash_speedup: fixture_clone_hash_seconds
            / fast_clone_hash_seconds.max(f64::MIN_POSITIVE),
    })
}

pub fn bench_fast_action_fixtures(
    input: &str,
    iterations: u64,
    max_mismatches: usize,
) -> Result<FastActionBenchReport, String> {
    let payload: ActionFixturePayload =
        serde_json::from_str(input).map_err(|err| err.to_string())?;
    let mut fixture_states = Vec::new();
    let mut card_names = Vec::new();
    for fixture in &payload.fixtures {
        fixture_states.push(fixture.state.clone());
        card_names.extend(fixture.state.hand.iter().cloned());
        card_names.extend(fixture.state.library.iter().cloned());
    }
    let mut ctx = FastContext::with_card_names(card_names);
    let fast_states: Vec<FastState> = fixture_states
        .iter()
        .map(|state| FastState::from_fixture(&mut ctx, state))
        .collect();

    let mut compared_fixtures = 0usize;
    let mut exact_fixtures = 0usize;
    let mut expected_actions = 0usize;
    let mut generated_actions = 0usize;
    let mut missing_actions = 0usize;
    let mut extra_actions = 0usize;
    let mut mismatches = Vec::new();

    for (fixture, state) in payload.fixtures.iter().zip(fast_states.iter()) {
        if fixture.actions_truncated {
            continue;
        }
        compared_fixtures += 1;
        let mut expected = BTreeMap::new();
        for action in &fixture.actions {
            *expected
                .entry(action.next_state_signature.clone())
                .or_insert(0usize) += 1;
        }

        let mut generated = BTreeMap::new();
        for action in generate_fast_actions(&mut ctx, state) {
            let fixture_state = action.next_state.to_fixture(&ctx);
            let signature = state_signature(&fixture_state);
            *generated.entry(signature).or_insert(0usize) += 1;
        }

        let expected_count: usize = expected.values().sum();
        let generated_count: usize = generated.values().sum();
        expected_actions += expected_count;
        generated_actions += generated_count;

        let mut missing = Vec::new();
        let mut extra = Vec::new();
        for (signature, count) in &expected {
            let got = generated.get(signature).copied().unwrap_or(0);
            if got < *count {
                missing_actions += *count - got;
                missing.extend(std::iter::repeat(signature.clone()).take(*count - got));
            }
        }
        for (signature, count) in &generated {
            let got = expected.get(signature).copied().unwrap_or(0);
            if got < *count {
                extra_actions += *count - got;
                extra.extend(std::iter::repeat(signature.clone()).take(*count - got));
            }
        }
        if missing.is_empty() && extra.is_empty() {
            exact_fixtures += 1;
        } else if mismatches.len() < max_mismatches {
            mismatches.push(FastActionMismatch {
                fixture_index: fixture.fixture_index,
                source: fixture.source.clone(),
                state_signature: fixture.state_signature.clone(),
                expected_count,
                generated_count,
                missing,
                extra,
            });
        }
    }

    let fixture_started = Instant::now();
    let mut fixture_checksum = 0u64;
    for _ in 0..iterations {
        for state in &fixture_states {
            let actions = generate_fixture_action_cores(state);
            fixture_checksum = fixture_checksum
                .wrapping_mul(131)
                .wrapping_add(actions.len() as u64);
        }
    }
    let fixture_action_seconds = fixture_started.elapsed().as_secs_f64();

    let fast_started = Instant::now();
    let mut fast_checksum = 0u64;
    for _ in 0..iterations {
        for state in &fast_states {
            let actions = generate_fast_actions(&mut ctx, state);
            fast_checksum = fast_checksum
                .wrapping_mul(131)
                .wrapping_add(actions.len() as u64);
        }
    }
    let fast_action_seconds = fast_started.elapsed().as_secs_f64();

    Ok(FastActionBenchReport {
        fixture_count: payload.fixture_count,
        iterations,
        compared_fixtures,
        exact_fixtures,
        expected_actions,
        generated_actions,
        missing_actions,
        extra_actions,
        mismatches,
        fixture_action_seconds,
        fast_action_seconds,
        fixture_action_count_checksum: fixture_checksum,
        fast_action_count_checksum: fast_checksum,
        action_generation_speedup: fixture_action_seconds
            / fast_action_seconds.max(f64::MIN_POSITIVE),
    })
}

fn color_mask(extra: &str) -> u8 {
    let mut mask = 0u8;
    for byte in extra.bytes() {
        if let Some(index) = COLORS.iter().position(|candidate| *candidate == byte) {
            mask |= 1 << index;
        }
    }
    mask
}

fn engine_target_mask(target: &str) -> u8 {
    match target {
        "ENCH" => 1 << 0,
        "ART" => 1 << 1,
        "CREATURE" => 1 << 2,
        "PERM" => 1 << 3,
        _ => 0,
    }
}

fn library_order_matters(ctx: &FastContext, hand: &[CardId]) -> bool {
    hand.iter().any(|id| {
        matches!(
            ctx.card_name(*id),
            "Gitaxian Probe"
                | "Manamorphose"
                | "Noxious Revival"
                | "Tataru Taru"
                | "Wheel of Fortune"
        )
    })
}

fn hash_value<T: Hash>(value: &T) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_action_set(actions: Vec<FastAction>) -> FxHashSet<(FastState, i32, bool)> {
        actions
            .into_iter()
            .map(|action| (action.next_state, action.priority, action.is_ragavan_attack))
            .collect()
    }

    fn generate_all_legacy_groups(ctx: &mut FastContext, state: &FastState) -> Vec<FastAction> {
        let mut actions = Vec::new();
        generate_fast_mana_actions(ctx, &mut actions, state);
        generate_fast_engine_actions(ctx, &mut actions, state);
        generate_fast_commander_actions(ctx, &mut actions, state);
        generate_fast_land_actions(ctx, &mut actions, state);
        generate_fast_zero_artifact_actions(ctx, &mut actions, state);
        generate_fast_chrome_mox_actions(ctx, &mut actions, state);
        generate_fast_mox_diamond_actions(ctx, &mut actions, state);
        generate_fast_artifact_spell_actions(ctx, &mut actions, state);
        generate_fast_creature_actions(ctx, &mut actions, state);
        generate_fast_mantle_equip_actions(ctx, &mut actions, state);
        generate_fast_spirit_guide_actions(ctx, &mut actions, state);
        generate_fast_ritual_actions(ctx, &mut actions, state);
        generate_fast_manamorphose_actions(ctx, &mut actions, state);
        generate_fast_rain_actions(ctx, &mut actions, state);
        generate_fast_sac_spell_actions(ctx, &mut actions, state);
        generate_fast_offer_actions(ctx, &mut actions, state);
        generate_fast_noxious_actions(ctx, &mut actions, state);
        generate_fast_summoners_pact_actions(ctx, &mut actions, state);
        generate_fast_green_sun_actions(ctx, &mut actions, state);
        generate_fast_ranger_captain_actions(ctx, &mut actions, state);
        generate_fast_eldritch_evolution_actions(ctx, &mut actions, state);
        generate_fast_neoform_actions(ctx, &mut actions, state);
        generate_fast_crop_rotation_actions(ctx, &mut actions, state);
        generate_fast_hand_tutor_actions(ctx, &mut actions, state);
        generate_fast_beseech_actions(ctx, &mut actions, state);
        generate_fast_top_tutor_actions(ctx, &mut actions, state);
        generate_fast_wishclaw_actions(ctx, &mut actions, state);
        generate_fast_gamble_actions(ctx, &mut actions, state, &FastSearchConfig::default());
        actions
    }

    fn fixture_state(battlefield: Vec<FixturePerm>) -> FixtureState {
        FixtureState {
            battlefield,
            engine_count: 0,
            engine_names: Vec::new(),
            engine_targets: Vec::new(),
            hand: Vec::new(),
            land_grave_count: 0,
            land_played: false,
            library: Vec::new(),
            mana: [0, 0, 0, 0, 0, 0],
            mantle_attached: Vec::new(),
            nature_attached: Vec::new(),
            nature_tap_used: false,
            nature_untap_used: false,
            pact_debt: 0,
            rain_active: false,
            spells_this_turn: 0,
            turn: 1,
        }
    }

    fn fixture_perm(name: &str, extra: &str) -> FixturePerm {
        FixturePerm {
            name: name.to_string(),
            extra: extra.to_string(),
            tapped: false,
        }
    }

    #[test]
    fn fast_glimmervoid_taps_without_artifact_but_sacrifices_at_end_step() {
        let mut ctx = FastContext::with_card_names(["Glimmervoid", "Lotus Petal"]);
        let state = FastState::from_fixture(
            &mut ctx,
            &fixture_state(vec![fixture_perm("GLIMMER", "BRUWG")]),
        );
        let options = fast_tap_options(&state, state.battlefield[0]);
        assert!(options
            .iter()
            .any(|option| matches!(option, FastTap::Color(2))));
        assert!(options
            .iter()
            .any(|option| matches!(option, FastTap::Color(0))));

        let ended = end_turn_fast(&mut ctx, &state);
        assert!(ended
            .battlefield
            .iter()
            .all(|perm| perm.kind_enum() != FastPermKind::Glimmer));

        let with_artifact = FastState::from_fixture(
            &mut ctx,
            &fixture_state(vec![
                fixture_perm("GLIMMER", "BRUWG"),
                fixture_perm("PETAL", ""),
            ]),
        );
        let ended_with_artifact = end_turn_fast(&mut ctx, &with_artifact);
        assert!(ended_with_artifact
            .battlefield
            .iter()
            .any(|perm| perm.kind_enum() == FastPermKind::Glimmer));
    }

    #[test]
    fn persistent_library_clones_share_until_mutated() {
        let library = PersistentLibrary::from(vec![1, 2, 3, 4, 300]);
        let mut clone = library.clone();
        assert!(library.is_shared_with(&clone));
        assert!(library.contains_card(1));
        assert!(library.contains_card(300));
        assert_eq!(clone.remove(0), 1);
        assert!(!library.is_shared_with(&clone));
        assert!(library.contains_card(1));
        assert!(!clone.contains_card(1));
        assert!(clone.contains_card(300));
        assert_eq!(&*library, &[1, 2, 3, 4, 300]);
        assert_eq!(&*clone, &[2, 3, 4, 300]);
    }

    #[test]
    fn state_fingerprint_index_checks_equality_within_hash_bucket() {
        let mut context = FastContext::with_card_names(["Rhystic Study"]);
        let first = FastState::from_fixture(&mut context, &fixture_state(Vec::new()));
        let mut second_fixture = fixture_state(Vec::new());
        second_fixture.hand.push("Rhystic Study".to_string());
        let second = FastState::from_fixture(&mut context, &second_fixture);
        let states = vec![first.clone()];
        let index = FastStateIndex::from_states(&states);
        let first_digest = hash_value(&first);

        assert!(index.contains(&states, &first, first_digest));
        assert!(!index.contains(&states, &second, first_digest));
    }

    #[test]
    fn shared_variant_context_can_intern_more_than_one_deck_of_candidates() {
        let names: Vec<String> = (0..140).map(|index| format!("Candidate {index}")).collect();
        let context = FastContext::with_card_names(&names);
        assert_eq!(context.card_count(), 140);
        assert_eq!(context.card_specs.len(), 140);
    }

    #[test]
    fn nick_fury_enables_all_mox_amber_colors() {
        let mut context = FastContext::with_card_names(["Mox Amber"]);
        let mut state = FastState::from_fixture(&mut context, &fixture_state(Vec::new()));
        let nick = make_perm(
            &mut context,
            FastPermKind::Nick,
            false,
            color_mask("WUBRG"),
            false,
            0,
        );
        let amber = make_perm(&mut context, FastPermKind::Amber, false, 0, false, 0);
        state.push_perm(&context, nick);
        state.push_perm(&context, amber);

        let colors = fast_tap_options(&state, amber)
            .into_iter()
            .filter_map(|tap| match tap {
                FastTap::Color(color) => Some(color),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(colors, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn action_priorities_preserve_legacy_classes() {
        for name in [
            "Rhystic Study",
            "Demonic Tutor",
            "Vampiric Tutor",
            "Wishclaw Talisman",
            "Heartwood Storyteller",
            "Ranger-Captain of Eos",
        ] {
            assert_eq!(label_priority_name(name), ENGINE_TUTOR_PRIORITY, "{name}");
        }
        for name in [
            "Arcane Signet",
            "Lion's Eye Diamond",
            "Lotus Petal",
            "Mox Diamond",
            "Springleaf Drum",
        ] {
            assert_eq!(label_priority_name(name), FAST_MANA_PRIORITY, "{name}");
        }
        for name in ["Diabolic Intent", "Imperial Seal", "Birds of Paradise"] {
            assert_eq!(label_priority_name(name), DEFAULT_PRIORITY, "{name}");
        }
    }

    #[test]
    fn static_tutor_targets_preserve_rank_then_name_order() {
        let names = [
            "An Offer You Can't Refuse",
            "Beseech the Mirror",
            "Birds of Paradise",
            "Chrome Mox",
            "Crop Rotation",
            "Culling the Weak",
            "Dark Ritual",
            "Deathrite Shaman",
            "Demonic Tutor",
            "Diabolic Intent",
            "Eldritch Evolution",
            "Elvish Spirit Guide",
            "Enlightened Tutor",
            "Gamble",
            "Green Sun's Zenith",
            "Grim Tutor",
            "Heartwood Storyteller",
            "Idyllic Tutor",
            "Ignoble Hierarch",
            "Imperial Seal",
            "Infernal Plunge",
            "Lion's Eye Diamond",
            "Lotus Petal",
            "Mana Vault",
            "Manamorphose",
            "Mox Amber",
            "Mox Diamond",
            "Mox Opal",
            "Mystical Tutor",
            "Neoform",
            "Noble Hierarch",
            "Paradise Mantle",
            "Rain of Filth",
            "Rhystic Study",
            "Rite of Flame",
            "Scheming Symmetry",
            "Simian Spirit Guide",
            "Sol Ring",
            "Springleaf Drum",
            "Summoner's Pact",
            "Tinder Wall",
            "Vampiric Tutor",
            "Wild Cantor",
            "Wishclaw Talisman",
            "Worldly Tutor",
        ];
        let mut context = FastContext::with_card_names(names);
        let mut fixture = fixture_state(Vec::new());
        fixture.library = names.iter().map(|name| (*name).to_string()).collect();
        let state = FastState::from_fixture(&mut context, &fixture);

        for tutor in [
            "Imperial Seal",
            "Scheming Symmetry",
            "Vampiric Tutor",
            "Mystical Tutor",
            "Worldly Tutor",
        ] {
            let targets = tutor_targets_fast(&context, tutor, &state);
            assert!(
                targets.windows(2).all(|pair| {
                    let left = context.card_name(pair[0]);
                    let right = context.card_name(pair[1]);
                    (engine_target_priority_rank(left), left)
                        <= (engine_target_priority_rank(right), right)
                }),
                "{tutor}: {:?}",
                targets
                    .iter()
                    .map(|card| {
                        let name = context.card_name(*card);
                        (engine_target_priority_rank(name), name)
                    })
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn immediate_success_prefers_rhystic_over_heartwood() {
        let mut context = FastContext::with_card_names(["Rhystic Study", "Heartwood Storyteller"]);
        let mut heartwood_fixture = fixture_state(Vec::new());
        heartwood_fixture.engine_count = 1;
        heartwood_fixture
            .engine_names
            .push("Heartwood Storyteller@2".to_string());
        heartwood_fixture.turn = 2;
        let mut rhystic_fixture = fixture_state(Vec::new());
        rhystic_fixture.engine_count = 1;
        rhystic_fixture
            .engine_names
            .push("Rhystic Study@2".to_string());
        rhystic_fixture.turn = 2;
        let actions = vec![
            FastAction::new(
                FastState::from_fixture(&mut context, &heartwood_fixture),
                ENGINE_TUTOR_PRIORITY,
            ),
            FastAction::new(
                FastState::from_fixture(&mut context, &rhystic_fixture),
                ENGINE_TUTOR_PRIORITY,
            ),
        ];
        let request = CloseTurnRequest {
            states: Vec::new(),
            state_limit: 100,
            max_turns: 2,
            goal: "engine".to_string(),
            engine_target_count: 1,
            engine_success_policy: "resilient".to_string(),
            remora_upkeep_payments: 2,
            action_sort: true,
        };

        assert_eq!(
            best_immediate_success_fast(&mut context, &request, &actions).map(|(_, label)| label),
            Some("Rhystic Study".to_string())
        );
    }

    #[test]
    fn optimized_tutor_priorities_match_legacy_classification() {
        let context = FastContext::with_card_names([
            "Imperial Seal",
            "Scheming Symmetry",
            "Demonic Tutor",
            "Rhystic Study",
            "Sol Ring",
            "Birds of Paradise",
        ]);
        for tutor in ["Imperial Seal", "Scheming Symmetry", "Demonic Tutor"] {
            for target_name in ["Rhystic Study", "Sol Ring", "Birds of Paradise"] {
                let target = context.card_id(target_name).expect("target card");
                assert_eq!(
                    tutor_target_priority_fast(&context, tutor, target),
                    label_priority_name(tutor).max(label_priority_name(target_name)),
                    "{tutor} -> {target_name}"
                );
            }
        }
    }

    #[test]
    fn payment_directed_actions_fold_mana_taps_into_casts() {
        let mut context = FastContext::with_card_names(["Rhystic Study"]);
        context.payment_mode = FastPaymentMode::Generic;
        let mut fixture = fixture_state(vec![
            fixture_perm("LAND", "U"),
            fixture_perm("LAND", "U"),
            fixture_perm("LAND", "U"),
        ]);
        fixture.hand.push("Rhystic Study".to_string());
        let state = FastState::from_fixture(&mut context, &fixture);

        let actions = generate_fast_actions(&mut context, &state);
        let cast = actions
            .iter()
            .find(|action| action.next_state.engine_count() == 1)
            .expect("payment closure should expose the Rhystic cast directly");
        assert!(
            cast.next_state
                .battlefield
                .iter()
                .filter(|perm| perm.tapped())
                .count()
                >= 3
        );
        assert!(!actions.iter().any(|action| {
            action.next_state.engine_count() == 0
                && action.next_state.mana().iter().any(|amount| *amount > 0)
                && action
                    .next_state
                    .battlefield
                    .iter()
                    .all(|perm| perm.kind_enum() != FastPermKind::EngineEnch)
        }));
    }

    #[test]
    fn payment_directed_actions_preserve_city_float_before_land() {
        let mut context = FastContext::with_card_names(["City of Brass", "City of Traitors"]);
        context.payment_mode = FastPaymentMode::Generic;
        let mut fixture = fixture_state(vec![fixture_perm("CITY", "")]);
        fixture.hand.push("City of Brass".to_string());
        let state = FastState::from_fixture(&mut context, &fixture);

        let actions = generate_fast_actions(&mut context, &state);
        assert!(actions.iter().any(|action| {
            action.next_state.land_played()
                && action.next_state.mana()[5] == 2
                && action
                    .next_state
                    .battlefield
                    .iter()
                    .all(|perm| perm.kind_enum() != FastPermKind::City)
        }));
    }

    #[test]
    fn packed_payment_preserves_mana_vault_surplus() {
        let mut context = FastContext::with_card_names(["Mana Vault", "Sol Ring"]);
        context.payment_mode = FastPaymentMode::Packed;
        let mut fixture = fixture_state(vec![fixture_perm("VAULT", "")]);
        fixture.hand.push("Sol Ring".to_string());
        let state = FastState::from_fixture(&mut context, &fixture);

        let actions = generate_fast_actions(&mut context, &state);
        assert!(actions.iter().any(|action| {
            action.next_state.mana()[5] == 2
                && action
                    .next_state
                    .battlefield
                    .iter()
                    .any(|perm| perm.kind_enum() == FastPermKind::Sol)
                && action
                    .next_state
                    .battlefield
                    .iter()
                    .any(|perm| perm.kind_enum() == FastPermKind::Vault && perm.tapped())
        }));
    }

    #[test]
    fn packed_payment_applies_last_gemstone_mine_counter() {
        let mut context = FastContext::with_card_names(["Gemstone Mine", "Mana Vault", "Sol Ring"]);
        context.payment_mode = FastPaymentMode::Packed;
        let mut fixture = fixture_state(vec![fixture_perm("MINE", "1")]);
        fixture.hand.push("Sol Ring".to_string());
        let state = FastState::from_fixture(&mut context, &fixture);

        let actions = generate_fast_actions(&mut context, &state);
        assert!(actions.iter().any(|action| {
            action.next_state.land_grave_count == 1
                && action
                    .next_state
                    .battlefield
                    .iter()
                    .all(|perm| perm.kind_enum() != FastPermKind::Mine)
                && action
                    .next_state
                    .battlefield
                    .iter()
                    .any(|perm| perm.kind_enum() == FastPermKind::Sol)
        }));
    }

    #[test]
    fn packed_frontier_limit_falls_back_to_legacy_actions() {
        let mut context = FastContext::with_card_names(["Rhystic Study"]);
        let mut fixture = fixture_state(vec![
            fixture_perm("LAND", "U"),
            fixture_perm("LAND", "U"),
            fixture_perm("LAND", "U"),
        ]);
        fixture.hand.push("Rhystic Study".to_string());
        let state = FastState::from_fixture(&mut context, &fixture);

        context.payment_mode = FastPaymentMode::Off;
        let legacy = fast_action_set(generate_fast_actions(&mut context, &state));
        context.payment_mode = FastPaymentMode::Packed;
        context.funding_frontier_limit = 0;
        let bounded = fast_action_set(generate_fast_actions(&mut context, &state));

        assert_eq!(bounded, legacy);
    }

    #[test]
    fn packed_payment_actions_are_legal_under_generic_oracle() {
        let payload: ActionFixturePayload = serde_json::from_str(include_str!(
            "../../../fixtures/parity/action_fixtures_pass48_20260703.json"
        ))
        .expect("action parity fixture");
        let mut names = Vec::new();
        for fixture in &payload.fixtures {
            names.extend(fixture.state.hand.iter().cloned());
            names.extend(fixture.state.library.iter().cloned());
        }
        let mut context = FastContext::with_card_names(names);
        let mut compared = 0;
        for fixture in &payload.fixtures {
            let state = FastState::from_fixture(&mut context, &fixture.state);
            if !fast_packed_payment_supported(&context, &state) {
                continue;
            }
            context.payment_mode = FastPaymentMode::Generic;
            let generic = fast_action_set(generate_fast_actions(&mut context, &state));
            context.payment_mode = FastPaymentMode::Packed;
            let packed = fast_action_set(generate_fast_actions(&mut context, &state));
            let illegal: Vec<_> = packed.difference(&generic).collect();
            assert!(
                illegal.is_empty(),
                "fixture {} produced {} packed-only actions",
                fixture.fixture_index,
                illegal.len()
            );
            compared += 1;
        }
        assert!(
            compared >= 20,
            "insufficient packed fixture coverage: {compared}"
        );
    }

    #[test]
    fn allocation_free_battlefield_sort_matches_fixture_ordering() {
        let context = FastContext::with_card_names(Vec::<String>::new());
        let mut expected = SmallVec::<[FastPerm; 16]>::new();
        for kind_value in 1..=46 {
            let kind = FastPermKind::from_u8(kind_value);
            for tapped in [false, true] {
                for fresh in [false, true] {
                    for colors in 0..64 {
                        expected.push(FastPerm::new(kind, tapped, fresh, colors, colors & 7, 0));
                    }
                }
            }
        }
        let mut actual = expected.clone();
        expected.sort_by(|left, right| {
            left.kind_enum()
                .as_name()
                .cmp(right.kind_enum().as_name())
                .then_with(|| left.tapped().cmp(&right.tapped()))
                .then_with(|| {
                    left.fixture_extra(&context)
                        .cmp(&right.fixture_extra(&context))
                })
        });
        sort_fast_battlefield(&context, &mut actual);
        assert_eq!(actual, expected);
    }

    #[test]
    fn registry_dispatch_matches_all_legacy_generators_on_parity_corpus() {
        let payload: ActionFixturePayload = serde_json::from_str(include_str!(
            "../../../fixtures/parity/action_fixtures_pass48_20260703.json"
        ))
        .expect("action parity fixture");
        let mut names = Vec::new();
        for fixture in &payload.fixtures {
            names.extend(fixture.state.hand.iter().cloned());
            names.extend(fixture.state.library.iter().cloned());
        }
        let mut context = FastContext::with_card_names(names);
        for fixture in &payload.fixtures {
            let state = FastState::from_fixture(&mut context, &fixture.state);
            let expected = generate_all_legacy_groups(&mut context, &state);
            let generated = generate_fast_actions(&mut context, &state);
            assert_eq!(
                expected.len(),
                generated.len(),
                "fixture {}",
                fixture.fixture_index
            );
            for (index, (left, right)) in expected.iter().zip(&generated).enumerate() {
                assert_eq!(
                    left.priority, right.priority,
                    "fixture {} action {index}",
                    fixture.fixture_index
                );
                assert_eq!(
                    left.is_ragavan_attack, right.is_ragavan_attack,
                    "fixture {} action {index}",
                    fixture.fixture_index
                );
                assert_eq!(
                    left.next_state, right.next_state,
                    "fixture {} action {index}",
                    fixture.fixture_index
                );
            }
        }
    }
}
