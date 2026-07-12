use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::hash::Hash;

use super::{CompiledPolicy, InformationModel, InformationTransition, SearchMetrics};

#[derive(Debug, Copy, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpeningOutcome {
    pub weighted_ev: f64,
    pub rhystic_turn_1: f64,
    pub rhystic_turn_2: f64,
    pub heartwood_turn_1: f64,
    pub heartwood_turn_2: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MisleadingModel;

    impl InformationModel for MisleadingModel {
        type State = u8;

        fn terminal_value(&self, state: Self::State) -> Option<f64> {
            (state != 0).then_some(f64::from(state == 1))
        }

        fn transitions(
            &self,
            state: Self::State,
            out: &mut SmallVec<[InformationTransition<Self::State>; 16]>,
        ) {
            if state == 0 {
                out.push(InformationTransition::Deterministic(1));
                out.push(InformationTransition::Deterministic(2));
            }
        }
    }

    impl OpeningOutcomeModel for MisleadingModel {
        fn terminal_opening_outcome(&self, state: Self::State) -> Option<OpeningOutcome> {
            (state != 0).then_some(OpeningOutcome {
                weighted_ev: f64::from(state == 1),
                rhystic_turn_1: f64::from(state == 1),
                ..OpeningOutcome::default()
            })
        }

        fn transition_priority(
            &self,
            _state: Self::State,
            transition: &InformationTransition<Self::State>,
        ) -> i64 {
            match transition {
                InformationTransition::Deterministic(next) => i64::from(*next),
                InformationTransition::Chance(_) => 0,
            }
        }
    }

    #[test]
    fn discrepancy_budget_recovers_a_win_missed_by_greedy_policy() {
        let mut greedy = OpeningOutcomeDiscrepancySolver::new(&MisleadingModel, 2);
        assert_eq!(greedy.solve(0, 1, 0).outcome.weighted_ev, 0.0);

        let mut corrected = OpeningOutcomeDiscrepancySolver::new(&MisleadingModel, 2);
        assert_eq!(corrected.solve(0, 1, 1).outcome.rhystic_turn_1, 1.0);
    }
}

impl OpeningOutcome {
    pub const fn any_engine(self) -> f64 {
        self.rhystic_turn_1 + self.rhystic_turn_2 + self.heartwood_turn_1 + self.heartwood_turn_2
    }

    fn add_scaled(&mut self, other: Self, scale: f64) {
        self.weighted_ev += other.weighted_ev * scale;
        self.rhystic_turn_1 += other.rhystic_turn_1 * scale;
        self.rhystic_turn_2 += other.rhystic_turn_2 * scale;
        self.heartwood_turn_1 += other.heartwood_turn_1 * scale;
        self.heartwood_turn_2 += other.heartwood_turn_2 * scale;
    }
}

pub trait OpeningOutcomeModel: InformationModel {
    fn terminal_opening_outcome(&self, state: Self::State) -> Option<OpeningOutcome>;

    fn maximum_opening_value(&self, _state: Self::State) -> f64 {
        1.0
    }

    fn transition_priority(
        &self,
        _state: Self::State,
        _transition: &InformationTransition<Self::State>,
    ) -> i64 {
        0
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq)]
pub struct OpeningOutcomeResult {
    pub outcome: OpeningOutcome,
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub capped: bool,
    pub metrics: SearchMetrics,
}

#[derive(Debug, Copy, Clone, Default)]
struct BoundedOutcome {
    outcome: OpeningOutcome,
    upper_bound: f64,
}

pub struct OpeningOutcomeSolver<'a, M: OpeningOutcomeModel> {
    model: &'a M,
    table: FxHashMap<(M::State, u8), BoundedOutcome>,
    visiting: FxHashSet<(M::State, u8)>,
    metrics: SearchMetrics,
}

pub struct OpeningOutcomeDiscrepancySolver<'a, M: OpeningOutcomeModel> {
    model: &'a M,
    action_candidate_limit: usize,
    table: FxHashMap<(M::State, u8, u8), BoundedOutcome>,
    visiting: FxHashSet<(M::State, u8, u8)>,
    metrics: SearchMetrics,
}

impl<'a, M: OpeningOutcomeModel> OpeningOutcomeDiscrepancySolver<'a, M>
where
    M::State: Copy + Eq + Hash,
{
    pub fn new(model: &'a M, action_candidate_limit: usize) -> Self {
        Self {
            model,
            action_candidate_limit: action_candidate_limit.max(1),
            table: FxHashMap::default(),
            visiting: FxHashSet::default(),
            metrics: SearchMetrics::default(),
        }
    }

    pub fn solve(
        &mut self,
        state: M::State,
        depth: u8,
        discrepancy_budget: u8,
    ) -> OpeningOutcomeResult {
        let bounded = self.value(state, depth, discrepancy_budget);
        OpeningOutcomeResult {
            outcome: bounded.outcome,
            lower_bound: bounded.outcome.weighted_ev,
            upper_bound: bounded.upper_bound,
            capped: bounded.upper_bound > bounded.outcome.weighted_ev + f64::EPSILON,
            metrics: self.metrics,
        }
    }

    fn value(&mut self, state: M::State, depth: u8, discrepancies: u8) -> BoundedOutcome {
        if let Some(outcome) = self.model.terminal_opening_outcome(state) {
            self.metrics.terminal_successes += 1;
            return BoundedOutcome {
                outcome,
                upper_bound: outcome.weighted_ev,
            };
        }
        let maximum = self.model.maximum_opening_value(state);
        if depth == 0 {
            self.metrics.depth_cutoffs += 1;
            return BoundedOutcome {
                outcome: OpeningOutcome::default(),
                upper_bound: maximum,
            };
        }
        let key = (state, depth, discrepancies);
        if let Some(outcome) = self.table.get(&key) {
            self.metrics.transposition_hits += 1;
            return *outcome;
        }
        if !self.visiting.insert(key) {
            self.metrics.cycle_cutoffs += 1;
            return BoundedOutcome {
                outcome: OpeningOutcome::default(),
                upper_bound: maximum,
            };
        }

        self.metrics.states_expanded += 1;
        let mut transitions = SmallVec::new();
        self.model.transitions(state, &mut transitions);
        let generated = transitions.len();
        let mut deterministic = FxHashSet::default();
        transitions.retain(|transition| match transition {
            InformationTransition::Deterministic(next) => deterministic.insert(*next),
            InformationTransition::Chance(_) => true,
        });
        transitions.sort_unstable_by_key(|transition| {
            std::cmp::Reverse(self.model.transition_priority(state, transition))
        });
        self.metrics.states_deduplicated += (generated - transitions.len()) as u64;
        self.metrics.strategic_actions_generated += transitions.len() as u64;

        let mut best = BoundedOutcome::default();
        let searched = if discrepancies == 0 {
            1
        } else {
            self.action_candidate_limit
        };
        for (rank, transition) in transitions.into_iter().take(searched).enumerate() {
            let next_discrepancies = if rank == 0 {
                discrepancies
            } else if discrepancies == 0 {
                continue;
            } else {
                discrepancies - 1
            };
            let candidate = self.transition_value(transition, depth - 1, next_discrepancies);
            if candidate.outcome.weighted_ev > best.outcome.weighted_ev {
                best.outcome = candidate.outcome;
            }
            if best.outcome.weighted_ev + f64::EPSILON >= maximum {
                best.upper_bound = maximum;
                break;
            }
        }
        if best.upper_bound < maximum && best.outcome.weighted_ev + f64::EPSILON < maximum {
            best.upper_bound = maximum;
        }
        self.visiting.remove(&key);
        self.table.insert(key, best);
        best
    }

    fn transition_value(
        &mut self,
        transition: InformationTransition<M::State>,
        depth: u8,
        discrepancies: u8,
    ) -> BoundedOutcome {
        match transition {
            InformationTransition::Deterministic(next) => self.value(next, depth, discrepancies),
            InformationTransition::Chance(outcomes) => {
                self.metrics.chance_nodes += 1;
                let total: f64 = outcomes.iter().map(|(_, probability)| probability).sum();
                let mut expected = BoundedOutcome::default();
                if total > 0.0 {
                    for (next, probability) in outcomes {
                        let child = self.value(next, depth, discrepancies);
                        let weight = probability / total;
                        expected.outcome.add_scaled(child.outcome, weight);
                        expected.upper_bound += child.upper_bound * weight;
                    }
                }
                expected
            }
        }
    }
}

impl<'a, M: OpeningOutcomeModel> OpeningOutcomeSolver<'a, M>
where
    M::State: Copy + Eq + Hash,
{
    pub fn new(model: &'a M) -> Self {
        Self {
            model,
            table: FxHashMap::default(),
            visiting: FxHashSet::default(),
            metrics: SearchMetrics::default(),
        }
    }

    pub fn solve(&mut self, state: M::State, depth: u8) -> OpeningOutcomeResult {
        let bounded = self.value(state, depth);
        OpeningOutcomeResult {
            outcome: bounded.outcome,
            lower_bound: bounded.outcome.weighted_ev,
            upper_bound: bounded.upper_bound,
            capped: bounded.upper_bound > bounded.outcome.weighted_ev + f64::EPSILON,
            metrics: self.metrics,
        }
    }

    pub fn solve_many(
        &mut self,
        states: impl IntoIterator<Item = M::State>,
        depth: u8,
    ) -> Vec<OpeningOutcome> {
        states
            .into_iter()
            .map(|state| self.value(state, depth).outcome)
            .collect()
    }

    fn value(&mut self, state: M::State, depth: u8) -> BoundedOutcome {
        if let Some(outcome) = self.model.terminal_opening_outcome(state) {
            self.metrics.terminal_successes += 1;
            return BoundedOutcome {
                outcome,
                upper_bound: outcome.weighted_ev,
            };
        }
        if depth == 0 {
            self.metrics.depth_cutoffs += 1;
            return BoundedOutcome {
                outcome: OpeningOutcome::default(),
                upper_bound: self.model.maximum_opening_value(state),
            };
        }
        let key = (state, depth);
        if let Some(outcome) = self.table.get(&key) {
            self.metrics.transposition_hits += 1;
            return *outcome;
        }
        if !self.visiting.insert(key) {
            self.metrics.cycle_cutoffs += 1;
            return BoundedOutcome {
                outcome: OpeningOutcome::default(),
                upper_bound: self.model.maximum_opening_value(state),
            };
        }

        self.metrics.states_expanded += 1;
        let mut transitions = SmallVec::new();
        self.model.transitions(state, &mut transitions);
        let generated = transitions.len();
        let mut deterministic = FxHashSet::default();
        transitions.retain(|transition| match transition {
            InformationTransition::Deterministic(next) => deterministic.insert(*next),
            InformationTransition::Chance(_) => true,
        });
        transitions.sort_unstable_by_key(|transition| {
            std::cmp::Reverse(self.model.transition_priority(state, transition))
        });
        self.metrics.states_deduplicated += (generated - transitions.len()) as u64;
        self.metrics.strategic_actions_generated += transitions.len() as u64;
        let mut best = BoundedOutcome::default();
        let maximum = self.model.maximum_opening_value(state);
        for transition in transitions {
            let candidate = self.transition_value(transition, depth - 1);
            best.upper_bound = best.upper_bound.max(candidate.upper_bound);
            if candidate.outcome.weighted_ev > best.outcome.weighted_ev {
                best.outcome = candidate.outcome;
            }
            if best.outcome.weighted_ev + f64::EPSILON >= maximum {
                best.upper_bound = maximum;
                break;
            }
        }
        self.visiting.remove(&key);
        self.table.insert(key, best);
        best
    }

    fn transition_value(
        &mut self,
        transition: InformationTransition<M::State>,
        depth: u8,
    ) -> BoundedOutcome {
        match transition {
            InformationTransition::Deterministic(next) => self.value(next, depth),
            InformationTransition::Chance(outcomes) => {
                self.metrics.chance_nodes += 1;
                let total: f64 = outcomes.iter().map(|(_, probability)| probability).sum();
                let mut expected = BoundedOutcome::default();
                if total > 0.0 {
                    for (next, probability) in outcomes {
                        let child = self.value(next, depth);
                        let weight = probability / total;
                        expected.outcome.add_scaled(child.outcome, weight);
                        expected.upper_bound += child.upper_bound * weight;
                    }
                }
                expected
            }
        }
    }
}

pub fn evaluate_opening_outcome_policy<M, P>(
    model: &M,
    policy: &P,
    state: M::State,
    depth: u8,
) -> OpeningOutcomeResult
where
    M: OpeningOutcomeModel,
    M::State: Copy + Eq + Hash,
    P: CompiledPolicy<M::State>,
{
    OpeningOutcomePolicySolver::new(model, policy).solve(state, depth)
}

pub struct OpeningOutcomePolicySolver<'a, M: OpeningOutcomeModel, P> {
    model: &'a M,
    policy: &'a P,
    table: FxHashMap<(M::State, u8), BoundedOutcome>,
    metrics: SearchMetrics,
}

impl<'a, M, P> OpeningOutcomePolicySolver<'a, M, P>
where
    M: OpeningOutcomeModel,
    M::State: Copy + Eq + Hash,
    P: CompiledPolicy<M::State>,
{
    pub fn new(model: &'a M, policy: &'a P) -> Self {
        Self {
            model,
            policy,
            table: FxHashMap::default(),
            metrics: SearchMetrics::default(),
        }
    }

    pub fn solve(&mut self, state: M::State, depth: u8) -> OpeningOutcomeResult {
        let bounded = self.value(state, depth);
        OpeningOutcomeResult {
            outcome: bounded.outcome,
            lower_bound: bounded.outcome.weighted_ev,
            upper_bound: bounded.upper_bound,
            capped: bounded.upper_bound > bounded.outcome.weighted_ev + f64::EPSILON,
            metrics: self.metrics,
        }
    }

    pub fn solve_many(
        &mut self,
        states: impl IntoIterator<Item = M::State>,
        depth: u8,
    ) -> Vec<OpeningOutcome> {
        states
            .into_iter()
            .map(|state| self.value(state, depth).outcome)
            .collect()
    }

    fn value(&mut self, state: M::State, depth: u8) -> BoundedOutcome {
        if let Some(outcome) = self.model.terminal_opening_outcome(state) {
            self.metrics.terminal_successes += 1;
            return BoundedOutcome {
                outcome,
                upper_bound: outcome.weighted_ev,
            };
        }
        if depth == 0 {
            self.metrics.depth_cutoffs += 1;
            return BoundedOutcome {
                outcome: OpeningOutcome::default(),
                upper_bound: 1.0,
            };
        }
        let key = (state, depth);
        if let Some(outcome) = self.table.get(&key) {
            self.metrics.transposition_hits += 1;
            return *outcome;
        }
        self.metrics.states_expanded += 1;
        let mut transitions = SmallVec::new();
        self.model.transitions(state, &mut transitions);
        let generated = transitions.len();
        let mut deterministic = FxHashSet::default();
        transitions.retain(|transition| match transition {
            InformationTransition::Deterministic(next) => deterministic.insert(*next),
            InformationTransition::Chance(_) => true,
        });
        self.metrics.states_deduplicated += (generated - transitions.len()) as u64;
        self.metrics.strategic_actions_generated += transitions.len() as u64;
        let bounded = self
            .policy
            .choose(state, &transitions)
            .and_then(|index| transitions.into_iter().nth(index))
            .map(|transition| self.transition_value(transition, depth - 1))
            .unwrap_or_default();
        self.table.insert(key, bounded);
        bounded
    }

    fn transition_value(
        &mut self,
        transition: InformationTransition<M::State>,
        depth: u8,
    ) -> BoundedOutcome {
        match transition {
            InformationTransition::Deterministic(next) => self.value(next, depth),
            InformationTransition::Chance(outcomes) => {
                self.metrics.chance_nodes += 1;
                let total: f64 = outcomes.iter().map(|(_, probability)| probability).sum();
                let mut expected = BoundedOutcome::default();
                if total > 0.0 {
                    for (next, probability) in outcomes {
                        let child = self.value(next, depth);
                        let weight = probability / total;
                        expected.outcome.add_scaled(child.outcome, weight);
                        expected.upper_bound += child.upper_bound * weight;
                    }
                }
                expected
            }
        }
    }
}
