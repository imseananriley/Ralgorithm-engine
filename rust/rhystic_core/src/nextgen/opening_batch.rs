use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::{
    evaluate_opening_outcome_policy, independent_sample_selected, merge_batch_reports,
    slot_permutation, BatchConfig, BatchEvaluation, BatchEvaluator, BatchReport, CardMask,
    DeckSpec, EngineOpeningModel, Estimate, MultiFidelityAccumulator, OpeningOutcome,
    OpeningOutcomeSolver, PackedLibrary, PackedStateV2, VisibleOpeningPolicy,
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
    #[serde(default)]
    pub exact_slot_draws: bool,
    #[serde(default = "default_mulligan_pilot_samples")]
    pub mulligan_pilot_samples: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningBatchResponse {
    pub evaluator: String,
    pub mulligan_policy: String,
    pub report: BatchReport,
    pub multifidelity: Vec<Option<Estimate>>,
    pub outcomes: Vec<OpeningOutcomeSummary>,
    pub mulligan_continuation_ev: Vec<[f64; 6]>,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpeningOutcomeAccumulator {
    pub samples: u64,
    pub weighted_ev_sum: f64,
    pub rhystic_turn_1_sum: f64,
    pub rhystic_turn_2_sum: f64,
    pub heartwood_turn_1_sum: f64,
    pub heartwood_turn_2_sum: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningOutcomeSummary {
    pub name: String,
    pub samples: u64,
    pub weighted_ev: f64,
    pub rhystic_turn_1: f64,
    pub rhystic_by_turn_2: f64,
    pub heartwood_turn_1: f64,
    pub heartwood_by_turn_2: f64,
    pub any_engine_by_turn_1: f64,
    pub any_engine_by_turn_2: f64,
}

impl OpeningOutcomeAccumulator {
    fn push(&mut self, outcome: OpeningOutcome) {
        self.samples += 1;
        self.weighted_ev_sum += outcome.weighted_ev;
        self.rhystic_turn_1_sum += outcome.rhystic_turn_1;
        self.rhystic_turn_2_sum += outcome.rhystic_turn_2;
        self.heartwood_turn_1_sum += outcome.heartwood_turn_1;
        self.heartwood_turn_2_sum += outcome.heartwood_turn_2;
    }

    fn merge(&mut self, other: Self) {
        self.samples += other.samples;
        self.weighted_ev_sum += other.weighted_ev_sum;
        self.rhystic_turn_1_sum += other.rhystic_turn_1_sum;
        self.rhystic_turn_2_sum += other.rhystic_turn_2_sum;
        self.heartwood_turn_1_sum += other.heartwood_turn_1_sum;
        self.heartwood_turn_2_sum += other.heartwood_turn_2_sum;
    }

    fn summarize(self, name: String) -> OpeningOutcomeSummary {
        let n = self.samples.max(1) as f64;
        let rhystic_turn_1 = self.rhystic_turn_1_sum / n;
        let rhystic_turn_2 = self.rhystic_turn_2_sum / n;
        let heartwood_turn_1 = self.heartwood_turn_1_sum / n;
        let heartwood_turn_2 = self.heartwood_turn_2_sum / n;
        OpeningOutcomeSummary {
            name,
            samples: self.samples,
            weighted_ev: self.weighted_ev_sum / n,
            rhystic_turn_1,
            rhystic_by_turn_2: rhystic_turn_1 + rhystic_turn_2,
            heartwood_turn_1,
            heartwood_by_turn_2: heartwood_turn_1 + heartwood_turn_2,
            any_engine_by_turn_1: rhystic_turn_1 + heartwood_turn_1,
            any_engine_by_turn_2: rhystic_turn_1
                + rhystic_turn_2
                + heartwood_turn_1
                + heartwood_turn_2,
        }
    }
}

struct CompiledVariant {
    model: EngineOpeningModel,
    reference_model: EngineOpeningModel,
    deck_mask: CardMask,
    deck_len: usize,
    influence: CardMask,
}

#[derive(Debug, Copy, Clone)]
struct OpeningGameEvaluation {
    outcome: OpeningOutcome,
    influenced: bool,
}

#[derive(Debug, Copy, Clone, Default)]
struct MulliganEvPolicy {
    continuation_ev: [f64; 6],
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
            model: EngineOpeningModel::compile(&deck, request.max_turn)
                .with_quotient_draws(!request.exact_slot_draws),
            reference_model: EngineOpeningModel::compile(&deck, request.max_turn)
                .with_quotient_draws(false),
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
    let low_mulligans: Vec<_> = variants
        .iter()
        .map(|variant| train_mulligan_policy(request, variant, false))
        .collect();
    let high_mulligans = if request.strict_reference || request.correction_numerator > 0 {
        variants
            .iter()
            .map(|variant| train_mulligan_policy(request, variant, true))
            .collect()
    } else {
        Vec::new()
    };
    let workers = request.workers.max(1).min(request.samples.max(1) as usize);
    let started = Instant::now();
    let outputs = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        let base = request.samples / workers as u64;
        let remainder = request.samples % workers as u64;
        let mut start = request.sample_start;
        let variants = &variants;
        let names = &names;
        let low_mulligans = &low_mulligans;
        let high_mulligans = &high_mulligans;
        for worker in 0..workers {
            let count = base + u64::from((worker as u64) < remainder);
            let shard_start = start;
            start += count;
            handles.push(scope.spawn(move || {
                run_shard(
                    request,
                    variants,
                    names,
                    low_mulligans,
                    high_mulligans,
                    shard_start,
                    count,
                )
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("opening batch worker panicked"))
            .collect::<Vec<_>>()
    });
    let mut reports = Vec::with_capacity(outputs.len());
    let mut multifidelity = vec![MultiFidelityAccumulator::default(); variants.len()];
    let mut outcomes = vec![OpeningOutcomeAccumulator::default(); variants.len()];
    for (report, shard_multifidelity, shard_outcomes) in outputs {
        reports.push(report);
        for (total, shard) in multifidelity.iter_mut().zip(shard_multifidelity) {
            total.merge(shard);
        }
        for (total, shard) in outcomes.iter_mut().zip(shard_outcomes) {
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
        mulligan_policy:
            "commander London 7,7,6,5,4,3; independent-pilot continuation EV; exact bottoms"
                .to_string(),
        report,
        multifidelity: multifidelity
            .into_iter()
            .map(MultiFidelityAccumulator::estimate)
            .collect(),
        outcomes: outcomes
            .into_iter()
            .zip(names.clone())
            .map(|(accumulator, name)| accumulator.summarize(name))
            .collect(),
        mulligan_continuation_ev: if request.strict_reference {
            high_mulligans
        } else {
            low_mulligans
        }
        .into_iter()
        .map(|policy| policy.continuation_ev)
        .collect(),
    })
}

fn run_shard(
    request: &OpeningBatchRequest,
    variants: &[CompiledVariant],
    names: &[String],
    low_mulligans: &[MulliganEvPolicy],
    high_mulligans: &[MulliganEvPolicy],
    sample_start: u64,
    samples: u64,
) -> (
    BatchReport,
    Vec<MultiFidelityAccumulator>,
    Vec<OpeningOutcomeAccumulator>,
) {
    let batch = BatchEvaluator::new(BatchConfig {
        root_seed: request.seed,
        sample_start,
        samples,
        success_threshold: f64::EPSILON,
        discordance_limit: request.discordance_limit,
        progress_interval: request.progress_interval,
    });
    let mut multifidelity = vec![MultiFidelityAccumulator::default(); variants.len()];
    let mut outcomes = vec![OpeningOutcomeAccumulator::default(); variants.len()];
    let report = batch.run(names, |sample_index, sample_seed, variant_index| {
        let low = evaluate_game(
            &variants[variant_index],
            sample_index,
            sample_seed,
            request.depth,
            request.strict_reference,
            if request.strict_reference {
                high_mulligans[variant_index]
            } else {
                low_mulligans[variant_index]
            },
        );
        outcomes[variant_index].push(low.outcome);
        multifidelity[variant_index].push_low(low.outcome.weighted_ev);
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
                high_mulligans[variant_index],
            );
            multifidelity[variant_index]
                .push_correction(low.outcome.weighted_ev, high.outcome.weighted_ev);
        }
        BatchEvaluation {
            value: low.outcome.weighted_ev,
            influenced: low.influenced,
        }
    });
    (report, multifidelity, outcomes)
}

fn evaluate_game(
    variant: &CompiledVariant,
    sample_index: u64,
    sample_seed: u64,
    depth: u8,
    strict_reference: bool,
    mulligan: MulliganEvPolicy,
) -> OpeningGameEvaluation {
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
        let outcome = best_keep_outcome(
            variant,
            visible,
            hand_size,
            gemstone_live,
            depth,
            strict_reference,
        );
        let keep =
            hand_size == 3 || outcome.weighted_ev >= mulligan.continuation_ev[(stage + 1).min(5)];
        if !keep {
            continue;
        }
        return OpeningGameEvaluation {
            outcome,
            influenced,
        };
    }
    unreachable!("the mulligan floor always keeps the final hand")
}

fn train_mulligan_policy(
    request: &OpeningBatchRequest,
    variant: &CompiledVariant,
    strict_reference: bool,
) -> MulliganEvPolicy {
    let samples = request.mulligan_pilot_samples.max(1);
    let hand_sizes = [7usize, 7, 6, 5, 4, 3];
    let pilot_seed = request.seed ^ 0x4d55_4c4c_5049_4c4f;
    let mut continuation_ev = [0.0; 6];
    for stage in (0..hand_sizes.len()).rev() {
        let mut total = 0.0;
        for pilot in 0..samples {
            let permutation = slot_permutation(
                variant.deck_len,
                pilot_seed ^ pilot.rotate_left(23),
                stage as u64,
            );
            let visible: CardMask = permutation.iter().take(7).copied().collect();
            let gemstone_live = independent_sample_selected(pilot_seed, pilot, 3, 4);
            let keep = best_keep_outcome(
                variant,
                visible,
                hand_sizes[stage],
                gemstone_live,
                request.depth,
                strict_reference,
            )
            .weighted_ev;
            total += if stage + 1 == hand_sizes.len() {
                keep
            } else {
                keep.max(continuation_ev[stage + 1])
            };
        }
        continuation_ev[stage] = total / samples as f64;
    }
    MulliganEvPolicy { continuation_ev }
}

fn best_keep_outcome(
    variant: &CompiledVariant,
    visible: CardMask,
    hand_size: usize,
    gemstone_live: bool,
    depth: u8,
    strict_reference: bool,
) -> OpeningOutcome {
    let model = if strict_reference {
        &variant.reference_model
    } else {
        &variant.model
    };
    let slots: Vec<_> = visible.iter().collect();
    let bottom_count = 7usize.saturating_sub(hand_size);
    let mut best = OpeningOutcome::default();
    for selection in 0u8..(1u8 << slots.len()) {
        if selection.count_ones() as usize != bottom_count {
            continue;
        }
        let mut hand = visible;
        let mut library = PackedLibrary::new(variant.deck_mask.difference(visible));
        for (index, slot) in slots.iter().copied().enumerate() {
            if selection & (1 << index) == 0 {
                continue;
            }
            hand.remove(slot);
            library.insert_unknown(slot);
            library.push_known_bottom(slot);
        }
        let state = PackedStateV2 {
            hand,
            library,
            ..PackedStateV2::default()
        };
        let candidate = model
            .pregame_states(state, gemstone_live)
            .into_iter()
            .map(|start| {
                if strict_reference {
                    OpeningOutcomeSolver::new(model).solve(start, depth).outcome
                } else {
                    evaluate_opening_outcome_policy(
                        model,
                        &VisibleOpeningPolicy::new(model),
                        start,
                        depth,
                    )
                    .outcome
                }
            })
            .max_by(|left, right| left.weighted_ev.total_cmp(&right.weighted_ev))
            .unwrap_or_default();
        if candidate.weighted_ev > best.weighted_ev {
            best = candidate;
        }
    }
    best
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

const fn default_mulligan_pilot_samples() -> u64 {
    64
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
            exact_slot_draws: false,
            mulligan_pilot_samples: 8,
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
            exact_slot_draws: false,
            mulligan_pilot_samples: 8,
        };
        assert!(evaluate_opening_batch(&request).is_err());
    }

    #[test]
    fn exact_london_bottoms_preserve_the_winning_subset() {
        let names = [
            "Ancient Tomb",
            "Lotus Petal",
            "Rhystic Study",
            "Blank A",
            "Blank B",
            "Blank C",
            "Blank D",
            "Blank library",
        ]
        .map(str::to_string);
        let deck = DeckSpec::compile(&names).expect("fixture deck");
        let variant = CompiledVariant {
            model: EngineOpeningModel::compile(&deck, 2),
            reference_model: EngineOpeningModel::compile(&deck, 2).with_quotient_draws(false),
            deck_mask: deck.card_mask(),
            deck_len: deck.cards().len(),
            influence: CardMask::EMPTY,
        };
        let visible = (0..7).collect();

        let outcome = best_keep_outcome(&variant, visible, 3, false, 16, true);

        assert_eq!(outcome.rhystic_turn_1, 1.0);
    }
}
