use smallvec::SmallVec;

use super::{
    CardMask, DeckSpec, InformationModel, InformationTransition, ManaPool, OpeningManaProfile,
    PackedStateV2, PermanentInstance, PermanentSource, SlotId, Zone,
};
use crate::{pay_options, Cost};

const LAND_PLAYED: u32 = 1;
const TURN_DRAW_DONE: u32 = 1 << 1;

#[derive(Debug, Clone)]
struct LandSemantics {
    mana: SmallVec<[ManaPool; 5]>,
    enters_tapped: bool,
}

#[derive(Debug, Clone)]
enum OpeningCard {
    Inert,
    Land(LandSemantics),
    Engine { cost: Cost, values: [f64; 2] },
}

#[derive(Debug, Clone)]
pub struct EngineOpeningModel {
    cards: Box<[OpeningCard]>,
    engine_slots: CardMask,
    supported_slots: CardMask,
    deck_slots: CardMask,
    max_turn: u8,
}

impl EngineOpeningModel {
    pub fn compile(deck: &DeckSpec, max_turn: u8) -> Self {
        let mut cards = Vec::with_capacity(deck.cards().len());
        let mut engine_slots = CardMask::EMPTY;
        let mut supported_slots = CardMask::EMPTY;
        for card in deck.cards() {
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
            max_turn,
        }
    }

    pub const fn supported_slots(&self) -> CardMask {
        self.supported_slots
    }

    pub const fn unsupported_slots(&self) -> CardMask {
        self.deck_slots.difference(self.supported_slots)
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
            let permanent =
                PermanentInstance::new(PermanentSource::card(slot)).with_tapped(land.enters_tapped);
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
            for produced in &land.mana {
                let mut next = state;
                next.mana = next.mana.add_capped(*produced, 15);
                next.battlefield
                    .replace(*permanent, permanent.with_tapped(true));
                out.push(InformationTransition::Deterministic(next));
            }
        }
    }

    fn generate_engine_casts(
        &self,
        state: PackedStateV2,
        out: &mut SmallVec<[InformationTransition<PackedStateV2>; 16]>,
    ) {
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
        let battlefield = next.battlefield;
        for permanent in battlefield.as_slice() {
            next.battlefield
                .replace(*permanent, permanent.with_tapped(false).with_fresh(false));
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
        self.generate_mana_activations(state, out);
        self.generate_engine_casts(state, out);
        self.generate_end_turn(state, out);
    }
}

fn land_semantics(profile: OpeningManaProfile) -> LandSemantics {
    let mut mana = SmallVec::new();
    for color in 0..5 {
        if profile.color_mask & (1 << color) != 0 {
            let mut produced = [0; 6];
            produced[color] = 1;
            mana.push(ManaPool(produced));
        }
    }
    if profile.colorless != 0 {
        mana.push(ManaPool([0, 0, 0, 0, 0, profile.colorless]));
    }
    LandSemantics {
        mana,
        enters_tapped: profile.enters_tapped,
    }
}
