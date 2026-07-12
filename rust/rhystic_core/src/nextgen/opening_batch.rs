use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use super::{
    independent_sample_selected, merge_batch_reports, slot_permutation, ActionClass, BatchConfig,
    BatchEvaluation, BatchEvaluator, BatchReport, CardMask, DeckSpec, EngineOpeningModel, Estimate,
    MultiFidelityAccumulator, OpeningOutcome, OpeningOutcomePolicySolver, OpeningOutcomeResult,
    OpeningOutcomeSolver, PackedLibrary, PackedStateV2, SearchMetrics, VisibleOpeningPolicy,
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
    #[serde(default)]
    pub fixture_mode: bool,
    #[serde(default = "default_commander_identity")]
    pub commander_identity_mask: u8,
    #[serde(default = "default_true")]
    pub publication_mode: bool,
    #[serde(default = "default_work_chunk_size")]
    pub work_chunk_size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningBatchResponse {
    pub evaluator: String,
    pub mulligan_policy: String,
    pub report: BatchReport,
    pub multifidelity: Vec<Option<Estimate>>,
    pub paired_multifidelity_delta: Vec<Option<Estimate>>,
    pub outcomes: Vec<OpeningOutcomeSummary>,
    pub mulligan_continuation_ev: Vec<[f64; 6]>,
    pub deck_validation: String,
    pub model_digest: String,
    pub request_digest: String,
    pub support_manifests: Vec<OpeningSupportManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpeningSupportManifest {
    pub variant: String,
    pub supported: Vec<String>,
    pub inert: Vec<String>,
    pub unsupported_opening_relevant: Vec<String>,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpeningOutcomeAccumulator {
    pub samples: u64,
    pub weighted_ev_sum: f64,
    pub rhystic_turn_1_sum: f64,
    pub rhystic_turn_2_sum: f64,
    pub heartwood_turn_1_sum: f64,
    pub heartwood_turn_2_sum: f64,
    pub weighted_upper_sum: f64,
    pub capped_samples: u64,
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
    pub weighted_ev_upper: f64,
    pub capped_rate: f64,
}

impl OpeningOutcomeAccumulator {
    fn push(&mut self, result: OpeningOutcomeResult) {
        let outcome = result.outcome;
        self.samples += 1;
        self.weighted_ev_sum += outcome.weighted_ev;
        self.rhystic_turn_1_sum += outcome.rhystic_turn_1;
        self.rhystic_turn_2_sum += outcome.rhystic_turn_2;
        self.heartwood_turn_1_sum += outcome.heartwood_turn_1;
        self.heartwood_turn_2_sum += outcome.heartwood_turn_2;
        self.weighted_upper_sum += result.upper_bound;
        self.capped_samples += u64::from(result.capped);
    }

    fn merge(&mut self, other: Self) {
        self.samples += other.samples;
        self.weighted_ev_sum += other.weighted_ev_sum;
        self.rhystic_turn_1_sum += other.rhystic_turn_1_sum;
        self.rhystic_turn_2_sum += other.rhystic_turn_2_sum;
        self.heartwood_turn_1_sum += other.heartwood_turn_1_sum;
        self.heartwood_turn_2_sum += other.heartwood_turn_2_sum;
        self.weighted_upper_sum += other.weighted_upper_sum;
        self.capped_samples += other.capped_samples;
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
            weighted_ev_upper: self.weighted_upper_sum / n,
            capped_rate: self.capped_samples as f64 / n,
        }
    }
}

struct CompiledVariant {
    model: EngineOpeningModel,
    reference_model: EngineOpeningModel,
    deck_mask: CardMask,
    deck_len: usize,
    influence: CardMask,
    support_manifest: OpeningSupportManifest,
}

#[derive(Debug, Copy, Clone)]
struct OpeningGameEvaluation {
    result: OpeningOutcomeResult,
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
    if request.samples == 0 {
        return Err("opening batch requires at least one sample".to_string());
    }
    let expected_len = request.variants[0].deck.len();
    if request.fixture_mode && expected_len < 7 {
        return Err("opening batch decks require at least seven cards".to_string());
    }
    if !request.fixture_mode && expected_len != 99 {
        return Err(format!(
            "production Commander decks require exactly 99 library cards; received {expected_len}"
        ));
    }
    let mut variants = Vec::with_capacity(request.variants.len());
    for variant in &request.variants {
        if variant.deck.len() != expected_len {
            return Err("all aligned variants must have the same deck length".to_string());
        }
        let deck = DeckSpec::compile(&variant.deck)?;
        for card in deck.cards() {
            if card.color_mask & !request.commander_identity_mask != 0 {
                return Err(format!(
                    "{} is outside commander color identity mask {:05b}",
                    card.name, request.commander_identity_mask
                ));
            }
        }
        let influence = variant.influence_slots.iter().copied().collect();
        let model = EngineOpeningModel::compile(&deck, request.max_turn)
            .with_quotient_draws(!request.exact_slot_draws);
        let support_manifest = support_manifest(&variant.name, &deck, &model);
        if request.publication_mode && !support_manifest.unsupported_opening_relevant.is_empty() {
            return Err(format!(
                "variant {} has unsupported opening-relevant cards: {}",
                variant.name,
                support_manifest.unsupported_opening_relevant.join(", ")
            ));
        }
        variants.push(CompiledVariant {
            model,
            reference_model: EngineOpeningModel::compile(&deck, request.max_turn)
                .with_quotient_draws(false),
            deck_mask: deck.card_mask(),
            deck_len: deck.cards().len(),
            influence,
            support_manifest,
        });
    }
    validate_variant_alignment(request)?;
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
    let next_offset = AtomicU64::new(0);
    let mut outputs = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        let variants = &variants;
        let names = &names;
        let low_mulligans = &low_mulligans;
        let high_mulligans = &high_mulligans;
        for _ in 0..workers {
            let next_offset = &next_offset;
            handles.push(scope.spawn(move || {
                let mut worker_outputs = Vec::new();
                loop {
                    let offset =
                        next_offset.fetch_add(request.work_chunk_size.max(1), Ordering::Relaxed);
                    if offset >= request.samples {
                        break;
                    }
                    let count = request.work_chunk_size.max(1).min(request.samples - offset);
                    worker_outputs.push(run_shard(
                        request,
                        variants,
                        names,
                        low_mulligans,
                        high_mulligans,
                        request.sample_start + offset,
                        count,
                    ));
                }
                worker_outputs
            }));
        }
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("opening batch worker panicked"))
            .collect::<Vec<_>>()
    });
    outputs.sort_by_key(|(report, _, _, _)| report.sample_start);
    let mut reports = Vec::with_capacity(outputs.len());
    let mut multifidelity = vec![MultiFidelityAccumulator::default(); variants.len()];
    let mut paired_multifidelity = vec![MultiFidelityAccumulator::default(); variants.len()];
    let mut outcomes = vec![OpeningOutcomeAccumulator::default(); variants.len()];
    for (report, shard_multifidelity, shard_paired_multifidelity, shard_outcomes) in outputs {
        reports.push(report);
        for (total, shard) in multifidelity.iter_mut().zip(shard_multifidelity) {
            total.merge(shard);
        }
        for (total, shard) in paired_multifidelity
            .iter_mut()
            .zip(shard_paired_multifidelity)
        {
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
        paired_multifidelity_delta: paired_multifidelity
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
        deck_validation: if request.fixture_mode {
            "fixture: singleton packed slots; minimum seven cards".to_string()
        } else {
            "production: 99-card singleton library and commander color identity".to_string()
        },
        model_digest: model_digest(request),
        request_digest: digest_bytes(
            &serde_json::to_vec(request).map_err(|error| error.to_string())?,
        ),
        support_manifests: variants
            .into_iter()
            .map(|variant| variant.support_manifest)
            .collect(),
    })
}

fn validate_variant_alignment(request: &OpeningBatchRequest) -> Result<(), String> {
    if request.variants.len() < 2 {
        return Ok(());
    }
    let baseline = &request.variants[0].deck;
    let mut baseline_differences = Vec::new();
    for variant in request.variants.iter().skip(1) {
        let differences: Vec<u8> = baseline
            .iter()
            .zip(&variant.deck)
            .enumerate()
            .filter_map(|(slot, (left, right))| (left != right).then_some(slot as u8))
            .collect();
        let mut declared = variant.influence_slots.clone();
        declared.sort_unstable();
        declared.dedup();
        if declared != differences {
            return Err(format!(
                "variant {} influence slots {:?} do not equal changed slots {:?}",
                variant.name, declared, differences
            ));
        }
        baseline_differences.extend(differences);
    }
    baseline_differences.sort_unstable();
    baseline_differences.dedup();
    let mut baseline_declared = request.variants[0].influence_slots.clone();
    baseline_declared.sort_unstable();
    baseline_declared.dedup();
    if baseline_declared != baseline_differences {
        return Err(format!(
            "baseline influence slots {:?} do not equal the union of changed slots {:?}",
            baseline_declared, baseline_differences
        ));
    }
    Ok(())
}

fn support_manifest(
    variant_name: &str,
    deck: &DeckSpec,
    model: &EngineOpeningModel,
) -> OpeningSupportManifest {
    let supported_slots = model.supported_slots();
    let mut manifest = OpeningSupportManifest {
        variant: variant_name.to_string(),
        supported: Vec::new(),
        inert: Vec::new(),
        unsupported_opening_relevant: Vec::new(),
    };
    for card in deck.cards() {
        if supported_slots.contains(card.slot) {
            manifest.supported.push(card.name.to_string());
        } else if matches!(
            card.action_class,
            ActionClass::Land | ActionClass::Mana | ActionClass::Tutor | ActionClass::Engine
        ) {
            manifest
                .unsupported_opening_relevant
                .push(card.name.to_string());
        } else {
            manifest.inert.push(card.name.to_string());
        }
    }
    manifest
}

fn model_digest(request: &OpeningBatchRequest) -> String {
    let model_input = (
        "rhystic-nextgen-opening-v2",
        request.max_turn,
        request.exact_slot_draws,
        request.commander_identity_mask,
        request
            .variants
            .iter()
            .map(|variant| (&variant.name, &variant.deck))
            .collect::<Vec<_>>(),
    );
    digest_bytes(&serde_json::to_vec(&model_input).expect("model digest input serializes"))
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Blake2bVar::new(16).expect("valid digest size");
    hasher.update(bytes);
    let mut digest = [0; 16];
    hasher
        .finalize_variable(&mut digest)
        .expect("digest output size");
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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
    let mut paired_multifidelity = vec![MultiFidelityAccumulator::default(); variants.len()];
    let mut outcomes = vec![OpeningOutcomeAccumulator::default(); variants.len()];
    let mut baseline_low = 0.0;
    let mut baseline_high = 0.0;
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
        outcomes[variant_index].push(low.result);
        multifidelity[variant_index].push_low(low.result.outcome.weighted_ev);
        if variant_index == 0 {
            baseline_low = low.result.outcome.weighted_ev;
        }
        let low_delta = low.result.outcome.weighted_ev - baseline_low;
        paired_multifidelity[variant_index].push_low(low_delta);
        let corrected = !request.strict_reference
            && independent_sample_selected(
                request.seed,
                sample_index,
                request.correction_numerator,
                request.correction_denominator,
            );
        if corrected {
            let high = evaluate_game(
                &variants[variant_index],
                sample_index,
                sample_seed,
                request.depth,
                true,
                high_mulligans[variant_index],
            );
            multifidelity[variant_index].push_correction(
                low.result.outcome.weighted_ev,
                high.result.outcome.weighted_ev,
            );
            if variant_index == 0 {
                baseline_high = high.result.outcome.weighted_ev;
            }
            paired_multifidelity[variant_index]
                .push_correction(low_delta, high.result.outcome.weighted_ev - baseline_high);
        }
        BatchEvaluation {
            value: low.result.outcome.weighted_ev,
            influenced: low.influenced,
        }
    });
    (report, multifidelity, paired_multifidelity, outcomes)
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
        let result = best_keep_outcome(
            variant,
            visible,
            hand_size,
            gemstone_live,
            depth,
            strict_reference,
        );
        let keep = hand_size == 3
            || result.outcome.weighted_ev >= mulligan.continuation_ev[(stage + 1).min(5)];
        if !keep {
            continue;
        }
        return OpeningGameEvaluation { result, influenced };
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
            .outcome
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
) -> OpeningOutcomeResult {
    let model = if strict_reference {
        &variant.reference_model
    } else {
        &variant.model
    };
    if strict_reference {
        let mut solver = OpeningOutcomeSolver::new(model);
        best_keep_outcome_with(variant, model, visible, hand_size, gemstone_live, |start| {
            solver.solve(start, depth)
        })
    } else {
        let policy = VisibleOpeningPolicy::new(model);
        let mut solver = OpeningOutcomePolicySolver::new(model, &policy);
        best_keep_outcome_with(variant, model, visible, hand_size, gemstone_live, |start| {
            solver.solve(start, depth)
        })
    }
}

fn best_keep_outcome_with(
    variant: &CompiledVariant,
    model: &EngineOpeningModel,
    visible: CardMask,
    hand_size: usize,
    gemstone_live: bool,
    mut solve: impl FnMut(PackedStateV2) -> OpeningOutcomeResult,
) -> OpeningOutcomeResult {
    let slots: Vec<_> = visible.iter().collect();
    let bottom_count = 7usize.saturating_sub(hand_size);
    let mut best = OpeningOutcomeResult {
        outcome: OpeningOutcome::default(),
        lower_bound: 0.0,
        upper_bound: 0.0,
        capped: false,
        metrics: SearchMetrics::default(),
    };
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
        let mut candidate = OpeningOutcomeResult {
            outcome: OpeningOutcome::default(),
            lower_bound: 0.0,
            upper_bound: 0.0,
            capped: false,
            metrics: SearchMetrics::default(),
        };
        for result in model
            .pregame_states(state, gemstone_live)
            .into_iter()
            .map(&mut solve)
        {
            candidate.upper_bound = candidate.upper_bound.max(result.upper_bound);
            if result.outcome.weighted_ev > candidate.outcome.weighted_ev {
                candidate.outcome = result.outcome;
                candidate.lower_bound = result.lower_bound;
                candidate.metrics = result.metrics;
            }
        }
        candidate.capped = candidate.upper_bound > candidate.lower_bound + f64::EPSILON;
        best.upper_bound = best.upper_bound.max(candidate.upper_bound);
        if candidate.outcome.weighted_ev > best.outcome.weighted_ev {
            best.outcome = candidate.outcome;
            best.lower_bound = candidate.lower_bound;
            best.metrics = candidate.metrics;
        }
        best.capped = best.upper_bound > best.lower_bound + f64::EPSILON;
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

const fn default_commander_identity() -> u8 {
    0b1_1111
}

const fn default_true() -> bool {
    true
}

const fn default_work_chunk_size() -> u64 {
    8
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
            fixture_mode: true,
            commander_identity_mask: 0b1_1111,
            publication_mode: true,
            work_chunk_size: 2,
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
        let paired = corrected.paired_multifidelity_delta[0].expect("paired baseline");
        assert_eq!(paired.mean, 0.0);
        assert_eq!(paired.standard_error, 0.0);
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
            fixture_mode: true,
            commander_identity_mask: 0b1_1111,
            publication_mode: true,
            work_chunk_size: 2,
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
            support_manifest: support_manifest(
                "fixture",
                &deck,
                &EngineOpeningModel::compile(&deck, 2),
            ),
        };
        let visible = (0..7).collect();

        let outcome = best_keep_outcome(&variant, visible, 3, false, 16, true);

        assert_eq!(outcome.outcome.rhystic_turn_1, 1.0);
    }

    #[test]
    fn production_mode_requires_a_ninety_nine_card_library() {
        let request = OpeningBatchRequest {
            variants: vec![OpeningVariantSpec {
                name: "short".to_string(),
                deck: (0..7).map(|index| format!("Blank {index}")).collect(),
                influence_slots: Vec::new(),
            }],
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
            mulligan_pilot_samples: 1,
            fixture_mode: false,
            commander_identity_mask: 0b1_1111,
            publication_mode: true,
            work_chunk_size: 1,
        };
        assert!(evaluate_opening_batch(&request)
            .expect_err("short production deck")
            .contains("exactly 99"));
    }

    #[test]
    fn publication_mode_rejects_unimplemented_opening_relevant_cards() {
        let request = OpeningBatchRequest {
            variants: vec![OpeningVariantSpec {
                name: "unsupported".to_string(),
                deck: [
                    "Beseech the Mirror",
                    "Blank A",
                    "Blank B",
                    "Blank C",
                    "Blank D",
                    "Blank E",
                    "Blank F",
                ]
                .map(str::to_string)
                .to_vec(),
                influence_slots: Vec::new(),
            }],
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
            mulligan_pilot_samples: 1,
            fixture_mode: true,
            commander_identity_mask: 0b1_1111,
            publication_mode: true,
            work_chunk_size: 1,
        };
        assert!(evaluate_opening_batch(&request)
            .expect_err("unsupported tutor")
            .contains("Beseech the Mirror"));
    }
}
