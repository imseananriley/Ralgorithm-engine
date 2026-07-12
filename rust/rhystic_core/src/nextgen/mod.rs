//! Fixed-size primitives for the nonanticipating reference and policy engines.

mod batch;
mod benchmark;
mod card_mask;
mod card_spec;
mod library;
mod mana_closure;
mod metrics;
mod multifidelity;
mod opening_batch;
mod opening_model;
mod policy;
mod reference_solver;
mod resource_transition;
mod state;
mod state_v2;

pub use batch::{
    independent_sample_selected, merge_batch_reports, slot_permutation, BatchConfig,
    BatchEvaluation, BatchEvaluator, BatchReport, DiscordanceRecord, VariantAccumulator,
    VariantSummary,
};
pub use benchmark::{
    bench_nextgen, bench_opening_model, bench_packed_state_v2, NextgenBenchReport,
    OpeningModelBenchReport, PackedStateV2BenchReport,
};
pub use card_mask::{CardMask, SlotId, MAX_DECK_SLOTS};
pub use card_spec::{
    ActionClass, ActionTemplateMask, CardFlags, CardMetadata, CardSpec, DeckSpec,
    OpeningArtifactKind, OpeningCreatureKind, OpeningLandKind, OpeningManaProfile,
    OpeningSpellKind,
};
pub use library::{
    ChanceDraw, ClassChanceDraw, PackedLibrary, KNOWN_BOTTOM_CAPACITY, KNOWN_TOP_CAPACITY,
};
pub use mana_closure::{
    compute_mana_closure, compute_payment_plans, ManaOption, ManaOutcome, ManaPool, ManaSource,
    PaymentPlan, ResourceConsumption, ResourceUse,
};
pub use metrics::SearchMetrics;
pub use multifidelity::{
    evaluate_multifidelity, Estimate, MultiFidelityAccumulator, MultiFidelityConfig,
    MultiFidelityReport,
};
pub use opening_batch::{
    evaluate_opening_batch, OpeningBatchRequest, OpeningBatchResponse, OpeningVariantSpec,
};
pub use opening_model::{EngineOpeningModel, OpeningMulliganPolicy, VisibleOpeningPolicy};
pub use policy::{evaluate_compiled_policy, CompiledPolicy, PolicyResult};
pub use reference_solver::{
    InformationModel, InformationTransition, ReferenceResult, ReferenceSolver,
};
pub use resource_transition::apply_payment_plan;
pub use state::{PackedState, Zone};
pub use state_v2::{
    CommanderState, CommanderZone, PackedStateV2, PermanentInstance, PermanentSet, PermanentSource,
    TokenKind, MAX_BATTLEFIELD_PERMANENTS, NO_ATTACHMENT,
};

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use smallvec::{smallvec, SmallVec};

    use super::*;

    #[test]
    fn card_mask_round_trips_all_commander_slots() {
        let mask: CardMask = (0..99).map(|slot| slot as SlotId).collect();
        assert_eq!(mask.len(), 99);
        assert_eq!(
            mask.iter().collect::<Vec<_>>(),
            (0..99).map(|slot| slot as SlotId).collect::<Vec<_>>()
        );
    }

    #[test]
    fn manamorphose_payment_search_matches_supported_hybrid_costs() {
        let deck = DeckSpec::compile(&["Manamorphose".to_string()]).expect("valid card registry");
        assert_eq!(
            deck.cards()[0].payment_gate_costs,
            [Some([1, 0, 1, 0, 0, 0]), Some([1, 0, 0, 0, 0, 1])]
        );
    }

    #[test]
    fn library_exposes_only_known_top_or_uniform_unknown_draws() {
        let mut library = PackedLibrary::new([2, 7, 11].into_iter().collect());
        let unknown = library.chance_draws();
        assert_eq!(unknown.len(), 3);
        assert!(unknown
            .iter()
            .all(|draw| draw.numerator == 1 && draw.denominator == 3));

        library.push_known_top(7);
        assert_eq!(library.card_count(), 3);
        assert_eq!(
            library.chance_draws(),
            vec![ChanceDraw {
                slot: 7,
                numerator: 1,
                denominator: 1
            }]
        );
        assert!(library.draw(7));
        assert_eq!(library.card_count(), 2);
    }

    #[test]
    fn library_can_aggregate_only_verified_semantic_classes() {
        let library = PackedLibrary::new([2, 7, 11, 20].into_iter().collect());
        let mut classes = [0u8; 128];
        classes[2] = 1;
        classes[7] = 1;
        classes[11] = 2;
        classes[20] = 2;
        let draws = library.class_chance_draws(&classes);
        assert_eq!(draws.len(), 2);
        assert!(draws
            .iter()
            .all(|draw| draw.numerator == 2 && draw.denominator == 4));
    }

    #[test]
    fn library_shuffle_forgets_known_top_without_losing_cards() {
        let mut library = PackedLibrary::new([2].into_iter().collect());
        library.push_known_top(7);
        library.push_known_top(11);
        assert_eq!(library.known_top_len(), 2);

        library.shuffle_all_unknown();
        assert_eq!(library.known_top_len(), 0);
        assert_eq!(library.cards(), [2, 7, 11].into_iter().collect());
    }

    #[test]
    fn london_bottoms_are_drawn_only_after_the_unknown_library() {
        let mut library = PackedLibrary::new([2, 7, 11, 20].into_iter().collect());
        library.push_known_bottom(7);
        library.push_known_bottom(11);

        assert_eq!(library.known_bottom_len(), 2);
        assert_eq!(
            library.chance_draws(),
            vec![
                ChanceDraw {
                    slot: 2,
                    numerator: 1,
                    denominator: 2,
                },
                ChanceDraw {
                    slot: 20,
                    numerator: 1,
                    denominator: 2,
                },
            ]
        );
        assert!(!library.draw(7));
        assert!(library.draw(2));
        assert!(library.draw(20));
        assert_eq!(library.chance_draws()[0].slot, 7);
        assert!(library.draw(7));
        assert_eq!(library.chance_draws()[0].slot, 11);
    }

    #[test]
    fn tutors_can_remove_bottomed_cards_and_shuffles_forget_bottom_order() {
        let mut library = PackedLibrary::new([2, 7, 11].into_iter().collect());
        library.push_known_bottom(7);
        assert!(library.remove_known_or_unknown(7));
        assert_eq!(library.known_bottom_len(), 0);
        assert_eq!(library.cards(), [2, 11].into_iter().collect());

        library.insert_unknown(7);
        library.push_known_bottom(7);
        library.shuffle_all_unknown();
        assert_eq!(library.known_bottom_len(), 0);
        assert_eq!(library.cards(), [2, 7, 11].into_iter().collect());
        assert_eq!(library.chance_draws().len(), 3);
    }

    #[test]
    fn packed_state_zone_moves_preserve_disjointness() {
        let mut state = PackedState::default();
        state.hand.insert(4);
        assert!(state.move_card(4, Zone::Hand, Zone::Battlefield));
        state.tapped.insert(4);
        assert!(state.all_zones_disjoint());
        assert!(state.move_card(4, Zone::Battlefield, Zone::Graveyard));
        assert!(!state.tapped.contains(4));
        assert!(state.all_zones_disjoint());
    }

    #[test]
    fn packed_state_stays_fixed_and_small() {
        assert!(size_of::<PackedLibrary>() <= 32);
        assert!(size_of::<PackedState>() <= 160);
        assert!(PackedState::default().all_zones_disjoint());
    }

    #[test]
    fn packed_state_v2_is_canonical_and_supports_duplicate_tokens() {
        let card = PermanentInstance::new(PermanentSource::card(4))
            .with_counters(2)
            .with_tapped(true);
        let treasure = PermanentInstance::new(PermanentSource::token(TokenKind::Treasure));
        let mut left = PackedStateV2::default();
        left.hand.insert(4);
        assert!(left.move_card_to_battlefield(4, Zone::Hand, card));
        assert!(left.add_token(TokenKind::Treasure));
        assert!(left.add_token(TokenKind::Treasure));

        let mut right = PackedStateV2::default();
        assert!(right.add_token(TokenKind::Treasure));
        right.hand.insert(4);
        assert!(right.move_card_to_battlefield(4, Zone::Hand, card));
        assert!(right.battlefield.insert(treasure));

        assert_eq!(left, right);
        assert_eq!(left.battlefield.len(), 3);
        assert!(left.is_valid());
    }

    #[test]
    fn packed_state_v2_tracks_commander_attachments_and_zone_disjointness() {
        let mut state = PackedStateV2::default();
        state.hand.insert(7);
        assert!(state.put_commander_on_battlefield(false, true));
        let mantle = PermanentInstance::new(PermanentSource::card(7))
            .with_attachment(PermanentSource::commander());
        assert!(state.move_card_to_battlefield(7, Zone::Hand, mantle));
        assert!(state.is_valid());

        assert!(state.return_commander_to_command_zone());
        assert_eq!(state.commander.zone, CommanderZone::Command);
        assert!(state.battlefield.as_slice()[0].attached_to().is_none());
        assert!(state.move_card_from_battlefield(7, Zone::Graveyard));
        assert!(state.graveyard.contains(7));
        assert!(state.is_valid());
    }

    #[test]
    fn packed_state_v2_stays_fixed_and_bounded() {
        assert!(size_of::<PermanentInstance>() <= 4);
        assert!(size_of::<PermanentSet>() <= 68);
        assert!(size_of::<PackedStateV2>() <= 176);
        let mut state = PackedStateV2::default();
        for _ in 0..MAX_BATTLEFIELD_PERMANENTS {
            assert!(state.add_token(TokenKind::Treasure));
        }
        assert!(!state.add_token(TokenKind::Treasure));
        assert!(state.is_valid());
    }

    #[test]
    fn opening_model_finds_deterministic_turn_two_rhystic() {
        let deck = DeckSpec::compile(&[
            "Command Tower".to_string(),
            "Ancient Tomb".to_string(),
            "Rhystic Study".to_string(),
            "Blank".to_string(),
        ])
        .expect("opening model deck");
        let model = EngineOpeningModel::compile(&deck, 2);
        let mut state = PackedStateV2::default();
        state.hand = [0, 1, 2].into_iter().collect();

        let result = ReferenceSolver::new(&model).solve(state, 12);
        assert_eq!(result.value, 0.75);
        assert!(result.metrics.states_expanded > 0);
    }

    #[test]
    fn opening_model_averages_unknown_draws_without_looking_ahead() {
        let deck = DeckSpec::compile(&[
            "Ancient Tomb".to_string(),
            "Command Tower".to_string(),
            "Rhystic Study".to_string(),
            "Blank".to_string(),
            "Blank 2".to_string(),
        ])
        .expect("opening model deck");
        let model = EngineOpeningModel::compile(&deck, 2);
        let exact_model = EngineOpeningModel::compile(&deck, 2).with_quotient_draws(false);
        let mut state = PackedStateV2 {
            library: PackedLibrary::new([1, 3, 4].into_iter().collect()),
            ..PackedStateV2::default()
        };
        state.hand = [0, 2].into_iter().collect();

        let result = ReferenceSolver::new(&model).solve(state, 12);
        let exact = ReferenceSolver::new(&exact_model).solve(state, 12);
        assert!((result.value - 0.5).abs() < 1e-12);
        assert_eq!(result.value, exact.value);
        assert!(result.metrics.states_expanded < exact.metrics.states_expanded);
        assert!(result.metrics.chance_nodes > 0);
    }

    #[test]
    fn opening_model_reports_unsupported_cards_as_inert() {
        let deck = DeckSpec::compile(&[
            "Command Tower".to_string(),
            "Rhystic Study".to_string(),
            "Polluted Delta".to_string(),
            "Chrome Mox".to_string(),
        ])
        .expect("opening model deck");
        let model = EngineOpeningModel::compile(&deck, 2);

        assert_eq!(model.supported_slots().len(), 4);
        assert_eq!(model.unsupported_slots().len(), 0);
    }

    #[test]
    fn opening_mana_profiles_are_compiled_by_the_shared_registry() {
        let deck = DeckSpec::compile(&[
            "Command Tower".to_string(),
            "Gemstone Caverns".to_string(),
            "Sink into Stupor".to_string(),
            "City of Traitors".to_string(),
            "Glimmervoid".to_string(),
        ])
        .expect("opening mana registry");

        assert_eq!(deck.card(0).opening_mana.color_mask, 0b1_1111);
        assert_eq!(deck.card(1).opening_mana.colorless, 1);
        assert!(deck.card(2).opening_mana.enters_tapped);
        assert_eq!(
            deck.card(3).opening_mana.kind,
            OpeningLandKind::CityOfTraitors
        );
        assert_eq!(deck.card(4).opening_mana.kind, OpeningLandKind::Glimmervoid);
    }

    #[test]
    fn land_only_reachability_matches_fast_oracle() {
        use crate::fast_engine::solve_keep_fast;
        use crate::SolveKeepRequest;

        let cases = [
            vec!["Command Tower", "Ancient Tomb", "Rhystic Study"],
            vec!["City of Traitors", "Command Tower", "Rhystic Study"],
            vec!["City of Traitors", "Ancient Tomb", "Rhystic Study"],
            vec!["Crystal Vein", "Command Tower", "Rhystic Study"],
            vec!["Glimmervoid", "Ancient Tomb", "Rhystic Study"],
            vec!["Gemstone Mine", "Ancient Tomb", "Rhystic Study"],
            vec!["Gemstone Caverns", "Ancient Tomb", "Rhystic Study"],
            vec!["Tundra", "Ancient Tomb", "Rhystic Study"],
            vec!["Tropical Island", "Ancient Tomb", "Heartwood Storyteller"],
            vec!["Sink into Stupor", "Ancient Tomb", "Rhystic Study"],
        ];

        for (case_index, cards) in cases.into_iter().enumerate() {
            let mut hand: Vec<String> = cards.into_iter().map(str::to_string).collect();
            while hand.len() < 7 {
                hand.push(format!("Blank {case_index} {}", hand.len()));
            }
            let deck = DeckSpec::compile(&hand).expect("land parity deck");
            let model = EngineOpeningModel::compile(&deck, 2).with_resource_microsteps(false);
            let state = PackedStateV2 {
                hand: deck.card_mask(),
                ..PackedStateV2::default()
            };
            let packed_hit = ReferenceSolver::new(&model).solve(state, 16).value > 0.0;
            let fast = solve_keep_fast(&SolveKeepRequest {
                hand: hand.clone(),
                library: Vec::new(),
                gemstone_live: false,
                state_limit: 20_000,
                max_turns: 2,
                goal: "engine".to_string(),
                engine_target_count: 1,
                engine_success_policy: "resilient".to_string(),
                remora_upkeep_payments: 2,
                action_sort: true,
                gamble_mode: None,
                gamble_seed: None,
                simplified_gamble: false,
            });
            assert_eq!(
                packed_hit,
                fast.turn.is_some(),
                "land parity case {case_index}: {hand:?}"
            );
        }
    }

    #[test]
    fn packed_fetch_and_live_caverns_match_fast_oracle_lines() {
        use crate::fast_engine::solve_keep_fast;
        use crate::SolveKeepRequest;

        let names = vec![
            "Polluted Delta".to_string(),
            "Ancient Tomb".to_string(),
            "Rhystic Study".to_string(),
            "Blank".to_string(),
            "Underground Sea".to_string(),
        ];
        let deck = DeckSpec::compile(&names).expect("fetch parity deck");
        let model = EngineOpeningModel::compile(&deck, 2).with_resource_microsteps(false);
        let mut library = PackedLibrary::new([4].into_iter().collect());
        library.push_known_top(3);
        let state = PackedStateV2 {
            hand: [0, 1, 2].into_iter().collect(),
            library,
            ..PackedStateV2::default()
        };
        let packed = ReferenceSolver::new(&model).solve(state, 16);
        let fast = solve_keep_fast(&SolveKeepRequest {
            hand: names[..3].to_vec(),
            library: vec![names[3].clone(), names[4].clone()],
            gemstone_live: false,
            state_limit: 20_000,
            max_turns: 2,
            goal: "engine".to_string(),
            engine_target_count: 1,
            engine_success_policy: "resilient".to_string(),
            remora_upkeep_payments: 2,
            action_sort: true,
            gamble_mode: None,
            gamble_seed: None,
            simplified_gamble: false,
        });
        assert_eq!(packed.value, 0.75);
        assert_eq!(fast.turn, Some(2));

        let cavern_hand = vec![
            "Gemstone Caverns".to_string(),
            "Ancient Tomb".to_string(),
            "Rhystic Study".to_string(),
            "Blank A".to_string(),
            "Blank B".to_string(),
            "Blank C".to_string(),
            "Blank D".to_string(),
        ];
        let cavern_deck = DeckSpec::compile(&cavern_hand).expect("Caverns parity deck");
        let cavern_model =
            EngineOpeningModel::compile(&cavern_deck, 2).with_resource_microsteps(false);
        let cavern_state = PackedStateV2 {
            hand: cavern_deck.card_mask(),
            ..PackedStateV2::default()
        };
        let packed_value = cavern_model
            .pregame_states(cavern_state, true)
            .into_iter()
            .map(|start| ReferenceSolver::new(&cavern_model).solve(start, 16).value)
            .fold(0.0, f64::max);
        let fast = solve_keep_fast(&SolveKeepRequest {
            hand: cavern_hand,
            library: Vec::new(),
            gemstone_live: true,
            state_limit: 20_000,
            max_turns: 2,
            goal: "engine".to_string(),
            engine_target_count: 1,
            engine_success_policy: "resilient".to_string(),
            remora_upkeep_payments: 2,
            action_sort: true,
            gamble_mode: None,
            gamble_seed: None,
            simplified_gamble: false,
        });
        assert_eq!(packed_value, 1.0);
        assert_eq!(fast.turn, Some(1));
    }

    #[test]
    fn artifact_opening_reachability_matches_fast_oracle() {
        use crate::fast_engine::solve_keep_fast;
        use crate::SolveKeepRequest;

        let cases = [
            vec!["Ancient Tomb", "Lotus Petal", "Rhystic Study"],
            vec![
                "Ancient Tomb",
                "Chrome Mox",
                "Force of Will",
                "Rhystic Study",
            ],
            vec![
                "Ancient Tomb",
                "Mox Diamond",
                "Command Tower",
                "Rhystic Study",
            ],
            vec![
                "Ancient Tomb",
                "Mox Opal",
                "Lotus Petal",
                "Paradise Mantle",
                "Rhystic Study",
            ],
            vec![
                "City of Traitors",
                "Mana Vault",
                "Lotus Petal",
                "Rhystic Study",
            ],
            vec!["Command Tower", "Sol Ring", "Lotus Petal", "Rhystic Study"],
            vec!["Ancient Tomb", "Mox Opal", "Rhystic Study"],
            vec!["Ancient Tomb", "Chrome Mox", "Sol Ring", "Rhystic Study"],
        ];

        for (case_index, cards) in cases.into_iter().enumerate() {
            let mut hand: Vec<String> = cards.into_iter().map(str::to_string).collect();
            while hand.len() < 7 {
                hand.push(format!("Blank artifact {case_index} {}", hand.len()));
            }
            let deck = DeckSpec::compile(&hand).expect("artifact parity deck");
            let model = EngineOpeningModel::compile(&deck, 2).with_resource_microsteps(false);
            let state = PackedStateV2 {
                hand: deck.card_mask(),
                ..PackedStateV2::default()
            };
            let packed_hit = ReferenceSolver::new(&model).solve(state, 24).value > 0.0;
            let fast = solve_keep_fast(&SolveKeepRequest {
                hand: hand.clone(),
                library: Vec::new(),
                gemstone_live: false,
                state_limit: 100_000,
                max_turns: 2,
                goal: "engine".to_string(),
                engine_target_count: 1,
                engine_success_policy: "resilient".to_string(),
                remora_upkeep_payments: 2,
                action_sort: true,
                gamble_mode: None,
                gamble_seed: None,
                simplified_gamble: false,
            });
            assert_eq!(
                packed_hit,
                fast.turn.is_some(),
                "artifact parity case {case_index}: {hand:?}"
            );
        }
    }

    #[test]
    fn mana_closure_is_source_order_independent() {
        let rainbow = [
            ManaPool([1, 0, 0, 0, 0, 0]),
            ManaPool([0, 0, 1, 0, 0, 0]),
            ManaPool([0, 0, 0, 0, 1, 0]),
        ];
        let sources = vec![
            ManaSource::new(9, rainbow),
            ManaSource::new(2, [ManaPool([0, 0, 0, 0, 0, 2])]),
            ManaSource::new(5, [ManaPool([0, 1, 0, 0, 0, 0])]),
        ];
        let mut reversed = sources.clone();
        reversed.reverse();
        assert_eq!(
            compute_mana_closure(ManaPool::default(), &sources, 10),
            compute_mana_closure(ManaPool::default(), &reversed, 10)
        );
    }

    #[test]
    fn mana_closure_retains_distinct_color_and_consumption_choices() {
        let sources = [
            ManaSource::new(
                1,
                [ManaPool([1, 0, 0, 0, 0, 0]), ManaPool([0, 0, 1, 0, 0, 0])],
            ),
            ManaSource::new(2, [ManaPool([0, 0, 0, 0, 0, 2])]),
        ];
        let outcomes = compute_mana_closure(ManaPool::default(), &sources, 10);
        assert!(outcomes
            .iter()
            .any(|outcome| outcome.pool == ManaPool([1, 0, 0, 0, 0, 2])));
        assert!(outcomes
            .iter()
            .any(|outcome| outcome.pool == ManaPool([0, 0, 1, 0, 0, 2])));
        assert!(outcomes
            .iter()
            .any(|outcome| outcome.consumption.is_empty()));
    }

    #[test]
    fn payment_plans_collapse_irrelevant_excess_mana() {
        let rainbow = [
            ManaPool([1, 0, 0, 0, 0, 0]),
            ManaPool([0, 0, 1, 0, 0, 0]),
            ManaPool([0, 0, 0, 0, 1, 0]),
        ];
        let sources = [
            ManaSource::new(1, rainbow),
            ManaSource::new(2, [ManaPool([0, 0, 0, 0, 0, 2])]),
            ManaSource::new(3, rainbow),
            ManaSource::new(4, [ManaPool([3, 0, 0, 0, 0, 0])]),
        ];
        let all_outcomes = compute_mana_closure(ManaPool::default(), &sources, 10);
        let plans = compute_payment_plans(ManaPool::default(), &sources, [2, 0, 0, 1, 0, 0]);
        assert!(!plans.is_empty());
        assert!(plans.len() < all_outcomes.len());
        assert!(plans.iter().all(|plan| plan.consumption.used().len() >= 2));
    }

    #[test]
    fn payment_plans_preserve_color_constraints() {
        let sources = [
            ManaSource::new(1, [ManaPool([1, 0, 0, 0, 0, 0])]),
            ManaSource::new(2, [ManaPool([0, 0, 0, 0, 0, 2])]),
        ];
        assert!(
            compute_payment_plans(ManaPool::default(), &sources, [2, 0, 0, 1, 0, 0]).is_empty()
        );
    }

    #[test]
    fn payment_plans_preserve_resource_disposition() {
        let sources = [
            ManaSource::with_options(
                1,
                [
                    ManaOption::new(ManaPool([0, 0, 0, 0, 0, 1]), ResourceUse::Tap),
                    ManaOption::new(ManaPool([0, 0, 0, 0, 0, 2]), ResourceUse::Sacrifice),
                ],
            ),
            ManaSource::with_options(
                2,
                [ManaOption::new(
                    ManaPool([0, 0, 1, 0, 0, 0]),
                    ResourceUse::Exile,
                )],
            ),
        ];
        let plans = compute_payment_plans(ManaPool::default(), &sources, [2, 0, 0, 1, 0, 0]);
        assert_eq!(plans.len(), 1);
        assert!(plans[0].consumption.sacrificed.contains(1));
        assert!(plans[0].consumption.exiled.contains(2));
        assert!(!plans[0].consumption.tapped.contains(1));
    }

    #[test]
    fn payment_transition_updates_zones_without_resource_microsteps() {
        let mut state = PackedState::default();
        state.battlefield.insert(1);
        state.battlefield.insert(2);
        state.hand.insert(3);
        let plan = PaymentPlan {
            leftover: ManaPool([0, 0, 1, 0, 0, 0]),
            consumption: ResourceConsumption {
                tapped: [1].into_iter().collect(),
                sacrificed: [2].into_iter().collect(),
                exiled: [3].into_iter().collect(),
            },
        };
        let paid = apply_payment_plan(state, plan).expect("legal payment transition");
        assert!(paid.tapped.contains(1));
        assert!(paid.graveyard.contains(2));
        assert!(paid.exile.contains(3));
        assert_eq!(ManaPool::unpack(paid.mana), plan.leftover);
        assert!(paid.all_zones_disjoint());
    }

    #[test]
    fn multifidelity_estimate_adds_independent_policy_correction() {
        let mut estimate = MultiFidelityAccumulator::default();
        for value in [0.0, 1.0, 1.0, 1.0] {
            estimate.push_low(value);
        }
        for (low, high) in [(0.0, 1.0), (1.0, 1.0)] {
            estimate.push_correction(low, high);
        }
        let result = estimate
            .estimate()
            .expect("both estimator levels are populated");
        assert_eq!(result.low_mean, 0.75);
        assert_eq!(result.correction_mean, 0.5);
        assert_eq!(result.mean, 1.25);
        assert!(result.standard_error > 0.0);
    }

    #[test]
    fn deck_spec_compiles_exact_slots_and_card_roles() {
        let payload = include_str!("../../../../fixtures/decks/champion_working_list.json");
        let value: serde_json::Value =
            serde_json::from_str(payload).expect("champion fixture JSON");
        let deck: Vec<String> = value["deck"]
            .as_array()
            .expect("deck array")
            .iter()
            .map(|card| card.as_str().expect("card name").to_string())
            .collect();
        let spec = DeckSpec::compile(&deck).expect("singleton Commander deck compiles");
        assert_eq!(spec.cards().len(), 99);
        assert_eq!(spec.card_mask().len(), 99);
        let rhystic = spec.card(spec.slot("Rhystic Study").expect("Rhystic slot"));
        assert!(rhystic.flags.contains(CardFlags::ENGINE));
        assert_eq!(rhystic.action_class, ActionClass::Engine);
        let tomb = spec.card(spec.slot("Ancient Tomb").expect("Tomb slot"));
        assert!(tomb.flags.contains(CardFlags::LAND));
        assert!(tomb.flags.contains(CardFlags::MANA));
        let demonic = spec.card(spec.slot("Demonic Tutor").expect("Demonic slot"));
        assert!(demonic.flags.contains(CardFlags::TUTOR));
        let heartwood = spec.card(spec.slot("Heartwood Storyteller").expect("Heartwood slot"));
        assert_eq!(
            spec.semantic_classes()[rhystic.slot as usize],
            rhystic.semantic_class
        );
        assert_ne!(rhystic.semantic_class, heartwood.semantic_class);
    }

    #[derive(Debug, Copy, Clone)]
    struct AnalyticInformationModel;

    impl InformationModel for AnalyticInformationModel {
        type State = u8;

        fn terminal_value(&self, state: Self::State) -> Option<f64> {
            match state {
                2 => Some(1.0),
                3 => Some(0.0),
                _ => None,
            }
        }

        fn transitions(
            &self,
            state: Self::State,
            out: &mut SmallVec<[InformationTransition<Self::State>; 16]>,
        ) {
            match state {
                0 => {
                    out.push(InformationTransition::Deterministic(1));
                    out.push(InformationTransition::Deterministic(1));
                }
                1 => out.push(InformationTransition::Chance(smallvec![(2, 1.0), (3, 3.0)])),
                _ => {}
            }
        }
    }

    struct FirstActionPolicy;

    impl CompiledPolicy<u8> for FirstActionPolicy {
        fn choose(&self, _state: u8, transitions: &[InformationTransition<u8>]) -> Option<usize> {
            (!transitions.is_empty()).then_some(0)
        }
    }

    #[test]
    fn reference_solver_normalizes_chance_and_hits_transpositions() {
        let result = ReferenceSolver::new(&AnalyticInformationModel).solve(0, 3);
        assert!((result.value - 0.25).abs() < f64::EPSILON);
        assert!(result.metrics.transposition_hits >= 1);
        assert_eq!(result.metrics.chance_nodes, 1);
    }

    #[test]
    fn compiled_policy_uses_the_same_information_chance_model() {
        let result = evaluate_compiled_policy(&AnalyticInformationModel, &FirstActionPolicy, 0, 3);
        assert!((result.value - 0.25).abs() < f64::EPSILON);
        assert_eq!(result.metrics.chance_nodes, 1);
    }
}
