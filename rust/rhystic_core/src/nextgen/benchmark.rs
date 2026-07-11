use rustc_hash::FxHasher;
use serde::Serialize;
use std::hash::{Hash, Hasher};
use std::hint::black_box;
use std::mem::size_of;
use std::time::Instant;

use super::{
    compute_mana_closure, compute_payment_plans, CardMask, DeckSpec, EngineOpeningModel, ManaPool,
    ManaSource, PackedLibrary, PackedState, PackedStateV2, PermanentInstance, PermanentSource,
    ReferenceSolver, SearchMetrics, TokenKind, Zone,
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

#[derive(Debug, Clone, Serialize)]
pub struct PackedStateV2BenchReport {
    pub iterations: u64,
    pub packed_state_bytes: usize,
    pub packed_state_v2_bytes: usize,
    pub legacy_seconds: f64,
    pub v2_seconds: f64,
    pub permanent_stress_seconds: f64,
    pub legacy_operations_per_second: f64,
    pub v2_operations_per_second: f64,
    pub permanent_stress_operations_per_second: f64,
    pub v2_throughput_ratio: f64,
    pub v2_size_ratio: f64,
    pub checksum: u64,
}

pub fn bench_packed_state_v2(iterations: u64) -> PackedStateV2BenchReport {
    let unknown: CardMask = (0..99).map(|slot| slot as u8).collect();
    let library = PackedLibrary::new(unknown);
    let legacy_base = PackedState {
        library,
        ..PackedState::default()
    };
    let v2_base = PackedStateV2 {
        library,
        ..PackedStateV2::default()
    };

    let mut checksum = 0u64;
    let legacy_started = Instant::now();
    for iteration in 0..iterations {
        let mut state = black_box(legacy_base);
        let slot = (iteration % 99) as u8;
        state.library.remove_known_or_unknown(slot);
        state.hand.insert(slot);
        state.move_card(slot, Zone::Hand, Zone::Battlefield);
        state.tapped.insert(slot);
        let mut hasher = FxHasher::default();
        state.hash(&mut hasher);
        checksum = checksum.wrapping_add(hasher.finish());
    }
    let legacy_seconds = legacy_started.elapsed().as_secs_f64();
    black_box(checksum);

    let v2_started = Instant::now();
    for iteration in 0..iterations {
        let mut state = black_box(v2_base);
        let slot = (iteration % 99) as u8;
        state.draw(slot);
        state.move_card_to_battlefield(
            slot,
            Zone::Hand,
            PermanentInstance::new(PermanentSource::card(slot)).with_tapped(true),
        );
        let mut hasher = FxHasher::default();
        state.hash(&mut hasher);
        checksum = checksum.wrapping_add(hasher.finish());
    }
    let v2_seconds = v2_started.elapsed().as_secs_f64();
    black_box(checksum);

    let permanent_started = Instant::now();
    for iteration in 0..iterations {
        let mut state = black_box(PackedStateV2::default());
        state.put_commander_on_battlefield(false, true);
        state.add_token(TokenKind::Treasure);
        state.add_token(TokenKind::Treasure);
        let slot = (iteration % 99) as u8;
        state.hand.insert(slot);
        state.move_card_to_battlefield(
            slot,
            Zone::Hand,
            PermanentInstance::new(PermanentSource::card(slot))
                .with_attachment(PermanentSource::commander())
                .with_counters((iteration & 3) as u8),
        );
        state.remove_token(TokenKind::Treasure);
        let mut hasher = FxHasher::default();
        state.hash(&mut hasher);
        checksum = checksum.wrapping_add(hasher.finish());
    }
    let permanent_stress_seconds = permanent_started.elapsed().as_secs_f64();
    black_box(checksum);

    let legacy_operations_per_second = iterations as f64 / legacy_seconds;
    let v2_operations_per_second = iterations as f64 / v2_seconds;
    PackedStateV2BenchReport {
        iterations,
        packed_state_bytes: size_of::<PackedState>(),
        packed_state_v2_bytes: size_of::<PackedStateV2>(),
        legacy_seconds,
        v2_seconds,
        permanent_stress_seconds,
        legacy_operations_per_second,
        v2_operations_per_second,
        permanent_stress_operations_per_second: iterations as f64 / permanent_stress_seconds,
        v2_throughput_ratio: v2_operations_per_second / legacy_operations_per_second,
        v2_size_ratio: size_of::<PackedStateV2>() as f64 / size_of::<PackedState>() as f64,
        checksum,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpeningModelBenchReport {
    pub iterations: u64,
    pub elapsed_seconds: f64,
    pub solves_per_second: f64,
    pub microstep_elapsed_seconds: f64,
    pub microstep_solves_per_second: f64,
    pub direct_speedup: f64,
    pub states_expanded_per_solve: u64,
    pub transposition_hits_per_solve: u64,
    pub chance_nodes_per_solve: u64,
    pub expected_value: f64,
    pub checksum: f64,
}

pub fn bench_opening_model(iterations: u64) -> OpeningModelBenchReport {
    let deck = DeckSpec::compile(&[
        "Ancient Tomb".to_string(),
        "Command Tower".to_string(),
        "Rhystic Study".to_string(),
        "Blank".to_string(),
        "Blank 2".to_string(),
    ])
    .expect("opening benchmark deck");
    let direct_model = EngineOpeningModel::compile(&deck, 2).with_resource_microsteps(false);
    let microstep_model = EngineOpeningModel::compile(&deck, 2)
        .with_direct_payments(false)
        .with_resource_microsteps(true);
    let mut state = PackedStateV2 {
        library: PackedLibrary::new([1, 3, 4].into_iter().collect()),
        ..PackedStateV2::default()
    };
    state.hand = [0, 2].into_iter().collect();

    let started = Instant::now();
    let mut checksum = 0.0;
    let mut metrics = SearchMetrics::default();
    let mut expected_value = 0.0;
    for _ in 0..iterations {
        let result = ReferenceSolver::new(black_box(&direct_model)).solve(black_box(state), 12);
        expected_value = result.value;
        metrics = result.metrics;
        checksum += result.value;
    }
    let elapsed_seconds = started.elapsed().as_secs_f64();
    black_box(checksum);
    let microstep_started = Instant::now();
    let mut microstep_checksum = 0.0;
    for _ in 0..iterations {
        let result = ReferenceSolver::new(black_box(&microstep_model)).solve(black_box(state), 12);
        microstep_checksum += result.value;
    }
    let microstep_elapsed_seconds = microstep_started.elapsed().as_secs_f64();
    black_box(microstep_checksum);
    assert!((checksum - microstep_checksum).abs() < f64::EPSILON);
    let solves_per_second = iterations as f64 / elapsed_seconds;
    let microstep_solves_per_second = iterations as f64 / microstep_elapsed_seconds;
    OpeningModelBenchReport {
        iterations,
        elapsed_seconds,
        solves_per_second,
        microstep_elapsed_seconds,
        microstep_solves_per_second,
        direct_speedup: solves_per_second / microstep_solves_per_second,
        states_expanded_per_solve: metrics.states_expanded,
        transposition_hits_per_solve: metrics.transposition_hits,
        chance_nodes_per_solve: metrics.chance_nodes,
        expected_value,
        checksum,
    }
}
