use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::{
    CardMask, DeckSpec, EngineOpeningModel, OpeningOutcome, OpeningOutcomeDiscrepancySolver,
    OpeningOutcomeResult, PackedLibrary, PackedStateV2, SearchMetrics,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningReplayGame {
    pub game_index: u64,
    pub hand: Vec<String>,
    #[serde(default)]
    pub bottomed: Vec<String>,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningReplayTierOutcome {
    pub discrepancy_budget: u8,
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
    let state = PackedStateV2 {
        hand,
        library,
        ..PackedStateV2::default()
    };
    let pregame_states = model.pregame_states(state, game.gemstone_caverns_live);
    let mut tiers = Vec::with_capacity(discrepancy_budgets.len());
    for &budget in discrepancy_budgets {
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
        };

        let response = evaluate_opening_replay(&request).expect("replay");
        assert_eq!(response.games[0].game_index, 17);
        assert_eq!(response.games[0].tiers[0].outcome.rhystic_turn_1, 1.0);
        assert_eq!(response.games[0].tiers[1].outcome.rhystic_turn_1, 1.0);
    }
}
