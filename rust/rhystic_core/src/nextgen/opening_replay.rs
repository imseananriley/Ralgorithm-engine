use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::{
    CardMask, DeckSpec, EngineOpeningModel, OpeningExistenceDiscrepancySolver, OpeningOutcome,
    OpeningOutcomeDiscrepancySolver, OpeningOutcomeResult, OpeningWitnessTransition, PackedLibrary,
    PackedStateV2, PermanentSource, SearchMetrics, SlotId, TokenKind,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningReplayGame {
    pub game_index: u64,
    pub hand: Vec<String>,
    #[serde(default)]
    pub bottomed: Vec<String>,
    #[serde(default)]
    pub library_top: Vec<String>,
    #[serde(default)]
    pub library_order: Vec<String>,
    pub gemstone_caverns_live: bool,
    #[serde(default)]
    pub legacy_hit: bool,
    #[serde(default)]
    pub legacy_capped: bool,
    #[serde(default)]
    pub legacy_turn: Option<u8>,
    #[serde(default)]
    pub legacy_engine_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningReplayRequest {
    pub deck: Vec<String>,
    pub games: Vec<OpeningReplayGame>,
    #[serde(default = "default_max_turn")]
    pub max_turn: u8,
    #[serde(default = "default_depth")]
    pub depth: u8,
    #[serde(default = "default_discrepancy_budgets")]
    pub discrepancy_budgets: Vec<u8>,
    #[serde(default = "default_action_candidate_limit")]
    pub action_candidate_limit: usize,
    #[serde(default = "default_workers")]
    pub workers: usize,
    #[serde(default)]
    pub existence_only: bool,
    #[serde(default)]
    pub validate_witnesses: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningWitnessValidationStatus {
    Confirmed,
    Probabilistic,
    Invalid,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningWitnessValidation {
    pub status: OpeningWitnessValidationStatus,
    pub steps: usize,
    pub chance_product: f64,
    pub reason: Option<String>,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningReplayTierOutcome {
    pub discrepancy_budget: u8,
    pub found: bool,
    pub witness_validation: Option<OpeningWitnessValidation>,
    pub outcome: OpeningOutcome,
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub capped: bool,
    pub search_metrics: SearchMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningReplayGameOutcome {
    pub game_index: u64,
    pub legacy_hit: bool,
    pub legacy_capped: bool,
    pub legacy_turn: Option<u8>,
    pub legacy_engine_label: Option<String>,
    pub tiers: Vec<OpeningReplayTierOutcome>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningReplayResponse {
    pub games: Vec<OpeningReplayGameOutcome>,
}

pub fn evaluate_opening_replay(
    request: &OpeningReplayRequest,
) -> Result<OpeningReplayResponse, String> {
    if request.games.is_empty() {
        return Err("opening replay requires at least one game".to_string());
    }
    if request.discrepancy_budgets.is_empty() {
        return Err("opening replay requires at least one discrepancy budget".to_string());
    }
    let deck = DeckSpec::compile(&request.deck)?;
    let model = EngineOpeningModel::compile(&deck, request.max_turn);
    let workers = request.workers.max(1).min(request.games.len());
    let next = AtomicUsize::new(0);
    let outcomes = Mutex::new(Vec::with_capacity(request.games.len()));
    let error = Mutex::new(None);

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                if error.lock().expect("replay error lock").is_some() {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(game) = request.games.get(index) else {
                    break;
                };
                match evaluate_game(
                    &deck,
                    &model,
                    game,
                    request.depth,
                    &request.discrepancy_budgets,
                    request.action_candidate_limit,
                    request.existence_only,
                    request.validate_witnesses,
                ) {
                    Ok(outcome) => outcomes.lock().expect("replay outcomes lock").push(outcome),
                    Err(message) => {
                        *error.lock().expect("replay error lock") = Some(message);
                        break;
                    }
                }
            });
        }
    });
    if let Some(message) = error.into_inner().expect("replay error mutex") {
        return Err(message);
    }
    let mut games = outcomes.into_inner().expect("replay outcomes mutex");
    games.sort_by_key(|game| game.game_index);
    Ok(OpeningReplayResponse { games })
}

fn evaluate_game(
    deck: &DeckSpec,
    model: &EngineOpeningModel,
    game: &OpeningReplayGame,
    depth: u8,
    discrepancy_budgets: &[u8],
    action_candidate_limit: usize,
    existence_only: bool,
    validate_witnesses: bool,
) -> Result<OpeningReplayGameOutcome, String> {
    let mut hand = CardMask::EMPTY;
    for name in &game.hand {
        let slot = deck
            .slot(name)
            .ok_or_else(|| format!("game {} hand contains unknown card {name}", game.game_index))?;
        if !hand.insert(slot) {
            return Err(format!(
                "game {} hand contains duplicate card {name}",
                game.game_index
            ));
        }
    }
    let mut bottomed = CardMask::EMPTY;
    for name in &game.bottomed {
        let slot = deck.slot(name).ok_or_else(|| {
            format!(
                "game {} bottom contains unknown card {name}",
                game.game_index
            )
        })?;
        if hand.contains(slot) || !bottomed.insert(slot) {
            return Err(format!(
                "game {} has duplicate hand/bottom card {name}",
                game.game_index
            ));
        }
    }
    let mut library = PackedLibrary::new(deck.card_mask().difference(hand).difference(bottomed));
    for name in &game.bottomed {
        let slot = deck.slot(name).expect("validated bottom slot");
        library.insert_unknown(slot);
        library.push_known_bottom(slot);
    }
    let mut known_top = CardMask::EMPTY;
    for name in &game.library_top {
        let slot = deck.slot(name).ok_or_else(|| {
            format!(
                "game {} known top contains unknown card {name}",
                game.game_index
            )
        })?;
        if hand.contains(slot) || bottomed.contains(slot) || !known_top.insert(slot) {
            return Err(format!(
                "game {} has duplicate hand/bottom/top card {name}",
                game.game_index
            ));
        }
    }
    for name in game.library_top.iter().rev() {
        library.push_known_top(deck.slot(name).expect("validated known-top slot"));
    }
    let state = PackedStateV2 {
        hand,
        library,
        ..PackedStateV2::default()
    };
    let pregame_states = model.pregame_states(state, game.gemstone_caverns_live);
    let mut tiers = Vec::with_capacity(discrepancy_budgets.len());
    for &budget in discrepancy_budgets {
        if existence_only {
            let mut solver = OpeningExistenceDiscrepancySolver::new(model, action_candidate_limit);
            let mut found = false;
            let mut metrics = SearchMetrics::default();
            let mut witness_validation = None;
            for pregame in &pregame_states {
                let result = solver.solve(*pregame, depth, budget);
                metrics = result.metrics;
                if result.found {
                    found = true;
                    if validate_witnesses {
                        witness_validation =
                            Some(validate_full_library_witness(deck, game, &result.witness));
                    }
                    break;
                }
            }
            tiers.push(OpeningReplayTierOutcome {
                discrepancy_budget: budget,
                found,
                witness_validation,
                outcome: OpeningOutcome::default(),
                lower_bound: f64::from(found),
                upper_bound: f64::from(found),
                capped: false,
                search_metrics: metrics,
            });
            continue;
        }
        let mut solver = OpeningOutcomeDiscrepancySolver::new(model, action_candidate_limit);
        let mut best = OpeningOutcomeResult::default();
        for pregame in &pregame_states {
            let candidate = solver.solve(*pregame, depth, budget);
            best.metrics = candidate.metrics;
            best.upper_bound = best.upper_bound.max(candidate.upper_bound);
            best.capped |= candidate.capped;
            if candidate.outcome.weighted_ev > best.outcome.weighted_ev {
                best.outcome = candidate.outcome;
                best.lower_bound = candidate.lower_bound;
            }
        }
        tiers.push(OpeningReplayTierOutcome {
            discrepancy_budget: budget,
            found: best.outcome.weighted_ev > 0.0,
            witness_validation: None,
            outcome: best.outcome,
            lower_bound: best.lower_bound,
            upper_bound: best.upper_bound,
            capped: best.capped,
            search_metrics: best.metrics,
        });
    }
    Ok(OpeningReplayGameOutcome {
        game_index: game.game_index,
        legacy_hit: game.legacy_hit,
        legacy_capped: game.legacy_capped,
        legacy_turn: game.legacy_turn,
        legacy_engine_label: game.legacy_engine_label.clone(),
        tiers,
    })
}

fn validate_full_library_witness(
    deck: &DeckSpec,
    game: &OpeningReplayGame,
    witness: &[OpeningWitnessTransition<PackedStateV2>],
) -> OpeningWitnessValidation {
    let actions = describe_witness(deck, witness);
    if game.library_order.is_empty() || witness.is_empty() {
        return OpeningWitnessValidation {
            status: OpeningWitnessValidationStatus::Unavailable,
            steps: witness.len(),
            chance_product: 1.0,
            reason: Some("full library order or witness is unavailable".to_string()),
            actions,
        };
    }
    let mut order = Vec::with_capacity(game.library_order.len());
    for name in &game.library_order {
        let Some(slot) = deck.slot(name) else {
            return OpeningWitnessValidation {
                status: OpeningWitnessValidationStatus::Invalid,
                steps: witness.len(),
                chance_product: 0.0,
                reason: Some(format!("library order contains unknown card {name}")),
                actions,
            };
        };
        order.push(slot);
    }

    let mut randomized_tail = false;
    let mut stochastic = false;
    let mut uncertain_draws = Vec::<SlotId>::new();
    let mut chance_product = 1.0;

    for step in witness {
        chance_product *= step.chance_probability;
        let before_library = step.before.library.cards();
        let after_library = step.after.library.cards();
        let removed_library = before_library.difference(after_library);
        let added_library = after_library.difference(before_library);
        let added_hand = step.after.hand.difference(step.before.hand);

        if step.is_chance {
            let drawn = removed_library
                .iter()
                .find(|slot| added_hand.contains(*slot));
            if let Some(slot) = drawn {
                if step.before.library.known_top_len() > 0 {
                    let expected = step.before.library.chance_draws()[0].slot;
                    if slot != expected {
                        return invalid_witness(
                            witness.len(),
                            chance_product,
                            format!("forced draw expected slot {expected}, witness chose {slot}"),
                            actions.clone(),
                        );
                    }
                    remove_from_order(&mut order, slot);
                } else if randomized_tail {
                    remove_from_order(&mut order, slot);
                    uncertain_draws.push(slot);
                } else if order.first().copied() == Some(slot) {
                    order.remove(0);
                } else {
                    return invalid_witness(
                        witness.len(),
                        chance_product,
                        format!(
                            "recorded draw expected slot {:?}, witness chose {slot}",
                            order.first()
                        ),
                        actions.clone(),
                    );
                }
            } else {
                stochastic = true;
                reconcile_library_delta(&mut order, removed_library, added_library);
            }
            continue;
        }

        let before_top = known_top_slot(step.before);
        let after_top = known_top_slot(step.after);
        let top_changed = after_top.is_some() && after_top != before_top;

        for slot in removed_library.iter() {
            remove_from_order(&mut order, slot);
            randomized_tail = true;
        }
        for slot in added_library.iter() {
            remove_from_order(&mut order, slot);
            if after_top == Some(slot) && !before_library.contains(slot) {
                order.insert(0, slot);
            } else {
                order.push(slot);
                randomized_tail = true;
            }
        }
        if top_changed {
            let top = after_top.expect("checked known top");
            let searched_from_library = before_library.contains(top);
            remove_from_order(&mut order, top);
            order.insert(0, top);
            randomized_tail |= searched_from_library;
        }
    }

    let final_hand = witness.last().expect("non-empty witness").after.hand;
    if uncertain_draws
        .iter()
        .any(|slot| !final_hand.contains(*slot))
    {
        stochastic = true;
    }
    OpeningWitnessValidation {
        status: if stochastic {
            OpeningWitnessValidationStatus::Probabilistic
        } else {
            OpeningWitnessValidationStatus::Confirmed
        },
        steps: witness.len(),
        chance_product,
        reason: stochastic
            .then(|| "witness consumes unresolved stochastic information".to_string()),
        actions,
    }
}

fn known_top_slot(state: PackedStateV2) -> Option<SlotId> {
    (state.library.known_top_len() > 0).then(|| state.library.chance_draws()[0].slot)
}

fn remove_from_order(order: &mut Vec<SlotId>, slot: SlotId) {
    if let Some(index) = order.iter().position(|candidate| *candidate == slot) {
        order.remove(index);
    }
}

fn reconcile_library_delta(order: &mut Vec<SlotId>, removed: CardMask, added: CardMask) {
    for slot in removed.iter() {
        remove_from_order(order, slot);
    }
    for slot in added.iter() {
        remove_from_order(order, slot);
        order.push(slot);
    }
}

fn invalid_witness(
    steps: usize,
    chance_product: f64,
    reason: String,
    actions: Vec<String>,
) -> OpeningWitnessValidation {
    OpeningWitnessValidation {
        status: OpeningWitnessValidationStatus::Invalid,
        steps,
        chance_product,
        reason: Some(reason),
        actions,
    }
}

fn describe_witness(
    deck: &DeckSpec,
    witness: &[OpeningWitnessTransition<PackedStateV2>],
) -> Vec<String> {
    witness
        .iter()
        .map(|step| describe_witness_step(deck, step))
        .collect()
}

fn describe_witness_step(
    deck: &DeckSpec,
    step: &OpeningWitnessTransition<PackedStateV2>,
) -> String {
    let removed_library = step
        .before
        .library
        .cards()
        .difference(step.after.library.cards());
    let added_hand = step.after.hand.difference(step.before.hand);
    if step.is_chance {
        if let Some(slot) = removed_library
            .iter()
            .find(|slot| added_hand.contains(*slot))
        {
            return format!(
                "draw {} [p={:.6}]",
                deck.card(slot).name,
                step.chance_probability
            );
        }
    }

    let mut parts = Vec::new();
    for slot in step.before.hand.difference(step.after.hand).iter() {
        let destination = if step.after.graveyard.contains(slot) {
            "to graveyard"
        } else if step.after.exile.contains(slot) {
            "to exile"
        } else if battlefield_contains(step.after, PermanentSource::card(slot)) {
            "to battlefield"
        } else {
            "from hand"
        };
        parts.push(format!("{} {destination}", deck.card(slot).name));
    }
    for slot in added_hand.iter() {
        parts.push(format!("{} to hand", deck.card(slot).name));
    }
    let before_top = known_top_slot(step.before);
    let after_top = known_top_slot(step.after);
    if let Some(after_top) = after_top.filter(|top| Some(*top) != before_top) {
        parts.push(format!("{} to library top", deck.card(after_top).name));
    }

    for source in battlefield_sources(step.after) {
        if !battlefield_contains(step.before, source) {
            parts.push(format!("{} enters battlefield", source_name(deck, source)));
        }
    }
    for source in battlefield_sources(step.before) {
        if !battlefield_contains(step.after, source) {
            parts.push(format!("{} leaves battlefield", source_name(deck, source)));
        } else if !source_tapped(step.before, source) && source_tapped(step.after, source) {
            parts.push(format!("tap {}", source_name(deck, source)));
        }
    }
    let before_turn = step.before.counters & 0xff;
    let after_turn = step.after.counters & 0xff;
    if before_turn != after_turn {
        parts.push(format!("advance to turn {}", after_turn.max(1)));
    }
    if step.before.mana != step.after.mana {
        parts.push(format!(
            "mana {:?} -> {:?}",
            step.before.mana.0, step.after.mana.0
        ));
    }
    if step.is_chance {
        parts.push(format!("chance p={:.6}", step.chance_probability));
    }
    if parts.is_empty() {
        "pass/state transition".to_string()
    } else {
        parts.join("; ")
    }
}

fn battlefield_sources(state: PackedStateV2) -> Vec<PermanentSource> {
    state
        .battlefield
        .as_slice()
        .iter()
        .map(|permanent| permanent.source())
        .collect()
}

fn battlefield_contains(state: PackedStateV2, source: PermanentSource) -> bool {
    state.battlefield.contains_source(source)
}

fn source_tapped(state: PackedStateV2, source: PermanentSource) -> bool {
    state
        .battlefield
        .as_slice()
        .iter()
        .find(|permanent| permanent.source() == source)
        .is_some_and(|permanent| permanent.tapped())
}

fn source_name(deck: &DeckSpec, source: PermanentSource) -> String {
    if let Some(slot) = source.card_slot() {
        return deck.card(slot).name.to_string();
    }
    if source.is_commander() {
        return "Nick Fury, Agent of S.H.I.E.L.D.".to_string();
    }
    match source.token_kind() {
        Some(TokenKind::Treasure) => "Treasure".to_string(),
        Some(TokenKind::GenericArtifact) => "Artifact token".to_string(),
        Some(TokenKind::GenericCreature) => "Creature token".to_string(),
        Some(TokenKind::Copy) => "Copy token".to_string(),
        None => "Unknown permanent".to_string(),
    }
}

const fn default_max_turn() -> u8 {
    2
}

const fn default_depth() -> u8 {
    14
}

fn default_discrepancy_budgets() -> Vec<u8> {
    vec![0, 1, 2]
}

const fn default_action_candidate_limit() -> usize {
    2
}

const fn default_workers() -> usize {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validation_fixture() -> (DeckSpec, OpeningReplayGame, SlotId, SlotId, SlotId) {
        let names = ["Rhystic Study", "Blank A", "Blank B"];
        let deck = DeckSpec::compile(&names.map(str::to_string)).expect("validator fixture");
        let engine = deck.slot("Rhystic Study").unwrap();
        let first = deck.slot("Blank A").unwrap();
        let second = deck.slot("Blank B").unwrap();
        let game = OpeningReplayGame {
            game_index: 1,
            hand: vec!["Rhystic Study".to_string()],
            bottomed: Vec::new(),
            library_top: Vec::new(),
            library_order: ["Blank A", "Blank B"].map(str::to_string).to_vec(),
            gemstone_caverns_live: false,
            legacy_hit: false,
            legacy_capped: false,
            legacy_turn: None,
            legacy_engine_label: None,
        };
        (deck, game, engine, first, second)
    }

    fn draw_witness(
        first: SlotId,
        second: SlotId,
        chosen: SlotId,
    ) -> OpeningWitnessTransition<PackedStateV2> {
        let mut library_cards = CardMask::EMPTY;
        library_cards.insert(first);
        library_cards.insert(second);
        let before = PackedStateV2 {
            library: PackedLibrary::new(library_cards),
            ..PackedStateV2::default()
        };
        let mut after = before;
        assert!(after.draw(chosen));
        OpeningWitnessTransition {
            before,
            after,
            chance_probability: 0.5,
            is_chance: true,
        }
    }

    #[test]
    fn full_library_witness_accepts_recorded_draw_and_rejects_mismatch() {
        let (deck, game, _engine, first, second) = validation_fixture();
        let confirmed =
            validate_full_library_witness(&deck, &game, &[draw_witness(first, second, first)]);
        assert_eq!(confirmed.status, OpeningWitnessValidationStatus::Confirmed);

        let invalid =
            validate_full_library_witness(&deck, &game, &[draw_witness(first, second, second)]);
        assert_eq!(invalid.status, OpeningWitnessValidationStatus::Invalid);
    }

    #[test]
    fn full_library_witness_marks_consumed_post_shuffle_draw_probabilistic() {
        let (deck, game, _engine, first, second) = validation_fixture();
        let mut cards = CardMask::EMPTY;
        cards.insert(first);
        cards.insert(second);
        let before_search = PackedStateV2 {
            library: PackedLibrary::new(cards),
            ..PackedStateV2::default()
        };
        let mut after_search = before_search;
        assert!(after_search.library.remove_known_or_unknown(second));
        let search = OpeningWitnessTransition {
            before: before_search,
            after: after_search,
            chance_probability: 1.0,
            is_chance: false,
        };
        let mut after_draw = after_search;
        assert!(after_draw.draw(first));
        let draw = OpeningWitnessTransition {
            before: after_search,
            after: after_draw,
            chance_probability: 1.0,
            is_chance: true,
        };
        let mut after_use = after_draw;
        assert!(after_use.move_card(
            first,
            crate::nextgen::Zone::Hand,
            crate::nextgen::Zone::Graveyard
        ));
        let consume = OpeningWitnessTransition {
            before: after_draw,
            after: after_use,
            chance_probability: 1.0,
            is_chance: false,
        };

        let validation = validate_full_library_witness(&deck, &game, &[search, draw, consume]);
        assert_eq!(
            validation.status,
            OpeningWitnessValidationStatus::Probabilistic
        );
    }

    #[test]
    fn full_library_witness_does_not_treat_known_top_recursion_as_shuffle() {
        let (deck, mut game, _engine, first, second) = validation_fixture();
        game.library_order = vec!["Blank A".to_string()];
        let mut library = CardMask::EMPTY;
        library.insert(first);
        let before_revival = PackedStateV2 {
            graveyard: [second].into_iter().collect(),
            library: PackedLibrary::new(library),
            ..PackedStateV2::default()
        };
        let mut after_revival = before_revival;
        after_revival.graveyard.remove(second);
        after_revival.library.insert_unknown(second);
        after_revival.library.push_known_top(second);
        let revival = OpeningWitnessTransition {
            before: before_revival,
            after: after_revival,
            chance_probability: 1.0,
            is_chance: false,
        };
        let mut after_forced_draw = after_revival;
        assert!(after_forced_draw.draw(second));
        let forced_draw = OpeningWitnessTransition {
            before: after_revival,
            after: after_forced_draw,
            chance_probability: 1.0,
            is_chance: true,
        };
        let mut after_recorded_draw = after_forced_draw;
        assert!(after_recorded_draw.draw(first));
        let recorded_draw = OpeningWitnessTransition {
            before: after_forced_draw,
            after: after_recorded_draw,
            chance_probability: 1.0,
            is_chance: true,
        };
        let mut terminal = after_recorded_draw;
        assert!(terminal.move_card(
            first,
            crate::nextgen::Zone::Hand,
            crate::nextgen::Zone::Graveyard
        ));
        let consume = OpeningWitnessTransition {
            before: after_recorded_draw,
            after: terminal,
            chance_probability: 1.0,
            is_chance: false,
        };

        let validation = validate_full_library_witness(
            &deck,
            &game,
            &[revival, forced_draw, recorded_draw, consume],
        );
        assert_eq!(validation.status, OpeningWitnessValidationStatus::Confirmed);
    }

    #[test]
    fn replay_evaluates_an_explicit_keep_without_library_order_lookahead() {
        let request = OpeningReplayRequest {
            deck: [
                "Ancient Tomb",
                "Lotus Petal",
                "Rhystic Study",
                "Blank A",
                "Blank B",
                "Blank C",
                "Blank D",
            ]
            .map(str::to_string)
            .to_vec(),
            games: vec![OpeningReplayGame {
                game_index: 17,
                hand: ["Ancient Tomb", "Lotus Petal", "Rhystic Study"]
                    .map(str::to_string)
                    .to_vec(),
                bottomed: ["Blank A", "Blank B", "Blank C", "Blank D"]
                    .map(str::to_string)
                    .to_vec(),
                library_top: Vec::new(),
                library_order: Vec::new(),
                gemstone_caverns_live: false,
                legacy_hit: true,
                legacy_capped: false,
                legacy_turn: Some(1),
                legacy_engine_label: Some("Rhystic Study".to_string()),
            }],
            max_turn: 2,
            depth: 8,
            discrepancy_budgets: vec![0, 1],
            action_candidate_limit: 2,
            workers: 1,
            existence_only: false,
            validate_witnesses: false,
        };

        let response = evaluate_opening_replay(&request).expect("replay");
        assert_eq!(response.games[0].game_index, 17);
        assert_eq!(response.games[0].tiers[0].outcome.rhystic_turn_1, 1.0);
        assert_eq!(response.games[0].tiers[1].outcome.rhystic_turn_1, 1.0);
    }

    #[test]
    fn discrepancy_escalation_preserves_deep_prefork_lines() {
        let payload: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/decks/champion_working_list.json"
        ))
        .expect("champion fixture JSON");
        let deck = payload["deck"]
            .as_array()
            .expect("deck array")
            .iter()
            .map(|card| card.as_str().expect("card name").to_string())
            .collect();
        let games = vec![
            OpeningReplayGame {
                game_index: 779,
                hand: [
                    "City of Brass",
                    "Crop Rotation",
                    "Lion's Eye Diamond",
                    "Lotho, Corrupt Shirriff",
                    "Starting Town",
                    "Summoner's Pact",
                    "Wishclaw Talisman",
                ]
                .map(str::to_string)
                .to_vec(),
                bottomed: Vec::new(),
                library_top: Vec::new(),
                library_order: Vec::new(),
                gemstone_caverns_live: false,
                legacy_hit: true,
                legacy_capped: false,
                legacy_turn: Some(2),
                legacy_engine_label: Some("Rhystic Study".to_string()),
            },
            OpeningReplayGame {
                game_index: 283,
                hand: [
                    "Diabolic Intent",
                    "Forbidden Orchard",
                    "Lion's Eye Diamond",
                    "Summoner's Pact",
                    "Tropical Island",
                ]
                .map(str::to_string)
                .to_vec(),
                bottomed: ["Mental Misstep", "Noxious Revival"]
                    .map(str::to_string)
                    .to_vec(),
                library_top: Vec::new(),
                library_order: Vec::new(),
                gemstone_caverns_live: false,
                legacy_hit: true,
                legacy_capped: false,
                legacy_turn: Some(2),
                legacy_engine_label: Some("Heartwood Storyteller".to_string()),
            },
            OpeningReplayGame {
                game_index: 976,
                hand: [
                    "An Offer You Can't Refuse",
                    "Lion's Eye Diamond",
                    "Marsh Flats",
                    "Mystical Tutor",
                    "Summoner's Pact",
                ]
                .map(str::to_string)
                .to_vec(),
                bottomed: ["Force of Will", "Swan Song"].map(str::to_string).to_vec(),
                library_top: Vec::new(),
                library_order: Vec::new(),
                gemstone_caverns_live: false,
                legacy_hit: true,
                legacy_capped: false,
                legacy_turn: Some(2),
                legacy_engine_label: Some("Rhystic Study".to_string()),
            },
        ];
        let response = evaluate_opening_replay(&OpeningReplayRequest {
            deck,
            games,
            max_turn: 2,
            depth: 14,
            discrepancy_budgets: vec![2, 3, 4],
            action_candidate_limit: 4,
            workers: 1,
            existence_only: false,
            validate_witnesses: false,
        })
        .expect("deep recall replay");

        for game in &response.games {
            if game.game_index == 976 {
                assert!(game
                    .tiers
                    .iter()
                    .all(|tier| tier.outcome.weighted_ev == 0.0));
                continue;
            }
            let first_positive = game
                .tiers
                .iter()
                .find(|tier| tier.outcome.weighted_ev > 0.0)
                .unwrap_or_else(|| panic!("legacy line recovered for game {}", game.game_index))
                .discrepancy_budget;
            assert!(first_positive <= 3);
        }
    }
}
