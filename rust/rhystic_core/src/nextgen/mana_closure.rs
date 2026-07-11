use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::{CardMask, SlotId};
use crate::{pay_options, Cost};

pub const MANA_COMPONENTS: usize = 6;

#[derive(
    Debug, Copy, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct ManaPool(pub [u8; MANA_COMPONENTS]);

impl ManaPool {
    pub fn add_capped(self, other: Self, cap: u8) -> Self {
        add_with_caps(self, other, [cap; MANA_COMPONENTS])
    }

    pub fn dominates(self, other: Self) -> bool {
        self.0.iter().zip(other.0).all(|(have, need)| *have >= need)
    }

    pub fn checksum(self) -> u64 {
        self.0
            .iter()
            .enumerate()
            .fold(0u64, |value, (index, amount)| {
                value.wrapping_add((*amount as u64) << (index * 8))
            })
    }

    pub fn pack(self) -> u32 {
        self.0
            .iter()
            .enumerate()
            .fold(0u32, |packed, (index, amount)| {
                packed | (u32::from((*amount).min(15)) << (index * 4))
            })
    }

    pub fn unpack(packed: u32) -> Self {
        let mut mana = [0; MANA_COMPONENTS];
        for (index, amount) in mana.iter_mut().enumerate() {
            *amount = ((packed >> (index * 4)) & 0xF) as u8;
        }
        Self(mana)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum ResourceUse {
    Tap,
    Sacrifice,
    Exile,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ManaOption {
    pub mana: ManaPool,
    pub resource_use: ResourceUse,
}

impl ManaOption {
    pub const fn new(mana: ManaPool, resource_use: ResourceUse) -> Self {
        Self { mana, resource_use }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaSource {
    pub slot: SlotId,
    pub options: SmallVec<[ManaOption; 5]>,
}

impl ManaSource {
    pub fn new(slot: SlotId, options: impl IntoIterator<Item = ManaPool>) -> Self {
        Self::with_options(
            slot,
            options
                .into_iter()
                .map(|mana| ManaOption::new(mana, ResourceUse::Tap)),
        )
    }

    pub fn with_options(slot: SlotId, options: impl IntoIterator<Item = ManaOption>) -> Self {
        Self {
            slot,
            options: options.into_iter().collect(),
        }
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceConsumption {
    pub tapped: CardMask,
    pub sacrificed: CardMask,
    pub exiled: CardMask,
}

impl ResourceConsumption {
    pub fn used(self) -> CardMask {
        self.tapped.union(self.sacrificed).union(self.exiled)
    }

    pub fn is_empty(self) -> bool {
        self.used().is_empty()
    }

    fn with_use(mut self, slot: SlotId, resource_use: ResourceUse) -> Self {
        match resource_use {
            ResourceUse::Tap => self.tapped.insert(slot),
            ResourceUse::Sacrifice => self.sacrificed.insert(slot),
            ResourceUse::Exile => self.exiled.insert(slot),
        };
        self
    }

    fn is_subset(self, other: Self) -> bool {
        self.tapped.is_subset(other.tapped)
            && self.sacrificed.is_subset(other.sacrificed)
            && self.exiled.is_subset(other.exiled)
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ManaOutcome {
    pub pool: ManaPool,
    pub consumption: ResourceConsumption,
}

impl ManaOutcome {
    fn dominates(self, other: Self) -> bool {
        self.pool.dominates(other.pool) && self.consumption.is_subset(other.consumption)
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaymentPlan {
    pub leftover: ManaPool,
    pub consumption: ResourceConsumption,
}

impl PaymentPlan {
    fn dominates(self, other: Self) -> bool {
        self.leftover.dominates(other.leftover) && self.consumption.is_subset(other.consumption)
    }
}

pub fn compute_mana_closure(
    initial: ManaPool,
    sources: &[ManaSource],
    cap: u8,
) -> Vec<ManaOutcome> {
    compute_mana_closure_with_caps(initial, sources, [cap; MANA_COMPONENTS])
}

pub fn compute_payment_plans(
    initial: ManaPool,
    sources: &[ManaSource],
    cost: Cost,
) -> Vec<PaymentPlan> {
    let generic = cost[0];
    let caps = [
        cost[1].saturating_add(generic),
        cost[2].saturating_add(generic),
        cost[3].saturating_add(generic),
        cost[4].saturating_add(generic),
        cost[5].saturating_add(generic),
        generic,
    ];
    let closure = compute_mana_closure_with_caps(initial, sources, caps);
    let mut plans = Vec::new();
    for outcome in closure {
        for leftover in pay_options(outcome.pool.0, cost) {
            plans.push(PaymentPlan {
                leftover: ManaPool(leftover),
                consumption: outcome.consumption,
            });
        }
    }
    pareto_prune_payments(plans)
}

fn compute_mana_closure_with_caps(
    initial: ManaPool,
    sources: &[ManaSource],
    caps: [u8; MANA_COMPONENTS],
) -> Vec<ManaOutcome> {
    let mut ordered_sources = sources.to_vec();
    ordered_sources.sort_by_key(|source| source.slot);
    let mut frontier = vec![ManaOutcome {
        pool: initial,
        consumption: ResourceConsumption::default(),
    }];

    for source in ordered_sources {
        let current = frontier;
        let mut candidates = Vec::with_capacity(current.len() * (source.options.len() + 1));
        candidates.extend(current.iter().copied());
        for outcome in &current {
            for option in &source.options {
                candidates.push(ManaOutcome {
                    pool: add_with_caps(outcome.pool, option.mana, caps),
                    consumption: outcome
                        .consumption
                        .with_use(source.slot, option.resource_use),
                });
            }
        }
        frontier = pareto_prune(candidates);
    }
    frontier.sort_by_key(|outcome| {
        (
            outcome.consumption.tapped.bits(),
            outcome.consumption.sacrificed.bits(),
            outcome.consumption.exiled.bits(),
            outcome.pool,
        )
    });
    frontier
}

fn add_with_caps(left: ManaPool, right: ManaPool, caps: [u8; MANA_COMPONENTS]) -> ManaPool {
    let mut out = [0; MANA_COMPONENTS];
    for (index, item) in out.iter_mut().enumerate() {
        *item = left.0[index]
            .saturating_add(right.0[index])
            .min(caps[index]);
    }
    ManaPool(out)
}

fn pareto_prune(candidates: Vec<ManaOutcome>) -> Vec<ManaOutcome> {
    let mut unique = FxHashSet::default();
    let mut deduplicated = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if unique.insert(candidate) {
            deduplicated.push(candidate);
        }
    }

    let mut keep = vec![true; deduplicated.len()];
    for left in 0..deduplicated.len() {
        if !keep[left] {
            continue;
        }
        for right in 0..deduplicated.len() {
            if left == right || !keep[right] {
                continue;
            }
            if deduplicated[left].dominates(deduplicated[right]) {
                keep[right] = false;
            }
        }
    }
    deduplicated
        .into_iter()
        .zip(keep)
        .filter_map(|(outcome, retain)| retain.then_some(outcome))
        .collect()
}

fn pareto_prune_payments(candidates: Vec<PaymentPlan>) -> Vec<PaymentPlan> {
    let mut unique = FxHashSet::default();
    let mut deduplicated = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if unique.insert(candidate) {
            deduplicated.push(candidate);
        }
    }

    let mut keep = vec![true; deduplicated.len()];
    for left in 0..deduplicated.len() {
        if !keep[left] {
            continue;
        }
        for right in 0..deduplicated.len() {
            if left == right || !keep[right] {
                continue;
            }
            if deduplicated[left].dominates(deduplicated[right]) {
                keep[right] = false;
            }
        }
    }
    let mut out: Vec<_> = deduplicated
        .into_iter()
        .zip(keep)
        .filter_map(|(plan, retain)| retain.then_some(plan))
        .collect();
    out.sort_by_key(|plan| {
        (
            plan.consumption.tapped.bits(),
            plan.consumption.sacrificed.bits(),
            plan.consumption.exiled.bits(),
            plan.leftover,
        )
    });
    out
}
