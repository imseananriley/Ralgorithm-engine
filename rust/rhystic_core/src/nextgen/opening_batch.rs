use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::{
    evaluate_compiled_policy, independent_sample_selected, merge_batch_reports, slot_permutation,
    BatchConfig, BatchEvaluation, BatchEvaluator, BatchReport, CardMask, DeckSpec,
    EngineOpeningModel, Estimate, MultiFidelityAccumulator, OpeningMulliganPolicy, PackedLibrary,
    PackedStateV2, ReferenceSolver, VisibleOpeningPolicy,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningVariantSpec {
    pub name: String,
    pub deck: Vec<String>,
    #[serde(default)]
    pub influence_slots: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningBatchRequest {
    pub variants: Vec<OpeningVariantSpec>,
    #[serde(default = "default_seed")]
    pub seed: u64,
    #[serde(default)]
    pub sample_start: u64,
    #[serde(default = "default_samples")]
    pub samples: u64,
    #[serde(default = "default_max_turn")]
    pub max_turn: u8,
    #[serde(default = "default_depth")]
    pub depth: u8,
    #[serde(default)]
    pub strict_reference: bool,
    #[serde(default = "default_discordance_limit")]
    pub discordance_limit: usize,
    #[serde(default = "default_progress_interval")]
    pub progress_interval: u64,
    #[serde(default)]
    pub correction_numerator: u64,
    #[serde(default = "default_correction_denominator")]
    pub correction_denominator: u64,
    #[serde(default = "default_workers")]
    pub workers: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningBatchResponse {
    pub evaluator: String,
    pub mulligan_policy: String,
    pub report: BatchReport,
    pub multifidelity: Vec<Option<Estimate>>,
}

struct CompiledVariant {
    model: EngineOpeningModel,
    deck_mask: CardMask,
    deck_len: usize,
    influence: CardMask,
}

pub fn evaluate_opening_batch(
    request: &OpeningBatchRequest,
) -> Result<OpeningBatchResponse, String> {
    if request.variants.is_empty() {
        return Err("opening batch requires at least one variant".to_string());
    }
    let expected_len = request.variants[0].deck.len();
    if expected_len < 7 {
        return Err("opening batch decks require at least seven cards".to_string());
    }
    let mut variants = Vec::with_capacity(request.variants.len());
    for variant in &request.variants {
        if variant.deck.len() != expected_len {
            return Err("all aligned variants must have the same deck length".to_string());
        }
        let deck = DeckSpec::compile(&variant.deck)?;
        let influence = variant.influence_slots.iter().copied().collect();
        variants.push(CompiledVariant {
            model: EngineOpeningModel::compile(&deck, request.max_turn),
            deck_mask: deck.card_mask(),
            deck_len: deck.cards().len(),
            influence,
        });
    }
    let names: Vec<_> = request
        .variants
        .iter()
        .map(|variant| variant.name.clone())
        .collect();
    let workers = request.workers.max(1).min(request.samples.max(1) as usize);
    let started = Instant::now();
    let outputs = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        let base = request.samples / workers as u64;
        let remainder = request.samples % workers as u64;
        let mut start = request.sample_start;
        let variants = &variants;
        let names = &names;
        for worker in 0..workers {
            let count = base + u64::from((worker as u64) < remainder);
            let shard_start = start;
            start += count;
            handles
                .push(scope.spawn(move || run_shard(request, variants, names, shard_start, count)));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("opening batch worker panicked"))
            .collect::<Vec<_>>()
    });
    let mut reports = Vec::with_capacity(outputs.len());
    let mut multifidelity = vec![MultiFidelityAccumulator::default(); variants.len()];
    for (report, shard_multifidelity) in outputs {
        reports.push(report);
        for (total, shard) in multifidelity.iter_mut().zip(shard_multifidelity) {
            total.merge(shard);
        }
    }
    let report = if reports.len() == 1 {
        reports.pop().expect("single report")
    } else {
        merge_batch_reports(
            &names,
            reports,
            started.elapsed().as_secs_f64(),
            request.discordance_limit,
        )
    };
    Ok(OpeningBatchResponse {
        evaluator: if request.strict_reference {
            "visible-information reference expectimax".to_string()
        } else {
            "compiled visible-information policy".to_string()
        },
        mulligan_policy: "commander London 7,7,6,5,4,3; frozen visible-hand v1".to_string(),
        report,
        multifidelity: multifidelity
            .into_iter()
            .map(MultiFidelityAccumulator::estimate)
            .collect(),
    })
}

fn run_shard(
    request: &OpeningBatchRequest,
    variants: &[CompiledVariant],
    names: &[String],
    sample_start: u64,
    samples: u64,
) -> (BatchReport, Vec<MultiFidelityAccumulator>) {
    let batch = BatchEvaluator::new(BatchConfig {
        root_seed: request.seed,
        sample_start,
        samples,
        success_threshold: f64::EPSILON,
        discordance_limit: request.discordance_limit,
        progress_interval: request.progress_interval,
    });
    let mulligan = OpeningMulliganPolicy::default();
    let mut multifidelity = vec![MultiFidelityAccumulator::default(); variants.len()];
    let report = batch.run(names, |sample_index, sample_seed, variant_index| {
        let low = evaluate_game(
            &variants[variant_index],
            sample_index,
            sample_seed,
            request.depth,
            request.strict_reference,
            mulligan,
        );
        multifidelity[variant_index].push_low(low.value);
        if !request.strict_reference
            && independent_sample_selected(
                request.seed,
                sample_index,
                request.correction_numerator,
                request.correction_denominator,
            )
        {
            let high = evaluate_game(
                &variants[variant_index],
                sample_index,
                sample_seed,
                request.depth,
                true,
                mulligan,
            );
            multifidelity[variant_index].push_correction(low.value, high.value);
        }
        low
    });
    (report, multifidelity)
}

fn evaluate_game(
    variant: &CompiledVariant,
    sample_index: u64,
    sample_seed: u64,
    depth: u8,
    strict_reference: bool,
    mulligan: OpeningMulliganPolicy,
) -> BatchEvaluation {
    let gemstone_live = !sample_seed.is_multiple_of(4);
    let hand_sizes = [7usize, 7, 6, 5, 4, 3];
    let mut influenced = false;
    for (stage, hand_size) in hand_sizes.into_iter().enumerate() {
        let permutation = slot_permutation(
            variant.deck_len,
            sample_seed ^ sample_index.rotate_left(23),
            stage as u64,
        );
        let visible: CardMask = permutation.iter().take(7).copied().collect();
        influenced |= !visible.intersect(variant.influence).is_empty();
        let visible_state = PackedStateV2 {
            hand: visible,
            library: PackedLibrary::new(variant.deck_mask.difference(visible)),
            ..PackedStateV2::default()
        };
        let keep = hand_size == 3
            || variant
                .model
                .should_keep(mulligan, visible_state, hand_size);
        if !keep {
            continue;
        }
        let mut bottom_order: Vec<_> = visible.iter().collect();
        bottom_order.sort_by_key(|slot| (variant.model.bottom_priority(*slot), *slot));
        let mut hand = visible;
        for bottom in bottom_order.into_iter().take(7 - hand_size) {
            hand.remove(bottom);
        }
        let state = PackedStateV2 {
            hand,
            library: PackedLibrary::new(variant.deck_mask.difference(hand)),
            ..PackedStateV2::default()
        };
        let value = variant
            .model
            .pregame_states(state, gemstone_live)
            .into_iter()
            .map(|start| {
                if strict_reference {
                    ReferenceSolver::new(&variant.model)
                        .solve(start, depth)
                        .value
                } else {
                    evaluate_compiled_policy(
                        &variant.model,
                        &VisibleOpeningPolicy::new(&variant.model),
                        start,
                        depth,
                    )
                    .value
                }
            })
            .fold(0.0, f64::max);
        return BatchEvaluation { value, influenced };
    }
    unreachable!("the mulligan floor always keeps the final hand")
}

const fn default_seed() -> u64 {
    1
}

const fn default_samples() -> u64 {
    1_000
}

const fn default_max_turn() -> u8 {
    2
}

const fn default_depth() -> u8 {
    24
}

const fn default_discordance_limit() -> usize {
    1_000
}

const fn default_progress_interval() -> u64 {
    1_000
}

const fn default_correction_denominator() -> u64 {
    1
}

const fn default_workers() -> usize {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_batch_is_reproducible_and_resumable() {
        let deck = vec![
            "Ancient Tomb",
            "Lotus Petal",
            "Rhystic Study",
            "Command Tower",
            "Blank A",
            "Blank B",
            "Blank C",
            "Blank D",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let request = OpeningBatchRequest {
            variants: vec![OpeningVariantSpec {
                name: "baseline".to_string(),
                deck,
                influence_slots: vec![0],
            }],
            seed: 17,
            sample_start: 20,
            samples: 10,
            max_turn: 2,
            depth: 16,
            strict_reference: true,
            discordance_limit: 10,
            progress_interval: 5,
            correction_numerator: 0,
            correction_denominator: 1,
            workers: 1,
        };
        let left = evaluate_opening_batch(&request).expect("batch");
        let right = evaluate_opening_batch(&request).expect("batch");
        assert_eq!(left.report.next_sample, 30);
        assert_eq!(left.report.summaries, right.report.summaries);
        assert_eq!(left.report.accumulators, right.report.accumulators);

        let mut parallel_request = request.clone();
        parallel_request.workers = 2;
        let parallel = evaluate_opening_batch(&parallel_request).expect("parallel batch");
        assert_eq!(left.report.summaries, parallel.report.summaries);
        assert_eq!(left.report.accumulators, parallel.report.accumulators);

        let mut corrected_request = request;
        corrected_request.strict_reference = false;
        corrected_request.correction_numerator = 1;
        corrected_request.correction_denominator = 1;
        let corrected = evaluate_opening_batch(&corrected_request).expect("corrected batch");
        let estimate = corrected.multifidelity[0].expect("all samples corrected");
        assert_eq!(estimate.low_samples, 10);
        assert_eq!(estimate.correction_samples, 10);
    }

    #[test]
    fn aligned_variants_must_preserve_slot_count() {
        let request = OpeningBatchRequest {
            variants: vec![
                OpeningVariantSpec {
                    name: "a".to_string(),
                    deck: vec!["Blank".to_string(); 7],
                    influence_slots: Vec::new(),
                },
                OpeningVariantSpec {
                    name: "b".to_string(),
                    deck: vec!["Blank".to_string(); 8],
                    influence_slots: Vec::new(),
                },
            ],
            seed: 1,
            sample_start: 0,
            samples: 1,
            max_turn: 2,
            depth: 8,
            strict_reference: false,
            discordance_limit: 1,
            progress_interval: 1,
            correction_numerator: 0,
            correction_denominator: 1,
            workers: 1,
        };
        assert!(evaluate_opening_batch(&request).is_err());
    }
}
