use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use std::hash::Hash;

use super::SearchMetrics;

#[derive(Debug, Clone)]
pub enum InformationTransition<S> {
    Deterministic(S),
    Chance(SmallVec<[(S, f64); 8]>),
}

pub trait InformationModel {
    type State: Copy + Eq + Hash;

    fn terminal_value(&self, state: Self::State) -> Option<f64>;
    fn transitions(
        &self,
        state: Self::State,
        out: &mut SmallVec<[InformationTransition<Self::State>; 16]>,
    );
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReferenceResult {
    pub value: f64,
    pub metrics: SearchMetrics,
}

pub struct ReferenceSolver<'a, M: InformationModel> {
    model: &'a M,
    table: FxHashMap<(M::State, u8), f64>,
    visiting: FxHashSet<(M::State, u8)>,
    metrics: SearchMetrics,
}

impl<'a, M: InformationModel> ReferenceSolver<'a, M> {
    pub fn new(model: &'a M) -> Self {
        Self {
            model,
            table: FxHashMap::default(),
            visiting: FxHashSet::default(),
            metrics: SearchMetrics::default(),
        }
    }

    pub fn solve(mut self, state: M::State, depth: u8) -> ReferenceResult {
        let value = self.value(state, depth);
        ReferenceResult {
            value,
            metrics: self.metrics,
        }
    }

    fn value(&mut self, state: M::State, depth: u8) -> f64 {
        if let Some(value) = self.model.terminal_value(state) {
            if value > 0.0 {
                self.metrics.terminal_successes += 1;
            } else {
                self.metrics.terminal_failures += 1;
            }
            return value;
        }
        if depth == 0 {
            self.metrics.depth_cutoffs += 1;
            return 0.0;
        }
        let key = (state, depth);
        if let Some(value) = self.table.get(&key) {
            self.metrics.transposition_hits += 1;
            return *value;
        }
        if !self.visiting.insert(key) {
            self.metrics.cycle_cutoffs += 1;
            return 0.0;
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
        let mut best = 0.0f64;
        for transition in transitions {
            let candidate = match transition {
                InformationTransition::Deterministic(next) => self.value(next, depth - 1),
                InformationTransition::Chance(outcomes) => {
                    self.metrics.chance_nodes += 1;
                    let probability_sum: f64 =
                        outcomes.iter().map(|(_, probability)| probability).sum();
                    if probability_sum <= 0.0 {
                        0.0
                    } else {
                        outcomes
                            .into_iter()
                            .map(|(next, probability)| {
                                (probability / probability_sum) * self.value(next, depth - 1)
                            })
                            .sum()
                    }
                }
            };
            best = best.max(candidate);
        }
        self.visiting.remove(&key);
        self.table.insert(key, best);
        best
    }
}
