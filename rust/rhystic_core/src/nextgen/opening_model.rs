use smallvec::SmallVec;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use rustc_hash::FxHashMap;

use super::{
    compute_payment_plans, CardFlags, CardMask, CommanderZone, CompiledPolicy, DeckSpec,
    InformationModel, InformationTransition, ManaOption, ManaPool, ManaSource, OpeningArtifactKind,
    OpeningCreatureKind, OpeningLandKind, OpeningManaProfile, OpeningOutcome, OpeningOutcomeModel,
    OpeningSpellKind, PackedStateV2, PaymentPlan, PermanentInstance, PermanentSource, ResourceUse,
    SlotId, TokenKind, Zone,
};
use crate::{pay_options, Cost};

const LAND_PLAYED: u32 = 1;
const TURN_DRAW_DONE: u32 = 1 << 1;
const PRETURN_WINDOW: u32 = 1 << 2;
const PACT_DUE: u32 = 1 << 3;
const RAIN_ACTIVE: u32 = 1 << 4;
const PAYMENT_CACHE_CAPACITY: usize = 65_536;
static NEXT_MODEL_CACHE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct PaymentCacheKey {
    model_id: u64,
    state: PackedStateV2,
    cost: Cost,
}

thread_local! {
    static PAYMENT_CACHE: RefCell<FxHashMap<PaymentCacheKey, Vec<PaymentPlan>>> =
        RefCell::new(FxHashMap::default());
}

#[derive(Debug, Clone)]
struct LandSemantics {
    mana: SmallVec<[ManaPool; 5]>,
    profile: OpeningManaProfile,
}

#[derive(Debug, Clone)]
enum OpeningCard {
    Inert,
    Land(LandSemantics),
    Artifact(OpeningArtifactKind),
    Spell(OpeningSpellKind),
    Creature(OpeningCreatureKind),
    Engine {
        kind: OpeningEngineKind,
        cost: Cost,
        values: [f64; 2],
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum OpeningEngineKind {
    RhysticStudy,
    HeartwoodStoryteller,
}

#[derive(Debug, Clone)]
pub struct EngineOpeningModel {
    cache_id: u64,
    cards: Box<[OpeningCard]>,
    engine_slots: CardMask,
    supported_slots: CardMask,
    deck_slots: CardMask,
    artifact_slots: CardMask,
    card_flags: Box<[CardFlags]>,
    card_colors: Box<[u8]>,
    max_turn: u8,
    direct_payments: bool,
    resource_microsteps: bool,
    deathrite_external_land: bool,
    angels_grace_slot: Option<SlotId>,
    semantic_classes: [u8; 128],
    quotient_draws: bool,
}

#[derive(Debug, Copy, Clone)]
pub struct VisibleOpeningPolicy<'a> {
    model: &'a EngineOpeningModel,
}

impl<'a> VisibleOpeningPolicy<'a> {
    pub const fn new(model: &'a EngineOpeningModel) -> Self {
        Self { model }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct OpeningMulliganPolicy {
    pub minimum_score_by_hand_size: [i32; 8],
}

impl Default for OpeningMulliganPolicy {
    fn default() -> Self {
        Self {
            minimum_score_by_hand_size: [i32::MIN, i32::MIN, i32::MIN, 8, 9, 10, 11, 12],
        }
    }
}

impl EngineOpeningModel {
    pub fn compile(deck: &DeckSpec, max_turn: u8) -> Self {
        let mut cards = Vec::with_capacity(deck.cards().len());
        let mut engine_slots = CardMask::EMPTY;
        let mut supported_slots = CardMask::EMPTY;
        let mut artifact_slots = CardMask::EMPTY;
        let mut card_flags = Vec::with_capacity(deck.cards().len());
        let mut card_colors = Vec::with_capacity(deck.cards().len());
        let mut angels_grace_slot = None;
        for card in deck.cards() {
            card_flags.push(card.flags);
            card_colors.push(card.color_mask);
            if card.name.as_ref() == "Angel's Grace" {
                angels_grace_slot = Some(card.slot);
            }
            if card.flags.contains(CardFlags::ARTIFACT) {
                artifact_slots.insert(card.slot);
            }
            let compiled = match card.name.as_ref() {
                "Rhystic Study" => OpeningCard::Engine {
                    kind: OpeningEngineKind::RhysticStudy,
                    cost: [2, 0, 0, 1, 0, 0],
                    values: [1.0, 0.75],
                },
                "Heartwood Storyteller" => OpeningCard::Engine {
                    kind: OpeningEngineKind::HeartwoodStoryteller,
                    cost: [1, 0, 0, 0, 0, 2],
                    values: [0.70, 0.55],
                },
                _ if card.opening_mana.is_supported() => {
                    OpeningCard::Land(land_semantics(card.opening_mana))
                }
                _ if !matches!(card.opening_artifact, OpeningArtifactKind::None) => {
                    OpeningCard::Artifact(card.opening_artifact)
                }
                _ if !matches!(card.opening_spell, OpeningSpellKind::None) => {
                    OpeningCard::Spell(card.opening_spell)
                }
                _ if !matches!(card.opening_creature, OpeningCreatureKind::None) => {
                    OpeningCard::Creature(card.opening_creature)
                }
                _ => OpeningCard::Inert,
            };
            if matches!(compiled, OpeningCard::Engine { .. }) {
                engine_slots.insert(card.slot);
            }
            if !matches!(compiled, OpeningCard::Inert) {
                supported_slots.insert(card.slot);
            }
            cards.push(compiled);
        }
        Self {
            cache_id: NEXT_MODEL_CACHE_ID.fetch_add(1, Ordering::Relaxed),
            cards: cards.into_boxed_slice(),
            engine_slots,
            supported_slots,
            deck_slots: deck.card_mask(),
            artifact_slots,
            card_flags: card_flags.into_boxed_slice(),
            card_colors: card_colors.into_boxed_slice(),
            max_turn,
            direct_payments: true,
            resource_microsteps: false,
            deathrite_external_land: true,
            angels_grace_slot,
            semantic_classes: deck.semantic_classes(),
            quotient_draws: true,
        }
    }

    pub fn with_direct_payments(mut self, enabled: bool) -> Self {
        self.direct_payments = enabled;
        self
    }

    pub fn with_resource_microsteps(mut self, enabled: bool) -> Self {
        self.resource_microsteps = enabled;
        self
    }

    pub fn with_deathrite_external_land(mut self, available: bool) -> Self {
        self.deathrite_external_land = available;
        self
    }

    pub fn with_quotient_draws(mut self, enabled: bool) -> Self {
        self.quotient_draws = enabled;
        self
    }

    fn chance_draws(&self, state: PackedStateV2) -> SmallVec<[(SlotId, f64); 16]> {
        if self.quotient_draws {
            state
                .library
                .class_chance_draws(&self.semantic_classes)
                .into_iter()
                .map(|draw| {
                    (
                        draw.representative,
                        f64::from(draw.numerator) / f64::from(draw.denominator),
                    )
                })
                .collect()
        } else {
            state
                .library
                .chance_draws()
                .into_iter()
                .map(|draw| {
                    (
                        draw.slot,
                        f64::from(draw.numerator) / f64::from(draw.denominator),
                    )
                })
                .collect()
        }
    }

    pub const fn supported_slots(&self) -> CardMask {
        self.supported_slots
    }

    pub const fn unsupported_slots(&self) -> CardMask {
        self.deck_slots.difference(self.supported_slots)
    }

    pub fn pregame_states(
        &self,
        state: PackedStateV2,
        gemstone_caverns_live: bool,
    ) -> SmallVec<[PackedStateV2; 8]> {
        let mut states = smallvec::smallvec![state];
        if !gemstone_caverns_live {
            return states;
        }
        let Some(cavern) = state.hand.iter().find(|slot| {
            matches!(
                self.land_kind(*slot),
                Some(OpeningLandKind::GemstoneCaverns)
            )
        }) else {
            return states;
        };
        for exile in state.hand.iter().filter(|slot| *slot != cavern) {
            let mut next = state;
            if !next.move_card(exile, Zone::Hand, Zone::Exile)
                || !next.move_card_to_battlefield(
                    cavern,
                    Zone::Hand,
                    PermanentInstance::new(PermanentSource::card(cavern)).with_counters(1),
                )
            {
                continue;
            }
            next.flags |= PRETURN_WINDOW;
            states.push(next);
        }
        states
    }

    pub fn turn(state: PackedStateV2) -> u8 {
        let turn = (state.counters & 0xff) as u8;
        turn.max(1)
    }

    pub fn set_turn(state: &mut PackedStateV2, turn: u8) {
        state.counters = (state.counters & !0xff) | u32::from(turn.max(1));
    }

    fn card(&self, slot: SlotId) -> Option<&OpeningCard> {
        self.cards.get(slot as usize)
    }

    fn land_kind(&self, slot: SlotId) -> Option<OpeningLandKind> {
        match self.card(slot) {
            Some(OpeningCard::Land(land)) => Some(land.profile.kind),
            _ => None,
        }
    }

    fn artifact_kind(&self, slot: SlotId) -> Option<OpeningArtifactKind> {
        match self.card(slot) {
            Some(OpeningCard::Artifact(kind)) => Some(*kind),
            _ => None,
        }
    }

    fn spell_kind(&self, slot: SlotId) -> Option<OpeningSpellKind> {
        match self.card(slot) {
            Some(OpeningCard::Spell(kind)) => Some(*kind),
            _ => None,
        }
    }

    fn creature_kind(&self, slot: SlotId) -> Option<OpeningCreatureKind> {
        match self.card(slot) {
            Some(OpeningCard::Creature(kind)) => Some(*kind),
            _ => None,
        }
    }

    fn engine_value(&self, state: PackedStateV2) -> Option<f64> {
        let turn_index = usize::from(Self::turn(state).saturating_sub(1)).min(1);
        let value = state
            .battlefield
            .as_slice()
            .iter()
            .filter_map(|permanent| permanent.source().card_slot())
            .filter(|slot| self.engine_slots.contains(*slot))
            .filter_map(|slot| match self.card(slot) {
                Some(OpeningCard::Engine { values, .. }) => Some(values[turn_index]),
                _ => None,
            })
            .reduce(f64::max);
        if value.is_some() && state.flags & PACT_DUE != 0 && !self.can_pay_pact_next_upkeep(state) {
            None
        } else {
            value
        }
    }

    fn opening_outcome(&self, state: PackedStateV2) -> Option<OpeningOutcome> {
        if state.flags & PACT_DUE != 0 && !self.can_pay_pact_next_upkeep(state) {
            return None;
        }
        let turn = Self::turn(state).min(2);
        state
            .battlefield
            .as_slice()
            .iter()
            .filter_map(|permanent| permanent.source().card_slot())
            .filter_map(|slot| match self.card(slot) {
                Some(OpeningCard::Engine { kind, values, .. }) => {
                    let mut outcome = OpeningOutcome {
                        weighted_ev: values[usize::from(turn - 1)],
                        ..OpeningOutcome::default()
                    };
                    match (*kind, turn) {
                        (OpeningEngineKind::RhysticStudy, 1) => outcome.rhystic_turn_1 = 1.0,
                        (OpeningEngineKind::RhysticStudy, _) => outcome.rhystic_turn_2 = 1.0,
                        (OpeningEngineKind::HeartwoodStoryteller, 1) => {
                            outcome.heartwood_turn_1 = 1.0;
                        }
                        (OpeningEngineKind::HeartwoodStoryteller, _) => {
                            outcome.heartwood_turn_2 = 1.0;
                        }
                    }
                    Some(outcome)
                }
                _ => None,
            })
            .max_by(|left, right| left.weighted_ev.total_cmp(&right.weighted_ev))
    }

    fn visible_policy_score(&self, state: PackedStateV2) -> i64 {
        if let Some(value) = self.engine_value(state) {
            return 1_000_000 + (value * 10_000.0).round() as i64;
        }
        let mut score = -10_000 * i64::from(Self::turn(state));
        score += i64::from(
            state
                .mana
                .0
                .iter()
                .map(|mana| u16::from(*mana))
                .sum::<u16>(),
        ) * 320;
        for source in self.payment_sources(state) {
            let best_quantity = source
                .options
                .iter()
                .map(|option| {
                    option
                        .mana
                        .0
                        .iter()
                        .map(|amount| u16::from(*amount))
                        .sum::<u16>()
                })
                .max()
                .unwrap_or(0);
            score += i64::from(best_quantity) * 190;
            if source
                .options
                .iter()
                .any(|option| option.mana.0[2] > 0 || option.mana.0[4] > 0)
            {
                score += 70;
            }
        }
        score += state.battlefield.len() as i64 * 35;
        score -= state.hand.len() as i64;
        if state.library.known_top_len() > 0 {
            let top = state.library.chance_draws()[0].slot;
            score += self.card_policy_value(top, true);
        }
        for slot in state.hand.iter() {
            match self.card(slot) {
                Some(OpeningCard::Engine { cost, .. }) => {
                    score += 5_000;
                    if !pay_options(state.mana.0, *cost).is_empty() {
                        score += 4_000;
                    }
                }
                Some(OpeningCard::Land(land)) if state.flags & LAND_PLAYED == 0 => {
                    let quantity = land
                        .mana
                        .iter()
                        .map(|pool| pool.0.iter().copied().max().unwrap_or(0))
                        .max()
                        .unwrap_or(0);
                    score += 150 + i64::from(quantity) * 60;
                    if land.profile.color_mask & ((1 << 2) | (1 << 4)) != 0 {
                        score += 45;
                    }
                }
                Some(OpeningCard::Artifact(kind)) => {
                    score += match kind {
                        OpeningArtifactKind::LotusPetal
                        | OpeningArtifactKind::MoxDiamond
                        | OpeningArtifactKind::ChromeMox => 125,
                        OpeningArtifactKind::MoxOpal
                        | OpeningArtifactKind::MoxAmber
                        | OpeningArtifactKind::SolRing
                        | OpeningArtifactKind::ManaVault
                        | OpeningArtifactKind::WishclawTalisman => 100,
                        OpeningArtifactKind::LionsEyeDiamond
                        | OpeningArtifactKind::ParadiseMantle => 60,
                        OpeningArtifactKind::None => 0,
                    };
                }
                Some(OpeningCard::Spell(_)) | Some(OpeningCard::Creature(_)) => {
                    score += self.card_policy_value(slot, false);
                }
                _ => {}
            }
        }
        score
    }

    fn card_policy_value(&self, slot: SlotId, known_top: bool) -> i64 {
        let scale = if known_top { 3 } else { 1 };
        let value = match self.card(slot) {
            Some(OpeningCard::Engine { .. }) => 1_800,
            Some(OpeningCard::Spell(kind)) => match kind {
                OpeningSpellKind::DemonicTutor
                | OpeningSpellKind::ImperialSeal
                | OpeningSpellKind::VampiricTutor
                | OpeningSpellKind::EnlightenedTutor
                | OpeningSpellKind::SchemingSymmetry
                | OpeningSpellKind::MysticalTutor
                | OpeningSpellKind::GreenSunsZenith
                | OpeningSpellKind::SummonersPact
                | OpeningSpellKind::DiabolicIntent
                | OpeningSpellKind::EldritchEvolution => 700,
                OpeningSpellKind::DarkRitual
                | OpeningSpellKind::RiteOfFlame
                | OpeningSpellKind::Manamorphose
                | OpeningSpellKind::CropRotation
                | OpeningSpellKind::CullingTheWeak
                | OpeningSpellKind::InfernalPlunge
                | OpeningSpellKind::RainOfFilth
                | OpeningSpellKind::ElvishSpiritGuide
                | OpeningSpellKind::SimianSpiritGuide => 300,
                OpeningSpellKind::Gamble
                | OpeningSpellKind::NoxiousRevival
                | OpeningSpellKind::AnOfferYouCantRefuse => 250,
                OpeningSpellKind::None => 0,
            },
            Some(OpeningCard::Creature(kind)) => match kind {
                OpeningCreatureKind::BirdsOfParadise
                | OpeningCreatureKind::DeathriteShaman
                | OpeningCreatureKind::TinderWall => 300,
                OpeningCreatureKind::Ragavan => 220,
                OpeningCreatureKind::RangerCaptainOfEos => 100,
                OpeningCreatureKind::EsperSentinel => 80,
                OpeningCreatureKind::None => 0,
            },
            Some(OpeningCard::Land(land)) => 180 + i64::from(land.profile.colorless) * 50,
            Some(OpeningCard::Artifact(_)) => 250,
            Some(OpeningCard::Inert) | None => 0,
        };
        value * scale
    }

    fn visible_hand_score(&self, state: PackedStateV2) -> i32 {
        let mut score = 0;
        let mut lands = 0;
        for slot in state.hand.iter() {
            score += match self.card(slot) {
                Some(OpeningCard::Engine { .. }) => 6,
                Some(OpeningCard::Land(land)) => {
                    lands += 1;
                    if land.profile.colorless >= 2 {
                        3
                    } else {
                        2
                    }
                }
                Some(OpeningCard::Artifact(kind)) => match kind {
                    OpeningArtifactKind::LotusPetal
                    | OpeningArtifactKind::ChromeMox
                    | OpeningArtifactKind::MoxDiamond => 3,
                    OpeningArtifactKind::SolRing
                    | OpeningArtifactKind::ManaVault
                    | OpeningArtifactKind::MoxOpal
                    | OpeningArtifactKind::MoxAmber
                    | OpeningArtifactKind::WishclawTalisman => 2,
                    OpeningArtifactKind::LionsEyeDiamond | OpeningArtifactKind::ParadiseMantle => 1,
                    OpeningArtifactKind::None => 0,
                },
                Some(OpeningCard::Spell(kind)) => match kind {
                    OpeningSpellKind::DemonicTutor
                    | OpeningSpellKind::ImperialSeal
                    | OpeningSpellKind::VampiricTutor
                    | OpeningSpellKind::EnlightenedTutor
                    | OpeningSpellKind::SchemingSymmetry
                    | OpeningSpellKind::MysticalTutor => 3,
                    OpeningSpellKind::DarkRitual
                    | OpeningSpellKind::ElvishSpiritGuide
                    | OpeningSpellKind::SimianSpiritGuide
                    | OpeningSpellKind::RiteOfFlame
                    | OpeningSpellKind::Manamorphose
                    | OpeningSpellKind::Gamble
                    | OpeningSpellKind::NoxiousRevival
                    | OpeningSpellKind::GreenSunsZenith
                    | OpeningSpellKind::SummonersPact
                    | OpeningSpellKind::CropRotation
                    | OpeningSpellKind::CullingTheWeak
                    | OpeningSpellKind::DiabolicIntent
                    | OpeningSpellKind::InfernalPlunge
                    | OpeningSpellKind::RainOfFilth
                    | OpeningSpellKind::EldritchEvolution => 2,
                    OpeningSpellKind::AnOfferYouCantRefuse => 2,
                    OpeningSpellKind::None => 0,
                },
                Some(OpeningCard::Creature(kind)) => match kind {
                    OpeningCreatureKind::BirdsOfParadise
                    | OpeningCreatureKind::DeathriteShaman
                    | OpeningCreatureKind::TinderWall => 2,
                    OpeningCreatureKind::Ragavan => 1,
                    OpeningCreatureKind::RangerCaptainOfEos
                    | OpeningCreatureKind::EsperSentinel => 1,
                    OpeningCreatureKind::None => 0,
                },
                _ => 0,
            };
        }
        if lands == 0 {
            score -= 5;
        } else if lands > 3 {
            score -= lands - 3;
        }
        score
    }

    fn visible_mulligan_score(&self, state: PackedStateV2, gemstone_caverns_live: bool) -> i32 {
        self.pregame_states(state, gemstone_caverns_live)
            .into_iter()
            .map(|start| {
                let active_caverns = start
                    .battlefield
                    .as_slice()
                    .iter()
                    .filter(|permanent| {
                        permanent.counters() > 0
                            && permanent.source().card_slot().is_some_and(|slot| {
                                matches!(
                                    self.land_kind(slot),
                                    Some(OpeningLandKind::GemstoneCaverns)
                                )
                            })
                    })
                    .count() as i32;
                self.visible_hand_score(start) + active_caverns * 8
            })
            .max()
            .unwrap_or(i32::MIN)
    }

    pub(crate) fn mulligan_state_score(
        &self,
        state: PackedStateV2,
        gemstone_caverns_live: bool,
    ) -> i32 {
        self.visible_mulligan_score(state, gemstone_caverns_live)
    }

    pub fn should_keep(
        &self,
        policy: OpeningMulliganPolicy,
        state: PackedStateV2,
        hand_size: usize,
        gemstone_caverns_live: bool,
    ) -> bool {
        self.visible_mulligan_score(state, gemstone_caverns_live)
            >= policy.minimum_score_by_hand_size[hand_size.min(7)]
    }

    pub fn bottom_priority(&self, slot: SlotId) -> i32 {
        match self.card(slot) {
            Some(OpeningCard::Inert) | None => 0,
            Some(OpeningCard::Artifact(OpeningArtifactKind::ParadiseMantle)) => 1,
            Some(OpeningCard::Artifact(OpeningArtifactKind::LionsEyeDiamond)) => 2,
            Some(OpeningCard::Land(_)) => 3,
            Some(OpeningCard::Artifact(_)) => 4,
            Some(OpeningCard::Spell(_)) => 5,
            Some(OpeningCard::Creature(_)) => 4,
            Some(OpeningCard::Engine { .. }) => 6,
        }
    }

    fn generate_land_plays(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        if state.flags & LAND_PLAYED != 0 {
            return;
        }
        for slot in state.hand.iter() {
            let Some(OpeningCard::Land(land)) = self.card(slot) else {
                continue;
            };
            let mut next = state;
            if !matches!(land.profile.kind, OpeningLandKind::CityOfTraitors) {
                let cities: SmallVec<[SlotId; 2]> = next
                    .battlefield
                    .as_slice()
                    .iter()
                    .filter_map(|permanent| permanent.source().card_slot())
                    .filter(|city| {
                        matches!(self.land_kind(*city), Some(OpeningLandKind::CityOfTraitors))
                    })
                    .collect();
                for city in cities {
                    next.move_card_from_battlefield(city, Zone::Graveyard);
                }
            }
            let counters = u8::from(matches!(land.profile.kind, OpeningLandKind::GemstoneMine)) * 3;
            let permanent = PermanentInstance::new(PermanentSource::card(slot))
                .with_tapped(land.profile.enters_tapped)
                .with_counters(counters);
            if next.move_card_to_battlefield(slot, Zone::Hand, permanent) {
                next.flags |= LAND_PLAYED;
                out.push(InformationTransition::Deterministic(next));
            }
        }
    }

    fn generate_mana_activations(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for permanent in state.battlefield.as_slice() {
            if permanent.tapped() {
                continue;
            }
            let Some(slot) = permanent.source().card_slot() else {
                continue;
            };
            let Some(OpeningCard::Land(land)) = self.card(slot) else {
                continue;
            };
            let mana = if matches!(land.profile.kind, OpeningLandKind::GemstoneCaverns)
                && permanent.counters() > 0
            {
                mana_options(0b1_1111, 0)
            } else {
                land.mana.clone()
            };
            for produced in mana {
                let mut next = state;
                next.mana = next.mana.add_capped(produced, 15);
                if matches!(land.profile.kind, OpeningLandKind::GemstoneMine) {
                    if permanent.counters() <= 1 {
                        next.move_card_from_battlefield(slot, Zone::Graveyard);
                    } else {
                        next.battlefield.replace(
                            *permanent,
                            permanent
                                .with_tapped(true)
                                .with_counters(permanent.counters() - 1),
                        );
                    }
                } else {
                    next.battlefield
                        .replace(*permanent, permanent.with_tapped(true));
                }
                out.push(InformationTransition::Deterministic(next));
            }
            if matches!(land.profile.kind, OpeningLandKind::CrystalVein) {
                let mut next = state;
                next.mana = next.mana.add_capped(ManaPool([0, 0, 0, 0, 0, 2]), 15);
                if next.move_card_from_battlefield(slot, Zone::Graveyard) {
                    out.push(InformationTransition::Deterministic(next));
                }
            }
        }
    }

    fn generate_city_float(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for permanent in state.battlefield.as_slice() {
            let Some(slot) = permanent.source().card_slot() else {
                continue;
            };
            if permanent.tapped()
                || !matches!(self.land_kind(slot), Some(OpeningLandKind::CityOfTraitors))
            {
                continue;
            }
            let mut next = state;
            next.mana = next.mana.add_capped(ManaPool([0, 0, 0, 0, 0, 2]), 15);
            next.battlefield
                .replace(*permanent, permanent.with_tapped(true));
            out.push(InformationTransition::Deterministic(next));
        }
    }

    fn generate_fetches(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for permanent in state.battlefield.as_slice() {
            let Some(fetch_slot) = permanent.source().card_slot() else {
                continue;
            };
            let Some(OpeningCard::Land(fetch)) = self.card(fetch_slot) else {
                continue;
            };
            if !matches!(fetch.profile.kind, OpeningLandKind::Fetch) {
                continue;
            }
            for target_slot in state.library.cards().iter() {
                let Some(OpeningCard::Land(target)) = self.card(target_slot) else {
                    continue;
                };
                if target.profile.land_types & fetch.profile.fetch_types == 0 {
                    continue;
                }
                let mut next = state;
                if !next.move_card_from_battlefield(fetch_slot, Zone::Graveyard)
                    || !next.library.remove_known_or_unknown(target_slot)
                {
                    continue;
                }
                next.library.shuffle_all_unknown();
                let target_permanent = PermanentInstance::new(PermanentSource::card(target_slot))
                    .with_tapped(target.profile.enters_tapped);
                if next.battlefield.insert(target_permanent) {
                    out.push(InformationTransition::Deterministic(next));
                }
            }
        }
    }

    fn has_artifact(&self, state: PackedStateV2) -> bool {
        state.battlefield.as_slice().iter().any(|permanent| {
            permanent.source().token_kind().is_some_and(|kind| {
                matches!(kind, TokenKind::Treasure | TokenKind::GenericArtifact)
            }) || permanent.source().card_slot().is_some_and(|slot| {
                self.artifact_slots.contains(slot)
                    && !(matches!(
                        self.artifact_kind(slot),
                        Some(OpeningArtifactKind::WishclawTalisman)
                    ) && permanent.counters() == 0)
            })
        })
    }

    fn artifact_count(&self, state: PackedStateV2) -> usize {
        state
            .battlefield
            .as_slice()
            .iter()
            .filter(|permanent| {
                permanent.source().token_kind().is_some_and(|kind| {
                    matches!(kind, TokenKind::Treasure | TokenKind::GenericArtifact)
                }) || permanent.source().card_slot().is_some_and(|slot| {
                    self.artifact_slots.contains(slot)
                        && !(matches!(
                            self.artifact_kind(slot),
                            Some(OpeningArtifactKind::WishclawTalisman)
                        ) && permanent.counters() == 0)
                })
            })
            .count()
    }

    fn generate_artifact_casts(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for slot in state.hand.iter() {
            let Some(kind) = self.artifact_kind(slot) else {
                continue;
            };
            match kind {
                OpeningArtifactKind::MoxDiamond => {
                    for discard in state.hand.iter().filter(|card| {
                        *card != slot && self.card_flags[*card as usize].contains(CardFlags::LAND)
                    }) {
                        let mut next = state;
                        if next.move_card(discard, Zone::Hand, Zone::Graveyard)
                            && next.move_card_to_battlefield(
                                slot,
                                Zone::Hand,
                                PermanentInstance::new(PermanentSource::card(slot)),
                            )
                        {
                            out.push(InformationTransition::Deterministic(next));
                        }
                    }
                }
                OpeningArtifactKind::ChromeMox => {
                    let mut no_imprint = state;
                    if no_imprint.move_card_to_battlefield(
                        slot,
                        Zone::Hand,
                        PermanentInstance::new(PermanentSource::card(slot)),
                    ) {
                        out.push(InformationTransition::Deterministic(no_imprint));
                    }
                    for imprint in state.hand.iter().filter(|card| {
                        *card != slot
                            && !self.card_flags[*card as usize].contains(CardFlags::ARTIFACT)
                            && !self.card_flags[*card as usize].contains(CardFlags::LAND)
                            && self.card_colors[*card as usize] != 0
                    }) {
                        let mut next = state;
                        let permanent = PermanentInstance::new(PermanentSource::card(slot))
                            .with_counters(self.card_colors[imprint as usize]);
                        if next.move_card(imprint, Zone::Hand, Zone::Exile)
                            && next.move_card_to_battlefield(slot, Zone::Hand, permanent)
                        {
                            out.push(InformationTransition::Deterministic(next));
                        }
                    }
                }
                OpeningArtifactKind::WishclawTalisman => {
                    for plan in self.payment_plans(state, [1, 1, 0, 0, 0, 0]) {
                        let Some(mut next) = self.apply_payment_plan(state, plan) else {
                            continue;
                        };
                        if next.move_card_to_battlefield(
                            slot,
                            Zone::Hand,
                            PermanentInstance::new(PermanentSource::card(slot)).with_counters(3),
                        ) {
                            out.push(InformationTransition::Deterministic(next));
                        }
                    }
                }
                OpeningArtifactKind::SolRing | OpeningArtifactKind::ManaVault => {
                    if self.direct_payments {
                        for plan in self.payment_plans(state, [1, 0, 0, 0, 0, 0]) {
                            let Some(mut next) = self.apply_payment_plan(state, plan) else {
                                continue;
                            };
                            if next.move_card_to_battlefield(
                                slot,
                                Zone::Hand,
                                PermanentInstance::new(PermanentSource::card(slot)),
                            ) {
                                out.push(InformationTransition::Deterministic(next));
                            }
                        }
                    }
                    if self.resource_microsteps {
                        for leftover in pay_options(state.mana.0, [1, 0, 0, 0, 0, 0]) {
                            let mut next = state;
                            next.mana = ManaPool(leftover);
                            if next.move_card_to_battlefield(
                                slot,
                                Zone::Hand,
                                PermanentInstance::new(PermanentSource::card(slot)),
                            ) {
                                out.push(InformationTransition::Deterministic(next));
                            }
                        }
                    }
                }
                OpeningArtifactKind::LotusPetal
                | OpeningArtifactKind::LionsEyeDiamond
                | OpeningArtifactKind::MoxOpal
                | OpeningArtifactKind::MoxAmber
                | OpeningArtifactKind::ParadiseMantle => {
                    let mut next = state;
                    if next.move_card_to_battlefield(
                        slot,
                        Zone::Hand,
                        PermanentInstance::new(PermanentSource::card(slot)),
                    ) {
                        out.push(InformationTransition::Deterministic(next));
                    }
                }
                OpeningArtifactKind::None => {}
            }
        }
    }

    fn generate_artifact_mana(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for permanent in state.battlefield.as_slice() {
            if permanent.tapped() {
                continue;
            }
            let Some(slot) = permanent.source().card_slot() else {
                continue;
            };
            let Some(kind) = self.artifact_kind(slot) else {
                continue;
            };
            let (mana, sacrifice, discard_hand) = match kind {
                OpeningArtifactKind::LotusPetal => (mana_options(0b1_1111, 0), true, false),
                OpeningArtifactKind::LionsEyeDiamond => {
                    let mut options = SmallVec::new();
                    for color in 0..5 {
                        let mut produced = [0; 6];
                        produced[color] = 3;
                        options.push(ManaPool(produced));
                    }
                    (options, true, true)
                }
                OpeningArtifactKind::ChromeMox => {
                    (mana_options(permanent.counters(), 0), false, false)
                }
                OpeningArtifactKind::MoxDiamond => (mana_options(0b1_1111, 0), false, false),
                OpeningArtifactKind::MoxOpal if self.artifact_count(state) >= 3 => {
                    (mana_options(0b1_1111, 0), false, false)
                }
                OpeningArtifactKind::MoxAmber
                    if state.commander.zone == CommanderZone::Battlefield =>
                {
                    (mana_options(0b1_1111, 0), false, false)
                }
                OpeningArtifactKind::SolRing => (
                    smallvec::smallvec![ManaPool([0, 0, 0, 0, 0, 2])],
                    false,
                    false,
                ),
                OpeningArtifactKind::ManaVault => (
                    smallvec::smallvec![ManaPool([0, 0, 0, 0, 0, 3])],
                    false,
                    false,
                ),
                _ => continue,
            };
            for produced in mana {
                let mut next = state;
                next.mana = next.mana.add_capped(produced, 15);
                if sacrifice {
                    next.move_card_from_battlefield(slot, Zone::Graveyard);
                } else {
                    next.battlefield
                        .replace(*permanent, permanent.with_tapped(true));
                }
                if discard_hand {
                    let hand: SmallVec<[SlotId; 16]> = next.hand.iter().collect();
                    for card in hand {
                        next.move_card(card, Zone::Hand, Zone::Graveyard);
                    }
                }
                out.push(InformationTransition::Deterministic(next));
            }
        }
    }

    fn generate_mantle_equips(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for mantle in state.battlefield.as_slice() {
            let Some(mantle_slot) = mantle.source().card_slot() else {
                continue;
            };
            if !matches!(
                self.artifact_kind(mantle_slot),
                Some(OpeningArtifactKind::ParadiseMantle)
            ) {
                continue;
            }
            for target in state.battlefield.as_slice().iter().filter(|target| {
                target.source().is_commander()
                    || target.source().card_slot().is_some_and(|slot| {
                        self.card_flags[slot as usize].contains(CardFlags::CREATURE)
                    })
            }) {
                if mantle.attached_to() == Some(target.source()) {
                    continue;
                }
                if self.direct_payments {
                    for plan in self.payment_plans(state, [1, 0, 0, 0, 0, 0]) {
                        let Some(mut next) = self.apply_payment_plan(state, plan) else {
                            continue;
                        };
                        let Some(current_mantle) = next
                            .battlefield
                            .as_slice()
                            .iter()
                            .find(|permanent| permanent.source() == mantle.source())
                            .copied()
                        else {
                            continue;
                        };
                        next.battlefield.replace(
                            current_mantle,
                            current_mantle.with_attachment(target.source()),
                        );
                        out.push(InformationTransition::Deterministic(next));
                    }
                }
                if self.resource_microsteps {
                    for leftover in pay_options(state.mana.0, [1, 0, 0, 0, 0, 0]) {
                        let mut next = state;
                        next.mana = ManaPool(leftover);
                        next.battlefield
                            .replace(*mantle, mantle.with_attachment(target.source()));
                        out.push(InformationTransition::Deterministic(next));
                    }
                }
            }
        }
    }

    fn generate_mantle_mana(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for mantle in state.battlefield.as_slice() {
            let Some(mantle_slot) = mantle.source().card_slot() else {
                continue;
            };
            if !matches!(
                self.artifact_kind(mantle_slot),
                Some(OpeningArtifactKind::ParadiseMantle)
            ) {
                continue;
            }
            let Some(target_source) = mantle.attached_to() else {
                continue;
            };
            let Some(target) = state
                .battlefield
                .as_slice()
                .iter()
                .find(|permanent| permanent.source() == target_source)
            else {
                continue;
            };
            if target.tapped() || target.fresh() {
                continue;
            }
            for produced in mana_options(0b1_1111, 0) {
                let mut next = state;
                next.mana = next.mana.add_capped(produced, 15);
                next.battlefield.replace(*target, target.with_tapped(true));
                out.push(InformationTransition::Deterministic(next));
            }
        }
    }

    fn generate_commander_cast(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        if state.commander.zone != CommanderZone::Command {
            return;
        }
        let cost = [state.commander.tax, 0, 0, 0, 1, 0];
        if self.direct_payments {
            for plan in self.payment_plans(state, cost) {
                let Some(mut next) = self.apply_payment_plan(state, plan) else {
                    continue;
                };
                if next.put_commander_on_battlefield(false, true) {
                    next.commander.cast_count = next.commander.cast_count.saturating_add(1);
                    next.commander.tax = next.commander.cast_count.saturating_mul(2);
                    out.push(InformationTransition::Deterministic(next));
                }
            }
        }
        if self.resource_microsteps {
            for leftover in pay_options(state.mana.0, cost) {
                let mut next = state;
                next.mana = ManaPool(leftover);
                if next.put_commander_on_battlefield(false, true) {
                    next.commander.cast_count = next.commander.cast_count.saturating_add(1);
                    next.commander.tax = next.commander.cast_count.saturating_mul(2);
                    out.push(InformationTransition::Deterministic(next));
                }
            }
        }
    }

    fn generate_engine_casts(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        if !self.resource_microsteps {
            return;
        }
        for slot in state.hand.iter() {
            let Some(OpeningCard::Engine { cost, .. }) = self.card(slot) else {
                continue;
            };
            for leftover in pay_options(state.mana.0, *cost) {
                let mut next = state;
                next.mana = ManaPool(leftover);
                if next.move_card_to_battlefield(
                    slot,
                    Zone::Hand,
                    PermanentInstance::new(PermanentSource::card(slot)),
                ) {
                    out.push(InformationTransition::Deterministic(next));
                }
            }
        }
    }

    fn payment_sources(&self, state: PackedStateV2) -> Vec<ManaSource> {
        let mut sources = Vec::new();
        for permanent in state.battlefield.as_slice() {
            if permanent.tapped() {
                continue;
            }
            let Some(slot) = permanent.source().card_slot() else {
                continue;
            };
            if let Some(OpeningCard::Land(land)) = self.card(slot) {
                let mut options: SmallVec<[ManaOption; 5]> = land
                    .mana
                    .iter()
                    .copied()
                    .map(|mana| {
                        let resource_use =
                            if matches!(land.profile.kind, OpeningLandKind::GemstoneMine)
                                && permanent.counters() <= 1
                            {
                                ResourceUse::Sacrifice
                            } else {
                                ResourceUse::Tap
                            };
                        ManaOption::new(mana, resource_use)
                    })
                    .collect();
                if matches!(land.profile.kind, OpeningLandKind::GemstoneCaverns)
                    && permanent.counters() > 0
                {
                    options = mana_options(0b1_1111, 0)
                        .into_iter()
                        .map(|mana| ManaOption::new(mana, ResourceUse::Tap))
                        .collect();
                }
                if matches!(land.profile.kind, OpeningLandKind::CrystalVein) {
                    options.push(ManaOption::new(
                        ManaPool([0, 0, 0, 0, 0, 2]),
                        ResourceUse::Sacrifice,
                    ));
                }
                if state.flags & RAIN_ACTIVE != 0 {
                    let tap_options: SmallVec<[ManaOption; 5]> = options
                        .iter()
                        .copied()
                        .filter(|option| matches!(option.resource_use, ResourceUse::Tap))
                        .collect();
                    options.push(ManaOption::new(
                        ManaPool([1, 0, 0, 0, 0, 0]),
                        ResourceUse::Sacrifice,
                    ));
                    if !matches!(land.profile.kind, OpeningLandKind::GemstoneMine)
                        || permanent.counters() > 1
                    {
                        for option in tap_options {
                            options.push(ManaOption::new(
                                option.mana.add_capped(ManaPool([1, 0, 0, 0, 0, 0]), 15),
                                ResourceUse::Sacrifice,
                            ));
                        }
                    }
                }
                sources.push(ManaSource::with_options(slot, options));
                continue;
            }
            match self.creature_kind(slot) {
                Some(OpeningCreatureKind::BirdsOfParadise) if !permanent.fresh() => {
                    sources.push(ManaSource::new(slot, mana_options(0b1_1111, 0)));
                    continue;
                }
                Some(OpeningCreatureKind::TinderWall) => {
                    sources.push(ManaSource::with_options(
                        slot,
                        [ManaOption::new(
                            ManaPool([0, 2, 0, 0, 0, 0]),
                            ResourceUse::Sacrifice,
                        )],
                    ));
                    continue;
                }
                Some(_) | None => {}
            }
            let Some(kind) = self.artifact_kind(slot) else {
                continue;
            };
            let (mana, resource_use) = match kind {
                OpeningArtifactKind::LotusPetal => {
                    (mana_options(0b1_1111, 0), ResourceUse::Sacrifice)
                }
                OpeningArtifactKind::ChromeMox => {
                    (mana_options(permanent.counters(), 0), ResourceUse::Tap)
                }
                OpeningArtifactKind::MoxDiamond => (mana_options(0b1_1111, 0), ResourceUse::Tap),
                OpeningArtifactKind::MoxOpal if self.artifact_count(state) >= 3 => {
                    (mana_options(0b1_1111, 0), ResourceUse::Tap)
                }
                OpeningArtifactKind::MoxAmber
                    if state.commander.zone == CommanderZone::Battlefield =>
                {
                    (mana_options(0b1_1111, 0), ResourceUse::Tap)
                }
                OpeningArtifactKind::SolRing => (
                    smallvec::smallvec![ManaPool([0, 0, 0, 0, 0, 2])],
                    ResourceUse::Tap,
                ),
                OpeningArtifactKind::ManaVault => (
                    smallvec::smallvec![ManaPool([0, 0, 0, 0, 0, 3])],
                    ResourceUse::Tap,
                ),
                _ => continue,
            };
            sources.push(ManaSource::with_options(
                slot,
                mana.into_iter()
                    .map(|produced| ManaOption::new(produced, resource_use)),
            ));
        }
        for slot in state.hand.iter() {
            let produced = match self.spell_kind(slot) {
                Some(OpeningSpellKind::ElvishSpiritGuide) => Some(ManaPool([0, 0, 0, 0, 1, 0])),
                Some(OpeningSpellKind::SimianSpiritGuide) => Some(ManaPool([0, 1, 0, 0, 0, 0])),
                _ => None,
            };
            if let Some(mana) = produced {
                sources.push(ManaSource::with_options(
                    slot,
                    [ManaOption::new(mana, ResourceUse::Exile)],
                ));
            }
        }
        sources
    }

    fn payment_plans(&self, state: PackedStateV2, cost: Cost) -> Vec<PaymentPlan> {
        let key = PaymentCacheKey {
            model_id: self.cache_id,
            state,
            cost,
        };
        PAYMENT_CACHE.with(|cache| {
            if let Some(plans) = cache.borrow().get(&key) {
                return plans.clone();
            }
            let plans = compute_payment_plans(state.mana, &self.payment_sources(state), cost);
            let mut cache = cache.borrow_mut();
            if cache.len() >= PAYMENT_CACHE_CAPACITY {
                cache.clear();
            }
            cache.insert(key, plans.clone());
            plans
        })
    }

    fn apply_payment_plan(&self, state: PackedStateV2, plan: PaymentPlan) -> Option<PackedStateV2> {
        let mut next = state;
        for slot in plan.consumption.tapped.iter() {
            let permanent = *next
                .battlefield
                .as_slice()
                .iter()
                .find(|permanent| permanent.source() == PermanentSource::card(slot))?;
            if permanent.tapped() {
                return None;
            }
            let replacement = if matches!(self.land_kind(slot), Some(OpeningLandKind::GemstoneMine))
            {
                permanent
                    .with_tapped(true)
                    .with_counters(permanent.counters().saturating_sub(1))
            } else {
                permanent.with_tapped(true)
            };
            next.battlefield.replace(permanent, replacement);
        }
        for slot in plan.consumption.sacrificed.iter() {
            if !next.move_card_from_battlefield(slot, Zone::Graveyard) {
                return None;
            }
        }
        for slot in plan.consumption.exiled.iter() {
            if !next.move_card(slot, Zone::Hand, Zone::Exile) {
                return None;
            }
        }
        next.mana = plan.leftover;
        next.is_valid().then_some(next)
    }

    fn generate_direct_engine_casts(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for slot in state.hand.iter() {
            let Some(OpeningCard::Engine { cost, .. }) = self.card(slot) else {
                continue;
            };
            for plan in self.payment_plans(state, *cost) {
                let Some(mut next) = self.apply_payment_plan(state, plan) else {
                    continue;
                };
                if next.move_card_to_battlefield(
                    slot,
                    Zone::Hand,
                    PermanentInstance::new(PermanentSource::card(slot)),
                ) {
                    out.push(InformationTransition::Deterministic(next));
                }
            }
        }
    }

    fn generate_rituals(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for slot in state.hand.iter() {
            let (cost, produced) = match self.spell_kind(slot) {
                Some(OpeningSpellKind::DarkRitual) => {
                    ([0, 1, 0, 0, 0, 0], ManaPool([3, 0, 0, 0, 0, 0]))
                }
                Some(OpeningSpellKind::RiteOfFlame) => {
                    ([0, 0, 1, 0, 0, 0], ManaPool([0, 2, 0, 0, 0, 0]))
                }
                _ => continue,
            };
            if state.flags & PRETURN_WINDOW != 0
                && !matches!(self.spell_kind(slot), Some(OpeningSpellKind::DarkRitual))
            {
                continue;
            }
            for plan in self.payment_plans(state, cost) {
                let Some(mut next) = self.apply_payment_plan(state, plan) else {
                    continue;
                };
                if next.move_card(slot, Zone::Hand, Zone::Graveyard) {
                    next.mana = next.mana.add_capped(produced, 15);
                    out.push(InformationTransition::Deterministic(next));
                }
            }
        }
    }

    fn offer_bait_cost(&self, state: PackedStateV2, slot: SlotId) -> Option<Cost> {
        match self.card(slot) {
            Some(OpeningCard::Artifact(
                OpeningArtifactKind::LotusPetal
                | OpeningArtifactKind::LionsEyeDiamond
                | OpeningArtifactKind::ChromeMox
                | OpeningArtifactKind::MoxDiamond
                | OpeningArtifactKind::MoxOpal
                | OpeningArtifactKind::MoxAmber
                | OpeningArtifactKind::ParadiseMantle,
            )) => Some([0, 0, 0, 0, 0, 0]),
            Some(OpeningCard::Artifact(
                OpeningArtifactKind::SolRing | OpeningArtifactKind::ManaVault,
            )) => Some([1, 0, 0, 0, 0, 0]),
            Some(OpeningCard::Spell(OpeningSpellKind::SummonersPact)) => Some([0, 0, 0, 0, 0, 0]),
            Some(OpeningCard::Spell(OpeningSpellKind::NoxiousRevival))
                if !state.graveyard.is_empty() =>
            {
                Some([0, 0, 0, 0, 0, 0])
            }
            Some(OpeningCard::Spell(OpeningSpellKind::DarkRitual))
            | Some(OpeningCard::Spell(OpeningSpellKind::ImperialSeal))
            | Some(OpeningCard::Spell(OpeningSpellKind::VampiricTutor))
            | Some(OpeningCard::Spell(OpeningSpellKind::SchemingSymmetry)) => {
                Some([0, 1, 0, 0, 0, 0])
            }
            Some(OpeningCard::Spell(OpeningSpellKind::RiteOfFlame)) => Some([0, 0, 1, 0, 0, 0]),
            Some(OpeningCard::Spell(OpeningSpellKind::EnlightenedTutor)) => {
                Some([0, 0, 0, 0, 1, 0])
            }
            Some(OpeningCard::Spell(OpeningSpellKind::MysticalTutor)) => Some([0, 0, 0, 1, 0, 0]),
            Some(OpeningCard::Spell(OpeningSpellKind::GreenSunsZenith)) => Some([0, 0, 0, 0, 0, 1]),
            _ => None,
        }
    }

    fn offer_bait_is_instant(&self, slot: SlotId) -> bool {
        matches!(
            self.spell_kind(slot),
            Some(
                OpeningSpellKind::SummonersPact
                    | OpeningSpellKind::NoxiousRevival
                    | OpeningSpellKind::DarkRitual
                    | OpeningSpellKind::VampiricTutor
                    | OpeningSpellKind::EnlightenedTutor
                    | OpeningSpellKind::MysticalTutor
            )
        )
    }

    fn generate_offer(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for offer in state.hand.iter().filter(|slot| {
            matches!(
                self.spell_kind(*slot),
                Some(OpeningSpellKind::AnOfferYouCantRefuse)
            )
        }) {
            for bait in state.hand.iter().filter(|slot| *slot != offer) {
                if state.flags & PRETURN_WINDOW != 0 && !self.offer_bait_is_instant(bait) {
                    continue;
                }
                let Some(bait_cost) = self.offer_bait_cost(state, bait) else {
                    continue;
                };
                for bait_plan in self.payment_plans(state, bait_cost) {
                    let Some(mut bait_cast) = self.apply_payment_plan(state, bait_plan) else {
                        continue;
                    };
                    if !bait_cast.move_card(bait, Zone::Hand, Zone::Graveyard) {
                        continue;
                    }
                    for offer_plan in self.payment_plans(bait_cast, [0, 0, 0, 1, 0, 0]) {
                        let Some(mut next) = self.apply_payment_plan(bait_cast, offer_plan) else {
                            continue;
                        };
                        if next.move_card(offer, Zone::Hand, Zone::Graveyard)
                            && next.add_token(TokenKind::Treasure)
                            && next.add_token(TokenKind::Treasure)
                        {
                            out.push(InformationTransition::Deterministic(next));
                        }
                    }
                }
            }
        }
    }

    fn generate_tutors(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for tutor in state.hand.iter() {
            let Some(kind) = self.spell_kind(tutor) else {
                continue;
            };
            if state.flags & PRETURN_WINDOW != 0
                && !matches!(
                    kind,
                    OpeningSpellKind::VampiricTutor
                        | OpeningSpellKind::EnlightenedTutor
                        | OpeningSpellKind::MysticalTutor
                )
            {
                continue;
            }
            let (cost, to_hand) = match kind {
                OpeningSpellKind::DemonicTutor => ([1, 1, 0, 0, 0, 0], true),
                OpeningSpellKind::ImperialSeal
                | OpeningSpellKind::VampiricTutor
                | OpeningSpellKind::SchemingSymmetry => ([0, 1, 0, 0, 0, 0], false),
                OpeningSpellKind::EnlightenedTutor => ([0, 0, 0, 0, 1, 0], false),
                OpeningSpellKind::MysticalTutor => ([0, 0, 0, 1, 0, 0], false),
                _ => continue,
            };
            let plans = self.payment_plans(state, cost);
            for target in state.library.cards().iter() {
                if matches!(kind, OpeningSpellKind::EnlightenedTutor)
                    && !self.card_flags[target as usize].contains(CardFlags::ARTIFACT)
                    && !self.card_flags[target as usize].contains(CardFlags::ENCHANTMENT)
                {
                    continue;
                }
                if matches!(kind, OpeningSpellKind::MysticalTutor)
                    && !matches!(
                        self.card(target),
                        Some(OpeningCard::Spell(
                            OpeningSpellKind::DarkRitual
                                | OpeningSpellKind::RiteOfFlame
                                | OpeningSpellKind::DemonicTutor
                                | OpeningSpellKind::ImperialSeal
                                | OpeningSpellKind::VampiricTutor
                                | OpeningSpellKind::EnlightenedTutor
                                | OpeningSpellKind::SchemingSymmetry
                                | OpeningSpellKind::Manamorphose
                                | OpeningSpellKind::Gamble
                                | OpeningSpellKind::NoxiousRevival
                                | OpeningSpellKind::GreenSunsZenith
                                | OpeningSpellKind::SummonersPact
                                | OpeningSpellKind::CropRotation
                                | OpeningSpellKind::CullingTheWeak
                                | OpeningSpellKind::DiabolicIntent
                                | OpeningSpellKind::InfernalPlunge
                                | OpeningSpellKind::RainOfFilth
                                | OpeningSpellKind::MysticalTutor
                                | OpeningSpellKind::EldritchEvolution
                        ))
                    )
                {
                    continue;
                }
                for plan in &plans {
                    let Some(mut next) = self.apply_payment_plan(state, *plan) else {
                        continue;
                    };
                    if !next.move_card(tutor, Zone::Hand, Zone::Graveyard)
                        || !next.library.remove_known_or_unknown(target)
                    {
                        continue;
                    }
                    next.library.shuffle_all_unknown();
                    if to_hand {
                        next.hand.insert(target);
                    } else {
                        next.library.push_known_top(target);
                    }
                    out.push(InformationTransition::Deterministic(next));
                }
            }
        }
    }

    fn generate_wishclaw_tutors(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for permanent in state.battlefield.as_slice() {
            let Some(slot) = permanent.source().card_slot() else {
                continue;
            };
            if permanent.tapped()
                || permanent.counters() == 0
                || !matches!(
                    self.artifact_kind(slot),
                    Some(OpeningArtifactKind::WishclawTalisman)
                )
            {
                continue;
            }
            let plans = self.payment_plans(state, [1, 0, 0, 0, 0, 0]);
            for target in state.library.cards().iter() {
                for plan in &plans {
                    let Some(mut next) = self.apply_payment_plan(state, *plan) else {
                        continue;
                    };
                    let Some(current) = next
                        .battlefield
                        .as_slice()
                        .iter()
                        .find(|item| item.source() == permanent.source())
                        .copied()
                    else {
                        continue;
                    };
                    if !next.library.remove_known_or_unknown(target) {
                        continue;
                    }
                    next.library.shuffle_all_unknown();
                    next.hand.insert(target);
                    next.battlefield
                        .replace(current, current.with_tapped(true).with_counters(0));
                    out.push(InformationTransition::Deterministic(next));
                }
            }
        }
    }

    fn generate_manamorphose(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for slot in state
            .hand
            .iter()
            .filter(|slot| matches!(self.spell_kind(*slot), Some(OpeningSpellKind::Manamorphose)))
        {
            let mut plans = self.payment_plans(state, [1, 0, 1, 0, 0, 0]);
            plans.extend(self.payment_plans(state, [1, 0, 0, 0, 0, 1]));
            plans.sort_by_key(|plan| {
                (
                    plan.consumption.tapped.bits(),
                    plan.consumption.sacrificed.bits(),
                    plan.consumption.exiled.bits(),
                    plan.leftover,
                )
            });
            plans.dedup();
            for plan in plans {
                let Some(mut paid) = self.apply_payment_plan(state, plan) else {
                    continue;
                };
                if !paid.move_card(slot, Zone::Hand, Zone::Graveyard) {
                    continue;
                }
                for left_color in 0..5 {
                    for right_color in left_color..5 {
                        let mut next = paid;
                        let mut produced = [0; 6];
                        produced[left_color] += 1;
                        produced[right_color] += 1;
                        next.mana = next.mana.add_capped(ManaPool(produced), 15);
                        let draws = self.chance_draws(next);
                        if draws.is_empty() {
                            out.push(InformationTransition::Deterministic(next));
                            continue;
                        }
                        let outcomes = draws
                            .into_iter()
                            .filter_map(|(slot, probability)| {
                                let mut drawn = next;
                                drawn.draw(slot).then_some((drawn, probability))
                            })
                            .collect();
                        out.push(InformationTransition::Chance(outcomes));
                    }
                }
            }
        }
    }

    fn generate_gamble(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        if state.flags & PRETURN_WINDOW != 0 {
            return;
        }
        for gamble in state
            .hand
            .iter()
            .filter(|slot| matches!(self.spell_kind(*slot), Some(OpeningSpellKind::Gamble)))
        {
            let plans = self.payment_plans(state, [0, 0, 1, 0, 0, 0]);
            for target in state.library.cards().iter() {
                for plan in &plans {
                    let Some(mut searched) = self.apply_payment_plan(state, *plan) else {
                        continue;
                    };
                    if !searched.move_card(gamble, Zone::Hand, Zone::Graveyard)
                        || !searched.library.remove_known_or_unknown(target)
                    {
                        continue;
                    }
                    searched.library.shuffle_all_unknown();
                    searched.hand.insert(target);
                    let denominator = searched.hand.len() as f64;
                    let outcomes = searched
                        .hand
                        .iter()
                        .filter_map(|discard| {
                            let mut next = searched;
                            next.move_card(discard, Zone::Hand, Zone::Graveyard)
                                .then_some((next, 1.0 / denominator))
                        })
                        .collect();
                    out.push(InformationTransition::Chance(outcomes));
                }
            }
        }
    }

    fn generate_noxious_revival(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for noxious in state.hand.iter().filter(|slot| {
            matches!(
                self.spell_kind(*slot),
                Some(OpeningSpellKind::NoxiousRevival)
            )
        }) {
            for target in state.graveyard.iter() {
                let mut next = state;
                if !next.move_card(noxious, Zone::Hand, Zone::Graveyard)
                    || !next.graveyard.remove(target)
                {
                    continue;
                }
                next.library.push_known_top(target);
                out.push(InformationTransition::Deterministic(next));
            }
        }
    }

    fn generate_demonic_led_tutors(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        let leds: SmallVec<[SlotId; 2]> = state
            .battlefield
            .as_slice()
            .iter()
            .filter(|permanent| !permanent.tapped())
            .filter_map(|permanent| permanent.source().card_slot())
            .filter(|slot| {
                matches!(
                    self.artifact_kind(*slot),
                    Some(OpeningArtifactKind::LionsEyeDiamond)
                )
            })
            .collect();
        if leds.is_empty() {
            return;
        }
        for tutor in state
            .hand
            .iter()
            .filter(|slot| matches!(self.spell_kind(*slot), Some(OpeningSpellKind::DemonicTutor)))
        {
            let plans = self.payment_plans(state, [1, 1, 0, 0, 0, 0]);
            for led in &leds {
                for target in state.library.cards().iter() {
                    for plan in &plans {
                        let Some(mut paid) = self.apply_payment_plan(state, *plan) else {
                            continue;
                        };
                        if !paid.move_card(tutor, Zone::Hand, Zone::Graveyard)
                            || !paid.move_card_from_battlefield(*led, Zone::Graveyard)
                        {
                            continue;
                        }
                        let hand: SmallVec<[SlotId; 16]> = paid.hand.iter().collect();
                        for card in hand {
                            paid.move_card(card, Zone::Hand, Zone::Graveyard);
                        }
                        for color in 0..5 {
                            let mut next = paid;
                            let mut produced = [0; 6];
                            produced[color] = 3;
                            next.mana = next.mana.add_capped(ManaPool(produced), 15);
                            if !next.library.remove_known_or_unknown(target) {
                                continue;
                            }
                            next.library.shuffle_all_unknown();
                            next.hand.insert(target);
                            out.push(InformationTransition::Deterministic(next));
                        }
                    }
                }
            }
        }
    }

    fn generate_green_engine_tutors(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for tutor in state.hand.iter() {
            match self.spell_kind(tutor) {
                Some(OpeningSpellKind::GreenSunsZenith) if state.flags & PRETURN_WINDOW == 0 => {
                    let plans = self.payment_plans(state, [3, 0, 0, 0, 0, 1]);
                    for target in state.library.cards().iter().filter(|target| {
                        matches!(self.card(*target), Some(OpeningCard::Engine { .. }))
                            && self.card_colors[*target as usize] & (1 << 4) != 0
                    }) {
                        for plan in &plans {
                            let Some(mut next) = self.apply_payment_plan(state, *plan) else {
                                continue;
                            };
                            if !next.hand.remove(tutor)
                                || !next.library.remove_known_or_unknown(target)
                                || !next.library.insert_unknown(tutor)
                            {
                                continue;
                            }
                            next.library.shuffle_all_unknown();
                            if next
                                .battlefield
                                .insert(PermanentInstance::new(PermanentSource::card(target)))
                            {
                                out.push(InformationTransition::Deterministic(next));
                            }
                        }
                    }
                }
                Some(OpeningSpellKind::SummonersPact) => {
                    for target in state.library.cards().iter().filter(|target| {
                        self.card_flags[*target as usize].contains(CardFlags::CREATURE)
                            && self.card_colors[*target as usize] & (1 << 4) != 0
                    }) {
                        let mut next = state;
                        if !next.move_card(tutor, Zone::Hand, Zone::Exile)
                            || !next.library.remove_known_or_unknown(target)
                        {
                            continue;
                        }
                        next.library.shuffle_all_unknown();
                        next.hand.insert(target);
                        next.flags |= PACT_DUE;
                        out.push(InformationTransition::Deterministic(next));
                    }
                }
                _ => {}
            }
        }
    }

    fn can_pay_pact_next_upkeep(&self, state: PackedStateV2) -> bool {
        let mut upkeep = state;
        upkeep.mana = ManaPool::default();
        let battlefield = upkeep.battlefield;
        for permanent in battlefield.as_slice() {
            let stays_tapped = permanent.source().card_slot().is_some_and(|slot| {
                matches!(
                    self.artifact_kind(slot),
                    Some(OpeningArtifactKind::ManaVault)
                )
            }) && permanent.tapped();
            upkeep.battlefield.replace(
                *permanent,
                permanent.with_tapped(stays_tapped).with_fresh(false),
            );
        }
        if !self.payment_plans(upkeep, [2, 0, 0, 0, 0, 2]).is_empty() {
            return true;
        }
        self.angels_grace_slot.is_some_and(|grace| {
            upkeep.hand.contains(grace)
                && !self.payment_plans(upkeep, [0, 0, 0, 0, 1, 0]).is_empty()
        })
    }

    fn sacrifice_creature(&self, state: &mut PackedStateV2, source: PermanentSource) -> bool {
        if source.is_commander() {
            state.return_commander_to_command_zone()
        } else if let Some(slot) = source.card_slot() {
            self.card_flags[slot as usize].contains(CardFlags::CREATURE)
                && state.move_card_from_battlefield(slot, Zone::Graveyard)
        } else {
            matches!(source.token_kind(), Some(TokenKind::GenericCreature))
                && state.remove_token(TokenKind::GenericCreature)
        }
    }

    fn creature_sources(&self, state: PackedStateV2) -> SmallVec<[PermanentSource; 8]> {
        state
            .battlefield
            .as_slice()
            .iter()
            .filter_map(|permanent| {
                let source = permanent.source();
                (source.is_commander()
                    || source.card_slot().is_some_and(|slot| {
                        self.card_flags[slot as usize].contains(CardFlags::CREATURE)
                    })
                    || matches!(source.token_kind(), Some(TokenKind::GenericCreature)))
                .then_some(source)
            })
            .collect()
    }

    fn generate_sacrifice_spells(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for spell in state.hand.iter() {
            let kind = self.spell_kind(spell);
            if matches!(kind, Some(OpeningSpellKind::RainOfFilth)) {
                for plan in self.payment_plans(state, [0, 1, 0, 0, 0, 0]) {
                    let Some(mut next) = self.apply_payment_plan(state, plan) else {
                        continue;
                    };
                    if next.move_card(spell, Zone::Hand, Zone::Graveyard) {
                        next.flags |= RAIN_ACTIVE;
                        out.push(InformationTransition::Deterministic(next));
                    }
                }
                continue;
            }
            let (cost, produced) = match kind {
                Some(OpeningSpellKind::CullingTheWeak) => {
                    ([0, 1, 0, 0, 0, 0], Some(ManaPool([4, 0, 0, 0, 0, 0])))
                }
                Some(OpeningSpellKind::InfernalPlunge) if state.flags & PRETURN_WINDOW == 0 => {
                    ([0, 0, 1, 0, 0, 0], Some(ManaPool([0, 3, 0, 0, 0, 0])))
                }
                Some(OpeningSpellKind::DiabolicIntent) if state.flags & PRETURN_WINDOW == 0 => {
                    ([1, 1, 0, 0, 0, 0], None)
                }
                _ => continue,
            };
            let plans = self.payment_plans(state, cost);
            for creature in self.creature_sources(state) {
                for plan in &plans {
                    let Some(mut paid) = self.apply_payment_plan(state, *plan) else {
                        continue;
                    };
                    if !self.sacrifice_creature(&mut paid, creature)
                        || !paid.move_card(spell, Zone::Hand, Zone::Graveyard)
                    {
                        continue;
                    }
                    if let Some(mana) = produced {
                        paid.mana = paid.mana.add_capped(mana, 15);
                        out.push(InformationTransition::Deterministic(paid));
                        continue;
                    }
                    for target in state.library.cards().iter() {
                        let mut next = paid;
                        if !next.library.remove_known_or_unknown(target) {
                            continue;
                        }
                        next.library.shuffle_all_unknown();
                        next.hand.insert(target);
                        out.push(InformationTransition::Deterministic(next));
                    }
                }
            }
        }
    }

    fn generate_crop_rotation(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for crop in state
            .hand
            .iter()
            .filter(|slot| matches!(self.spell_kind(*slot), Some(OpeningSpellKind::CropRotation)))
        {
            let lands: SmallVec<[SlotId; 8]> = state
                .battlefield
                .as_slice()
                .iter()
                .filter_map(|permanent| permanent.source().card_slot())
                .filter(|slot| self.land_kind(*slot).is_some())
                .collect();
            let plans = self.payment_plans(state, [0, 0, 0, 0, 0, 1]);
            for sacrificed in &lands {
                for target in state.library.cards().iter() {
                    let Some(OpeningCard::Land(target_land)) = self.card(target) else {
                        continue;
                    };
                    for plan in &plans {
                        let Some(mut next) = self.apply_payment_plan(state, *plan) else {
                            continue;
                        };
                        if !next.move_card_from_battlefield(*sacrificed, Zone::Graveyard)
                            || !next.move_card(crop, Zone::Hand, Zone::Graveyard)
                            || !next.library.remove_known_or_unknown(target)
                        {
                            continue;
                        }
                        next.library.shuffle_all_unknown();
                        let counters = u8::from(matches!(
                            target_land.profile.kind,
                            OpeningLandKind::GemstoneMine
                        )) * 3;
                        let permanent = PermanentInstance::new(PermanentSource::card(target))
                            .with_tapped(target_land.profile.enters_tapped)
                            .with_counters(counters);
                        if next.battlefield.insert(permanent) {
                            out.push(InformationTransition::Deterministic(next));
                        }
                    }
                }
            }
        }
    }

    fn generate_creature_casts(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        if state.flags & PRETURN_WINDOW != 0 {
            return;
        }
        for slot in state.hand.iter() {
            let costs: SmallVec<[Cost; 2]> = match self.creature_kind(slot) {
                Some(OpeningCreatureKind::BirdsOfParadise)
                | Some(OpeningCreatureKind::TinderWall) => {
                    smallvec::smallvec![[0, 0, 0, 0, 0, 1]]
                }
                Some(OpeningCreatureKind::DeathriteShaman) => {
                    smallvec::smallvec![[0, 1, 0, 0, 0, 0], [0, 0, 0, 0, 0, 1]]
                }
                Some(OpeningCreatureKind::Ragavan) => {
                    smallvec::smallvec![[0, 0, 1, 0, 0, 0]]
                }
                Some(OpeningCreatureKind::EsperSentinel) => {
                    smallvec::smallvec![[0, 0, 0, 0, 1, 0]]
                }
                Some(OpeningCreatureKind::RangerCaptainOfEos) => continue,
                _ => continue,
            };
            for cost in costs {
                for plan in self.payment_plans(state, cost) {
                    let Some(mut next) = self.apply_payment_plan(state, plan) else {
                        continue;
                    };
                    if next.move_card_to_battlefield(
                        slot,
                        Zone::Hand,
                        PermanentInstance::new(PermanentSource::card(slot)).with_fresh(true),
                    ) {
                        out.push(InformationTransition::Deterministic(next));
                    }
                }
            }
        }
    }

    fn generate_ranger_captain(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        if state.flags & PRETURN_WINDOW != 0 {
            return;
        }
        for ranger in state.hand.iter().filter(|slot| {
            matches!(
                self.creature_kind(*slot),
                Some(OpeningCreatureKind::RangerCaptainOfEos)
            )
        }) {
            for plan in self.payment_plans(state, [1, 0, 0, 0, 2, 0]) {
                let Some(mut cast) = self.apply_payment_plan(state, plan) else {
                    continue;
                };
                if !cast.move_card_to_battlefield(
                    ranger,
                    Zone::Hand,
                    PermanentInstance::new(PermanentSource::card(ranger)).with_fresh(true),
                ) {
                    continue;
                }
                out.push(InformationTransition::Deterministic(cast));
                for esper in state.library.cards().iter().filter(|slot| {
                    matches!(
                        self.creature_kind(*slot),
                        Some(OpeningCreatureKind::EsperSentinel)
                    )
                }) {
                    let mut searched = cast;
                    if searched.library.remove_known_or_unknown(esper) {
                        searched.library.shuffle_all_unknown();
                        searched.hand.insert(esper);
                        out.push(InformationTransition::Deterministic(searched));
                    }
                }
            }
        }
    }

    fn generate_eldritch_evolution(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        if state.flags & PRETURN_WINDOW != 0 {
            return;
        }
        for evolution in state.hand.iter().filter(|slot| {
            matches!(
                self.spell_kind(*slot),
                Some(OpeningSpellKind::EldritchEvolution)
            )
        }) {
            let plans = self.payment_plans(state, [1, 0, 0, 0, 0, 2]);
            for creature in self.creature_sources(state) {
                for target in state.library.cards().iter().filter(|target| {
                    matches!(self.card(*target), Some(OpeningCard::Engine { .. }))
                        && self.card_colors[*target as usize] & (1 << 4) != 0
                }) {
                    for plan in &plans {
                        let Some(mut next) = self.apply_payment_plan(state, *plan) else {
                            continue;
                        };
                        if !self.sacrifice_creature(&mut next, creature)
                            || !next.move_card(evolution, Zone::Hand, Zone::Exile)
                            || !next.library.remove_known_or_unknown(target)
                        {
                            continue;
                        }
                        next.library.shuffle_all_unknown();
                        if next
                            .battlefield
                            .insert(PermanentInstance::new(PermanentSource::card(target)))
                        {
                            out.push(InformationTransition::Deterministic(next));
                        }
                    }
                }
            }
        }
    }

    fn generate_deathrite_mana(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        let grave_lands: SmallVec<[SlotId; 8]> = state
            .graveyard
            .iter()
            .filter(|slot| self.card_flags[*slot as usize].contains(CardFlags::LAND))
            .collect();
        for permanent in state.battlefield.as_slice() {
            let Some(slot) = permanent.source().card_slot() else {
                continue;
            };
            if permanent.tapped()
                || permanent.fresh()
                || !matches!(
                    self.creature_kind(slot),
                    Some(OpeningCreatureKind::DeathriteShaman)
                )
            {
                continue;
            }
            if self.deathrite_external_land {
                for produced in mana_options(0b1_1111, 0) {
                    let mut next = state;
                    next.mana = next.mana.add_capped(produced, 15);
                    next.battlefield
                        .replace(*permanent, permanent.with_tapped(true));
                    out.push(InformationTransition::Deterministic(next));
                }
            }
            for land in &grave_lands {
                for produced in mana_options(0b1_1111, 0) {
                    let mut next = state;
                    next.mana = next.mana.add_capped(produced, 15);
                    next.battlefield
                        .replace(*permanent, permanent.with_tapped(true));
                    if next.move_card(*land, Zone::Graveyard, Zone::Exile) {
                        out.push(InformationTransition::Deterministic(next));
                    }
                }
            }
        }
    }

    fn generate_ragavan_attacks(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        for permanent in state.battlefield.as_slice() {
            let Some(slot) = permanent.source().card_slot() else {
                continue;
            };
            if permanent.tapped()
                || permanent.fresh()
                || !matches!(self.creature_kind(slot), Some(OpeningCreatureKind::Ragavan))
            {
                continue;
            }
            let mut next = state;
            next.battlefield
                .replace(*permanent, permanent.with_tapped(true));
            if next.add_token(TokenKind::Treasure) {
                out.push(InformationTransition::Deterministic(next));
            }
        }
    }

    fn generate_treasure_mana(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        if !state
            .battlefield
            .contains_source(PermanentSource::token(TokenKind::Treasure))
        {
            return;
        }
        for produced in mana_options(0b1_1111, 0) {
            let mut next = state;
            if next.remove_token(TokenKind::Treasure) {
                next.mana = next.mana.add_capped(produced, 15);
                out.push(InformationTransition::Deterministic(next));
            }
        }
    }

    fn generate_end_turn(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        let turn = Self::turn(state);
        if turn >= self.max_turn {
            return;
        }
        let mut next = state;
        next.flags &= !(LAND_PLAYED | TURN_DRAW_DONE | RAIN_ACTIVE);
        next.mana = ManaPool::default();
        Self::set_turn(&mut next, turn + 1);
        if !self.has_artifact(next) {
            let glimmervoids: SmallVec<[SlotId; 2]> = next
                .battlefield
                .as_slice()
                .iter()
                .filter_map(|permanent| permanent.source().card_slot())
                .filter(|slot| matches!(self.land_kind(*slot), Some(OpeningLandKind::Glimmervoid)))
                .collect();
            for glimmervoid in glimmervoids {
                next.move_card_from_battlefield(glimmervoid, Zone::Graveyard);
            }
        }
        let battlefield = next.battlefield;
        for permanent in battlefield.as_slice() {
            let stays_tapped = permanent.source().card_slot().is_some_and(|slot| {
                matches!(
                    self.artifact_kind(slot),
                    Some(OpeningArtifactKind::ManaVault)
                )
            }) && permanent.tapped();
            next.battlefield.replace(
                *permanent,
                permanent.with_tapped(stays_tapped).with_fresh(false),
            );
        }

        if next.flags & PACT_DUE != 0 {
            for plan in self.payment_plans(next, [2, 0, 0, 0, 0, 2]) {
                if let Some(mut paid) = self.apply_payment_plan(next, plan) {
                    paid.flags &= !PACT_DUE;
                    out.push(InformationTransition::Deterministic(paid));
                }
            }
            if let Some(grace) = self
                .angels_grace_slot
                .filter(|grace| next.hand.contains(*grace))
            {
                for plan in self.payment_plans(next, [0, 0, 0, 0, 1, 0]) {
                    if let Some(mut paid) = self.apply_payment_plan(next, plan) {
                        if paid.move_card(grace, Zone::Hand, Zone::Graveyard) {
                            paid.flags &= !PACT_DUE;
                            out.push(InformationTransition::Deterministic(paid));
                        }
                    }
                }
            }
            return;
        }

        out.push(InformationTransition::Deterministic(next));
    }

    fn generate_turn_draw(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        let mut next = state;
        next.flags |= TURN_DRAW_DONE;
        let draws = self.chance_draws(next);
        if draws.is_empty() {
            out.push(InformationTransition::Deterministic(next));
            return;
        }
        let outcomes = draws
            .into_iter()
            .filter_map(|(slot, probability)| {
                let mut drawn = next;
                drawn.draw(slot).then_some((drawn, probability))
            })
            .collect();
        out.push(InformationTransition::Chance(outcomes));
    }
}

impl CompiledPolicy<PackedStateV2> for VisibleOpeningPolicy<'_> {
    fn choose(
        &self,
        _state: PackedStateV2,
        transitions: &[InformationTransition<PackedStateV2>],
    ) -> Option<usize> {
        transitions
            .iter()
            .enumerate()
            .map(|(index, transition)| {
                let score = match transition {
                    InformationTransition::Deterministic(next) => {
                        self.model.visible_policy_score(*next) as f64
                    }
                    InformationTransition::Chance(outcomes) => {
                        let total: f64 = outcomes.iter().map(|(_, probability)| probability).sum();
                        if total <= 0.0 {
                            f64::NEG_INFINITY
                        } else {
                            outcomes
                                .iter()
                                .map(|(next, probability)| {
                                    probability * self.model.visible_policy_score(*next) as f64
                                        / total
                                })
                                .sum()
                        }
                    }
                };
                (index, score)
            })
            .max_by(|left, right| {
                left.1
                    .total_cmp(&right.1)
                    .then_with(|| right.0.cmp(&left.0))
            })
            .map(|(index, _)| index)
    }
}

impl InformationModel for EngineOpeningModel {
    type State = PackedStateV2;

    fn terminal_value(&self, state: Self::State) -> Option<f64> {
        self.engine_value(state)
    }

    fn transitions(
        &self,
        state: Self::State,
        out: &mut SmallVec<[InformationTransition<Self::State>; 16]>,
    ) {
        if state.flags & PRETURN_WINDOW != 0 {
            self.generate_rituals(state, out);
            self.generate_offer(state, out);
            self.generate_tutors(state, out);
            self.generate_green_engine_tutors(state, out);
            self.generate_sacrifice_spells(state, out);
            self.generate_crop_rotation(state, out);
            self.generate_manamorphose(state, out);
            self.generate_noxious_revival(state, out);
            let mut pass = state;
            pass.flags &= !PRETURN_WINDOW;
            out.push(InformationTransition::Deterministic(pass));
            return;
        }
        if state.flags & TURN_DRAW_DONE == 0 {
            self.generate_turn_draw(state, out);
            return;
        }
        self.generate_land_plays(state, out);
        if self.resource_microsteps {
            self.generate_mana_activations(state, out);
        } else {
            self.generate_city_float(state, out);
        }
        self.generate_fetches(state, out);
        self.generate_artifact_casts(state, out);
        self.generate_rituals(state, out);
        self.generate_offer(state, out);
        self.generate_tutors(state, out);
        self.generate_green_engine_tutors(state, out);
        self.generate_sacrifice_spells(state, out);
        self.generate_crop_rotation(state, out);
        self.generate_ranger_captain(state, out);
        self.generate_creature_casts(state, out);
        self.generate_eldritch_evolution(state, out);
        self.generate_deathrite_mana(state, out);
        self.generate_ragavan_attacks(state, out);
        self.generate_treasure_mana(state, out);
        self.generate_demonic_led_tutors(state, out);
        self.generate_wishclaw_tutors(state, out);
        self.generate_manamorphose(state, out);
        self.generate_gamble(state, out);
        self.generate_noxious_revival(state, out);
        if self.resource_microsteps {
            self.generate_artifact_mana(state, out);
        }
        self.generate_mantle_equips(state, out);
        self.generate_mantle_mana(state, out);
        self.generate_commander_cast(state, out);
        if self.direct_payments {
            self.generate_direct_engine_casts(state, out);
        }
        self.generate_engine_casts(state, out);
        self.generate_end_turn(state, out);
    }
}

impl OpeningOutcomeModel for EngineOpeningModel {
    fn terminal_opening_outcome(&self, state: Self::State) -> Option<OpeningOutcome> {
        self.opening_outcome(state)
    }
}

fn land_semantics(profile: OpeningManaProfile) -> LandSemantics {
    LandSemantics {
        mana: mana_options(profile.color_mask, profile.colorless),
        profile,
    }
}

fn mana_options(color_mask: u8, colorless: u8) -> SmallVec<[ManaPool; 5]> {
    let mut mana = SmallVec::new();
    for color in 0..5 {
        if color_mask & (1 << color) != 0 {
            let mut produced = [0; 6];
            produced[color] = 1;
            mana.push(ManaPool(produced));
        }
    }
    if colorless != 0 {
        mana.push(ManaPool([0, 0, 0, 0, 0, colorless]));
    }
    mana
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nextgen::{OpeningOutcomeSolver, PackedLibrary, ReferenceSolver};

    fn model(names: &[&str]) -> EngineOpeningModel {
        let deck = DeckSpec::compile(
            &names
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>(),
        )
        .expect("opening-model fixture");
        EngineOpeningModel::compile(&deck, 2)
    }

    #[test]
    fn city_sacrifices_when_another_land_is_played() {
        let model = model(&["City of Traitors", "Command Tower"]);
        let mut state = PackedStateV2::default();
        state.flags |= TURN_DRAW_DONE;
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        state.hand.insert(1);
        let mut out = SmallVec::new();
        model.generate_land_plays(state, &mut out);

        let InformationTransition::Deterministic(next) = out[0] else {
            panic!("land play must be deterministic");
        };
        assert!(next.graveyard.contains(0));
        assert!(next.battlefield.contains_source(PermanentSource::card(1)));
    }

    #[test]
    fn crystal_vein_has_tap_and_sacrifice_activations() {
        let model = model(&["Crystal Vein"]);
        let mut state = PackedStateV2::default();
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        let mut out = SmallVec::new();
        model.generate_mana_activations(state, &mut out);

        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.mana == ManaPool([0, 0, 0, 0, 0, 1])
                    && next.battlefield.contains_source(PermanentSource::card(0))
        )));
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.mana == ManaPool([0, 0, 0, 0, 0, 2])
                    && next.graveyard.contains(0)
        )));
    }

    #[test]
    fn last_gemstone_mine_counter_moves_land_to_graveyard() {
        let model = model(&["Gemstone Mine"]);
        let mut state = PackedStateV2::default();
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)).with_counters(1));
        let mut out = SmallVec::new();
        model.generate_mana_activations(state, &mut out);

        assert_eq!(out.len(), 5);
        assert!(out.iter().all(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next) if next.graveyard.contains(0)
        )));
    }

    #[test]
    fn fetch_searches_typed_land_and_obscures_known_top() {
        let model = model(&[
            "Polluted Delta",
            "Underground Sea",
            "Command Tower",
            "Blank",
        ]);
        let mut state = PackedStateV2 {
            library: PackedLibrary::new([1, 2].into_iter().collect()),
            ..PackedStateV2::default()
        };
        state.library.push_known_top(3);
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        let mut out = SmallVec::new();
        model.generate_fetches(state, &mut out);

        assert_eq!(out.len(), 1);
        let InformationTransition::Deterministic(next) = out[0] else {
            panic!("fetch choice must be deterministic");
        };
        assert!(next.graveyard.contains(0));
        assert!(next.battlefield.contains_source(PermanentSource::card(1)));
        assert_eq!(next.library.known_top_len(), 0);
        assert_eq!(next.library.cards(), [2, 3].into_iter().collect());
    }

    #[test]
    fn glimmervoid_survives_only_with_an_artifact() {
        let model = model(&["Glimmervoid", "Lotus Petal"]);
        let mut state = PackedStateV2::default();
        state.flags |= TURN_DRAW_DONE;
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        let mut out = SmallVec::new();
        model.generate_end_turn(state, &mut out);
        let InformationTransition::Deterministic(without_artifact) = out[0] else {
            panic!("end turn must be deterministic");
        };
        assert!(without_artifact.graveyard.contains(0));

        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(1)));
        out.clear();
        model.generate_end_turn(state, &mut out);
        let InformationTransition::Deterministic(with_artifact) = out[0] else {
            panic!("end turn must be deterministic");
        };
        assert!(with_artifact
            .battlefield
            .contains_source(PermanentSource::card(0)));
    }

    #[test]
    fn live_caverns_enumerates_visible_exile_choices_and_makes_rainbow() {
        let model = model(&["Gemstone Caverns", "Rhystic Study", "Blank"]);
        let mut state = PackedStateV2::default();
        state.hand = [0, 1, 2].into_iter().collect();
        let starts = model.pregame_states(state, true);

        assert_eq!(starts.len(), 3);
        for start in starts.iter().skip(1) {
            let cavern = start
                .battlefield
                .as_slice()
                .iter()
                .find(|permanent| permanent.source() == PermanentSource::card(0))
                .expect("pregame Caverns");
            assert_eq!(cavern.counters(), 1);
            assert_eq!(start.exile.len(), 1);
            let mut out = SmallVec::new();
            model.generate_mana_activations(*start, &mut out);
            assert_eq!(out.len(), 5);
        }
    }

    #[test]
    fn opening_outcome_preserves_engine_identity_and_resolution_turn() {
        let model = model(&["Rhystic Study", "Heartwood Storyteller"]);
        let mut rhystic = PackedStateV2::default();
        rhystic
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        let turn_one = model.opening_outcome(rhystic).expect("terminal Rhystic");
        assert_eq!(turn_one.weighted_ev, 1.0);
        assert_eq!(turn_one.rhystic_turn_1, 1.0);
        assert_eq!(turn_one.any_engine(), 1.0);

        let mut heartwood = PackedStateV2::default();
        EngineOpeningModel::set_turn(&mut heartwood, 2);
        heartwood
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(1)));
        let turn_two = model
            .opening_outcome(heartwood)
            .expect("terminal Heartwood");
        assert_eq!(turn_two.weighted_ev, 0.55);
        assert_eq!(turn_two.heartwood_turn_2, 1.0);
        assert_eq!(turn_two.any_engine(), 1.0);
    }

    #[test]
    fn bounded_solver_reports_depth_truncation_instead_of_failure() {
        let model = model(&["Rhystic Study", "Blank"]);
        let state = PackedStateV2 {
            hand: [1].into_iter().collect(),
            library: PackedLibrary::new([0].into_iter().collect()),
            ..PackedStateV2::default()
        };
        let result = OpeningOutcomeSolver::new(&model).solve(state, 0);

        assert_eq!(result.lower_bound, 0.0);
        assert_eq!(result.upper_bound, 1.0);
        assert!(result.capped);
        assert_eq!(result.metrics.depth_cutoffs, 1);
    }

    #[test]
    fn mox_diamond_discards_only_a_true_land_card() {
        let model = model(&["Mox Diamond", "Command Tower", "Sink into Stupor"]);
        let mut state = PackedStateV2::default();
        state.hand = [0, 1, 2].into_iter().collect();
        let mut out = SmallVec::new();
        model.generate_artifact_casts(state, &mut out);

        assert_eq!(out.len(), 1);
        let InformationTransition::Deterministic(next) = out[0] else {
            panic!("Mox Diamond cast must be deterministic");
        };
        assert!(next.graveyard.contains(1));
        assert!(next.hand.contains(2));
        assert!(next.battlefield.contains_source(PermanentSource::card(0)));
    }

    #[test]
    fn chrome_mox_records_each_legal_imprint_color() {
        let model = model(&["Chrome Mox", "Rhystic Study", "Sol Ring"]);
        let mut state = PackedStateV2::default();
        state.hand = [0, 1, 2].into_iter().collect();
        let mut out = SmallVec::new();
        model.generate_artifact_casts(state, &mut out);

        assert_eq!(out.len(), 2, "no-imprint plus Rhystic imprint");
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.exile.contains(1)
                    && next.battlefield.as_slice().iter().any(|permanent|
                        permanent.source() == PermanentSource::card(0)
                            && permanent.counters() == 1 << 2)
        )));
    }

    #[test]
    fn mox_opal_requires_three_artifacts() {
        let model = model(&["Mox Opal", "Lotus Petal", "Paradise Mantle"]);
        let mut state = PackedStateV2::default();
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(1)));
        let mut out = SmallVec::new();
        model.generate_artifact_mana(state, &mut out);
        assert!(out.iter().all(|transition| !matches!(
            transition,
            InformationTransition::Deterministic(next) if next.battlefield.as_slice().iter().any(
                |permanent| permanent.source() == PermanentSource::card(0) && permanent.tapped()
            )
        )));

        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(2)));
        out.clear();
        model.generate_artifact_mana(state, &mut out);
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.mana == ManaPool([0, 0, 1, 0, 0, 0])
                    && next.battlefield.as_slice().iter().any(|permanent|
                        permanent.source() == PermanentSource::card(0) && permanent.tapped())
        )));
    }

    #[test]
    fn led_sacrifices_and_discards_while_petal_only_sacrifices() {
        let model = model(&["Lion's Eye Diamond", "Lotus Petal", "Rhystic Study"]);
        let mut state = PackedStateV2::default();
        state.hand.insert(2);
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(1)));
        let mut out = SmallVec::new();
        model.generate_artifact_mana(state, &mut out);

        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.mana == ManaPool([0, 0, 3, 0, 0, 0])
                    && next.graveyard.contains(0)
                    && next.graveyard.contains(2)
        )));
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.mana == ManaPool([0, 0, 1, 0, 0, 0])
                    && next.graveyard.contains(1)
                    && next.hand.contains(2)
        )));
    }

    #[test]
    fn mana_vault_does_not_untap_during_the_normal_untap_step() {
        let model = model(&["Mana Vault", "Sol Ring"]);
        let mut state = PackedStateV2::default();
        state.flags |= TURN_DRAW_DONE;
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)).with_tapped(true));
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(1)).with_tapped(true));
        let mut out = SmallVec::new();
        model.generate_end_turn(state, &mut out);
        let InformationTransition::Deterministic(next) = out[0] else {
            panic!("end turn must be deterministic");
        };
        assert!(next
            .battlefield
            .as_slice()
            .iter()
            .any(|permanent| permanent.source() == PermanentSource::card(0) && permanent.tapped()));
        assert!(
            next.battlefield
                .as_slice()
                .iter()
                .any(|permanent| permanent.source() == PermanentSource::card(1)
                    && !permanent.tapped())
        );
    }

    #[test]
    fn paradise_mantle_respects_summoning_sickness() {
        let model = model(&["Paradise Mantle"]);
        let mut state = PackedStateV2::default();
        state.mana = ManaPool([0, 0, 0, 0, 0, 1]);
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        assert!(state.put_commander_on_battlefield(false, false));
        let mut equips = SmallVec::new();
        model.generate_mantle_equips(state, &mut equips);
        let InformationTransition::Deterministic(equipped) = equips[0] else {
            panic!("equip must be deterministic");
        };
        let mut mana = SmallVec::new();
        model.generate_mantle_mana(equipped, &mut mana);
        assert_eq!(mana.len(), 5);

        let mut fresh = state;
        let commander = fresh
            .battlefield
            .remove_source(PermanentSource::commander())
            .expect("commander permanent");
        fresh.battlefield.insert(commander.with_fresh(true));
        equips.clear();
        model.generate_mantle_equips(fresh, &mut equips);
        let InformationTransition::Deterministic(equipped_fresh) = equips[0] else {
            panic!("equip must be deterministic");
        };
        mana.clear();
        model.generate_mantle_mana(equipped_fresh, &mut mana);
        assert!(mana.is_empty());
    }

    #[test]
    fn mox_amber_uses_a_cast_commander_color_identity() {
        let model = model(&["Mox Amber"]);
        let mut state = PackedStateV2::default();
        state.mana = ManaPool([0, 0, 0, 1, 0, 0]);
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        let mut casts = SmallVec::new();
        model.generate_commander_cast(state, &mut casts);
        let InformationTransition::Deterministic(with_commander) = casts[0] else {
            panic!("commander cast must be deterministic");
        };
        let mut mana = SmallVec::new();
        model.generate_artifact_mana(with_commander, &mut mana);
        assert_eq!(mana.len(), 5);
    }

    #[test]
    fn visible_policy_selects_an_observable_terminal_transition() {
        let model = model(&["Rhystic Study", "Command Tower"]);
        let policy = VisibleOpeningPolicy::new(&model);
        let mut failure = PackedStateV2::default();
        failure.hand.insert(1);
        let mut success = PackedStateV2::default();
        success
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        let transitions = vec![
            InformationTransition::Deterministic(failure),
            InformationTransition::Deterministic(success),
        ];
        assert_eq!(
            policy.choose(PackedStateV2::default(), &transitions),
            Some(1)
        );
    }

    #[test]
    fn visible_policy_values_the_card_selected_by_a_top_tutor() {
        let model = model(&["Imperial Seal", "Rhystic Study", "Blank"]);
        let policy = VisibleOpeningPolicy::new(&model);
        let state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            library: PackedLibrary::new([1, 2].into_iter().collect()),
            mana: ManaPool([1, 0, 0, 0, 0, 0]),
            flags: TURN_DRAW_DONE,
            ..PackedStateV2::default()
        };
        let mut transitions = SmallVec::new();
        model.transitions(state, &mut transitions);
        let chosen = policy
            .choose(state, &transitions)
            .and_then(|index| transitions.get(index))
            .expect("policy action");
        assert!(matches!(
            chosen,
            InformationTransition::Deterministic(next)
                if next.library.known_top_len() == 1
                    && next.library.chance_draws()[0].slot == 1
        ));
    }

    #[test]
    fn mulligan_decision_depends_only_on_the_visible_hand() {
        let model = model(&[
            "Rhystic Study",
            "Ancient Tomb",
            "Lotus Petal",
            "Blank A",
            "Blank B",
        ]);
        let mut left = PackedStateV2::default();
        left.hand = [0, 1, 2].into_iter().collect();
        left.library = PackedLibrary::new([3, 4].into_iter().collect());
        let mut right = left;
        right.library = PackedLibrary::new([3].into_iter().collect());
        right.library.push_known_top(4);
        let policy = OpeningMulliganPolicy::default();
        assert_eq!(
            model.should_keep(policy, left, 3, false),
            model.should_keep(policy, right, 3, false)
        );
    }

    #[test]
    fn mulligan_decision_uses_the_presampled_caverns_status() {
        let model = model(&["Gemstone Caverns", "Blank A", "Blank B"]);
        let state = PackedStateV2 {
            hand: [0, 1, 2].into_iter().collect(),
            ..PackedStateV2::default()
        };
        let policy = OpeningMulliganPolicy {
            minimum_score_by_hand_size: [3; 8],
        };

        assert!(!model.should_keep(policy, state, 7, false));
        assert!(model.should_keep(policy, state, 7, true));
    }

    #[test]
    fn spirit_guides_are_direct_hand_payment_sources() {
        let model = model(&["Elvish Spirit Guide", "Simian Spirit Guide"]);
        let mut state = PackedStateV2::default();
        state.hand = [0, 1].into_iter().collect();
        let sources = model.payment_sources(state);
        assert!(sources.iter().any(|source| {
            source.slot == 0
                && source.options.len() == 1
                && source.options[0]
                    == ManaOption::new(ManaPool([0, 0, 0, 0, 1, 0]), ResourceUse::Exile)
        }));
        assert!(sources.iter().any(|source| {
            source.slot == 1
                && source.options.len() == 1
                && source.options[0]
                    == ManaOption::new(ManaPool([0, 1, 0, 0, 0, 0]), ResourceUse::Exile)
        }));
    }

    #[test]
    fn rituals_pay_colored_costs_and_leave_net_mana() {
        let model = model(&["Dark Ritual", "Rite of Flame", "Command Tower"]);
        let mut dark = PackedStateV2::default();
        dark.hand.insert(0);
        dark.battlefield
            .insert(PermanentInstance::new(PermanentSource::card(2)));
        let mut out = SmallVec::new();
        model.generate_rituals(dark, &mut out);
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.graveyard.contains(0) && next.mana == ManaPool([3, 0, 0, 0, 0, 0])
        )));

        let mut rite = PackedStateV2::default();
        rite.hand.insert(1);
        rite.battlefield
            .insert(PermanentInstance::new(PermanentSource::card(2)));
        out.clear();
        model.generate_rituals(rite, &mut out);
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.graveyard.contains(1) && next.mana == ManaPool([0, 2, 0, 0, 0, 0])
        )));
    }

    #[test]
    fn hand_and_top_tutors_apply_exact_shuffle_information() {
        let model = model(&["Demonic Tutor", "Imperial Seal", "Rhystic Study", "Blank"]);
        let mut hand_tutor = PackedStateV2 {
            library: PackedLibrary::new([2, 3].into_iter().collect()),
            mana: ManaPool([1, 0, 0, 0, 0, 1]),
            ..PackedStateV2::default()
        };
        hand_tutor.hand.insert(0);
        let mut out = SmallVec::new();
        model.generate_tutors(hand_tutor, &mut out);
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.hand.contains(2)
                    && next.graveyard.contains(0)
                    && next.library.known_top_len() == 0
        )));

        let mut top_tutor = PackedStateV2 {
            library: PackedLibrary::new([2].into_iter().collect()),
            mana: ManaPool([1, 0, 0, 0, 0, 0]),
            ..PackedStateV2::default()
        };
        top_tutor.library.push_known_top(3);
        top_tutor.hand.insert(1);
        out.clear();
        model.generate_tutors(top_tutor, &mut out);
        let stacked = out.iter().find_map(|transition| match transition {
            InformationTransition::Deterministic(next)
                if next.library.chance_draws()[0].slot == 2 =>
            {
                Some(next)
            }
            _ => None,
        });
        let stacked = stacked.expect("Seal can stack Rhystic");
        assert_eq!(stacked.library.known_top_len(), 1);
        assert!(stacked.library.unknown().contains(3));
    }

    #[test]
    fn enlightened_tutor_only_selects_artifacts_or_enchantments() {
        let model = model(&[
            "Enlightened Tutor",
            "Rhystic Study",
            "Lotus Petal",
            "Demonic Tutor",
        ]);
        let state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            library: PackedLibrary::new([1, 2, 3].into_iter().collect()),
            mana: ManaPool([0, 0, 0, 1, 0, 0]),
            ..PackedStateV2::default()
        };
        let mut out = SmallVec::new();
        model.generate_tutors(state, &mut out);
        assert_eq!(out.len(), 2);
        for transition in out {
            let InformationTransition::Deterministic(next) = transition else {
                panic!("tutor choice is deterministic");
            };
            assert_ne!(next.library.chance_draws()[0].slot, 3);
        }
    }

    #[test]
    fn wishclaw_casts_in_two_payments_and_deactivates_after_tutoring() {
        let model = model(&["Wishclaw Talisman", "Rhystic Study"]);
        let mut cast_state = PackedStateV2::default();
        cast_state.hand.insert(0);
        cast_state.mana = ManaPool([1, 0, 0, 0, 0, 1]);
        let mut out = SmallVec::new();
        model.generate_artifact_casts(cast_state, &mut out);
        let InformationTransition::Deterministic(mut active) = out[0] else {
            panic!("Wishclaw cast is deterministic");
        };
        active.library = PackedLibrary::new([1].into_iter().collect());
        active.mana = ManaPool([0, 0, 0, 0, 0, 1]);
        out.clear();
        model.generate_wishclaw_tutors(active, &mut out);
        let InformationTransition::Deterministic(tutored) = out[0] else {
            panic!("Wishclaw activation is deterministic");
        };
        assert!(tutored.hand.contains(1));
        let claw = tutored
            .battlefield
            .as_slice()
            .iter()
            .find(|permanent| permanent.source() == PermanentSource::card(0))
            .expect("transferred Wishclaw remains represented");
        assert_eq!(claw.counters(), 0);
        assert!(!model.has_artifact(tutored));
    }

    #[test]
    fn offer_can_counter_own_zero_mana_bait_for_two_treasures() {
        let model = model(&["An Offer You Can't Refuse", "Lotus Petal", "Command Tower"]);
        let mut state = PackedStateV2 {
            hand: [0, 1].into_iter().collect(),
            ..PackedStateV2::default()
        };
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(2)));
        let mut out = SmallVec::new();

        model.generate_offer(state, &mut out);

        let next = out
            .iter()
            .filter_map(|transition| match transition {
                InformationTransition::Deterministic(next) => Some(next),
                InformationTransition::Chance(_) => None,
            })
            .find(|next| next.graveyard.contains(0) && next.graveyard.contains(1))
            .expect("Offer line");
        assert_eq!(
            next.battlefield
                .as_slice()
                .iter()
                .filter(|permanent| {
                    permanent.source().token_kind() == Some(TokenKind::Treasure)
                })
                .count(),
            2
        );
    }

    #[test]
    fn ranger_captain_has_fail_to_find_and_esper_search_branches() {
        let model = model(&["Ranger-Captain of Eos", "Esper Sentinel", "Blank"]);
        let state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            library: PackedLibrary::new([1, 2].into_iter().collect()),
            mana: ManaPool([0, 0, 0, 2, 0, 1]),
            ..PackedStateV2::default()
        };
        let mut out = SmallVec::new();

        model.generate_ranger_captain(state, &mut out);

        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.hand.contains(1)
                    && next.battlefield.contains_source(PermanentSource::card(0))
                    && !next.library.cards().contains(1)
        )));
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if !next.hand.contains(1) && next.library.cards().contains(1)
        )));
    }

    #[test]
    fn live_caverns_allows_instant_top_tutor_before_turn_one_draw() {
        let model = model(&[
            "Gemstone Caverns",
            "Vampiric Tutor",
            "Ancient Tomb",
            "Lotus Petal",
            "Blank",
            "Rhystic Study",
            "Blank library",
        ]);
        let state = PackedStateV2 {
            hand: [0, 1, 2, 3, 4].into_iter().collect(),
            library: PackedLibrary::new([5, 6].into_iter().collect()),
            ..PackedStateV2::default()
        };
        let value = model
            .pregame_states(state, true)
            .into_iter()
            .map(|start| ReferenceSolver::new(&model).solve(start, 12).value)
            .fold(0.0, f64::max);
        assert_eq!(value, 1.0);

        let preturn = model
            .pregame_states(state, true)
            .into_iter()
            .find(|start| start.exile.contains(4))
            .expect("Caverns exile choice");
        let mut out = SmallVec::new();
        model.transitions(preturn, &mut out);
        assert!(out.len() > 1, "Vampiric Tutor plus pass are available");
    }

    #[test]
    fn manamorphose_uses_explicit_mana_choices_and_draw_chance() {
        let model = model(&["Manamorphose", "Rhystic Study", "Blank"]);
        let state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            library: PackedLibrary::new([1, 2].into_iter().collect()),
            mana: ManaPool([0, 1, 0, 0, 0, 1]),
            ..PackedStateV2::default()
        };
        let mut out = SmallVec::new();
        model.generate_manamorphose(state, &mut out);
        assert_eq!(out.len(), 15);
        assert!(out.iter().all(|transition| matches!(
            transition,
            InformationTransition::Chance(outcomes)
                if outcomes.len() == 2
                    && outcomes.iter().all(|(next, probability)|
                        next.graveyard.contains(0) && (*probability - 0.5).abs() < f64::EPSILON)
        )));
    }

    #[test]
    fn gamble_randomly_discards_from_the_actual_post_search_hand() {
        let model = model(&["Gamble", "Blank", "Rhystic Study"]);
        let state = PackedStateV2 {
            hand: [0, 1].into_iter().collect(),
            library: PackedLibrary::new([2].into_iter().collect()),
            mana: ManaPool([0, 1, 0, 0, 0, 0]),
            ..PackedStateV2::default()
        };
        let mut out = SmallVec::new();
        model.generate_gamble(state, &mut out);
        assert_eq!(out.len(), 1);
        let InformationTransition::Chance(outcomes) = &out[0] else {
            panic!("Gamble discard must remain a chance node");
        };
        assert_eq!(outcomes.len(), 2);
        assert!(outcomes
            .iter()
            .any(|(next, probability)| next.hand.contains(2)
                && (*probability - 0.5).abs() < f64::EPSILON));
        assert!(outcomes
            .iter()
            .any(|(next, probability)| next.graveyard.contains(2)
                && (*probability - 0.5).abs() < f64::EPSILON));
    }

    #[test]
    fn noxious_revival_moves_a_graveyard_card_to_known_top() {
        let model = model(&["Noxious Revival", "Rhystic Study"]);
        let state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            graveyard: [1].into_iter().collect(),
            ..PackedStateV2::default()
        };
        let mut out = SmallVec::new();
        model.generate_noxious_revival(state, &mut out);
        let InformationTransition::Deterministic(next) = out[0] else {
            panic!("Noxious target is deterministic");
        };
        assert!(next.graveyard.contains(0));
        assert!(!next.graveyard.contains(1));
        assert_eq!(next.library.chance_draws()[0].slot, 1);
    }

    #[test]
    fn demonic_tutor_can_hold_priority_and_crack_led() {
        let model = model(&[
            "Demonic Tutor",
            "Lion's Eye Diamond",
            "Blank",
            "Rhystic Study",
        ]);
        let mut state = PackedStateV2 {
            hand: [0, 2].into_iter().collect(),
            library: PackedLibrary::new([3].into_iter().collect()),
            mana: ManaPool([1, 0, 0, 0, 0, 1]),
            ..PackedStateV2::default()
        };
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(1)));
        let mut out = SmallVec::new();
        model.generate_demonic_led_tutors(state, &mut out);
        assert_eq!(out.len(), 5);
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.hand.contains(3)
                    && next.graveyard.contains(0)
                    && next.graveyard.contains(1)
                    && next.graveyard.contains(2)
                    && next.mana == ManaPool([0, 0, 3, 0, 0, 0])
        )));
    }

    #[test]
    fn green_suns_zenith_puts_heartwood_into_play_and_shuffles_itself() {
        let model = model(&["Green Sun's Zenith", "Heartwood Storyteller"]);
        let state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            library: PackedLibrary::new([1].into_iter().collect()),
            mana: ManaPool([0, 0, 0, 0, 1, 3]),
            ..PackedStateV2::default()
        };
        let mut out = SmallVec::new();
        model.generate_green_engine_tutors(state, &mut out);
        let InformationTransition::Deterministic(next) = out[0] else {
            panic!("Zenith target is deterministic");
        };
        assert!(next.battlefield.contains_source(PermanentSource::card(1)));
        assert!(next.library.unknown().contains(0));
        assert!(!next.hand.contains(0));
    }

    #[test]
    fn summoners_pact_requires_a_real_next_upkeep_payment() {
        let model = model(&[
            "Summoner's Pact",
            "Heartwood Storyteller",
            "Ancient Tomb",
            "Tropical Island",
            "Elvish Spirit Guide",
        ]);
        let search = PackedStateV2 {
            hand: [0].into_iter().collect(),
            library: PackedLibrary::new([1].into_iter().collect()),
            ..PackedStateV2::default()
        };
        let mut out = SmallVec::new();
        model.generate_green_engine_tutors(search, &mut out);
        let InformationTransition::Deterministic(searched) = out[0] else {
            panic!("Pact target is deterministic");
        };
        assert!(searched.hand.contains(1));
        assert!(searched.exile.contains(0));
        assert_ne!(searched.flags & PACT_DUE, 0);

        let mut terminal = PackedStateV2::default();
        terminal.flags = TURN_DRAW_DONE | PACT_DUE;
        terminal
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(1)));
        terminal
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(2)));
        terminal
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(3)));
        assert_eq!(model.engine_value(terminal), None);
        terminal.hand.insert(4);
        assert_eq!(model.engine_value(terminal), Some(0.70));

        out.clear();
        model.generate_end_turn(terminal, &mut out);
        assert!(!out.is_empty());
        assert!(out.iter().all(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.flags & PACT_DUE == 0 && next.exile.contains(4)
        )));
    }

    #[test]
    fn rain_of_filth_includes_tap_then_sacrifice_composite_mana() {
        let model = model(&["Rain of Filth", "Command Tower"]);
        let mut state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            mana: ManaPool([1, 0, 0, 0, 0, 0]),
            ..PackedStateV2::default()
        };
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(1)));
        let mut out = SmallVec::new();
        model.generate_sacrifice_spells(state, &mut out);
        let InformationTransition::Deterministic(active) = out[0] else {
            panic!("Rain cast is deterministic");
        };
        assert_ne!(active.flags & RAIN_ACTIVE, 0);
        let source = model
            .payment_sources(active)
            .into_iter()
            .find(|source| source.slot == 1)
            .expect("rain-enabled land");
        assert!(source.options.iter().any(|option| {
            option.resource_use == ResourceUse::Sacrifice
                && option.mana == ManaPool([1, 0, 1, 0, 0, 0])
        }));
    }

    #[test]
    fn crop_rotation_can_tap_and_sacrifice_the_same_land() {
        let model = model(&["Crop Rotation", "Tropical Island", "Ancient Tomb"]);
        let mut state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            library: PackedLibrary::new([2].into_iter().collect()),
            ..PackedStateV2::default()
        };
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(1)));
        let mut out = SmallVec::new();
        model.generate_crop_rotation(state, &mut out);
        let InformationTransition::Deterministic(next) = out[0] else {
            panic!("Crop target is deterministic");
        };
        assert!(next.graveyard.contains(0));
        assert!(next.graveyard.contains(1));
        assert!(next.battlefield.contains_source(PermanentSource::card(2)));
    }

    #[test]
    fn sacrifice_spells_can_use_the_commander() {
        let model = model(&["Culling the Weak", "Diabolic Intent", "Rhystic Study"]);
        let mut culling = PackedStateV2 {
            hand: [0].into_iter().collect(),
            mana: ManaPool([1, 0, 0, 0, 0, 0]),
            ..PackedStateV2::default()
        };
        assert!(culling.put_commander_on_battlefield(false, true));
        let mut out = SmallVec::new();
        model.generate_sacrifice_spells(culling, &mut out);
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.commander.zone == CommanderZone::Command
                    && next.mana == ManaPool([4, 0, 0, 0, 0, 0])
        )));

        let mut intent = PackedStateV2 {
            hand: [1].into_iter().collect(),
            library: PackedLibrary::new([2].into_iter().collect()),
            mana: ManaPool([1, 0, 0, 0, 0, 1]),
            ..PackedStateV2::default()
        };
        assert!(intent.put_commander_on_battlefield(false, true));
        out.clear();
        model.generate_sacrifice_spells(intent, &mut out);
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.commander.zone == CommanderZone::Command && next.hand.contains(2)
        )));
    }

    #[test]
    fn birds_waits_a_turn_while_tinder_wall_makes_mana_immediately() {
        let model = model(&["Birds of Paradise", "Tinder Wall"]);
        let birds_state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            mana: ManaPool([0, 0, 0, 0, 1, 0]),
            flags: TURN_DRAW_DONE,
            ..PackedStateV2::default()
        };
        let mut out = SmallVec::new();
        model.generate_creature_casts(birds_state, &mut out);
        let InformationTransition::Deterministic(birds) = out[0] else {
            panic!("Birds cast is deterministic");
        };
        assert!(model.payment_sources(birds).is_empty());
        out.clear();
        model.generate_end_turn(birds, &mut out);
        let InformationTransition::Deterministic(ready_birds) = out[0] else {
            panic!("end turn is deterministic");
        };
        assert_eq!(model.payment_sources(ready_birds)[0].options.len(), 5);

        let tinder_state = PackedStateV2 {
            hand: [1].into_iter().collect(),
            mana: ManaPool([0, 0, 0, 0, 1, 0]),
            ..PackedStateV2::default()
        };
        out.clear();
        model.generate_creature_casts(tinder_state, &mut out);
        let InformationTransition::Deterministic(tinder) = out[0] else {
            panic!("Tinder cast is deterministic");
        };
        let source = &model.payment_sources(tinder)[0];
        assert_eq!(source.options[0].mana, ManaPool([0, 2, 0, 0, 0, 0]));
        assert_eq!(source.options[0].resource_use, ResourceUse::Sacrifice);
    }

    #[test]
    fn deathrite_external_land_assumption_is_explicit() {
        let enabled = model(&["Deathrite Shaman"]);
        let mut state = PackedStateV2::default();
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        let mut out = SmallVec::new();
        enabled.generate_deathrite_mana(state, &mut out);
        assert_eq!(out.len(), 5);

        let disabled = model(&["Deathrite Shaman"]).with_deathrite_external_land(false);
        out.clear();
        disabled.generate_deathrite_mana(state, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn ragavan_connects_and_treasure_converts_to_any_color() {
        let model = model(&["Ragavan, Nimble Pilferer"]);
        let mut state = PackedStateV2::default();
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(0)));
        let mut out = SmallVec::new();
        model.generate_ragavan_attacks(state, &mut out);
        let InformationTransition::Deterministic(with_treasure) = out[0] else {
            panic!("configured Ragavan connection is deterministic");
        };
        assert!(with_treasure
            .battlefield
            .contains_source(PermanentSource::token(TokenKind::Treasure)));
        out.clear();
        model.generate_treasure_mana(with_treasure, &mut out);
        assert_eq!(out.len(), 5);
        assert!(out.iter().all(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if !next.battlefield.contains_source(PermanentSource::token(TokenKind::Treasure))
        )));
    }

    #[test]
    fn mystical_tutor_only_stacks_modeled_instants_and_sorceries() {
        let model = model(&[
            "Mystical Tutor",
            "Rhystic Study",
            "Demonic Tutor",
            "Elvish Spirit Guide",
        ]);
        let state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            library: PackedLibrary::new([1, 2, 3].into_iter().collect()),
            mana: ManaPool([0, 0, 1, 0, 0, 0]),
            ..PackedStateV2::default()
        };
        let mut out = SmallVec::new();
        model.generate_tutors(state, &mut out);
        assert_eq!(out.len(), 1);
        let InformationTransition::Deterministic(next) = out[0] else {
            panic!("Mystical target is deterministic");
        };
        assert_eq!(next.library.chance_draws()[0].slot, 2);
    }

    #[test]
    fn eldritch_evolution_turns_the_commander_into_heartwood() {
        let model = model(&["Eldritch Evolution", "Heartwood Storyteller"]);
        let mut state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            library: PackedLibrary::new([1].into_iter().collect()),
            mana: ManaPool([0, 0, 0, 0, 2, 1]),
            ..PackedStateV2::default()
        };
        assert!(state.put_commander_on_battlefield(false, false));
        let mut out = SmallVec::new();
        model.generate_eldritch_evolution(state, &mut out);
        let InformationTransition::Deterministic(next) = out[0] else {
            panic!("Evolution target is deterministic");
        };
        assert_eq!(next.commander.zone, CommanderZone::Command);
        assert!(next.exile.contains(0));
        assert!(next.battlefield.contains_source(PermanentSource::card(1)));
    }

    #[test]
    fn angels_grace_can_cover_an_unpayable_pact_trigger() {
        let model = model(&["Angel's Grace", "Heartwood Storyteller", "Command Tower"]);
        let mut state = PackedStateV2 {
            hand: [0].into_iter().collect(),
            flags: TURN_DRAW_DONE | PACT_DUE,
            ..PackedStateV2::default()
        };
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(1)));
        state
            .battlefield
            .insert(PermanentInstance::new(PermanentSource::card(2)));
        assert_eq!(model.engine_value(state), Some(0.70));
        let mut out = SmallVec::new();
        model.generate_end_turn(state, &mut out);
        assert!(out.iter().any(|transition| matches!(
            transition,
            InformationTransition::Deterministic(next)
                if next.graveyard.contains(0) && next.flags & PACT_DUE == 0
        )));
    }
}
