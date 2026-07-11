//! Fixed-size primitives for the nonanticipating reference and policy engines.

mod benchmark;
mod card_mask;
mod library;
mod mana_closure;
mod metrics;
mod multifidelity;
mod resource_transition;
mod state;

pub use benchmark::{bench_nextgen, NextgenBenchReport};
pub use card_mask::{CardMask, SlotId, MAX_DECK_SLOTS};
pub use library::{ChanceDraw, ClassChanceDraw, PackedLibrary, KNOWN_TOP_CAPACITY};
pub use mana_closure::{
    compute_mana_closure, compute_payment_plans, ManaOption, ManaOutcome, ManaPool, ManaSource,
    PaymentPlan, ResourceConsumption, ResourceUse,
};
pub use metrics::SearchMetrics;
pub use multifidelity::{Estimate, MultiFidelityAccumulator};
pub use resource_transition::apply_payment_plan;
pub use state::{PackedState, Zone};

#[cfg(test)]
mod tests {
    use std::mem::size_of;

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
}
