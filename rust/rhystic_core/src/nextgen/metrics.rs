use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMetrics {
    pub states_expanded: u64,
    pub states_deduplicated: u64,
    pub strategic_actions_generated: u64,
    pub resource_actions_collapsed: u64,
    pub mana_closure_calls: u64,
    pub mana_closure_outcomes: u64,
    pub transposition_hits: u64,
    pub chance_nodes: u64,
    pub terminal_successes: u64,
    pub terminal_failures: u64,
}

impl SearchMetrics {
    pub fn merge(&mut self, other: Self) {
        self.states_expanded += other.states_expanded;
        self.states_deduplicated += other.states_deduplicated;
        self.strategic_actions_generated += other.strategic_actions_generated;
        self.resource_actions_collapsed += other.resource_actions_collapsed;
        self.mana_closure_calls += other.mana_closure_calls;
        self.mana_closure_outcomes += other.mana_closure_outcomes;
        self.transposition_hits += other.transposition_hits;
        self.chance_nodes += other.chance_nodes;
        self.terminal_successes += other.terminal_successes;
        self.terminal_failures += other.terminal_failures;
    }
}
