use rustc_hash::FxHasher;
use serde::Serialize;
use std::hash::{Hash, Hasher};
use std::hint::black_box;
use std::mem::size_of;
use std::time::Instant;

use super::{
    compute_mana_closure, compute_payment_plans, CardMask, ManaPool, ManaSource, PackedLibrary,
    PackedState, Zone,
};

#[derive(Debug, Clone, Serialize)]
pub struct NextgenBenchReport {
    pub iterations: u64,
    pub packed_state_bytes: usize,
    pub packed_library_bytes: usize,
    pub state_operations_per_second: f64,
    pub mana_closures_per_second: f64,
    pub mana_frontier_size: usize,
    pub payment_plans_per_second: f64,
    pub payment_plan_frontier_size: usize,
    pub checksum: u64,
}

pub fn bench_nextgen(iterations: u64) -> NextgenBenchReport {
    let unknown: CardMask = (0..99).map(|slot| slot as u8).collect();
    let library = PackedLibrary::new(unknown);
    let base = PackedState {
        library,
        ..PackedState::default()
    };

    let state_started = Instant::now();
    let mut checksum = 0u64;
    for iteration in 0..iterations {
        let mut state = black_box(base);
        let slot = (iteration % 99) as u8;
        state.library.remove_known_or_unknown(slot);
        state.hand.insert(slot);
        state.move_card(slot, Zone::Hand, Zone::Battlefield);
        state.tapped.insert(slot);
        let mut hasher = FxHasher::default();
        state.hash(&mut hasher);
        checksum = checksum.wrapping_add(hasher.finish());
    }
    let state_elapsed = state_started.elapsed().as_secs_f64();

    let rainbow = [
        ManaPool([1, 0, 0, 0, 0, 0]),
        ManaPool([0, 1, 0, 0, 0, 0]),
        ManaPool([0, 0, 1, 0, 0, 0]),
        ManaPool([0, 0, 0, 1, 0, 0]),
        ManaPool([0, 0, 0, 0, 1, 0]),
    ];
    let sources = [
        ManaSource::new(1, rainbow),
        ManaSource::new(2, [ManaPool([0, 0, 0, 0, 0, 2])]),
        ManaSource::new(3, rainbow),
        ManaSource::new(4, [ManaPool([3, 0, 0, 0, 0, 0])]),
    ];
    let closure_started = Instant::now();
    let mut last = Vec::new();
    for _ in 0..iterations {
        last = compute_mana_closure(black_box(ManaPool::default()), black_box(&sources), 10);
        checksum = checksum.wrapping_add(
            last.iter()
                .map(|outcome| outcome.pool.checksum())
                .sum::<u64>(),
        );
    }
    let closure_elapsed = closure_started.elapsed().as_secs_f64();

    let payment_started = Instant::now();
    let mut payment_plans = Vec::new();
    for _ in 0..iterations {
        payment_plans = compute_payment_plans(
            black_box(ManaPool::default()),
            black_box(&sources),
            black_box([2, 0, 0, 1, 0, 0]),
        );
        checksum = checksum.wrapping_add(
            payment_plans
                .iter()
                .map(|plan| {
                    plan.leftover
                        .checksum()
                        .wrapping_add(plan.consumption.used().bits() as u64)
                })
                .sum::<u64>(),
        );
    }
    let payment_elapsed = payment_started.elapsed().as_secs_f64();

    NextgenBenchReport {
        iterations,
        packed_state_bytes: size_of::<PackedState>(),
        packed_library_bytes: size_of::<PackedLibrary>(),
        state_operations_per_second: iterations as f64 / state_elapsed,
        mana_closures_per_second: iterations as f64 / closure_elapsed,
        mana_frontier_size: last.len(),
        payment_plans_per_second: iterations as f64 / payment_elapsed,
        payment_plan_frontier_size: payment_plans.len(),
        checksum,
    }
}
