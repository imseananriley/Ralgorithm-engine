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
                upper_bound: 1.0,
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
                upper_bound: 1.0,
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
        self.metrics.states_deduplicated += (generated - transitions.len()) as u64;
        self.metrics.strategic_actions_generated += transitions.len() as u64;
        let mut best = BoundedOutcome::default();
        for transition in transitions {
            let candidate = self.transition_value(transition, depth - 1);
            best.upper_bound = best.upper_bound.max(candidate.upper_bound);
            if candidate.outcome.weighted_ev > best.outcome.weighted_ev {
                best.outcome = candidate.outcome;
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
