#![allow(
    clippy::blocks_in_conditions,
    clippy::drop_non_drop,
    clippy::field_reassign_with_default,
    clippy::if_same_then_else,
    clippy::large_enum_variant,
    clippy::manual_contains,
    clippy::manual_repeat_n,
    clippy::never_loop,
    clippy::ptr_arg,
    clippy::too_many_arguments,
    clippy::unnecessary_map_or,
    clippy::unused_enumerate_index
)]
//! Hot-core primitives for the Rhystic/Heartwood simulator.
//!
//! The current Python simulator spends most time in repeated hand, bottoming,
//! and state-search loops. This crate intentionally starts with small,
//! deterministic pieces that can be cross-checked before porting the search
//! engine itself.

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

pub mod fast_engine;
pub mod nextgen;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BottomChoice {
    pub removed_indices: Vec<usize>,
    pub kept_indices: Vec<usize>,
}

pub const MANA_CAP: u8 = 10;
pub type Mana = [u8; 6]; // B, R, U, W, G, C
pub type Cost = [u8; 6]; // generic, B, R, U, W, G

pub fn add_mana(mana: Mana, add: Mana) -> Mana {
    [
        (mana[0] + add[0]).min(MANA_CAP),
        (mana[1] + add[1]).min(MANA_CAP),
        (mana[2] + add[2]).min(MANA_CAP),
        (mana[3] + add[3]).min(MANA_CAP),
        (mana[4] + add[4]).min(MANA_CAP),
        (mana[5] + add[5]).min(MANA_CAP),
    ]
}

pub fn cap_mana(mana: Mana) -> Mana {
    [
        mana[0].min(MANA_CAP),
        mana[1].min(MANA_CAP),
        mana[2].min(MANA_CAP),
        mana[3].min(MANA_CAP),
        mana[4].min(MANA_CAP),
        mana[5].min(MANA_CAP),
    ]
}

pub fn mana_for_color(color: char) -> Option<Mana> {
    match color {
        'B' => Some([1, 0, 0, 0, 0, 0]),
        'R' => Some([0, 1, 0, 0, 0, 0]),
        'U' => Some([0, 0, 1, 0, 0, 0]),
        'W' => Some([0, 0, 0, 1, 0, 0]),
        'G' => Some([0, 0, 0, 0, 1, 0]),
        _ => None,
    }
}

pub fn mana_for_option(option: &str) -> Option<Mana> {
    match option {
        "B" | "R" | "U" | "W" | "G" => option.chars().next().and_then(mana_for_color),
        "C" => Some([0, 0, 0, 0, 0, 1]),
        "CC" => Some([0, 0, 0, 0, 0, 2]),
        "CCC" => Some([0, 0, 0, 0, 0, 3]),
        _ => None,
    }
}

pub fn pay_options(mana: Mana, cost: Cost) -> Vec<Mana> {
    let generic = cost[0];
    let b = mana[0] as i16 - cost[1] as i16;
    let r = mana[1] as i16 - cost[2] as i16;
    let u = mana[2] as i16 - cost[3] as i16;
    let w = mana[3] as i16 - cost[4] as i16;
    let g = mana[4] as i16 - cost[5] as i16;
    let c = mana[5] as i16;
    if b < 0 || r < 0 || u < 0 || w < 0 || g < 0 {
        return Vec::new();
    }
    if b + r + u + w + g + c < generic as i16 {
        return Vec::new();
    }
    if generic == 0 {
        return vec![[b as u8, r as u8, u as u8, w as u8, g as u8, c as u8]];
    }

    let mut out = Vec::new();
    let generic_i = generic as i16;
    for xb in 0..=b.min(generic_i) {
        let rem_b = generic_i - xb;
        for xr in 0..=r.min(rem_b) {
            let rem_r = rem_b - xr;
            for xu in 0..=u.min(rem_r) {
                let rem_u = rem_r - xu;
                for xw in 0..=w.min(rem_u) {
                    let rem_w = rem_u - xw;
                    for xg in 0..=g.min(rem_w) {
                        let rem_g = rem_w - xg;
                        if rem_g <= c {
                            out.push([
                                (b - xb) as u8,
                                (r - xr) as u8,
                                (u - xu) as u8,
                                (w - xw) as u8,
                                (g - xg) as u8,
                                (c - rem_g) as u8,
                            ]);
                        }
                    }
                }
            }
        }
    }
    out
}

pub fn mana_bench_cases() -> Vec<(Mana, Cost)> {
    let mana_cases: [Mana; 16] = [
        [0, 0, 1, 0, 0, 2],
        [2, 1, 1, 0, 0, 0],
        [1, 0, 2, 1, 0, 1],
        [0, 3, 0, 0, 1, 0],
        [2, 2, 2, 2, 2, 2],
        [0, 0, 0, 0, 3, 3],
        [4, 0, 0, 0, 0, 0],
        [1, 1, 1, 1, 1, 5],
        [0, 0, 0, 1, 1, 1],
        [3, 0, 1, 0, 0, 2],
        [0, 1, 0, 1, 0, 4],
        [1, 0, 0, 0, 2, 2],
        [2, 0, 0, 1, 0, 3],
        [0, 2, 1, 0, 1, 1],
        [5, 0, 0, 0, 0, 5],
        [0, 0, 5, 0, 0, 5],
    ];
    let cost_cases: [Cost; 10] = [
        [2, 0, 0, 1, 0, 0],
        [1, 0, 0, 0, 0, 2],
        [0, 0, 0, 1, 0, 0],
        [1, 1, 0, 0, 0, 0],
        [3, 0, 0, 0, 0, 1],
        [0, 0, 1, 0, 0, 0],
        [2, 0, 0, 0, 1, 0],
        [1, 0, 1, 0, 0, 0],
        [0, 1, 0, 0, 0, 0],
        [4, 0, 0, 0, 0, 0],
    ];

    let mut cases = Vec::with_capacity(mana_cases.len() * cost_cases.len());
    for mana in mana_cases {
        for cost in cost_cases {
            cases.push((mana, cost));
        }
    }
    cases
}

pub fn mana_checksum(options: &[Mana]) -> u64 {
    let mut out = 0u64;
    for option in options {
        for value in option {
            out = out.wrapping_mul(131).wrapping_add(*value as u64 + 1);
        }
    }
    out
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FixturePerm {
    pub extra: String,
    pub name: String,
    pub tapped: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct FixtureState {
    pub battlefield: Vec<FixturePerm>,
    pub engine_count: u8,
    pub engine_names: Vec<String>,
    pub engine_targets: Vec<String>,
    pub hand: Vec<String>,
    pub land_grave_count: u8,
    pub land_played: bool,
    pub library: Vec<String>,
    pub mana: Mana,
    pub mantle_attached: Vec<String>,
    pub nature_attached: Vec<String>,
    pub nature_tap_used: bool,
    pub nature_untap_used: bool,
    pub pact_debt: u8,
    pub rain_active: bool,
    pub spells_this_turn: u8,
    pub turn: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FixtureAction {
    pub label: String,
    pub next_state_signature: String,
    pub priority: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActionFixture {
    pub fixture_index: usize,
    pub action_count: usize,
    pub actions_truncated: bool,
    pub source: Option<String>,
    pub state_signature: String,
    pub state: FixtureState,
    pub actions: Vec<FixtureAction>,
}

#[derive(Debug, Deserialize)]
pub struct ActionFixturePayload {
    pub fixture_count: usize,
    pub fixtures: Vec<ActionFixture>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedAction {
    pub label: String,
    pub next_state: FixtureState,
    pub next_state_signature: String,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionCore {
    pub label: String,
    pub next_state: FixtureState,
    pub priority: i32,
}

#[derive(Debug, Clone)]
struct BfsAction {
    next_state: FixtureState,
    priority: i32,
    is_ragavan_attack: bool,
}

enum ActionSink<'a> {
    Labeled(&'a mut Vec<ActionCore>),
    Bfs(&'a mut Vec<BfsAction>),
}

impl<'a> ActionSink<'a> {
    fn push<F>(
        &mut self,
        priority: i32,
        label_builder: F,
        next_state: FixtureState,
        is_ragavan_attack: bool,
    ) where
        F: FnOnce() -> String,
    {
        match self {
            ActionSink::Labeled(actions) => {
                let label = label_builder();
                actions.push(ActionCore {
                    priority: action_priority("engine", &label),
                    next_state,
                    label,
                });
            }
            ActionSink::Bfs(actions) => actions.push(BfsAction {
                priority,
                next_state,
                is_ragavan_attack,
            }),
        }
    }
}

const DEFAULT_PRIORITY: i32 = 100;
const LAND_PRIORITY: i32 = 800;
const FAST_MANA_PRIORITY: i32 = 850;
const ENGINE_TUTOR_PRIORITY: i32 = 900;

macro_rules! push_action {
    ($actions:expr, $priority:expr, $label:expr, $next_state:expr $(,)?) => {
        $actions.push($priority, || $label, $next_state, false)
    };
}

macro_rules! push_ragavan_action {
    ($actions:expr, $priority:expr, $label:expr, $next_state:expr $(,)?) => {
        $actions.push($priority, || $label, $next_state, true)
    };
}

#[derive(Debug, Clone, Deserialize)]
pub struct CloseTurnRequest {
    pub states: Vec<FixtureState>,
    pub state_limit: usize,
    pub max_turns: u8,
    pub goal: String,
    pub engine_target_count: u8,
    pub engine_success_policy: String,
    pub remora_upkeep_payments: u8,
    pub action_sort: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloseTurnResponse {
    pub closed: Vec<FixtureState>,
    pub success: bool,
    pub hit_limit: bool,
    pub label: Option<String>,
    pub seen_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SolveKeepRequest {
    pub hand: Vec<String>,
    pub library: Vec<String>,
    pub gemstone_live: bool,
    pub state_limit: usize,
    pub max_turns: u8,
    pub goal: String,
    pub engine_target_count: u8,
    pub engine_success_policy: String,
    pub remora_upkeep_payments: u8,
    pub action_sort: bool,
    #[serde(default)]
    pub gamble_mode: Option<String>,
    #[serde(default)]
    pub gamble_seed: Option<u64>,
    #[serde(default)]
    pub simplified_gamble: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EarliestRequest {
    pub deck_order: Vec<String>,
    pub bottom_count: usize,
    pub gemstone_live: bool,
    pub state_limit: usize,
    pub max_turns: u8,
    pub goal: String,
    pub engine_target_count: u8,
    pub engine_success_policy: String,
    pub remora_upkeep_payments: u8,
    pub action_sort: bool,
    #[serde(default)]
    pub gamble_mode: Option<String>,
    #[serde(default)]
    pub gamble_seed: Option<u64>,
    #[serde(default)]
    pub simplified_gamble: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VisibleHandTaskRequest {
    pub key: String,
    pub hand: Vec<String>,
    pub bottom_count: usize,
    pub seed: u64,
    pub gemstone_live: bool,
    #[serde(default)]
    pub keep_threshold: Option<f64>,
    #[serde(default)]
    pub force_keep: bool,
    #[serde(default)]
    pub adaptive_threshold_sampling: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VisibleHandBatchRequest {
    pub deck: Vec<String>,
    pub tasks: Vec<VisibleHandTaskRequest>,
    pub state_limit: usize,
    pub samples_per_bottom: usize,
    pub validation_samples: usize,
    pub cap_weight: f64,
    pub max_turns: u8,
    pub goal: String,
    pub engine_target_count: u8,
    pub engine_success_policy: String,
    pub remora_upkeep_payments: u8,
    pub action_sort: bool,
    #[serde(default)]
    pub gamble_mode: Option<String>,
    #[serde(default)]
    pub simplified_gamble: bool,
    #[serde(default)]
    pub weighted_policy_ev: bool,
    #[serde(default = "default_policy_rhystic_t1_weight")]
    pub rhystic_t1_weight: f64,
    #[serde(default = "default_policy_rhystic_t2_weight")]
    pub rhystic_t2_weight: f64,
    #[serde(default = "default_policy_heartwood_t1_weight")]
    pub heartwood_t1_weight: f64,
    #[serde(default = "default_policy_heartwood_t2_weight")]
    pub heartwood_t2_weight: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyEvalFastRequest {
    pub deck: Vec<String>,
    pub thresholds_dead: Vec<f64>,
    pub thresholds_live: Vec<f64>,
    pub games: usize,
    pub seed: u64,
    pub gemstone_caverns_live_rate: f64,
    pub state_limit: usize,
    #[serde(default)]
    pub actual_rerun_state_limit: usize,
    pub samples_per_bottom: usize,
    pub validation_samples: usize,
    pub cap_weight: f64,
    pub max_turns: u8,
    pub goal: String,
    pub engine_target_count: u8,
    pub engine_success_policy: String,
    pub remora_upkeep_payments: u8,
    pub action_sort: bool,
    #[serde(default)]
    pub adaptive_threshold_sampling: bool,
    #[serde(default)]
    pub include_game_records: bool,
    #[serde(default)]
    pub include_cap_replay_records: bool,
    #[serde(default)]
    pub include_validation_records: bool,
    #[serde(default)]
    pub trace_lines: bool,
    #[serde(default)]
    pub gamble_mode: Option<String>,
    #[serde(default)]
    pub simplified_gamble: bool,
    #[serde(default)]
    pub internal_shards: usize,
    #[serde(default)]
    pub internal_shard_workers: usize,
    #[serde(default)]
    pub weighted_policy_ev: bool,
    #[serde(default = "default_policy_rhystic_t1_weight")]
    pub rhystic_t1_weight: f64,
    #[serde(default = "default_policy_rhystic_t2_weight")]
    pub rhystic_t2_weight: f64,
    #[serde(default = "default_policy_heartwood_t1_weight")]
    pub heartwood_t1_weight: f64,
    #[serde(default = "default_policy_heartwood_t2_weight")]
    pub heartwood_t2_weight: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicySimFastRequest {
    pub deck: Vec<String>,
    pub threshold_hands: usize,
    pub eval_games: usize,
    pub seed: u64,
    #[serde(default)]
    pub eval_seed: Option<u64>,
    pub gemstone_caverns_live_rate: f64,
    pub state_limit: usize,
    #[serde(default)]
    pub actual_rerun_state_limit: usize,
    pub samples_per_bottom: usize,
    pub validation_samples: usize,
    pub cap_weight: f64,
    pub max_turns: u8,
    pub goal: String,
    pub engine_target_count: u8,
    pub engine_success_policy: String,
    pub remora_upkeep_payments: u8,
    pub action_sort: bool,
    #[serde(default)]
    pub adaptive_threshold_sampling: bool,
    #[serde(default)]
    pub include_game_records: bool,
    #[serde(default)]
    pub include_cap_replay_records: bool,
    #[serde(default)]
    pub include_validation_records: bool,
    #[serde(default)]
    pub trace_lines: bool,
    #[serde(default)]
    pub gamble_mode: Option<String>,
    #[serde(default)]
    pub simplified_gamble: bool,
    #[serde(default)]
    pub internal_shards: usize,
    #[serde(default)]
    pub internal_shard_workers: usize,
    #[serde(default)]
    pub weighted_policy_ev: bool,
    #[serde(default = "default_policy_rhystic_t1_weight")]
    pub rhystic_t1_weight: f64,
    #[serde(default = "default_policy_rhystic_t2_weight")]
    pub rhystic_t2_weight: f64,
    #[serde(default = "default_policy_heartwood_t1_weight")]
    pub heartwood_t1_weight: f64,
    #[serde(default = "default_policy_heartwood_t2_weight")]
    pub heartwood_t2_weight: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyThresholdVariantRequest {
    pub name: String,
    pub thresholds_dead: Vec<f64>,
    pub thresholds_live: Vec<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyThresholdSweepFastRequest {
    pub deck: Vec<String>,
    pub variants: Vec<PolicyThresholdVariantRequest>,
    pub games: usize,
    pub seed: u64,
    pub gemstone_caverns_live_rate: f64,
    pub state_limit: usize,
    #[serde(default)]
    pub actual_rerun_state_limit: usize,
    pub samples_per_bottom: usize,
    pub validation_samples: usize,
    pub cap_weight: f64,
    pub max_turns: u8,
    pub goal: String,
    pub engine_target_count: u8,
    pub engine_success_policy: String,
    pub remora_upkeep_payments: u8,
    pub action_sort: bool,
    #[serde(default)]
    pub gamble_mode: Option<String>,
    #[serde(default)]
    pub simplified_gamble: bool,
    #[serde(default)]
    pub weighted_policy_ev: bool,
    #[serde(default = "default_policy_rhystic_t1_weight")]
    pub rhystic_t1_weight: f64,
    #[serde(default = "default_policy_rhystic_t2_weight")]
    pub rhystic_t2_weight: f64,
    #[serde(default = "default_policy_heartwood_t1_weight")]
    pub heartwood_t1_weight: f64,
    #[serde(default = "default_policy_heartwood_t2_weight")]
    pub heartwood_t2_weight: f64,
}

pub fn default_policy_rhystic_t1_weight() -> f64 {
    1.0
}

pub fn default_policy_rhystic_t2_weight() -> f64 {
    0.75
}

pub fn default_policy_heartwood_t1_weight() -> f64 {
    0.65
}

pub fn default_policy_heartwood_t2_weight() -> f64 {
    0.50
}

pub fn default_raw_delta_draw_window() -> usize {
    2
}

pub fn default_raw_delta_naive_variant_limit() -> usize {
    usize::MAX
}

pub fn default_raw_delta_relevance_mode() -> String {
    "typed_direct".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawDeltaReplacementRequest {
    pub cut: String,
    pub add: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawDeltaSwapRequest {
    pub name: String,
    pub cut: String,
    pub add: String,
    #[serde(default)]
    pub replacements: Vec<RawDeltaReplacementRequest>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawDeltaFastRequest {
    pub deck: Vec<String>,
    pub swaps: Vec<RawDeltaSwapRequest>,
    pub samples_per_stage: usize,
    pub seed: u64,
    pub gemstone_caverns_live_rate: f64,
    pub state_limit: usize,
    pub max_turns: u8,
    pub goal: String,
    pub engine_target_count: u8,
    pub engine_success_policy: String,
    pub remora_upkeep_payments: u8,
    pub action_sort: bool,
    #[serde(default)]
    pub gamble_mode: Option<String>,
    #[serde(default)]
    pub simplified_gamble: bool,
    #[serde(default)]
    pub cap_weight: f64,
    #[serde(default = "default_policy_rhystic_t1_weight")]
    pub rhystic_t1_weight: f64,
    #[serde(default = "default_policy_rhystic_t2_weight")]
    pub rhystic_t2_weight: f64,
    #[serde(default = "default_policy_heartwood_t1_weight")]
    pub heartwood_t1_weight: f64,
    #[serde(default = "default_policy_heartwood_t2_weight")]
    pub heartwood_t2_weight: f64,
    #[serde(default = "default_raw_delta_draw_window")]
    pub draw_window: usize,
    #[serde(default = "default_raw_delta_relevance_mode")]
    pub relevance_mode: String,
    #[serde(default)]
    pub stages: Option<Vec<usize>>,
    #[serde(default)]
    pub run_naive: bool,
    #[serde(default = "default_raw_delta_naive_variant_limit")]
    pub naive_variant_limit: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RngShuffleAuditRequest {
    pub deck: Vec<String>,
    pub samples: usize,
    pub seed: u64,
    #[serde(default = "default_rng_audit_domain")]
    pub domain: String,
}

fn default_rng_audit_domain() -> String {
    "rng_audit_shuffle".to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct SolveKeepResponse {
    pub turn: Option<u8>,
    pub capped: bool,
    pub label: Option<String>,
    pub unsupported: bool,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyGameRecord {
    pub game_index: usize,
    pub stage: usize,
    pub bottom_count: usize,
    pub gemstone_caverns_live: bool,
    pub hit: bool,
    pub capped: bool,
    pub turn: Option<u8>,
    pub engine_label: Option<String>,
    pub trace_turn: Option<u8>,
    pub trace_capped: bool,
    pub trace_found: Option<bool>,
    pub line_action_count: Option<usize>,
    pub line_actions: Option<Vec<String>>,
    pub line_cards: Option<Vec<String>>,
    pub line_casts_nick_fury: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyCapReplayRecord {
    pub game_index: usize,
    pub stage: usize,
    pub bottom_count: usize,
    pub gemstone_caverns_live: bool,
    pub visible_hand: Vec<String>,
    pub bottomed: Vec<String>,
    pub keep: Vec<String>,
    pub library: Vec<String>,
    pub gamble_seed: u64,
    pub state_limit: usize,
    pub actual_cap_rerun_state_limit: usize,
    pub max_turns: u8,
    pub goal: String,
    pub engine_target_count: u8,
    pub engine_success_policy: String,
    pub remora_upkeep_payments: u8,
    pub action_sort: bool,
    pub gamble_mode: Option<String>,
    pub simplified_gamble: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyMulliganDecisionRecord {
    pub stage: usize,
    pub bottom_count: usize,
    pub gemstone_caverns_live: bool,
    pub visible_hand: Vec<String>,
    pub best_bottom: Vec<String>,
    pub keep: bool,
    pub force_keep: bool,
    pub score_ev: f64,
    pub upper_ev: f64,
    pub keep_threshold: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyValidationRecord {
    pub game_index: usize,
    pub stage: usize,
    pub bottom_count: usize,
    pub gemstone_caverns_live: bool,
    pub visible_hand: Vec<String>,
    pub bottomed: Vec<String>,
    pub keep: Vec<String>,
    pub library: Vec<String>,
    pub mulligan_decisions: Vec<PolicyMulliganDecisionRecord>,
    pub gamble_seed: u64,
    pub state_limit: usize,
    pub actual_cap_rerun_state_limit: usize,
    pub max_turns: u8,
    pub goal: String,
    pub engine_target_count: u8,
    pub engine_success_policy: String,
    pub remora_upkeep_payments: u8,
    pub action_sort: bool,
    pub gamble_mode: Option<String>,
    pub simplified_gamble: bool,
    pub hit: bool,
    pub capped: bool,
    pub turn: Option<u8>,
    pub engine_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyEvalFastResponse {
    pub games: usize,
    pub successes: usize,
    pub success_rate: f64,
    pub cap_misses: usize,
    pub initial_cap_misses_before_actual_rerun: usize,
    pub actual_cap_rerun_state_limit: usize,
    pub actual_cap_rerun_attempts: usize,
    pub actual_cap_rerun_successes: usize,
    pub actual_cap_rerun_remaining_caps: usize,
    pub internal_shards: usize,
    pub internal_shard_workers: usize,
    pub upper_success_rate_if_caps_hit: f64,
    pub wilson95: Vec<f64>,
    pub turn_counts: BTreeMap<String, usize>,
    pub keep_counts_by_stage: BTreeMap<String, usize>,
    pub keep_counts_by_bottom: BTreeMap<String, usize>,
    pub gemstone_caverns_live_counts: BTreeMap<String, usize>,
    pub gemstone_caverns_live_successes: BTreeMap<String, usize>,
    pub success_rate_by_gemstone_caverns_live: BTreeMap<String, f64>,
    pub visible_ev_cache_size: usize,
    pub rng_metadata: BTreeMap<String, String>,
    pub unsupported: bool,
    pub unsupported_reason: Option<String>,
    pub game_records: Option<Vec<PolicyGameRecord>>,
    pub cap_replay_records: Option<Vec<PolicyCapReplayRecord>>,
    pub validation_records: Option<Vec<PolicyValidationRecord>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyThresholdRow {
    pub stage: usize,
    pub bottom_count: usize,
    pub future_keep_threshold: f64,
    pub raw_mean_visible_ev: f64,
    pub stage_value: f64,
    pub hands: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicySimFastResponse {
    pub thresholds_dead: Vec<f64>,
    pub thresholds_live: Vec<f64>,
    pub threshold_rows_dead: Vec<PolicyThresholdRow>,
    pub threshold_rows_live: Vec<PolicyThresholdRow>,
    pub evaluation: PolicyEvalFastResponse,
    pub rng_metadata: BTreeMap<String, String>,
    pub unsupported: bool,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyThresholdSweepVariantResponse {
    pub name: String,
    pub thresholds_dead: Vec<f64>,
    pub thresholds_live: Vec<f64>,
    pub evaluation: PolicyEvalFastResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyThresholdSweepFastResponse {
    pub variants: Vec<PolicyThresholdSweepVariantResponse>,
    pub rng_metadata: BTreeMap<String, String>,
    pub unsupported: bool,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawDeltaVariantResponse {
    pub name: String,
    pub cut: String,
    pub add: String,
    pub slot_index: Option<usize>,
    pub samples: usize,
    pub relevant_samples: usize,
    pub irrelevant_samples: usize,
    pub relevance_rate: f64,
    pub baseline_score_mean: f64,
    pub candidate_score_mean_delta: f64,
    pub candidate_score_mean_naive: Option<f64>,
    pub mean_delta_delta_method: f64,
    pub mean_delta_naive: Option<f64>,
    pub delta_method_se: f64,
    pub naive_delta_se: Option<f64>,
    pub delta_abs_error_vs_naive: Option<f64>,
    pub baseline_hits: usize,
    pub candidate_hits_delta_method: usize,
    pub candidate_hits_naive: Option<usize>,
    pub positive_delta_samples: usize,
    pub negative_delta_samples: usize,
    pub zero_delta_samples: usize,
    pub candidate_delta_solver_calls: usize,
    pub candidate_naive_solver_calls: Option<usize>,
    pub delta_elapsed_ms: f64,
    pub naive_elapsed_ms: Option<f64>,
    pub speedup_vs_naive: Option<f64>,
    pub unsupported: bool,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawDeltaFastResponse {
    pub stages: Vec<usize>,
    pub bottom_counts: Vec<usize>,
    pub samples_per_stage: usize,
    pub total_samples: usize,
    pub draw_window: usize,
    pub relevance_mode: String,
    pub baseline_solver_calls: usize,
    pub baseline_elapsed_ms: f64,
    pub variants: Vec<RawDeltaVariantResponse>,
    pub rng_metadata: BTreeMap<String, String>,
    pub unsupported: bool,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "record_type", rename_all = "snake_case")]
pub enum RawDeltaStreamRecord {
    Start {
        stages: Vec<usize>,
        bottom_counts: Vec<usize>,
        samples_per_stage: usize,
        total_samples: usize,
        draw_window: usize,
        relevance_mode: String,
        baseline_solver_calls: usize,
        baseline_elapsed_ms: f64,
        baseline_score_mean: f64,
        baseline_hits: usize,
        variants_total: usize,
        rng_metadata: BTreeMap<String, String>,
    },
    Variant {
        variant_index: usize,
        variants_total: usize,
        variant: RawDeltaVariantResponse,
    },
    Complete {
        variants_total: usize,
        unsupported: bool,
        unsupported_reason: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct RngShuffleAuditResponse {
    pub samples: usize,
    pub deck_size: usize,
    pub domain: String,
    pub rng_metadata: BTreeMap<String, String>,
    pub position_counts_by_card: BTreeMap<String, Vec<usize>>,
    pub first_card_counts: BTreeMap<String, usize>,
    pub unsupported: bool,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VisibleHandResponse {
    pub key: String,
    pub hand: Vec<String>,
    pub bottom_count: usize,
    pub gemstone_caverns_live: bool,
    pub ev: f64,
    pub upper_ev: f64,
    pub score_ev: f64,
    pub hits: usize,
    pub cap_misses: usize,
    pub samples: usize,
    pub best_bottom: Vec<String>,
    pub deterministic: bool,
    pub selection_hits: usize,
    pub selection_cap_misses: usize,
    pub selection_samples: usize,
    pub selection_score: f64,
    pub solver_calls: usize,
    pub deterministic_checked: usize,
    pub bottom_candidates_checked: usize,
    pub selection_early_stopped: bool,
    pub selection_pruned_candidates: usize,
    pub selection_pruned_samples: usize,
    pub selection_skipped_single_candidate_samples: usize,
    pub validation_samples_used: usize,
    pub adaptive_threshold_resolved: bool,
    pub adaptive_threshold_resolution: String,
    pub adaptive_validation_samples_saved: usize,
    pub unsupported: bool,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixtureMismatch {
    pub fixture_index: usize,
    pub source: Option<String>,
    pub state_signature: String,
    pub expected_count: usize,
    pub generated_count: usize,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionParityReport {
    pub fixture_count: usize,
    pub compared_fixtures: usize,
    pub exact_fixtures: usize,
    pub expected_actions: usize,
    pub generated_actions: usize,
    pub missing_actions: usize,
    pub extra_actions: usize,
    pub actions_truncated_in_fixture: usize,
    pub mismatches: Vec<FixtureMismatch>,
}

fn colors() -> &'static [char; 5] {
    &['B', 'R', 'U', 'W', 'G']
}

fn color_index(color: char) -> Option<usize> {
    match color {
        'B' => Some(0),
        'R' => Some(1),
        'U' => Some(2),
        'W' => Some(3),
        'G' => Some(4),
        _ => None,
    }
}

fn mana_for_color_char(color: char) -> Mana {
    let mut out = [0, 0, 0, 0, 0, 0];
    if let Some(index) = color_index(color) {
        out[index] = 1;
    }
    out
}

fn norm_hand(hand: &mut Vec<String>) {
    hand.sort();
}

fn norm_battlefield(battlefield: &mut Vec<FixturePerm>) {
    battlefield.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.tapped.cmp(&right.tapped))
            .then_with(|| left.extra.cmp(&right.extra))
    });
}

fn remove_first(values: &[String], card: &str) -> Vec<String> {
    let mut removed = false;
    let mut out = Vec::with_capacity(values.len().saturating_sub(1));
    for value in values {
        if !removed && value == card {
            removed = true;
            continue;
        }
        out.push(value.clone());
    }
    out
}

fn with_battlefield(mut state: FixtureState, battlefield: Vec<FixturePerm>) -> FixtureState {
    state.battlefield = battlefield;
    norm_battlefield(&mut state.battlefield);
    state
}

fn after_cast(before: &FixtureState, mut after: FixtureState) -> FixtureState {
    after.spells_this_turn = before.spells_this_turn.saturating_add(1).min(5);
    let before_has_birgi = before.battlefield.iter().any(|p| p.name == "BIRGI");
    let after_has_birgi = after.battlefield.iter().any(|p| p.name == "BIRGI");
    if before_has_birgi && after_has_birgi {
        after.mana = add_mana(after.mana, [0, 1, 0, 0, 0, 0]);
    }
    if before.spells_this_turn == 1 && before.battlefield.iter().any(|p| p.name == "LOTHO") {
        after.battlefield.push(FixturePerm {
            name: "TREASURE".to_string(),
            tapped: false,
            extra: String::new(),
        });
        norm_battlefield(&mut after.battlefield);
    }
    after
}

fn add_engine(mut state: FixtureState, target_type: &str, engine_name: &str) -> FixtureState {
    let mut targets: BTreeSet<String> = state.engine_targets.into_iter().collect();
    targets.insert(target_type.to_string());
    targets.insert("PERM".to_string());
    state.engine_targets = targets.into_iter().collect();

    let mut names: BTreeSet<String> = state.engine_names.into_iter().collect();
    names.insert(format!("{engine_name}@{}", state.turn));
    state.engine_names = names.into_iter().collect();
    state.engine_count = state.engine_count.saturating_add(1).min(3);
    state
}

fn stable_state_value(state: &FixtureState) -> Value {
    let mut map = Map::new();
    map.insert(
        "battlefield".to_string(),
        Value::Array(
            state
                .battlefield
                .iter()
                .map(|perm| {
                    let mut perm_map = Map::new();
                    perm_map.insert("extra".to_string(), Value::String(perm.extra.clone()));
                    perm_map.insert("name".to_string(), Value::String(perm.name.clone()));
                    perm_map.insert("tapped".to_string(), Value::Bool(perm.tapped));
                    Value::Object(perm_map)
                })
                .collect(),
        ),
    );
    map.insert("engine_count".to_string(), Value::from(state.engine_count));
    map.insert(
        "engine_names".to_string(),
        strings_value(&state.engine_names),
    );
    map.insert(
        "engine_targets".to_string(),
        strings_value(&state.engine_targets),
    );
    map.insert("hand".to_string(), strings_value(&state.hand));
    map.insert(
        "land_grave_count".to_string(),
        Value::from(state.land_grave_count),
    );
    map.insert("land_played".to_string(), Value::Bool(state.land_played));
    map.insert("library".to_string(), strings_value(&state.library));
    map.insert(
        "mana".to_string(),
        Value::Array(state.mana.iter().map(|v| Value::from(*v)).collect()),
    );
    map.insert(
        "mantle_attached".to_string(),
        strings_value(&state.mantle_attached),
    );
    map.insert(
        "nature_attached".to_string(),
        strings_value(&state.nature_attached),
    );
    map.insert(
        "nature_tap_used".to_string(),
        Value::Bool(state.nature_tap_used),
    );
    map.insert(
        "nature_untap_used".to_string(),
        Value::Bool(state.nature_untap_used),
    );
    map.insert("pact_debt".to_string(), Value::from(state.pact_debt));
    map.insert("rain_active".to_string(), Value::Bool(state.rain_active));
    map.insert(
        "spells_this_turn".to_string(),
        Value::from(state.spells_this_turn),
    );
    map.insert("turn".to_string(), Value::from(state.turn));
    Value::Object(map)
}

fn strings_value(values: &[String]) -> Value {
    Value::Array(values.iter().map(|v| Value::String(v.clone())).collect())
}

pub fn state_signature(state: &FixtureState) -> String {
    let encoded =
        serde_json::to_string(&stable_state_value(state)).expect("state JSON serialization failed");
    let mut hasher = Blake2bVar::new(12).expect("valid digest size");
    hasher.update(encoded.as_bytes());
    let mut digest = [0u8; 12];
    hasher
        .finalize_variable(&mut digest)
        .expect("digest output size");
    let mut out = String::with_capacity(24);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut out, "{byte:02x}").expect("hex write failed");
    }
    out
}

fn action_priority(goal: &str, action: &str) -> i32 {
    if action.starts_with("tap ") || action.starts_with("sac ") || action.starts_with("exile ") {
        return 1000;
    }
    if action.contains("Rhystic Study")
        || action.contains("Tutor")
        || action.contains("Wishclaw")
        || action.contains("Beseech")
        || action.contains("Gamble")
    {
        return 900;
    }
    if goal == "engine"
        && [
            "Heartwood",
            "Mystic Remora",
            "Smothering Tithe",
            "Esper Sentinel",
            "Copy Enchantment",
            "Mirrormade",
            "Flash Photography",
            "Clever Impersonator",
            "Green Sun's Zenith",
            "Eldritch Evolution",
            "Summoner's Pact",
            "Neoform",
            "Ranger-Captain",
        ]
        .iter()
        .any(|needle| action.contains(needle))
    {
        return 900;
    }
    if [
        "Lotus",
        "Mox",
        "Sol Ring",
        "Mana Vault",
        "Chrome",
        "Diamond",
        "Opal",
    ]
    .iter()
    .any(|needle| action.contains(needle))
    {
        return 850;
    }
    if action.starts_with("play ") {
        return 800;
    }
    100
}

fn label_priority(label: &str) -> i32 {
    if [
        "Heartwood",
        "Mystic Remora",
        "Smothering Tithe",
        "Esper Sentinel",
        "Copy Enchantment",
        "Mirrormade",
        "Flash Photography",
        "Clever Impersonator",
        "Green Sun's Zenith",
        "Eldritch Evolution",
        "Summoner's Pact",
        "Neoform",
        "Ranger-Captain",
    ]
    .iter()
    .any(|needle| label.contains(needle))
    {
        return ENGINE_TUTOR_PRIORITY;
    }
    if [
        "Lotus",
        "Mox",
        "Sol Ring",
        "Mana Vault",
        "Chrome",
        "Diamond",
        "Opal",
    ]
    .iter()
    .any(|needle| label.contains(needle))
    {
        return FAST_MANA_PRIORITY;
    }
    DEFAULT_PRIORITY
}

fn tutor_target_priority(tutor: &str, target: &str) -> i32 {
    label_priority(tutor).max(label_priority(target))
}

fn is_land_card(card: &str) -> bool {
    if is_theoretical_rainbow_land(card) {
        return true;
    }
    matches!(
        card,
        "Ancient Tomb"
            | "Arid Mesa"
            | "Bayou"
            | "Boseiju, Who Endures"
            | "Bloodstained Mire"
            | "City of Brass"
            | "City of Traitors"
            | "Command Tower"
            | "Crystal Vein"
            | "Emergence Zone"
            | "Exotic Orchard"
            | "Flooded Strand"
            | "Forbidden Orchard"
            | "Gemstone Caverns"
            | "Gemstone Mine"
            | "Glimmervoid"
            | "Glittering Caves of Aglarond"
            | "Hallowed Fountain"
            | "Mana Confluence"
            | "Marsh Flats"
            | "Misty Rainforest"
            | "Otawara, Soaring City"
            | "Plateau"
            | "Polluted Delta"
            | "Phyrexian Tower"
            | "Savannah"
            | "Scalding Tarn"
            | "Scrubland"
            | "Sea of Clouds"
            | "Starting Town"
            | "Steam Vents"
            | "Taiga"
            | "Tarnished Citadel"
            | "Tropical Island"
            | "Tundra"
            | "Underground Sea"
            | "Verdant Catacombs"
            | "Volcanic Island"
            | "Windswept Heath"
            | "Wooded Foothills"
    )
}

fn is_theoretical_rainbow_land(card: &str) -> bool {
    card.starts_with("Theoretical Rainbow Land ")
}

fn is_gemstone_caverns_alias(card: &str) -> bool {
    matches!(card, "Gemstone Caverns" | "Glittering Caves of Aglarond")
}

fn is_mdfc_land(card: &str) -> bool {
    matches!(
        card,
        "Sink into Stupor" | "Sink into Stupor // Soporific Springs"
    )
}

fn is_artifact_card(card: &str) -> bool {
    matches!(
        card,
        "Chrome Mox"
            | "Lion's Eye Diamond"
            | "Lotus Petal"
            | "Mana Vault"
            | "Mox Amber"
            | "Mox Diamond"
            | "Mox Opal"
            | "Sol Ring"
    )
}

fn card_colors(card: &str) -> &'static str {
    match card {
        "Angel's Grace" => "W",
        "An Offer You Can't Refuse" => "U",
        "Beseech the Mirror" => "B",
        "Birgi, God of Storytelling" => "R",
        "Birds of Paradise" => "G",
        "Borne Upon a Wind" => "U",
        "Brain Freeze" => "U",
        "Chain of Vapor" => "U",
        "Clever Impersonator" => "U",
        "Commandeer" => "U",
        "Copy Enchantment" => "U",
        "Crop Rotation" => "G",
        "Culling the Weak" => "B",
        "Curse of Opulence" => "R",
        "Dark Ritual" => "B",
        "Deathrite Shaman" => "BG",
        "Deflecting Swat" => "R",
        "Demonic Tutor" => "B",
        "Diabolic Intent" => "B",
        "Dispel" => "U",
        "Disrupting Shoal" => "U",
        "Eldritch Evolution" => "G",
        "Elvish Spirit Guide" => "G",
        "Enlightened Tutor" => "W",
        "Esper Sentinel" => "W",
        "Faerie Mastermind" => "U",
        "Fierce Guardianship" => "U",
        "Firestorm" => "R",
        "Flash Photography" => "U",
        "Flesh Duplicate" => "U",
        "Flusterstorm" => "U",
        "Force of Negation" => "U",
        "Force of Will" => "U",
        "Flashback" => "R",
        "Gamble" => "R",
        "Gifts Ungiven" => "U",
        "Green Sun's Zenith" => "G",
        "Grim Tutor" => "B",
        "Heartwood Storyteller" => "G",
        "Hullbreaker Horror" => "U",
        "Idyllic Tutor" => "W",
        "Infernal Plunge" => "R",
        "Ignoble Hierarch" => "G",
        "Imperial Seal" => "B",
        "Intuition" => "U",
        "Into the Flood Maw" => "U",
        "Ishai, Ojutai Dragonspeaker" => "UW",
        "Jeska's Will" => "R",
        "Lotho, Corrupt Shirriff" => "BW",
        "Manamorphose" => "RG",
        "Mental Misstep" => "U",
        "Mindbreak Trap" => "U",
        "Mirrormade" => "U",
        "Misdirection" => "U",
        "Mockingbird" => "U",
        "Molten Disaster" => "R",
        "Mystic Remora" => "U",
        "Mystical Tutor" => "U",
        "Nature's Chosen" => "G",
        "Necropotence" => "B",
        "Neoform" => "UG",
        "Nick Fury, Agent of S.H.I.E.L.D." => "W",
        "Noble Hierarch" => "G",
        "Noxious Revival" => "G",
        "Orcish Bowmasters" => "B",
        "Orim's Chant" => "W",
        "Pact of Negation" => "U",
        "Phyrexian Metamorph" => "U",
        "Pyroblast" => "R",
        "Ragavan, Nimble Pilferer" => "R",
        "Rain of Filth" => "B",
        "Ranger-Captain of Eos" => "W",
        "Red Elemental Blast" => "R",
        "Redirect Lightning" => "R",
        "Rhystic Study" => "U",
        "Rite of Flame" => "R",
        "Rograkh, Son of Rohgahh" => "R",
        "Scheming Symmetry" => "B",
        "Sevinne's Reclamation" => "W",
        "Silence" => "W",
        "Simian Spirit Guide" => "R",
        "Sink into Stupor" => "U",
        "Sink into Stupor // Soporific Springs" => "U",
        "Smothering Tithe" => "W",
        "Snapback" => "U",
        "Storm-Kiln Artist" => "R",
        "Strike It Rich" => "R",
        "Subtlety" => "U",
        "Sudden Substitution" => "U",
        "Summoner's Pact" => "G",
        "Swan Song" => "U",
        "Tataru Taru" => "W",
        "The Cabbage Merchant" => "G",
        "Tinder Wall" => "G",
        "Underworld Breach" => "R",
        "Valley Floodcaller" => "U",
        "Vampiric Tutor" => "B",
        "Wan Shi Tong, Librarian" => "U",
        "Wild Cantor" => "RG",
        "Worldly Tutor" => "G",
        _ => "",
    }
}

fn is_land_perm(perm: &FixturePerm) -> bool {
    matches!(
        perm.name.as_str(),
        "LAND" | "CCLAND" | "CITY" | "GLIMMER" | "CAVERN" | "MINE" | "TOWER" | "VEIN"
    )
}

fn is_creature_perm(perm: &FixturePerm) -> bool {
    matches!(
        perm.name.as_str(),
        "NICK"
            | "ROG"
            | "BIRD"
            | "DEATHRITE"
            | "TINDER"
            | "TATARU"
            | "RAGAVAN"
            | "LOTHO"
            | "ESPER"
            | "HEARTWOOD"
            | "CREATURE"
    )
}

fn creature_mv_by_perm(perm: &FixturePerm) -> u8 {
    match perm.name.as_str() {
        "ROG" => 0,
        "NICK" | "BIRD" | "DEATHRITE" | "TINDER" | "RAGAVAN" | "ESPER" | "CREATURE" => 1,
        "TATARU" | "LOTHO" | "WAN" => 2,
        "BIRGI" | "HEARTWOOD" => 3,
        "ISHAI" => 4,
        _ => 0,
    }
}

fn is_legendary_perm(perm: &FixturePerm) -> bool {
    matches!(
        perm.name.as_str(),
        "NICK" | "ROG" | "TATARU" | "RAGAVAN" | "LOTHO"
    )
}

fn is_artifact_perm(perm: &FixturePerm) -> bool {
    matches!(
        perm.name.as_str(),
        "ARTIFACT"
            | "PETAL"
            | "TREASURE"
            | "LED"
            | "AMBER"
            | "OPAL"
            | "MANTLE"
            | "DIAMOND"
            | "CHROME"
            | "SOL"
            | "VAULT"
            | "SIGNET"
            | "WISHCLAW"
            | "DRUM"
            | "RELIC"
            | "ESPER"
    )
}

fn is_enchantment_perm(perm: &FixturePerm) -> bool {
    matches!(perm.name.as_str(), "ENGINE_ENCH" | "NATURE")
}

fn artifact_count(state: &FixtureState) -> usize {
    state
        .battlefield
        .iter()
        .filter(|perm| is_artifact_perm(perm))
        .count()
}

fn unique_creature_indices(state: &FixtureState) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (index, perm) in state.battlefield.iter().enumerate() {
        if !is_creature_perm(perm) {
            continue;
        }
        let key = (perm.name.clone(), perm.tapped, perm.extra.clone());
        if seen.insert(key) {
            out.push(index);
        }
    }
    out
}

fn creature_perm(card: &str) -> FixturePerm {
    let name = match card {
        "Birds of Paradise" => "BIRD",
        "Deathrite Shaman" => "DEATHRITE",
        "Esper Sentinel" => "ESPER",
        "Tinder Wall" => "TINDER",
        "Ragavan, Nimble Pilferer" => "RAGAVAN",
        "Lotho, Corrupt Shirriff" => "LOTHO",
        _ => "CREATURE",
    };
    FixturePerm {
        name: name.to_string(),
        tapped: false,
        extra: format!("{}*", card_colors(card)),
    }
}

fn land_type_colors(card: &str) -> Option<&'static str> {
    match card {
        "Badlands" => Some("BR"),
        "Bayou" => Some("BG"),
        "Blood Crypt" => Some("BR"),
        "Hallowed Fountain" => Some("UW"),
        "Plateau" => Some("RW"),
        "Savannah" => Some("GW"),
        "Scrubland" => Some("BW"),
        "Steam Vents" => Some("RU"),
        "Taiga" => Some("RG"),
        "Tropical Island" => Some("UG"),
        "Tundra" => Some("UW"),
        "Underground Sea" => Some("BU"),
        "Volcanic Island" => Some("RU"),
        "Watery Grave" => Some("BU"),
        _ => None,
    }
}

fn fetch_can_get(fetch: &str, target: &str) -> bool {
    matches!(
        (fetch, target),
        (
            "Arid Mesa",
            "Badlands"
                | "Blood Crypt"
                | "Hallowed Fountain"
                | "Plateau"
                | "Savannah"
                | "Scrubland"
                | "Steam Vents"
                | "Taiga"
                | "Tundra"
                | "Volcanic Island"
        ) | (
            "Bloodstained Mire",
            "Badlands"
                | "Bayou"
                | "Blood Crypt"
                | "Plateau"
                | "Scrubland"
                | "Steam Vents"
                | "Taiga"
                | "Underground Sea"
                | "Volcanic Island"
                | "Watery Grave"
        ) | (
            "Flooded Strand",
            "Hallowed Fountain"
                | "Plateau"
                | "Savannah"
                | "Scrubland"
                | "Steam Vents"
                | "Tropical Island"
                | "Tundra"
                | "Underground Sea"
                | "Volcanic Island"
                | "Watery Grave"
        ) | (
            "Marsh Flats",
            "Badlands"
                | "Bayou"
                | "Blood Crypt"
                | "Hallowed Fountain"
                | "Plateau"
                | "Savannah"
                | "Scrubland"
                | "Tundra"
                | "Underground Sea"
                | "Watery Grave"
        ) | (
            "Misty Rainforest",
            "Bayou"
                | "Hallowed Fountain"
                | "Savannah"
                | "Steam Vents"
                | "Taiga"
                | "Tropical Island"
                | "Tundra"
                | "Underground Sea"
                | "Volcanic Island"
                | "Watery Grave"
        ) | (
            "Polluted Delta",
            "Badlands"
                | "Bayou"
                | "Blood Crypt"
                | "Hallowed Fountain"
                | "Scrubland"
                | "Steam Vents"
                | "Tropical Island"
                | "Tundra"
                | "Underground Sea"
                | "Volcanic Island"
                | "Watery Grave"
        ) | (
            "Scalding Tarn",
            "Badlands"
                | "Blood Crypt"
                | "Hallowed Fountain"
                | "Plateau"
                | "Steam Vents"
                | "Taiga"
                | "Tropical Island"
                | "Tundra"
                | "Underground Sea"
                | "Volcanic Island"
                | "Watery Grave"
        ) | (
            "Verdant Catacombs",
            "Badlands"
                | "Bayou"
                | "Blood Crypt"
                | "Savannah"
                | "Scrubland"
                | "Taiga"
                | "Tropical Island"
                | "Underground Sea"
                | "Watery Grave"
        ) | (
            "Windswept Heath",
            "Bayou"
                | "Hallowed Fountain"
                | "Plateau"
                | "Savannah"
                | "Scrubland"
                | "Taiga"
                | "Tropical Island"
                | "Tundra"
        ) | (
            "Wooded Foothills",
            "Badlands"
                | "Bayou"
                | "Blood Crypt"
                | "Plateau"
                | "Savannah"
                | "Steam Vents"
                | "Taiga"
                | "Tropical Island"
                | "Volcanic Island"
        )
    )
}

fn land_options(card: &str, library: &[String]) -> Vec<(FixturePerm, Vec<String>, u8, String)> {
    if is_mdfc_land(card) {
        return vec![(
            FixturePerm {
                name: "LAND".to_string(),
                tapped: false,
                extra: "U".to_string(),
            },
            library.to_vec(),
            0,
            " as land".to_string(),
        )];
    }
    if matches!(
        card,
        "Arid Mesa"
            | "Bloodstained Mire"
            | "Flooded Strand"
            | "Marsh Flats"
            | "Misty Rainforest"
            | "Polluted Delta"
            | "Scalding Tarn"
            | "Verdant Catacombs"
            | "Windswept Heath"
            | "Wooded Foothills"
    ) {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for target in library {
            if !seen.insert(target.clone()) || !fetch_can_get(card, target) {
                continue;
            }
            if let Some(colors) = land_type_colors(target) {
                out.push((
                    FixturePerm {
                        name: "LAND".to_string(),
                        tapped: false,
                        extra: colors.to_string(),
                    },
                    remove_first(library, target),
                    1,
                    format!(" fetch {target}"),
                ));
            }
        }
        return out;
    }
    let perm = if card == "Ancient Tomb" {
        FixturePerm {
            name: "CCLAND".to_string(),
            tapped: false,
            extra: String::new(),
        }
    } else if card == "City of Traitors" {
        FixturePerm {
            name: "CITY".to_string(),
            tapped: false,
            extra: String::new(),
        }
    } else if card == "Crystal Vein" {
        FixturePerm {
            name: "VEIN".to_string(),
            tapped: false,
            extra: "C".to_string(),
        }
    } else if card == "Phyrexian Tower" {
        FixturePerm {
            name: "TOWER".to_string(),
            tapped: false,
            extra: "C".to_string(),
        }
    } else if card == "Glimmervoid" {
        FixturePerm {
            name: "GLIMMER".to_string(),
            tapped: false,
            extra: "BRUWG".to_string(),
        }
    } else if card == "Gemstone Mine" {
        FixturePerm {
            name: "MINE".to_string(),
            tapped: false,
            extra: "3".to_string(),
        }
    } else if is_theoretical_rainbow_land(card) {
        FixturePerm {
            name: "LAND".to_string(),
            tapped: false,
            extra: "BRUWG".to_string(),
        }
    } else if matches!(
        card,
        "City of Brass"
            | "Command Tower"
            | "Exotic Orchard"
            | "Forbidden Orchard"
            | "Mana Confluence"
            | "Starting Town"
            | "Tarnished Citadel"
    ) {
        FixturePerm {
            name: "LAND".to_string(),
            tapped: false,
            extra: "BRUWG".to_string(),
        }
    } else if card == "Boseiju, Who Endures" {
        FixturePerm {
            name: "LAND".to_string(),
            tapped: false,
            extra: "G".to_string(),
        }
    } else if card == "Otawara, Soaring City" {
        FixturePerm {
            name: "LAND".to_string(),
            tapped: false,
            extra: "U".to_string(),
        }
    } else if card == "Sea of Clouds" {
        FixturePerm {
            name: "LAND".to_string(),
            tapped: false,
            extra: "UW".to_string(),
        }
    } else if let Some(colors) = land_type_colors(card) {
        FixturePerm {
            name: "LAND".to_string(),
            tapped: false,
            extra: colors.to_string(),
        }
    } else {
        FixturePerm {
            name: "LAND".to_string(),
            tapped: false,
            extra: "C".to_string(),
        }
    };
    vec![(perm, library.to_vec(), 0, String::new())]
}

fn tap_options(perm: &FixturePerm, state: &FixtureState) -> Vec<String> {
    match perm.name.as_str() {
        "LAND" | "CHROME" => perm.extra.chars().map(|c| c.to_string()).collect(),
        "CAVERN" | "MINE" | "GLIMMER" => colors().iter().map(|c| c.to_string()).collect(),
        "CCLAND" | "CITY" | "SOL" => vec!["CC".to_string()],
        "VEIN" => vec!["C".to_string()],
        "VAULT" => vec!["VAULT".to_string()],
        "DIAMOND" => colors().iter().map(|c| c.to_string()).collect(),
        "OPAL" if artifact_count(state) >= 3 => colors().iter().map(|c| c.to_string()).collect(),
        "AMBER" => {
            let mut available = BTreeSet::new();
            for permanent in &state.battlefield {
                if is_legendary_perm(permanent) {
                    for color in permanent.extra.trim_end_matches('*').chars() {
                        available.insert(color);
                    }
                }
            }
            colors()
                .iter()
                .filter(|color| available.contains(color))
                .map(|c| c.to_string())
                .collect()
        }
        "BIRD" if !perm.extra.ends_with('*') => colors().iter().map(|c| c.to_string()).collect(),
        "DEATHRITE" if !perm.extra.ends_with('*') => {
            colors().iter().map(|c| c.to_string()).collect()
        }
        _ => Vec::new(),
    }
}

fn remove_battlefield_index(state: &FixtureState, index: usize) -> Vec<FixturePerm> {
    let mut battlefield = state.battlefield.clone();
    battlefield.remove(index);
    battlefield
}

fn sac_creature(state: &FixtureState, index: usize) -> FixtureState {
    with_battlefield(state.clone(), remove_battlefield_index(state, index))
}

fn sac_land(state: &FixtureState, index: usize) -> FixtureState {
    let mut next = with_battlefield(state.clone(), remove_battlefield_index(state, index));
    next.land_grave_count = next.land_grave_count.saturating_add(1).min(4);
    next
}

fn sac_perm(state: &FixtureState, index: usize) -> FixtureState {
    with_battlefield(state.clone(), remove_battlefield_index(state, index))
}

fn tutor_targets(tutor: &str, state: &FixtureState) -> Vec<String> {
    let library: BTreeSet<&str> = state.library.iter().map(String::as_str).collect();
    if tutor == "Mystical Tutor" {
        let mut candidates: Vec<String> = [
            "An Offer You Can't Refuse",
            "Beseech the Mirror",
            "Crop Rotation",
            "Culling the Weak",
            "Dark Ritual",
            "Demonic Tutor",
            "Diabolic Intent",
            "Eldritch Evolution",
            "Enlightened Tutor",
            "Green Sun's Zenith",
            "Imperial Seal",
            "Infernal Plunge",
            "Rain of Filth",
            "Rite of Flame",
            "Scheming Symmetry",
            "Summoner's Pact",
            "Vampiric Tutor",
        ]
        .iter()
        .filter(|target| library.contains(**target))
        .map(|target| (*target).to_string())
        .collect();
        candidates.sort_by_key(|target| engine_target_priority(target));
        return candidates;
    }
    if tutor == "Enlightened Tutor" {
        return if library.contains("Rhystic Study") {
            vec!["Rhystic Study".to_string()]
        } else {
            Vec::new()
        };
    }
    let mut candidates = vec![
        "Rhystic Study".to_string(),
        "Heartwood Storyteller".to_string(),
    ];
    candidates.retain(|target| library.contains(target.as_str()));
    candidates.sort_by_key(|target| engine_target_priority(target));
    candidates
}

fn preturn_top_tutor_targets(tutor: &str, state: &FixtureState) -> Vec<String> {
    if tutor == "Worldly Tutor" {
        let library: BTreeSet<&str> = state.library.iter().map(String::as_str).collect();
        let mut candidates: Vec<String> = [
            "Birds of Paradise",
            "Deathrite Shaman",
            "Heartwood Storyteller",
            "Ignoble Hierarch",
            "Noble Hierarch",
            "Tinder Wall",
            "Wild Cantor",
        ]
        .iter()
        .filter(|target| library.contains(**target))
        .map(|target| (*target).to_string())
        .collect();
        candidates.sort_by_key(|target| engine_target_priority(target));
        return candidates;
    }
    tutor_targets(tutor, state)
}

fn beseech_targets(state: &FixtureState) -> Vec<String> {
    tutor_targets("Beseech the Mirror", state)
}

fn engine_target_priority(target: &str) -> (u8, String) {
    let priority = match target {
        "Rhystic Study" => 0,
        "Heartwood Storyteller" => 1,
        "Mystic Remora" => 2,
        "Smothering Tithe" => 3,
        _ => 50,
    };
    (priority, target.to_string())
}

fn engine_native(target: &str) -> Option<(Cost, &'static str, FixturePerm)> {
    match target {
        "Rhystic Study" => Some((
            [2, 0, 0, 1, 0, 0],
            "ENCH",
            FixturePerm {
                name: "ENGINE_ENCH".to_string(),
                tapped: false,
                extra: String::new(),
            },
        )),
        "Heartwood Storyteller" => Some((
            [1, 0, 0, 0, 0, 2],
            "CREATURE",
            FixturePerm {
                name: "HEARTWOOD".to_string(),
                tapped: false,
                extra: "G*".to_string(),
            },
        )),
        _ => None,
    }
}

fn resolve_free_engine(mut state: FixtureState, target: &str) -> FixtureState {
    if let Some((_cost, target_type, perm)) = engine_native(target) {
        state.battlefield.push(perm);
        norm_battlefield(&mut state.battlefield);
        return add_engine(state, target_type, target);
    }
    state
}

fn led_target_state(state: &FixtureState, target: &str, mana: Mana) -> Option<FixtureState> {
    if !state.library.iter().any(|card| card == target) {
        return None;
    }
    if let Some((cost, target_type, perm)) = engine_native(target) {
        for remaining in pay_options(mana, cost) {
            let mut next = state.clone();
            if next.hand.iter().any(|card| card == target) {
                next.hand = remove_first(&next.hand, target);
            }
            next.library = remove_first(&next.library, target);
            next.battlefield.push(perm.clone());
            next.mana = remaining;
            norm_hand(&mut next.hand);
            norm_battlefield(&mut next.battlefield);
            return Some(add_engine(next, target_type, target));
        }
        return None;
    }
    let mut next = state.clone();
    next.hand = vec![target.to_string()];
    next.library = remove_first(&next.library, target);
    next.mana = mana;
    Some(next)
}

fn cast_cost_actions(
    actions: &mut ActionSink<'_>,
    state: &FixtureState,
    card: &str,
    perm: FixturePerm,
    cost: Cost,
    priority: i32,
) {
    if !state.hand.iter().any(|c| c == card) {
        return;
    }
    for mana in pay_options(state.mana, cost) {
        let mut next = state.clone();
        next.hand = remove_first(&state.hand, card);
        next.battlefield.push(perm.clone());
        next.mana = mana;
        norm_hand(&mut next.hand);
        norm_battlefield(&mut next.battlefield);
        push_action!(
            actions,
            priority,
            format!("cast {card}"),
            after_cast(state, next)
        );
    }
}

fn populate_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    generate_mana_actions(actions, state);
    generate_engine_actions(actions, state);
    generate_commander_actions(actions, state);
    generate_land_actions(actions, state);
    generate_zero_artifact_actions(actions, state);
    generate_chrome_mox_actions(actions, state);
    generate_mox_diamond_actions(actions, state);
    generate_artifact_spell_actions(actions, state);
    generate_creature_actions(actions, state);
    generate_spirit_guide_actions(actions, state);
    generate_ritual_actions(actions, state);
    generate_rain_actions(actions, state);
    generate_sac_spell_actions(actions, state);
    generate_offer_actions(actions, state);
    generate_summoners_pact_actions(actions, state);
    generate_green_sun_actions(actions, state);
    generate_ranger_captain_actions(actions, state);
    generate_eldritch_evolution_actions(actions, state);
    generate_crop_rotation_actions(actions, state);
    generate_hand_tutor_actions(actions, state);
    generate_beseech_actions(actions, state);
    generate_top_tutor_actions(actions, state);
}

pub fn generate_fixture_action_cores(state: &FixtureState) -> Vec<ActionCore> {
    let mut actions = Vec::new();
    let mut sink = ActionSink::Labeled(&mut actions);
    populate_actions(&mut sink, state);
    actions
}

fn generate_fixture_bfs_actions(state: &FixtureState) -> Vec<BfsAction> {
    let mut actions = Vec::new();
    let mut sink = ActionSink::Bfs(&mut actions);
    populate_actions(&mut sink, state);
    actions
}

pub fn generate_fixture_actions(state: &FixtureState) -> Vec<GeneratedAction> {
    generate_fixture_action_cores(state)
        .into_iter()
        .map(|action| {
            let next_state_signature = state_signature(&action.next_state);
            GeneratedAction {
                label: action.label,
                next_state: action.next_state,
                next_state_signature,
                priority: action.priority,
            }
        })
        .collect()
}

fn generate_mana_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    for (index, perm) in state.battlefield.iter().enumerate() {
        if !perm.tapped {
            for opt in tap_options(perm, state) {
                let mut next = state.clone();
                if perm.name == "MINE" {
                    let counters = perm.extra.parse::<u8>().unwrap_or(0);
                    if counters <= 1 {
                        next.battlefield.remove(index);
                    } else {
                        next.battlefield[index].tapped = true;
                        next.battlefield[index].extra = (counters - 1).to_string();
                    }
                    next.mana =
                        add_mana(state.mana, mana_for_color_char(opt.chars().next().unwrap()));
                    norm_battlefield(&mut next.battlefield);
                    push_action!(
                        actions,
                        DEFAULT_PRIORITY,
                        format!("tap Gemstone Mine for {opt}"),
                        next
                    );
                    continue;
                }
                if opt == "VAULT" {
                    next.battlefield[index].tapped = true;
                    next.mana = add_mana(state.mana, [0, 0, 0, 0, 0, 3]);
                    norm_battlefield(&mut next.battlefield);
                    push_action!(
                        actions,
                        FAST_MANA_PRIORITY,
                        "tap Mana Vault".to_string(),
                        next
                    );
                    continue;
                }
                next.battlefield[index].tapped = true;
                if perm.name == "DEATHRITE" {
                    next.land_grave_count = next.land_grave_count.saturating_sub(1);
                    next.mana =
                        add_mana(state.mana, mana_for_color_char(opt.chars().next().unwrap()));
                    norm_battlefield(&mut next.battlefield);
                    push_action!(
                        actions,
                        DEFAULT_PRIORITY,
                        format!("tap Deathrite for {opt}"),
                        next
                    );
                } else {
                    next.mana = add_mana(
                        state.mana,
                        mana_for_option(&opt).expect("known mana option"),
                    );
                    norm_battlefield(&mut next.battlefield);
                    push_action!(
                        actions,
                        DEFAULT_PRIORITY,
                        format!("tap {} for {opt}", perm.name),
                        next
                    );
                }
            }
            if perm.name == "VEIN" {
                let mut next = state.clone();
                next.battlefield.remove(index);
                next.land_grave_count = next.land_grave_count.saturating_add(1).min(4);
                next.mana = add_mana(state.mana, [0, 0, 0, 0, 0, 2]);
                norm_battlefield(&mut next.battlefield);
                push_action!(
                    actions,
                    DEFAULT_PRIORITY,
                    "sac Crystal Vein for CC".to_string(),
                    next
                );
            }
            if perm.name == "TOWER" {
                for creature_index in unique_creature_indices(state) {
                    if creature_index == index || creature_index >= state.battlefield.len() {
                        continue;
                    }
                    let creature = state.battlefield[creature_index].clone();
                    let mut next = state.clone();
                    next.battlefield[index].tapped = true;
                    next.battlefield.remove(creature_index);
                    next.mana = add_mana(state.mana, [2, 0, 0, 0, 0, 0]);
                    norm_battlefield(&mut next.battlefield);
                    push_action!(
                        actions,
                        DEFAULT_PRIORITY,
                        format!("tap Phyrexian Tower sacrificing {}", creature.name),
                        next
                    );
                }
            }
        }
        if matches!(perm.name.as_str(), "PETAL" | "TREASURE") && !perm.tapped {
            for color in colors() {
                let mut next = state.clone();
                next.battlefield.remove(index);
                next.mana = add_mana(state.mana, mana_for_color_char(*color));
                norm_battlefield(&mut next.battlefield);
                let label = if perm.name == "PETAL" {
                    "Lotus Petal"
                } else {
                    "Treasure"
                };
                let priority = if perm.name == "PETAL" {
                    FAST_MANA_PRIORITY
                } else {
                    DEFAULT_PRIORITY
                };
                push_action!(actions, priority, format!("sac {label} for {color}"), next);
            }
        }
        if perm.name == "TINDER" {
            let mut next = state.clone();
            next.battlefield.remove(index);
            next.mana = add_mana(state.mana, [0, 2, 0, 0, 0, 0]);
            norm_battlefield(&mut next.battlefield);
            push_action!(
                actions,
                DEFAULT_PRIORITY,
                "sac Tinder Wall".to_string(),
                next
            );
        }
        if perm.name == "RAGAVAN" && !perm.tapped && !perm.extra.ends_with('*') {
            let mut next = state.clone();
            next.battlefield[index].tapped = true;
            next.battlefield.push(FixturePerm {
                name: "TREASURE".to_string(),
                tapped: false,
                extra: String::new(),
            });
            norm_battlefield(&mut next.battlefield);
            push_ragavan_action!(
                actions,
                DEFAULT_PRIORITY,
                "attack Ragavan, connect, create Treasure".to_string(),
                next
            );
        }
        if state.rain_active && is_land_perm(perm) {
            let mut next = state.clone();
            next.battlefield.remove(index);
            next.land_grave_count = next.land_grave_count.saturating_add(1).min(4);
            next.mana = add_mana(state.mana, [1, 0, 0, 0, 0, 0]);
            norm_battlefield(&mut next.battlefield);
            push_action!(
                actions,
                DEFAULT_PRIORITY,
                format!("sac {} to Rain of Filth", perm.name),
                next
            );
        }
    }
}

fn generate_engine_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    for (card, cost, target_type, perm) in [
        (
            "Rhystic Study",
            [2, 0, 0, 1, 0, 0],
            "ENCH",
            FixturePerm {
                name: "ENGINE_ENCH".to_string(),
                tapped: false,
                extra: String::new(),
            },
        ),
        (
            "Heartwood Storyteller",
            [1, 0, 0, 0, 0, 2],
            "CREATURE",
            FixturePerm {
                name: "HEARTWOOD".to_string(),
                tapped: false,
                extra: "G*".to_string(),
            },
        ),
    ] {
        if !state.hand.iter().any(|c| c == card) {
            continue;
        }
        for mana in pay_options(state.mana, cost) {
            let mut next = state.clone();
            next.hand = remove_first(&state.hand, card);
            next.battlefield.push(perm.clone());
            next.mana = mana;
            norm_hand(&mut next.hand);
            norm_battlefield(&mut next.battlefield);
            next = after_cast(state, next);
            next = add_engine(next, target_type, card);
            push_action!(
                actions,
                label_priority(card),
                format!("cast engine {card}"),
                next
            );
        }
    }
}

fn generate_commander_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if state.battlefield.iter().any(|perm| perm.name == "NICK") {
        return;
    }
    for mana in pay_options(state.mana, [0, 0, 0, 0, 1, 0]) {
        let mut next = state.clone();
        next.battlefield.push(FixturePerm {
            name: "NICK".to_string(),
            tapped: false,
            extra: "W*".to_string(),
        });
        next.mana = mana;
        norm_battlefield(&mut next.battlefield);
        push_action!(
            actions,
            DEFAULT_PRIORITY,
            "cast Nick Fury, Agent of S.H.I.E.L.D.".to_string(),
            after_cast(state, next)
        );
    }
}

fn generate_land_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if state.land_played {
        return;
    }
    for card in &state.hand {
        if !is_land_card(card) && !is_theoretical_rainbow_land(card) && !is_mdfc_land(card) {
            continue;
        }
        for (perm, library, grave_inc, label) in land_options(card, &state.library) {
            let mut battlefield: Vec<FixturePerm> = state
                .battlefield
                .iter()
                .filter(|perm| perm.name != "CITY")
                .cloned()
                .collect();
            battlefield.push(perm);
            let mut next = state.clone();
            next.hand = remove_first(&state.hand, card);
            next.library = library;
            next.battlefield = battlefield;
            next.land_played = true;
            next.land_grave_count = next.land_grave_count.saturating_add(grave_inc).min(4);
            norm_hand(&mut next.hand);
            norm_battlefield(&mut next.battlefield);
            push_action!(actions, LAND_PRIORITY, format!("play {card}{label}"), next);
        }
    }
}

fn generate_zero_artifact_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    for (card, perm_name) in [
        ("Lotus Petal", "PETAL"),
        ("Lion's Eye Diamond", "LED"),
        ("Mox Amber", "AMBER"),
    ] {
        if state.hand.iter().any(|c| c == card) {
            let mut next = state.clone();
            next.hand = remove_first(&state.hand, card);
            next.battlefield.push(FixturePerm {
                name: perm_name.to_string(),
                tapped: false,
                extra: String::new(),
            });
            norm_hand(&mut next.hand);
            norm_battlefield(&mut next.battlefield);
            push_action!(
                actions,
                FAST_MANA_PRIORITY,
                format!("cast {card}"),
                after_cast(state, next)
            );
        }
    }
}

fn generate_chrome_mox_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if !state.hand.iter().any(|card| card == "Chrome Mox") {
        return;
    }
    for imprint in &state.hand {
        if imprint == "Chrome Mox"
            || is_land_card(imprint)
            || is_theoretical_rainbow_land(imprint)
            || is_artifact_card(imprint)
            || card_colors(imprint).is_empty()
        {
            continue;
        }
        let mut next = state.clone();
        next.hand = remove_first(&remove_first(&state.hand, "Chrome Mox"), imprint);
        next.battlefield.push(FixturePerm {
            name: "CHROME".to_string(),
            tapped: false,
            extra: card_colors(imprint).to_string(),
        });
        norm_hand(&mut next.hand);
        norm_battlefield(&mut next.battlefield);
        push_action!(
            actions,
            FAST_MANA_PRIORITY,
            format!("cast Chrome Mox imprint {imprint}"),
            after_cast(state, next)
        );
    }
}

fn generate_mox_diamond_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if !state.hand.iter().any(|card| card == "Mox Diamond") {
        return;
    }
    for land in &state.hand {
        if !is_land_card(land) && !is_theoretical_rainbow_land(land) {
            continue;
        }
        let mut next = state.clone();
        next.hand = remove_first(&remove_first(&state.hand, "Mox Diamond"), land);
        next.battlefield.push(FixturePerm {
            name: "DIAMOND".to_string(),
            tapped: false,
            extra: String::new(),
        });
        next.land_grave_count = next.land_grave_count.saturating_add(1).min(4);
        norm_hand(&mut next.hand);
        norm_battlefield(&mut next.battlefield);
        push_action!(
            actions,
            FAST_MANA_PRIORITY,
            format!("cast Mox Diamond discard {land}"),
            after_cast(state, next)
        );
    }
}

fn generate_artifact_spell_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    for (card, perm, cost) in [
        ("Sol Ring", "SOL", [1, 0, 0, 0, 0, 0]),
        ("Mana Vault", "VAULT", [1, 0, 0, 0, 0, 0]),
    ] {
        cast_cost_actions(
            actions,
            state,
            card,
            FixturePerm {
                name: perm.to_string(),
                tapped: false,
                extra: String::new(),
            },
            cost,
            FAST_MANA_PRIORITY,
        );
    }
}

fn generate_creature_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    for (card, costs) in [
        ("Birds of Paradise", vec![[0, 0, 0, 0, 0, 1]]),
        (
            "Deathrite Shaman",
            vec![[0, 1, 0, 0, 0, 0], [0, 0, 0, 0, 0, 1]],
        ),
        ("Esper Sentinel", vec![[0, 0, 0, 0, 1, 0]]),
        ("Lotho, Corrupt Shirriff", vec![[0, 1, 0, 0, 1, 0]]),
        ("Ragavan, Nimble Pilferer", vec![[0, 0, 1, 0, 0, 0]]),
        ("Tinder Wall", vec![[0, 0, 0, 0, 0, 1]]),
    ] {
        if !state.hand.iter().any(|c| c == card) {
            continue;
        }
        let hand = remove_first(&state.hand, card);
        for cost in costs {
            for mana in pay_options(state.mana, cost) {
                let mut next = state.clone();
                next.hand = hand.clone();
                next.battlefield.push(creature_perm(card));
                next.mana = mana;
                norm_hand(&mut next.hand);
                norm_battlefield(&mut next.battlefield);
                push_action!(
                    actions,
                    label_priority(card),
                    format!("cast {card}"),
                    after_cast(state, next)
                );
            }
        }
    }
}

fn generate_spirit_guide_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if state.hand.iter().any(|card| card == "Simian Spirit Guide") {
        let mut next = state.clone();
        next.hand = remove_first(&state.hand, "Simian Spirit Guide");
        next.mana = add_mana(state.mana, [0, 1, 0, 0, 0, 0]);
        norm_hand(&mut next.hand);
        push_action!(
            actions,
            DEFAULT_PRIORITY,
            "exile Simian Spirit Guide".to_string(),
            next
        );
    }
    if state.hand.iter().any(|card| card == "Elvish Spirit Guide") {
        let mut next = state.clone();
        next.hand = remove_first(&state.hand, "Elvish Spirit Guide");
        next.mana = add_mana(state.mana, [0, 0, 0, 0, 1, 0]);
        norm_hand(&mut next.hand);
        push_action!(
            actions,
            DEFAULT_PRIORITY,
            "exile Elvish Spirit Guide".to_string(),
            next
        );
    }
}

fn generate_ritual_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    for (card, cost, add) in [
        ("Dark Ritual", [0, 1, 0, 0, 0, 0], [3, 0, 0, 0, 0, 0]),
        ("Rite of Flame", [0, 0, 1, 0, 0, 0], [0, 2, 0, 0, 0, 0]),
    ] {
        if !state.hand.iter().any(|c| c == card) {
            continue;
        }
        for mana in pay_options(state.mana, cost) {
            let mut next = state.clone();
            next.hand = remove_first(&state.hand, card);
            next.mana = add_mana(mana, add);
            norm_hand(&mut next.hand);
            push_action!(
                actions,
                DEFAULT_PRIORITY,
                format!("cast {card}"),
                after_cast(state, next)
            );
        }
    }
}

fn generate_dark_ritual_action(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if !state.hand.iter().any(|card| card == "Dark Ritual") {
        return;
    }
    for mana in pay_options(state.mana, [0, 1, 0, 0, 0, 0]) {
        let mut next = state.clone();
        next.hand = remove_first(&state.hand, "Dark Ritual");
        next.mana = add_mana(mana, [3, 0, 0, 0, 0, 0]);
        norm_hand(&mut next.hand);
        push_action!(
            actions,
            DEFAULT_PRIORITY,
            "cast Dark Ritual".to_string(),
            after_cast(state, next)
        );
    }
}

fn generate_rain_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if !state.hand.iter().any(|card| card == "Rain of Filth") {
        return;
    }
    for mana in pay_options(state.mana, [0, 1, 0, 0, 0, 0]) {
        let mut next = state.clone();
        next.hand = remove_first(&state.hand, "Rain of Filth");
        next.mana = mana;
        next.rain_active = true;
        norm_hand(&mut next.hand);
        push_action!(
            actions,
            DEFAULT_PRIORITY,
            "cast Rain of Filth".to_string(),
            after_cast(state, next)
        );
    }
}

fn generate_sac_spell_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    for (card, cost, add) in [("Culling the Weak", [0, 1, 0, 0, 0, 0], [4, 0, 0, 0, 0, 0])] {
        if !state.hand.iter().any(|c| c == card) {
            continue;
        }
        for creature_index in unique_creature_indices(state) {
            for mana in pay_options(state.mana, cost) {
                let base = sac_creature(state, creature_index);
                let mut next = base.clone();
                next.hand = remove_first(&base.hand, card);
                next.mana = add_mana(mana, add);
                norm_hand(&mut next.hand);
                push_action!(
                    actions,
                    DEFAULT_PRIORITY,
                    format!("cast {card}"),
                    after_cast(&base, next)
                );
            }
        }
    }
}

fn generate_offer_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if !state
        .hand
        .iter()
        .any(|card| card == "An Offer You Can't Refuse")
    {
        return;
    }
    for bait in &state.hand {
        if bait == "An Offer You Can't Refuse" || !offer_bait_can_help(state, bait) {
            continue;
        }
        for bait_cost in offer_counterable_costs(bait) {
            for after_bait_mana in pay_options(state.mana, bait_cost) {
                let mut bait_cast = state.clone();
                bait_cast.hand = remove_first(&state.hand, bait);
                bait_cast.mana = after_bait_mana;
                norm_hand(&mut bait_cast.hand);
                bait_cast = after_cast(state, bait_cast);
                for after_offer_mana in pay_options(bait_cast.mana, [0, 0, 0, 1, 0, 0]) {
                    let mut next = bait_cast.clone();
                    next.hand = remove_first(&bait_cast.hand, "An Offer You Can't Refuse");
                    next.mana = after_offer_mana;
                    next.battlefield.push(FixturePerm {
                        name: "TREASURE".to_string(),
                        tapped: false,
                        extra: String::new(),
                    });
                    next.battlefield.push(FixturePerm {
                        name: "TREASURE".to_string(),
                        tapped: false,
                        extra: String::new(),
                    });
                    norm_hand(&mut next.hand);
                    norm_battlefield(&mut next.battlefield);
                    push_action!(
                        actions,
                        label_priority(bait),
                        format!("cast {bait}, counter it with An Offer You Can't Refuse"),
                        after_cast(&bait_cast, next),
                    );
                }
            }
        }
    }
}

fn offer_counterable_costs(card: &str) -> Vec<Cost> {
    match card {
        "Summoner's Pact" | "Lotus Petal" | "Chaos Emerald" | "Chrome Mox"
        | "Lion's Eye Diamond" | "Mox Amber" | "Mox Diamond" | "Mox Opal" | "Paradise Mantle"
        | "Noxious Revival" => vec![[0, 0, 0, 0, 0, 0]],
        "Sol Ring" | "Mana Vault" | "Springleaf Drum" => vec![[1, 0, 0, 0, 0, 0]],
        "Arcane Signet" => vec![[2, 0, 0, 0, 0, 0]],
        "Wishclaw Talisman" | "Demonic Tutor" => vec![[1, 1, 0, 0, 0, 0]],
        "Dark Ritual" | "Imperial Seal" | "Scheming Symmetry" | "Vampiric Tutor" => {
            vec![[0, 1, 0, 0, 0, 0]]
        }
        "Rite of Flame" | "Strike It Rich" => vec![[0, 0, 1, 0, 0, 0]],
        "Enlightened Tutor" => vec![[0, 0, 0, 0, 1, 0]],
        "Mystical Tutor" => vec![[0, 0, 0, 1, 0, 0]],
        "Worldly Tutor" | "Nature's Chosen" | "Green Sun's Zenith" => vec![[0, 0, 0, 0, 0, 1]],
        "Beseech the Mirror" => vec![[1, 3, 0, 0, 0, 0]],
        "Rhystic Study" | "Copy Enchantment" => vec![[2, 0, 0, 1, 0, 0]],
        "Mystic Remora" => vec![[0, 0, 0, 1, 0, 0]],
        "Mirrormade" | "Flash Photography" => vec![[1, 0, 0, 2, 0, 0]],
        "Necropotence" => vec![[0, 3, 0, 0, 0, 0]],
        "Smothering Tithe" => vec![[3, 0, 0, 0, 1, 0]],
        "Manamorphose" => vec![[1, 0, 1, 0, 0, 0], [1, 0, 0, 0, 0, 1]],
        _ => Vec::new(),
    }
}

fn offer_bait_can_help(state: &FixtureState, bait: &str) -> bool {
    if matches!(
        bait,
        "Rhystic Study"
            | "Mystic Remora"
            | "Esper Sentinel"
            | "Heartwood Storyteller"
            | "Copy Enchantment"
            | "Mirrormade"
            | "Flash Photography"
            | "Clever Impersonator"
            | "Necropotence"
            | "Smothering Tithe"
    ) {
        return false;
    }
    if bait == "Noxious Revival" && state.land_grave_count == 0 {
        return false;
    }
    let costs = offer_counterable_costs(bait);
    if costs.is_empty() {
        return false;
    }
    if costs
        .iter()
        .any(|cost| cost.iter().copied().sum::<u8>() <= 1)
    {
        return true;
    }
    state
        .battlefield
        .iter()
        .any(|perm| matches!(perm.name.as_str(), "BIRGI" | "LOTHO"))
}

fn generate_summoners_pact_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if !state.hand.iter().any(|card| card == "Summoner's Pact") {
        return;
    }
    for target in [
        "Elvish Spirit Guide",
        "Tinder Wall",
        "Birds of Paradise",
        "Deathrite Shaman",
    ] {
        if !state.library.iter().any(|card| card == target) {
            continue;
        }
        let mut hand = remove_first(&state.hand, "Summoner's Pact");
        hand.push(target.to_string());
        norm_hand(&mut hand);
        let mut next = state.clone();
        next.hand = hand;
        next.library = remove_first(&state.library, target);
        next.pact_debt = next.pact_debt.saturating_add(1).min(2);
        push_action!(
            actions,
            ENGINE_TUTOR_PRIORITY,
            format!("cast Summoner's Pact for {target}"),
            after_cast(state, next)
        );
    }
    if state
        .library
        .iter()
        .any(|card| card == "Heartwood Storyteller")
    {
        let mut hand = remove_first(&state.hand, "Summoner's Pact");
        hand.push("Heartwood Storyteller".to_string());
        norm_hand(&mut hand);
        let mut next = state.clone();
        next.hand = hand;
        next.library = remove_first(&state.library, "Heartwood Storyteller");
        next.pact_debt = next.pact_debt.saturating_add(1).min(2);
        push_action!(
            actions,
            ENGINE_TUTOR_PRIORITY,
            "cast Summoner's Pact for Heartwood".to_string(),
            after_cast(state, next)
        );
    }
}

fn generate_green_sun_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if !state.hand.iter().any(|card| card == "Green Sun's Zenith") {
        return;
    }
    for target in ["Tinder Wall", "Birds of Paradise", "Deathrite Shaman"] {
        if !state.library.iter().any(|card| card == target) {
            continue;
        }
        for mana in pay_options(state.mana, [1, 0, 0, 0, 0, 1]) {
            let mut next = state.clone();
            next.hand = remove_first(&state.hand, "Green Sun's Zenith");
            next.library = remove_first(&state.library, target);
            next.battlefield.push(creature_perm(target));
            next.mana = mana;
            norm_hand(&mut next.hand);
            norm_battlefield(&mut next.battlefield);
            push_action!(
                actions,
                ENGINE_TUTOR_PRIORITY,
                format!("cast Green Sun's Zenith for {target}"),
                after_cast(state, next)
            );
        }
    }
    if state
        .library
        .iter()
        .any(|card| card == "Heartwood Storyteller")
    {
        for mana in pay_options(state.mana, [3, 0, 0, 0, 0, 1]) {
            let mut next = state.clone();
            next.hand = remove_first(&state.hand, "Green Sun's Zenith");
            next.library = remove_first(&state.library, "Heartwood Storyteller");
            next.battlefield.push(FixturePerm {
                name: "HEARTWOOD".to_string(),
                tapped: false,
                extra: "G*".to_string(),
            });
            next.mana = mana;
            norm_hand(&mut next.hand);
            norm_battlefield(&mut next.battlefield);
            let next = add_engine(after_cast(state, next), "CREATURE", "Heartwood Storyteller");
            push_action!(
                actions,
                ENGINE_TUTOR_PRIORITY,
                "cast Green Sun's Zenith for Heartwood".to_string(),
                next
            );
        }
    }
}

fn generate_ranger_captain_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if !state
        .hand
        .iter()
        .any(|card| card == "Ranger-Captain of Eos")
        || !state.library.iter().any(|card| card == "Esper Sentinel")
    {
        return;
    }
    for mana in pay_options(state.mana, [1, 0, 0, 0, 2, 0]) {
        let mut hand = remove_first(&state.hand, "Ranger-Captain of Eos");
        hand.push("Esper Sentinel".to_string());
        norm_hand(&mut hand);
        let mut next = state.clone();
        next.hand = hand;
        next.library = remove_first(&state.library, "Esper Sentinel");
        next.battlefield.push(FixturePerm {
            name: "CREATURE".to_string(),
            tapped: false,
            extra: "W*".to_string(),
        });
        next.mana = mana;
        norm_battlefield(&mut next.battlefield);
        push_action!(
            actions,
            ENGINE_TUTOR_PRIORITY,
            "cast Ranger-Captain for Esper Sentinel".to_string(),
            after_cast(state, next)
        );
    }
}

fn generate_eldritch_evolution_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if !state.hand.iter().any(|card| card == "Eldritch Evolution")
        || !state
            .library
            .iter()
            .any(|card| card == "Heartwood Storyteller")
    {
        return;
    }
    for creature_index in unique_creature_indices(state) {
        let creature = &state.battlefield[creature_index];
        if creature_mv_by_perm(creature).saturating_add(2) < 3 {
            continue;
        }
        for mana in pay_options(state.mana, [1, 0, 0, 0, 0, 2]) {
            let base = sac_creature(state, creature_index);
            let mut next = base.clone();
            next.hand = remove_first(&base.hand, "Eldritch Evolution");
            next.library = remove_first(&base.library, "Heartwood Storyteller");
            next.battlefield.push(FixturePerm {
                name: "HEARTWOOD".to_string(),
                tapped: false,
                extra: "G*".to_string(),
            });
            next.mana = mana;
            norm_hand(&mut next.hand);
            norm_battlefield(&mut next.battlefield);
            let next = add_engine(after_cast(&base, next), "CREATURE", "Heartwood Storyteller");
            push_action!(
                actions,
                ENGINE_TUTOR_PRIORITY,
                "cast Eldritch Evolution for Heartwood".to_string(),
                next
            );
        }
    }
}

fn generate_crop_rotation_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if !state.hand.iter().any(|card| card == "Crop Rotation")
        || !state.battlefield.iter().any(is_land_perm)
    {
        return;
    }
    let library_set: BTreeSet<&str> = state.library.iter().map(String::as_str).collect();
    for mana in pay_options(state.mana, [0, 0, 0, 0, 0, 1]) {
        for (land_index, perm) in state.battlefield.iter().enumerate() {
            if !is_land_perm(perm) {
                continue;
            }
            for target in [
                "Ancient Tomb",
                "City of Brass",
                "City of Traitors",
                "Command Tower",
                "Crystal Vein",
                "Mana Confluence",
                "Phyrexian Tower",
                "Tropical Island",
                "Tundra",
                "Underground Sea",
                "Volcanic Island",
            ] {
                if !library_set.contains(target) {
                    continue;
                }
                let target_removed_library = remove_first(&state.library, target);
                for (target_perm, lib, grave_inc, label) in
                    land_options(target, &target_removed_library)
                {
                    let base = sac_land(state, land_index);
                    let mut next = base.clone();
                    next.hand = remove_first(&base.hand, "Crop Rotation");
                    next.library = lib;
                    next.battlefield.push(target_perm);
                    next.mana = mana;
                    next.land_grave_count = base.land_grave_count.saturating_add(grave_inc).min(4);
                    norm_hand(&mut next.hand);
                    norm_battlefield(&mut next.battlefield);
                    push_action!(
                        actions,
                        DEFAULT_PRIORITY,
                        format!("cast Crop Rotation for {target}{label}"),
                        after_cast(state, next)
                    );
                }
            }
        }
    }
}

fn led_mana_for_color(color: char) -> Mana {
    let mut out = [0, 0, 0, 0, 0, 0];
    if let Some(index) = color_index(color) {
        out[index] = 3;
    }
    out
}

fn generate_led_tutor_line(
    actions: &mut ActionSink<'_>,
    state: &FixtureState,
    tutor: &str,
    cost: Cost,
    creature_index: Option<usize>,
    target: &str,
) {
    if !state.hand.iter().any(|card| card == "Lion's Eye Diamond")
        && !state.battlefield.iter().any(|perm| perm.name == "LED")
    {
        return;
    }
    for (led_index, led) in state.battlefield.iter().enumerate() {
        if led.name != "LED" || led.tapped {
            continue;
        }
        for mana_after_cost in pay_options(state.mana, cost) {
            let base = if let Some(index) = creature_index {
                sac_creature(state, index)
            } else {
                state.clone()
            };
            let mut battlefield = base.battlefield.clone();
            if led_index >= battlefield.len() || battlefield[led_index].name != "LED" {
                continue;
            }
            battlefield.remove(led_index);
            for color in colors() {
                let floated = add_mana(mana_after_cost, led_mana_for_color(*color));
                let mut empty = base.clone();
                empty.hand = Vec::new();
                empty.battlefield = battlefield.clone();
                empty.mana = floated;
                norm_battlefield(&mut empty.battlefield);
                if let Some(next) = led_target_state(&empty, target, floated) {
                    push_action!(
                        actions,
                        tutor_target_priority(tutor, target),
                        format!("cast {tutor}, crack LED for {color}, tutor {target}"),
                        after_cast(&base, next),
                    );
                }
            }
        }
    }
}

fn generate_hand_tutor_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    for (tutor, cost) in [
        ("Demonic Tutor", [1, 1, 0, 0, 0, 0]),
        ("Diabolic Intent", [1, 1, 0, 0, 0, 0]),
    ] {
        if !state.hand.iter().any(|card| card == tutor) {
            continue;
        }
        let creature_indices: Vec<Option<usize>> = if tutor == "Diabolic Intent" {
            unique_creature_indices(state)
                .into_iter()
                .map(Some)
                .collect()
        } else {
            vec![None]
        };
        for creature_index in creature_indices {
            for target in tutor_targets(tutor, state) {
                for mana in pay_options(state.mana, cost) {
                    let base = if let Some(index) = creature_index {
                        sac_creature(state, index)
                    } else {
                        state.clone()
                    };
                    let mut hand = remove_first(&base.hand, tutor);
                    hand.push(target.clone());
                    norm_hand(&mut hand);
                    let mut next = base.clone();
                    next.hand = hand;
                    next.library = remove_first(&base.library, &target);
                    next.mana = mana;
                    push_action!(
                        actions,
                        tutor_target_priority(tutor, &target),
                        format!("cast {tutor} for {target}"),
                        after_cast(&base, next)
                    );
                    generate_led_tutor_line(actions, state, tutor, cost, creature_index, &target);
                }
            }
        }
    }
}

fn generate_beseech_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    if !state.hand.iter().any(|card| card == "Beseech the Mirror") {
        return;
    }
    let cost = [1, 3, 0, 0, 0, 0];
    for target in beseech_targets(state) {
        for mana in pay_options(state.mana, cost) {
            let mut hand = remove_first(&state.hand, "Beseech the Mirror");
            hand.push(target.clone());
            norm_hand(&mut hand);
            let mut next = state.clone();
            next.hand = hand;
            next.library = remove_first(&state.library, &target);
            next.mana = mana;
            push_action!(
                actions,
                tutor_target_priority("Beseech the Mirror", &target),
                format!("cast Beseech the Mirror for {target}"),
                after_cast(state, next),
            );
        }
        generate_led_beseech_line(actions, state, &target);
    }

    for bargain_index in 0..state.battlefield.len() {
        let permanent = &state.battlefield[bargain_index];
        if !is_artifact_perm(permanent) && !is_enchantment_perm(permanent) {
            continue;
        }
        for target in beseech_targets(state) {
            for mana in pay_options(state.mana, cost) {
                let base = sac_perm(state, bargain_index);
                let mut next = base.clone();
                next.hand = remove_first(&base.hand, "Beseech the Mirror");
                next.library = remove_first(&base.library, &target);
                next.mana = mana;
                norm_hand(&mut next.hand);
                next = after_cast(state, next);
                next = resolve_free_engine(next, &target);
                push_action!(
                    actions,
                    tutor_target_priority("Beseech the Mirror", &target),
                    format!("cast bargained Beseech for {target}"),
                    next,
                );
            }
        }
    }
}

fn generate_led_beseech_line(actions: &mut ActionSink<'_>, state: &FixtureState, target: &str) {
    if !state
        .battlefield
        .iter()
        .any(|perm| perm.name == "LED" && !perm.tapped)
    {
        return;
    }
    let cost = [1, 3, 0, 0, 0, 0];
    for (led_index, led) in state.battlefield.iter().enumerate() {
        if led.name != "LED" || led.tapped {
            continue;
        }
        for mana_after_cost in pay_options(state.mana, cost) {
            let mut battlefield = state.battlefield.clone();
            battlefield.remove(led_index);
            for color in colors() {
                let floated = add_mana(mana_after_cost, led_mana_for_color(*color));
                let mut empty = state.clone();
                empty.hand = Vec::new();
                empty.battlefield = battlefield.clone();
                empty.mana = floated;
                norm_battlefield(&mut empty.battlefield);
                if let Some(next) = led_target_state(&empty, target, floated) {
                    push_action!(
                        actions,
                        tutor_target_priority("Beseech the Mirror", target),
                        format!("cast Beseech the Mirror, crack LED for {color}, tutor {target}"),
                        after_cast(state, next),
                    );
                }
            }
        }
    }
}

fn generate_top_tutor_actions(actions: &mut ActionSink<'_>, state: &FixtureState) {
    for (tutor, cost) in [
        ("Enlightened Tutor", [0, 0, 0, 0, 1, 0]),
        ("Imperial Seal", [0, 1, 0, 0, 0, 0]),
        ("Mystical Tutor", [0, 0, 0, 1, 0, 0]),
        ("Scheming Symmetry", [0, 1, 0, 0, 0, 0]),
        ("Vampiric Tutor", [0, 1, 0, 0, 0, 0]),
    ] {
        if !state.hand.iter().any(|card| card == tutor) {
            continue;
        }
        for target in tutor_targets(tutor, state) {
            for mana in pay_options(state.mana, cost) {
                let mut library = vec![target.clone()];
                library.extend(
                    state
                        .library
                        .iter()
                        .filter(|card| *card != &target)
                        .cloned(),
                );
                let mut next = state.clone();
                next.hand = remove_first(&state.hand, tutor);
                next.library = library;
                next.mana = mana;
                norm_hand(&mut next.hand);
                push_action!(
                    actions,
                    tutor_target_priority(tutor, &target),
                    format!("cast {tutor} for {target}"),
                    after_cast(state, next)
                );
            }
        }
    }
}

fn zero_mana() -> Mana {
    [0, 0, 0, 0, 0, 0]
}

fn preturn_caverns_top_tutor_states(state: &FixtureState) -> Vec<FixtureState> {
    if !state.battlefield.iter().any(|perm| perm.name == "CAVERN") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for tutor in [
        "Enlightened Tutor",
        "Mystical Tutor",
        "Vampiric Tutor",
        "Worldly Tutor",
    ] {
        if !state.hand.iter().any(|card| card == tutor) {
            continue;
        }
        for target in preturn_top_tutor_targets(tutor, state) {
            if !state.library.iter().any(|card| card == &target) {
                continue;
            }
            let mut library = vec![target.clone()];
            library.extend(
                state
                    .library
                    .iter()
                    .filter(|card| *card != &target)
                    .cloned(),
            );
            let mut next = state.clone();
            next.hand = remove_first(&state.hand, tutor);
            next.library = library;
            next.mana = zero_mana();
            next.spells_this_turn = 0;
            norm_hand(&mut next.hand);
            out.push(next);
        }
    }
    out
}

fn begin_turn(state: &FixtureState) -> FixtureState {
    let mut next = state.clone();
    for perm in &mut next.battlefield {
        if !matches!(perm.name.as_str(), "VAULT" | "MONOLITH") {
            perm.tapped = false;
        }
        if perm.extra.ends_with('*') {
            perm.extra.pop();
        }
    }
    norm_battlefield(&mut next.battlefield);
    next.mana = zero_mana();
    next.land_played = false;
    next.nature_untap_used = false;
    next.nature_tap_used = false;
    next.rain_active = false;
    next.spells_this_turn = 0;
    next
}

fn end_turn(state: &FixtureState) -> FixtureState {
    let mut next = state.clone();
    if next.battlefield.iter().any(|perm| perm.name == "GLIMMER")
        && !next.battlefield.iter().any(is_artifact_perm)
    {
        next.battlefield.retain(|perm| perm.name != "GLIMMER");
    }
    next.mana = zero_mana();
    next.land_played = false;
    next.rain_active = false;
    next.spells_this_turn = 0;
    norm_battlefield(&mut next.battlefield);
    next
}

fn parse_engine_name_turn(item: &str) -> Option<(&str, u8)> {
    let (name, turn_text) = item.rsplit_once('@')?;
    let turn = turn_text.parse::<u8>().ok()?;
    Some((name, turn))
}

fn engine_success_label(request: &CloseTurnRequest, state: &FixtureState) -> Option<String> {
    if request.engine_success_policy == "count" {
        if state.engine_count < request.engine_target_count {
            return None;
        }
        return best_engine_name(state).or_else(|| Some("engine".to_string()));
    }
    if request.engine_success_policy != "resilient" {
        return None;
    }
    let mut candidates: Vec<(u8, u8, String)> = Vec::new();
    for item in &state.engine_names {
        let Some((name, turn)) = parse_engine_name_turn(item) else {
            continue;
        };
        if matches!(
            name,
            "Rhystic Study" | "Heartwood Storyteller" | "Smothering Tithe"
        ) && turn <= 2
        {
            candidates.push((engine_target_priority(name).0, turn, name.to_string()));
        }
    }
    for item in &state.engine_names {
        let Some((name, turn)) = parse_engine_name_turn(item) else {
            continue;
        };
        if name == "Mystic Remora"
            && turn == 1
            && can_keep_remora(request, state, request.remora_upkeep_payments)
        {
            candidates.push((engine_target_priority(name).0, turn, name.to_string()));
        }
    }
    candidates.into_iter().min().map(|(_, _, name)| name)
}

fn best_engine_name(state: &FixtureState) -> Option<String> {
    let mut candidates: Vec<(u8, u8, String)> = Vec::new();
    for item in &state.engine_names {
        if let Some((name, turn)) = parse_engine_name_turn(item) {
            candidates.push((engine_target_priority(name).0, turn, name.to_string()));
        } else if let Some((name, _turn_text)) = item.rsplit_once('@') {
            candidates.push((engine_target_priority(name).0, 99, name.to_string()));
        }
    }
    candidates.into_iter().min().map(|(_, _, name)| name)
}

fn success_label(request: &CloseTurnRequest, state: &FixtureState) -> Option<String> {
    let label = if request.goal == "engine" {
        engine_success_label(request, state)
    } else if state.engine_count > 0 {
        Some("Rhystic Study".to_string())
    } else if state.hand.iter().any(|card| card == "Rhystic Study")
        && !pay_options(state.mana, [2, 0, 0, 1, 0, 0]).is_empty()
    {
        Some("Rhystic Study".to_string())
    } else {
        None
    }?;
    if can_survive_next_pact_upkeep(request, state) {
        Some(label)
    } else {
        None
    }
}

fn can_survive_next_pact_upkeep(request: &CloseTurnRequest, state: &FixtureState) -> bool {
    if state.pact_debt == 0 {
        return true;
    }
    let next_turn = begin_turn(&end_turn(state));
    let cost = [2 * next_turn.pact_debt, 0, 0, 0, 0, 2 * next_turn.pact_debt];
    can_pay_with_simple_taps(&next_turn, cost)
        || pay_upkeep_pacts(request, &next_turn)
        || can_cast_angels_grace_upkeep(request, &next_turn)
}

fn can_pay_with_simple_taps(state: &FixtureState, cost: Cost) -> bool {
    let mut mana_options: HashSet<Mana> = HashSet::new();
    mana_options.insert(zero_mana());
    for perm in &state.battlefield {
        if perm.tapped {
            continue;
        }
        let mut additions = HashSet::new();
        for opt in tap_options(perm, state) {
            if opt == "PETAL" {
                continue;
            }
            if opt == "VAULT" {
                additions.insert([0, 0, 0, 0, 0, 3]);
            } else if opt.len() == 1 && colors().contains(&opt.chars().next().unwrap()) {
                additions.insert(mana_for_color_char(opt.chars().next().unwrap()));
            } else if let Some(mana) = mana_for_option(&opt) {
                additions.insert(mana);
            }
        }
        if additions.is_empty() {
            continue;
        }
        let mut updated = mana_options.clone();
        for current in &mana_options {
            for add in &additions {
                let candidate = add_mana(*current, *add);
                if !pay_options(candidate, cost).is_empty() {
                    return true;
                }
                updated.insert(candidate);
            }
        }
        mana_options = updated;
    }
    mana_options
        .into_iter()
        .any(|mana| !pay_options(mana, cost).is_empty())
}

fn normalize_upkeep_payment_state(state: &FixtureState, cost: Cost) -> FixtureState {
    let generic = cost[0];
    let black = cost[1];
    let red = cost[2];
    let blue = cost[3];
    let white = cost[4];
    let green = cost[5];
    if black != 0 || red != 0 || blue != 0 || white != 0 {
        return state.clone();
    }
    let [b, r, u, w, g, c] = state.mana;
    let mana = if green != 0 {
        let non_green = generic.min(
            b.saturating_add(r)
                .saturating_add(u)
                .saturating_add(w)
                .saturating_add(c),
        );
        let useful_green = generic.saturating_add(green).min(g);
        [non_green, 0, 0, 0, useful_green, 0]
    } else {
        [
            generic.min(
                b.saturating_add(r)
                    .saturating_add(u)
                    .saturating_add(w)
                    .saturating_add(g)
                    .saturating_add(c),
            ),
            0,
            0,
            0,
            0,
            0,
        ]
    };
    if mana == state.mana {
        return state.clone();
    }
    let mut next = state.clone();
    next.mana = cap_mana(mana);
    next
}

fn upkeep_mana_actions(state: &FixtureState) -> Vec<BfsAction> {
    let mut actions = Vec::new();
    let mut sink = ActionSink::Bfs(&mut actions);
    generate_mana_actions(&mut sink, state);
    generate_spirit_guide_actions(&mut sink, state);
    generate_dark_ritual_action(&mut sink, state);
    generate_rain_actions(&mut sink, state);
    generate_sac_spell_actions(&mut sink, state);
    for (index, perm) in state.battlefield.iter().enumerate() {
        if perm.name != "LED" || perm.tapped {
            continue;
        }
        for color in colors() {
            let mut next = state.clone();
            next.battlefield.remove(index);
            next.hand.clear();
            next.mana = add_mana(state.mana, led_mana_for_color(*color));
            norm_battlefield(&mut next.battlefield);
            push_action!(
                sink,
                FAST_MANA_PRIORITY,
                format!("sac Lion's Eye Diamond for {color}"),
                next
            );
        }
    }
    drop(sink);
    actions
}

fn pay_upkeep_options(
    request: &CloseTurnRequest,
    state: &FixtureState,
    cost: Cost,
    clear_pact: bool,
    limit: usize,
) -> Vec<FixtureState> {
    let start = normalize_upkeep_payment_state(state, cost);
    let mut queue = Vec::new();
    let mut seen = HashSet::new();
    let mut best_mana: HashMap<FixtureState, Vec<Mana>> = HashMap::new();
    let mut paid_states = Vec::new();
    queue.push(start.clone());
    seen.insert(start);
    while let Some(current) = queue.pop() {
        if !pay_options(current.mana, cost).is_empty() {
            let mut paid = current.clone();
            paid.mana = zero_mana();
            if clear_pact {
                paid.pact_debt = 0;
            }
            paid_states.push(paid);
            continue;
        }
        for action in upkeep_mana_actions(&current) {
            if action.is_ragavan_attack {
                continue;
            }
            let next_state = normalize_upkeep_payment_state(&action.next_state, cost);
            if seen.contains(&next_state) || mana_dominated(request, &next_state, &mut best_mana) {
                continue;
            }
            if seen.len() >= request.state_limit.min(limit) {
                continue;
            }
            seen.insert(next_state.clone());
            queue.push(next_state);
        }
    }
    paid_states
}

fn pay_upkeep_pacts(request: &CloseTurnRequest, state: &FixtureState) -> bool {
    if state.pact_debt == 0 {
        return true;
    }
    let cost = [2 * state.pact_debt, 0, 0, 0, 0, 2 * state.pact_debt];
    !pay_upkeep_options(request, state, cost, true, 4096).is_empty()
}

fn can_cast_angels_grace_upkeep(request: &CloseTurnRequest, state: &FixtureState) -> bool {
    if !state.hand.iter().any(|card| card == "Angel's Grace") {
        return false;
    }
    let cost = [0, 0, 0, 0, 1, 0];
    let mut queue = Vec::new();
    let mut seen = HashSet::new();
    let mut best_mana: HashMap<FixtureState, Vec<Mana>> = HashMap::new();
    queue.push(state.clone());
    seen.insert(state.clone());
    while let Some(current) = queue.pop() {
        if current.hand.iter().any(|card| card == "Angel's Grace")
            && !pay_options(current.mana, cost).is_empty()
        {
            return true;
        }
        let mut actions = Vec::new();
        let mut sink = ActionSink::Bfs(&mut actions);
        generate_mana_actions(&mut sink, &current);
        drop(sink);
        for action in actions {
            if action.is_ragavan_attack
                || !action
                    .next_state
                    .hand
                    .iter()
                    .any(|card| card == "Angel's Grace")
                || seen.contains(&action.next_state)
                || mana_dominated(request, &action.next_state, &mut best_mana)
            {
                continue;
            }
            if seen.len() >= request.state_limit.min(2048) {
                continue;
            }
            seen.insert(action.next_state.clone());
            queue.push(action.next_state);
        }
    }
    false
}

fn pact_upkeep_states(request: &CloseTurnRequest, state: &FixtureState) -> Vec<FixtureState> {
    if state.pact_debt == 0 {
        return vec![state.clone()];
    }
    let cost = [2 * state.pact_debt, 0, 0, 0, 0, 2 * state.pact_debt];
    pay_upkeep_options(request, state, cost, true, 4096)
}

fn pay_generic_upkeep_options(
    request: &CloseTurnRequest,
    state: &FixtureState,
    amount: u8,
) -> Vec<FixtureState> {
    pay_upkeep_options(request, state, [amount, 0, 0, 0, 0, 0], false, 2048)
}

fn future_visible_setup_states(state: &FixtureState) -> Vec<FixtureState> {
    let mut out = vec![state.clone()];
    if state.land_played {
        return out;
    }
    let mut cards: Vec<String> = state
        .hand
        .iter()
        .filter(|card| {
            is_land_card(card) || is_theoretical_rainbow_land(card) || is_mdfc_land(card)
        })
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    cards.sort();
    for card in cards {
        for (perm, library, grave_inc, _label) in land_options(&card, &state.library) {
            let mut next = state.clone();
            next.hand = remove_first(&state.hand, &card);
            next.library = library;
            next.battlefield
                .retain(|permanent| permanent.name != "CITY");
            next.battlefield.push(perm);
            next.land_played = true;
            next.land_grave_count = next.land_grave_count.saturating_add(grave_inc).min(4);
            norm_hand(&mut next.hand);
            norm_battlefield(&mut next.battlefield);
            out.push(next);
        }
    }
    out
}

fn can_keep_remora(request: &CloseTurnRequest, state: &FixtureState, payments: u8) -> bool {
    if payments == 0 {
        return true;
    }
    let mut states = vec![end_turn(state)];
    for amount in 1..=payments {
        let mut paid_set = HashSet::new();
        let mut paid_states = Vec::new();
        for state_at_end in &states {
            let mut begun = begin_turn(state_at_end);
            begun.turn = state_at_end.turn.saturating_add(1).min(8);
            for paid in pay_generic_upkeep_options(request, &begun, amount) {
                if paid_set.insert(paid.clone()) {
                    paid_states.push(paid);
                }
            }
        }
        if paid_states.is_empty() {
            return false;
        }
        if amount == payments {
            return true;
        }
        let mut setup_set = HashSet::new();
        let mut setup_states = Vec::new();
        for paid in &paid_states {
            for setup in future_visible_setup_states(paid) {
                let ended = end_turn(&setup);
                if setup_set.insert(ended.clone()) {
                    setup_states.push(ended);
                }
            }
        }
        states = setup_states;
    }
    false
}

fn library_order_matters_for_hand(hand: &[String]) -> bool {
    hand.iter().any(|card| {
        matches!(
            card.as_str(),
            "Gitaxian Probe" | "Manamorphose" | "Tataru Taru" | "Wheel of Fortune"
        )
    })
}

fn structural_key(request: &CloseTurnRequest, state: &FixtureState) -> FixtureState {
    let mut key = state.clone();
    key.mana = zero_mana();
    if key.turn >= request.max_turns && !library_order_matters_for_hand(&key.hand) {
        key.library.sort();
    }
    key
}

fn mana_dominated(
    request: &CloseTurnRequest,
    state: &FixtureState,
    best_mana: &mut HashMap<FixtureState, Vec<Mana>>,
) -> bool {
    let key = structural_key(request, state);
    let state_mana = state.mana;
    let Some(existing) = best_mana.get_mut(&key) else {
        best_mana.insert(key, vec![state_mana]);
        return false;
    };
    if existing.iter().any(|mana| {
        mana.iter()
            .zip(state_mana.iter())
            .all(|(have, need)| have >= need)
    }) {
        return true;
    }
    existing.retain(|mana| {
        !state_mana
            .iter()
            .zip(mana.iter())
            .all(|(have, old)| have >= old)
    });
    existing.push(state_mana);
    false
}

pub fn close_turn(request: &CloseTurnRequest) -> CloseTurnResponse {
    let mut queue: VecDeque<FixtureState> = request.states.iter().cloned().collect();
    let mut seen_set: HashSet<FixtureState> = request.states.iter().cloned().collect();
    let mut seen_order = request.states.clone();
    let mut best_mana: HashMap<FixtureState, Vec<Mana>> = HashMap::new();
    let mut hit_limit = false;
    while let Some(state) = queue.pop_back() {
        if let Some(label) = success_label(request, &state) {
            return CloseTurnResponse {
                seen_count: seen_order.len(),
                closed: seen_order,
                success: true,
                hit_limit,
                label: Some(label),
            };
        }
        let mut actions = generate_fixture_bfs_actions(&state);
        if request.action_sort {
            actions.sort_by_key(|action| action.priority);
        }
        for action in actions {
            let next_state = action.next_state;
            if seen_set.contains(&next_state)
                || mana_dominated(request, &next_state, &mut best_mana)
            {
                continue;
            }
            if let Some(label) = success_label(request, &next_state) {
                return CloseTurnResponse {
                    seen_count: seen_order.len(),
                    closed: seen_order,
                    success: true,
                    hit_limit,
                    label: Some(label),
                };
            }
            if seen_order.len() >= request.state_limit {
                hit_limit = true;
                continue;
            }
            seen_set.insert(next_state.clone());
            seen_order.push(next_state.clone());
            queue.push_back(next_state);
        }
    }
    CloseTurnResponse {
        seen_count: seen_order.len(),
        closed: seen_order,
        success: false,
        hit_limit,
        label: None,
    }
}

fn state_has_gamble(state: &FixtureState) -> bool {
    state.hand.iter().any(|card| card == "Gamble")
}

fn draw_card(mut state: FixtureState) -> FixtureState {
    if !state.library.is_empty() {
        let card = state.library.remove(0);
        state.hand.push(card);
        norm_hand(&mut state.hand);
    }
    state
}

fn starting_state_options(
    hand: &[String],
    library: &[String],
    gemstone_live: bool,
) -> Vec<FixtureState> {
    let mut hand_tuple = hand.to_vec();
    norm_hand(&mut hand_tuple);
    let library_tuple = library.to_vec();
    let mut out = vec![FixtureState {
        hand: hand_tuple.clone(),
        library: library_tuple.clone(),
        battlefield: Vec::new(),
        mana: zero_mana(),
        land_played: false,
        land_grave_count: 0,
        mantle_attached: Vec::new(),
        nature_attached: Vec::new(),
        nature_untap_used: false,
        nature_tap_used: false,
        rain_active: false,
        spells_this_turn: 0,
        pact_debt: 0,
        turn: 0,
        engine_count: 0,
        engine_targets: Vec::new(),
        engine_names: Vec::new(),
    }];
    let gemstone_card = hand_tuple
        .iter()
        .find(|card| is_gemstone_caverns_alias(card))
        .map(String::as_str);
    let Some(gemstone_card) = gemstone_card else {
        return out;
    };
    if !gemstone_live {
        return out;
    }
    let exiles: Vec<String> = hand_tuple
        .iter()
        .filter(|card| card.as_str() != gemstone_card)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    for exile in exiles {
        let mut exile_hand = remove_first(&remove_first(&hand_tuple, gemstone_card), &exile);
        norm_hand(&mut exile_hand);
        let live_state = FixtureState {
            hand: exile_hand,
            library: library_tuple.clone(),
            battlefield: vec![FixturePerm {
                name: "CAVERN".to_string(),
                tapped: false,
                extra: "BRUWG".to_string(),
            }],
            mana: zero_mana(),
            land_played: false,
            land_grave_count: 0,
            mantle_attached: Vec::new(),
            nature_attached: Vec::new(),
            nature_untap_used: false,
            nature_tap_used: false,
            rain_active: false,
            spells_this_turn: 0,
            pact_debt: 0,
            turn: 0,
            engine_count: 0,
            engine_targets: Vec::new(),
            engine_names: Vec::new(),
        };
        out.push(live_state.clone());
        out.extend(preturn_caverns_top_tutor_states(&live_state));
    }
    out
}

pub fn solve_keep(request: &SolveKeepRequest) -> SolveKeepResponse {
    let close_request = CloseTurnRequest {
        states: Vec::new(),
        state_limit: request.state_limit,
        max_turns: request.max_turns,
        goal: request.goal.clone(),
        engine_target_count: request.engine_target_count,
        engine_success_policy: request.engine_success_policy.clone(),
        remora_upkeep_payments: request.remora_upkeep_payments,
        action_sort: request.action_sort,
    };
    let chancellor = request
        .hand
        .iter()
        .any(|card| card == "Chancellor of the Tangle");
    let mut states: Vec<FixtureState> =
        starting_state_options(&request.hand, &request.library, request.gemstone_live);
    let mut capped = false;
    for turn in 1..=request.max_turns {
        let mut turn_state_set = HashSet::new();
        let mut turn_states = Vec::new();
        for state in &states {
            let mut begun = begin_turn(state);
            begun.turn = turn;
            let upkeep_states = pact_upkeep_states(&close_request, &begun);
            for upkeep_paid in upkeep_states {
                let mut drawn = draw_card(upkeep_paid);
                if turn == 1 && chancellor {
                    drawn.mana = add_mana(drawn.mana, [0, 0, 0, 0, 1, 0]);
                }
                if state_has_gamble(&drawn) {
                    return SolveKeepResponse {
                        turn: None,
                        capped,
                        label: None,
                        unsupported: true,
                        unsupported_reason: Some(
                            "Gamble reached in Rust solve_keep frontier".to_string(),
                        ),
                    };
                }
                if turn_state_set.insert(drawn.clone()) {
                    turn_states.push(drawn);
                }
            }
        }
        let mut close_turn_request = close_request.clone();
        close_turn_request.states = turn_states;
        let closed = close_turn(&close_turn_request);
        capped |= closed.hit_limit;
        if closed.success {
            return SolveKeepResponse {
                turn: Some(turn),
                capped,
                label: closed.label,
                unsupported: false,
                unsupported_reason: None,
            };
        }
        let mut next_state_set = HashSet::new();
        states.clear();
        for state in closed.closed {
            let ended = end_turn(&state);
            if next_state_set.insert(ended.clone()) {
                states.push(ended);
            }
        }
    }
    SolveKeepResponse {
        turn: None,
        capped,
        label: None,
        unsupported: false,
        unsupported_reason: None,
    }
}

fn action_key(label: &str, signature: &str) -> String {
    format!("{label}\u{1f}{signature}")
}

fn count_action_keys<'a>(
    actions: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (label, signature) in actions {
        *out.entry(action_key(label, signature)).or_insert(0) += 1;
    }
    out
}

pub fn verify_action_fixtures(
    input: &str,
    max_mismatches: usize,
) -> Result<ActionParityReport, String> {
    let payload: ActionFixturePayload =
        serde_json::from_str(input).map_err(|err| err.to_string())?;
    let mut report = ActionParityReport {
        fixture_count: payload.fixture_count,
        compared_fixtures: payload.fixtures.len(),
        exact_fixtures: 0,
        expected_actions: 0,
        generated_actions: 0,
        missing_actions: 0,
        extra_actions: 0,
        actions_truncated_in_fixture: 0,
        mismatches: Vec::new(),
    };
    for fixture in payload.fixtures {
        if fixture.actions_truncated {
            report.actions_truncated_in_fixture += 1;
        }
        let generated = generate_fixture_actions(&fixture.state);
        report.expected_actions += fixture.actions.len();
        report.generated_actions += generated.len();
        let expected_counts = count_action_keys(
            fixture
                .actions
                .iter()
                .map(|action| (action.label.as_str(), action.next_state_signature.as_str())),
        );
        let generated_counts = count_action_keys(
            generated
                .iter()
                .map(|action| (action.label.as_str(), action.next_state_signature.as_str())),
        );
        let mut missing = Vec::new();
        let mut extra = Vec::new();
        for (key, expected_count) in &expected_counts {
            let generated_count = generated_counts.get(key).copied().unwrap_or(0);
            if generated_count < *expected_count {
                for _ in 0..(*expected_count - generated_count) {
                    missing.push(key.clone());
                }
            }
        }
        for (key, generated_count) in &generated_counts {
            let expected_count = expected_counts.get(key).copied().unwrap_or(0);
            if expected_count < *generated_count {
                for _ in 0..(*generated_count - expected_count) {
                    extra.push(key.clone());
                }
            }
        }
        report.missing_actions += missing.len();
        report.extra_actions += extra.len();
        if missing.is_empty() && extra.is_empty() {
            report.exact_fixtures += 1;
        } else if report.mismatches.len() < max_mismatches {
            report.mismatches.push(FixtureMismatch {
                fixture_index: fixture.fixture_index,
                source: fixture.source,
                state_signature: fixture.state_signature,
                expected_count: fixture.actions.len(),
                generated_count: generated.len(),
                missing,
                extra,
            });
        }
    }
    Ok(report)
}

pub fn fnv1a_seed(parts: &[&str]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            hash ^= b'|' as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for byte in part.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

pub fn choose_count(n: usize, k: usize) -> u64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut out = 1u128;
    for i in 0..k {
        out = out * (n - i) as u128 / (i + 1) as u128;
    }
    out as u64
}

pub fn bottom_choice_count(hand_size: usize, bottom_count: usize) -> u64 {
    choose_count(hand_size, bottom_count)
}

pub fn bottom_choices(hand_size: usize, bottom_count: usize) -> Vec<BottomChoice> {
    if bottom_count > hand_size {
        return Vec::new();
    }
    if bottom_count == 0 {
        return vec![BottomChoice {
            removed_indices: Vec::new(),
            kept_indices: (0..hand_size).collect(),
        }];
    }

    let mut removed = Vec::with_capacity(bottom_count);
    let mut out = Vec::with_capacity(bottom_choice_count(hand_size, bottom_count) as usize);
    generate_bottom_choices(hand_size, bottom_count, 0, &mut removed, &mut out);
    out
}

fn generate_bottom_choices(
    hand_size: usize,
    bottom_count: usize,
    start: usize,
    removed: &mut Vec<usize>,
    out: &mut Vec<BottomChoice>,
) {
    if removed.len() == bottom_count {
        let mut is_removed = vec![false; hand_size];
        for index in removed.iter().copied() {
            is_removed[index] = true;
        }
        let kept_indices = (0..hand_size).filter(|index| !is_removed[*index]).collect();
        out.push(BottomChoice {
            removed_indices: removed.clone(),
            kept_indices,
        });
        return;
    }
    let remaining_needed = bottom_count - removed.len();
    let max_start = hand_size - remaining_needed;
    for index in start..=max_start {
        removed.push(index);
        generate_bottom_choices(hand_size, bottom_count, index + 1, removed, out);
        removed.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pact_upkeep_test_state(hand: Vec<&str>, battlefield: Vec<FixturePerm>) -> FixtureState {
        FixtureState {
            battlefield,
            engine_count: 1,
            engine_names: vec!["Heartwood Storyteller@2".to_string()],
            engine_targets: vec!["CREATURE".to_string(), "PERM".to_string()],
            hand: hand.into_iter().map(str::to_string).collect(),
            land_grave_count: 0,
            land_played: true,
            library: Vec::new(),
            mana: [0, 0, 0, 0, 0, 0],
            mantle_attached: Vec::new(),
            nature_attached: Vec::new(),
            nature_tap_used: false,
            nature_untap_used: false,
            pact_debt: 1,
            rain_active: false,
            spells_this_turn: 1,
            turn: 2,
        }
    }

    fn land(extra: &str) -> FixturePerm {
        FixturePerm {
            name: "LAND".to_string(),
            extra: extra.to_string(),
            tapped: false,
        }
    }

    fn perm(name: &str, extra: &str) -> FixturePerm {
        FixturePerm {
            name: name.to_string(),
            extra: extra.to_string(),
            tapped: false,
        }
    }

    fn pact_upkeep_request(state: FixtureState) -> CloseTurnRequest {
        CloseTurnRequest {
            states: vec![state],
            state_limit: 50_000,
            max_turns: 2,
            goal: "engine".to_string(),
            engine_target_count: 1,
            engine_success_policy: "resilient".to_string(),
            remora_upkeep_payments: 2,
            action_sort: true,
        }
    }

    fn assert_pact_upkeep_success_matches(state: FixtureState, expected: bool) {
        let request = pact_upkeep_request(state);
        assert_eq!(close_turn(&request).success, expected);
        assert_eq!(fast_engine::close_turn_fast(&request).success, expected);
    }

    fn solve_request(hand: Vec<&str>, library: Vec<&str>) -> SolveKeepRequest {
        SolveKeepRequest {
            hand: hand.into_iter().map(str::to_string).collect(),
            library: library.into_iter().map(str::to_string).collect(),
            gemstone_live: false,
            state_limit: 120_000,
            max_turns: 2,
            goal: "engine".to_string(),
            engine_target_count: 1,
            engine_success_policy: "resilient".to_string(),
            remora_upkeep_payments: 0,
            action_sort: true,
            gamble_mode: Some("stochastic".to_string()),
            gamble_seed: Some(1),
            simplified_gamble: true,
        }
    }

    #[test]
    fn choose_count_matches_small_values() {
        assert_eq!(choose_count(7, 0), 1);
        assert_eq!(choose_count(7, 1), 7);
        assert_eq!(choose_count(7, 2), 21);
        assert_eq!(choose_count(7, 3), 35);
        assert_eq!(choose_count(7, 4), 35);
        assert_eq!(choose_count(7, 8), 0);
    }

    #[test]
    fn bottom_choices_preserve_order_and_count() {
        let choices = bottom_choices(4, 2);
        assert_eq!(choices.len(), 6);
        assert_eq!(choices[0].removed_indices, vec![0, 1]);
        assert_eq!(choices[0].kept_indices, vec![2, 3]);
        assert_eq!(choices[5].removed_indices, vec![2, 3]);
        assert_eq!(choices[5].kept_indices, vec![0, 1]);
    }

    #[test]
    fn seed_is_stable() {
        assert_eq!(fnv1a_seed(&["a", "b"]), fnv1a_seed(&["a", "b"]));
        assert_ne!(fnv1a_seed(&["a", "b"]), fnv1a_seed(&["ab"]));
    }

    #[test]
    fn mana_helpers_match_python_layout() {
        assert_eq!(
            add_mana([9, 0, 0, 0, 0, 9], [3, 1, 0, 0, 0, 3]),
            [10, 1, 0, 0, 0, 10]
        );
        assert_eq!(cap_mana([11, 1, 2, 3, 4, 12]), [10, 1, 2, 3, 4, 10]);
        assert_eq!(mana_for_color('U'), Some([0, 0, 1, 0, 0, 0]));
        assert_eq!(mana_for_option("CC"), Some([0, 0, 0, 0, 0, 2]));
        assert_eq!(mana_for_option("X"), None);
    }

    #[test]
    fn pay_options_matches_known_cases() {
        assert_eq!(
            pay_options([2, 0, 1, 0, 0, 0], [2, 0, 0, 1, 0, 0]),
            vec![[0, 0, 0, 0, 0, 0]]
        );
        assert!(pay_options([0, 0, 1, 0, 0, 2], [3, 0, 0, 1, 0, 0]).is_empty());
        assert_eq!(
            pay_options([0, 0, 1, 0, 0, 2], [2, 0, 0, 1, 0, 0]),
            vec![[0, 0, 0, 0, 0, 0]]
        );
        assert_eq!(
            pay_options([1, 1, 0, 0, 0, 1], [1, 0, 0, 0, 0, 0]),
            vec![[1, 1, 0, 0, 0, 0], [1, 0, 0, 0, 0, 1], [0, 1, 0, 0, 0, 1]]
        );
    }

    #[test]
    fn bench_cases_have_stable_checksum() {
        let mut count = 0u64;
        let mut checksum = 0u64;
        for (mana, cost) in mana_bench_cases() {
            let options = pay_options(mana, cost);
            count += options.len() as u64;
            checksum = checksum.wrapping_add(mana_checksum(&options));
        }
        assert_eq!(count, 404);
        assert_eq!(checksum, 3602727453886118847);
    }

    #[test]
    fn pact_upkeep_counts_instant_speed_hand_mana() {
        let heartwood = perm("HEARTWOOD", "G");
        assert_pact_upkeep_success_matches(
            pact_upkeep_test_state(
                vec![],
                vec![land("G"), perm("CCLAND", ""), heartwood.clone()],
            ),
            false,
        );
        assert_pact_upkeep_success_matches(
            pact_upkeep_test_state(
                vec!["Elvish Spirit Guide"],
                vec![land("G"), perm("CCLAND", ""), heartwood.clone()],
            ),
            true,
        );
        assert_pact_upkeep_success_matches(
            pact_upkeep_test_state(
                vec!["Simian Spirit Guide"],
                vec![land("G"), land("G"), land("C"), heartwood.clone()],
            ),
            true,
        );
        assert_pact_upkeep_success_matches(
            pact_upkeep_test_state(
                vec!["Dark Ritual"],
                vec![land("B"), land("G"), land("G"), heartwood.clone()],
            ),
            true,
        );
        assert_pact_upkeep_success_matches(
            pact_upkeep_test_state(
                vec!["Rain of Filth"],
                vec![land("B"), land("G"), land("G"), heartwood.clone()],
            ),
            true,
        );
        assert_pact_upkeep_success_matches(
            pact_upkeep_test_state(
                vec!["Culling the Weak"],
                vec![
                    land("B"),
                    land("G"),
                    land("G"),
                    perm("CREATURE", "W"),
                    heartwood,
                ],
            ),
            true,
        );
    }

    #[test]
    fn noxious_revival_recurs_named_graveyard_cards() {
        let library = vec!["Blank", "Blank", "Rhystic Study", "Blank"];
        let positive = solve_request(
            vec![
                "Ancient Tomb",
                "Lotus Petal",
                "Gamble",
                "Noxious Revival",
                "Blank",
            ],
            library.clone(),
        );
        let negative = solve_request(
            vec!["Ancient Tomb", "Lotus Petal", "Gamble", "Blank", "Blank"],
            library,
        );

        let positive_response = fast_engine::solve_keep_fast(&positive);
        let negative_response = fast_engine::solve_keep_fast(&negative);

        assert_eq!(positive_response.turn, Some(2));
        assert_eq!(positive_response.label.as_deref(), Some("Rhystic Study"));
        assert_eq!(negative_response.turn, None);
    }

    #[test]
    fn imperial_seal_can_stack_demonic_for_led_rhystic_line() {
        let request = solve_request(
            vec![
                "City of Traitors",
                "Flash Photography",
                "Imperial Seal",
                "Lion's Eye Diamond",
                "Scrubland",
                "Summoner's Pact",
                "Wipe Away",
            ],
            vec!["Blank", "Demonic Tutor", "Rhystic Study", "Blank"],
        );

        let response = fast_engine::solve_keep_fast(&request);

        assert_eq!(response.turn, Some(2));
        assert_eq!(response.label.as_deref(), Some("Rhystic Study"));
    }

    #[test]
    fn glittering_caves_uses_gemstone_caverns_pregame_alias() {
        let mut live = solve_request(
            vec![
                "Glittering Caves of Aglarond",
                "Ancient Tomb",
                "Rhystic Study",
                "Blank",
            ],
            vec!["Blank"],
        );
        live.gemstone_live = true;
        live.max_turns = 1;

        let mut dead = live.clone();
        dead.gemstone_live = false;

        let live_fast = fast_engine::solve_keep_fast(&live);
        let live_slow = solve_keep(&live);
        let dead_fast = fast_engine::solve_keep_fast(&dead);
        let dead_slow = solve_keep(&dead);

        assert_eq!(live_fast.turn, Some(1));
        assert_eq!(live_fast.label.as_deref(), Some("Rhystic Study"));
        assert_eq!(live_slow.turn, Some(1));
        assert_eq!(live_slow.label.as_deref(), Some("Rhystic Study"));
        assert_eq!(dead_fast.turn, None);
        assert_eq!(dead_slow.turn, None);
    }

    #[test]
    fn live_caverns_allows_preturn_instant_top_tutor_before_first_draw() {
        let mut live = solve_request(
            vec![
                "Glittering Caves of Aglarond",
                "Ancient Tomb",
                "Vampiric Tutor",
                "Blank",
            ],
            vec!["Blank", "Rhystic Study"],
        );
        live.gemstone_live = true;
        live.max_turns = 1;

        let mut dead = live.clone();
        dead.gemstone_live = false;

        let live_fast = fast_engine::solve_keep_fast(&live);
        let live_slow = solve_keep(&live);
        let dead_fast = fast_engine::solve_keep_fast(&dead);
        let dead_slow = solve_keep(&dead);

        assert_eq!(live_fast.turn, Some(1));
        assert_eq!(live_fast.label.as_deref(), Some("Rhystic Study"));
        assert_eq!(live_slow.turn, Some(1));
        assert_eq!(live_slow.label.as_deref(), Some("Rhystic Study"));
        assert_eq!(dead_fast.turn, None);
        assert_eq!(dead_slow.turn, None);
    }

    #[test]
    fn glimmervoid_taps_without_artifact_but_sacrifices_at_end_step() {
        let mut state = pact_upkeep_test_state(vec![], vec![perm("GLIMMER", "BRUWG")]);
        state.engine_count = 0;
        state.engine_names = Vec::new();
        state.engine_targets = Vec::new();
        state.pact_debt = 0;

        let options = tap_options(&state.battlefield[0], &state);
        assert!(options.contains(&"U".to_string()));
        assert!(options.contains(&"B".to_string()));
        assert!(end_turn(&state)
            .battlefield
            .iter()
            .all(|item| item.name != "GLIMMER"));

        state.battlefield.push(perm("PETAL", ""));
        assert!(end_turn(&state)
            .battlefield
            .iter()
            .any(|item| item.name == "GLIMMER"));
    }

    #[test]
    fn hidden_shuffle_search_does_not_stack_future_draws() {
        std::env::set_var("RHYSTIC_STRICT_SHUFFLE_HIDDEN", "1");
        let request = SolveKeepRequest {
            hand: vec![
                "Crop Rotation".to_string(),
                "Gamble".to_string(),
                "Mystical Tutor".to_string(),
                "Ranger-Captain of Eos".to_string(),
                "Windswept Heath".to_string(),
            ],
            library: vec![
                "Exotic Orchard".to_string(),
                "City of Traitors".to_string(),
                "Rhystic Study".to_string(),
                "Verdant Catacombs".to_string(),
            ],
            gemstone_live: true,
            state_limit: 60_000,
            max_turns: 2,
            goal: "engine".to_string(),
            engine_target_count: 1,
            engine_success_policy: "resilient".to_string(),
            remora_upkeep_payments: 2,
            action_sort: true,
            gamble_mode: Some("off".to_string()),
            gamble_seed: Some(18_406_728_991_951_391_320),
            simplified_gamble: false,
        };
        let response = fast_engine::solve_keep_fast(&request);
        std::env::remove_var("RHYSTIC_STRICT_SHUFFLE_HIDDEN");
        assert_eq!(response.turn, None);
    }
}
