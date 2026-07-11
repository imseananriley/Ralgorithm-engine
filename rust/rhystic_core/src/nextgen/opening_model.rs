use smallvec::SmallVec;

use super::{
    compute_payment_plans, CardFlags, CardMask, CommanderZone, CompiledPolicy, DeckSpec,
    InformationModel, InformationTransition, ManaOption, ManaPool, ManaSource, OpeningArtifactKind,
    OpeningLandKind, OpeningManaProfile, PackedStateV2, PermanentInstance, PermanentSource,
    ResourceUse, SlotId, TokenKind, Zone,
};
use crate::{pay_options, Cost};

const LAND_PLAYED: u32 = 1;
const TURN_DRAW_DONE: u32 = 1 << 1;

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
    Engine { cost: Cost, values: [f64; 2] },
}

#[derive(Debug, Clone)]
pub struct EngineOpeningModel {
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
        for card in deck.cards() {
            card_flags.push(card.flags);
            card_colors.push(card.color_mask);
            if card.flags.contains(CardFlags::ARTIFACT) {
                artifact_slots.insert(card.slot);
            }
            let compiled = match card.name.as_ref() {
                "Rhystic Study" => OpeningCard::Engine {
                    cost: [2, 0, 0, 1, 0, 0],
                    values: [1.0, 0.75],
                },
                "Heartwood Storyteller" => OpeningCard::Engine {
                    cost: [1, 0, 0, 0, 0, 2],
                    values: [0.70, 0.55],
                },
                _ if card.opening_mana.is_supported() => {
                    OpeningCard::Land(land_semantics(card.opening_mana))
                }
                _ if !matches!(card.opening_artifact, OpeningArtifactKind::None) => {
                    OpeningCard::Artifact(card.opening_artifact)
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

    fn engine_value(&self, state: PackedStateV2) -> Option<f64> {
        let turn_index = usize::from(Self::turn(state).saturating_sub(1)).min(1);
        state
            .battlefield
            .as_slice()
            .iter()
            .filter_map(|permanent| permanent.source().card_slot())
            .filter(|slot| self.engine_slots.contains(*slot))
            .filter_map(|slot| match self.card(slot) {
                Some(OpeningCard::Engine { values, .. }) => Some(values[turn_index]),
                _ => None,
            })
            .reduce(f64::max)
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
        score += state.battlefield.len() as i64 * 35;
        score -= state.hand.len() as i64;
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
                        | OpeningArtifactKind::ManaVault => 100,
                        OpeningArtifactKind::LionsEyeDiamond
                        | OpeningArtifactKind::ParadiseMantle => 60,
                        OpeningArtifactKind::None => 0,
                    };
                }
                _ => {}
            }
        }
        score
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
                    | OpeningArtifactKind::MoxAmber => 2,
                    OpeningArtifactKind::LionsEyeDiamond | OpeningArtifactKind::ParadiseMantle => 1,
                    OpeningArtifactKind::None => 0,
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

    pub fn should_keep(
        &self,
        policy: OpeningMulliganPolicy,
        state: PackedStateV2,
        hand_size: usize,
    ) -> bool {
        self.visible_hand_score(state) >= policy.minimum_score_by_hand_size[hand_size.min(7)]
    }

    pub fn bottom_priority(&self, slot: SlotId) -> i32 {
        match self.card(slot) {
            Some(OpeningCard::Inert) | None => 0,
            Some(OpeningCard::Artifact(OpeningArtifactKind::ParadiseMantle)) => 1,
            Some(OpeningCard::Artifact(OpeningArtifactKind::LionsEyeDiamond)) => 2,
            Some(OpeningCard::Land(_)) => 3,
            Some(OpeningCard::Artifact(_)) => 4,
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
            }) || permanent
                .source()
                .card_slot()
                .is_some_and(|slot| self.artifact_slots.contains(slot))
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
                }) || permanent
                    .source()
                    .card_slot()
                    .is_some_and(|slot| self.artifact_slots.contains(slot))
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
                OpeningArtifactKind::SolRing | OpeningArtifactKind::ManaVault => {
                    let sources = self.payment_sources(state);
                    if self.direct_payments {
                        for plan in compute_payment_plans(state.mana, &sources, [1, 0, 0, 0, 0, 0])
                        {
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
                    for plan in compute_payment_plans(
                        state.mana,
                        &self.payment_sources(state),
                        [1, 0, 0, 0, 0, 0],
                    ) {
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
            for plan in compute_payment_plans(state.mana, &self.payment_sources(state), cost) {
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
                sources.push(ManaSource::with_options(slot, options));
                continue;
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
        sources
    }

    fn apply_payment_plan(
        &self,
        state: PackedStateV2,
        plan: super::PaymentPlan,
    ) -> Option<PackedStateV2> {
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
        let sources = self.payment_sources(state);
        for slot in state.hand.iter() {
            let Some(OpeningCard::Engine { cost, .. }) = self.card(slot) else {
                continue;
            };
            for plan in compute_payment_plans(state.mana, &sources, *cost) {
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
        next.flags &= !(LAND_PLAYED | TURN_DRAW_DONE);
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

        out.push(InformationTransition::Deterministic(next));
    }

    fn generate_turn_draw(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
        let mut next = state;
        next.flags |= TURN_DRAW_DONE;
        let draws = next.library.chance_draws();
        if draws.is_empty() {
            out.push(InformationTransition::Deterministic(next));
            return;
        }
        let outcomes = draws
            .into_iter()
            .filter_map(|draw| {
                let mut drawn = next;
                drawn.draw(draw.slot).then_some((
                    drawn,
                    f64::from(draw.numerator) / f64::from(draw.denominator),
                ))
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
    use crate::nextgen::PackedLibrary;

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
            model.should_keep(policy, left, 3),
            model.should_keep(policy, right, 3)
        );
    }
}
