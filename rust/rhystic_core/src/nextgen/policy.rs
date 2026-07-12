use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use std::hash::Hash;

use super::{InformationModel, InformationTransition, SearchMetrics};

pub trait CompiledPolicy<S> {
    fn choose(&self, state: S, transitions: &[InformationTransition<S>]) -> Option<usize>;
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PolicyResult {
    pub value: f64,
    pub metrics: SearchMetrics,
}

pub fn evaluate_compiled_policy<M, P>(
    model: &M,
    policy: &P,
    state: M::State,
    depth: u8,
) -> PolicyResult
where
    M: InformationModel,
    M::State: Copy + Eq + Hash,
    P: CompiledPolicy<M::State>,
{
    let mut evaluator = PolicyEvaluator {
        model,
        policy,
        table: FxHashMap::default(),
        metrics: SearchMetrics::default(),
    };
    let value = evaluator.value(state, depth);
    PolicyResult {
        value,
        metrics: evaluator.metrics,
    }
}

struct PolicyEvaluator<'a, M: InformationModel, P> {
    model: &'a M,
    policy: &'a P,
    table: FxHashMap<(M::State, u8), f64>,
    metrics: SearchMetrics,
}

impl<M, P> PolicyEvaluator<'_, M, P>
where
    M: InformationModel,
    M::State: Copy + Eq + Hash,
    P: CompiledPolicy<M::State>,
{
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
        self.metrics.states_expanded += 1;
        let mut transitions: SmallVec<[InformationTransition<M::State>; 16]> = SmallVec::new();
        self.model.transitions(state, &mut transitions);
        let generated = transitions.len();
        let mut deterministic = FxHashSet::default();
        transitions.retain(|transition| match transition {
            InformationTransition::Deterministic(next) => deterministic.insert(*next),
            InformationTransition::Chance(_) => true,
        });
        self.metrics.states_deduplicated += (generated - transitions.len()) as u64;
        self.metrics.strategic_actions_generated += transitions.len() as u64;
        let value = self
            .policy
            .choose(state, &transitions)
            .and_then(|index| transitions.into_iter().nth(index))
            .map(|transition| match transition {
                InformationTransition::Deterministic(next) => self.value(next, depth - 1),
                InformationTransition::Chance(outcomes) => {
                    self.metrics.chance_nodes += 1;
                    let total: f64 = outcomes.iter().map(|(_, probability)| probability).sum();
                    if total <= 0.0 {
                        0.0
                    } else {
                        outcomes
                            .into_iter()
                            .map(|(next, probability)| {
                                (probability / total) * self.value(next, depth - 1)
                            })
                            .sum()
                    }
                }
            })
            .unwrap_or(0.0);
        self.table.insert(key, value);
        value
    }
}
