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
    pub metrics: SearchMetrics,
}

pub struct OpeningOutcomeSolver<'a, M: OpeningOutcomeModel> {
    model: &'a M,
    table: FxHashMap<(M::State, u8), OpeningOutcome>,
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

    pub fn solve(mut self, state: M::State, depth: u8) -> OpeningOutcomeResult {
        let outcome = self.value(state, depth);
        OpeningOutcomeResult {
            outcome,
            metrics: self.metrics,
        }
    }

    fn value(&mut self, state: M::State, depth: u8) -> OpeningOutcome {
        if let Some(outcome) = self.model.terminal_opening_outcome(state) {
            self.metrics.terminal_successes += 1;
            return outcome;
        }
        if depth == 0 {
            self.metrics.depth_cutoffs += 1;
            return OpeningOutcome::default();
        }
        let key = (state, depth);
        if let Some(outcome) = self.table.get(&key) {
            self.metrics.transposition_hits += 1;
            return *outcome;
        }
        if !self.visiting.insert(key) {
            self.metrics.cycle_cutoffs += 1;
            return OpeningOutcome::default();
        }

        self.metrics.states_expanded += 1;
        let mut transitions = SmallVec::new();
        self.model.transitions(state, &mut transitions);
        self.metrics.strategic_actions_generated += transitions.len() as u64;
        let mut best = OpeningOutcome::default();
        for transition in transitions {
            let candidate = self.transition_value(transition, depth - 1);
            if candidate.weighted_ev > best.weighted_ev {
                best = candidate;
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
    ) -> OpeningOutcome {
        match transition {
            InformationTransition::Deterministic(next) => self.value(next, depth),
            InformationTransition::Chance(outcomes) => {
                self.metrics.chance_nodes += 1;
                let total: f64 = outcomes.iter().map(|(_, probability)| probability).sum();
                let mut expected = OpeningOutcome::default();
                if total > 0.0 {
                    for (next, probability) in outcomes {
                        expected.add_scaled(self.value(next, depth), probability / total);
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
    let mut evaluator = OpeningOutcomePolicyEvaluator {
        model,
        policy,
        table: FxHashMap::default(),
        metrics: SearchMetrics::default(),
    };
    let outcome = evaluator.value(state, depth);
    OpeningOutcomeResult {
        outcome,
        metrics: evaluator.metrics,
    }
}

struct OpeningOutcomePolicyEvaluator<'a, M: OpeningOutcomeModel, P> {
    model: &'a M,
    policy: &'a P,
    table: FxHashMap<(M::State, u8), OpeningOutcome>,
    metrics: SearchMetrics,
}

impl<M, P> OpeningOutcomePolicyEvaluator<'_, M, P>
where
    M: OpeningOutcomeModel,
    M::State: Copy + Eq + Hash,
    P: CompiledPolicy<M::State>,
{
    fn value(&mut self, state: M::State, depth: u8) -> OpeningOutcome {
        if let Some(outcome) = self.model.terminal_opening_outcome(state) {
            self.metrics.terminal_successes += 1;
            return outcome;
        }
        if depth == 0 {
            self.metrics.depth_cutoffs += 1;
            return OpeningOutcome::default();
        }
        let key = (state, depth);
        if let Some(outcome) = self.table.get(&key) {
            self.metrics.transposition_hits += 1;
            return *outcome;
        }
        self.metrics.states_expanded += 1;
        let mut transitions = SmallVec::new();
        self.model.transitions(state, &mut transitions);
        self.metrics.strategic_actions_generated += transitions.len() as u64;
        let outcome = self
            .policy
            .choose(state, &transitions)
            .and_then(|index| transitions.into_iter().nth(index))
            .map(|transition| self.transition_value(transition, depth - 1))
            .unwrap_or_default();
        self.table.insert(key, outcome);
        outcome
    }

    fn transition_value(
        &mut self,
        transition: InformationTransition<M::State>,
        depth: u8,
    ) -> OpeningOutcome {
        match transition {
            InformationTransition::Deterministic(next) => self.value(next, depth),
            InformationTransition::Chance(outcomes) => {
                self.metrics.chance_nodes += 1;
                let total: f64 = outcomes.iter().map(|(_, probability)| probability).sum();
                let mut expected = OpeningOutcome::default();
                if total > 0.0 {
                    for (next, probability) in outcomes {
                        expected.add_scaled(self.value(next, depth), probability / total);
                    }
                }
                expected
            }
        }
    }
}
