use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Copy, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BatchEvaluation {
    pub value: f64,
    pub influenced: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchConfig {
    pub root_seed: u64,
    pub sample_start: u64,
    pub samples: u64,
    pub success_threshold: f64,
    pub discordance_limit: usize,
    pub progress_interval: u64,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            root_seed: 1,
            sample_start: 0,
            samples: 1_000,
            success_threshold: f64::EPSILON,
            discordance_limit: 1_000,
            progress_interval: 1_000,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VariantAccumulator {
    pub samples: u64,
    pub value_sum: f64,
    pub value_sum_squares: f64,
    pub delta_sum: f64,
    pub delta_sum_squares: f64,
    pub successes: u64,
    pub candidate_only: u64,
    pub baseline_only: u64,
    pub influenced: u64,
    pub influenced_delta_sum: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantSummary {
    pub name: String,
    pub samples: u64,
    pub mean: f64,
    pub success_rate: f64,
    pub paired_delta: f64,
    pub paired_standard_error: f64,
    pub candidate_only: u64,
    pub baseline_only: u64,
    pub mcnemar_exact_p: f64,
    pub influenced: u64,
    pub influenced_rate: f64,
    pub influenced_mean_delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscordanceRecord {
    pub sample_index: u64,
    pub variant_index: usize,
    pub baseline_value: f64,
    pub candidate_value: f64,
    pub baseline_influenced: bool,
    pub candidate_influenced: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchReport {
    pub root_seed: u64,
    pub sample_start: u64,
    pub next_sample: u64,
    pub elapsed_seconds: f64,
    pub games_per_second: f64,
    pub latency_p50_micros: u64,
    pub latency_p95_micros: u64,
    pub latency_p99_micros: u64,
    pub latency_histogram: Vec<u64>,
    pub accumulators: Vec<VariantAccumulator>,
    pub summaries: Vec<VariantSummary>,
    pub discordances: Vec<DiscordanceRecord>,
}

pub struct BatchEvaluator {
    config: BatchConfig,
}

impl BatchEvaluator {
    pub const fn new(config: BatchConfig) -> Self {
        Self { config }
    }

    pub fn run<F>(&self, variant_names: &[String], evaluate: F) -> BatchReport
    where
        F: FnMut(u64, u64, usize) -> BatchEvaluation,
    {
        self.run_with_progress(variant_names, evaluate, |_, _| {})
    }

    pub fn run_with_progress<F, P>(
        &self,
        variant_names: &[String],
        mut evaluate: F,
        mut progress: P,
    ) -> BatchReport
    where
        F: FnMut(u64, u64, usize) -> BatchEvaluation,
        P: FnMut(u64, &[VariantAccumulator]),
    {
        assert!(
            !variant_names.is_empty(),
            "batch requires a baseline variant"
        );
        let started = Instant::now();
        let mut accumulators = vec![VariantAccumulator::default(); variant_names.len()];
        let mut discordances = Vec::new();
        let mut latencies = Vec::with_capacity(self.config.samples as usize);
        let mut latency_histogram = vec![0u64; 64];
        let end = self.config.sample_start.saturating_add(self.config.samples);
        for sample_index in self.config.sample_start..end {
            let sample_started = Instant::now();
            let sample_seed = sample_seed(self.config.root_seed, sample_index);
            let baseline = evaluate(sample_index, sample_seed, 0);
            for (variant_index, accumulator) in accumulators.iter_mut().enumerate() {
                let candidate = if variant_index == 0 {
                    baseline
                } else {
                    evaluate(sample_index, sample_seed, variant_index)
                };
                let delta = candidate.value - baseline.value;
                let candidate_success = candidate.value >= self.config.success_threshold;
                let baseline_success = baseline.value >= self.config.success_threshold;
                accumulator.samples += 1;
                accumulator.value_sum += candidate.value;
                accumulator.value_sum_squares += candidate.value * candidate.value;
                accumulator.delta_sum += delta;
                accumulator.delta_sum_squares += delta * delta;
                accumulator.successes += u64::from(candidate_success);
                accumulator.candidate_only += u64::from(candidate_success && !baseline_success);
                accumulator.baseline_only += u64::from(!candidate_success && baseline_success);
                if candidate.influenced {
                    accumulator.influenced += 1;
                    accumulator.influenced_delta_sum += delta;
                }
                if variant_index != 0
                    && (candidate_success != baseline_success || delta.abs() > f64::EPSILON)
                    && discordances.len() < self.config.discordance_limit
                {
                    discordances.push(DiscordanceRecord {
                        sample_index,
                        variant_index,
                        baseline_value: baseline.value,
                        candidate_value: candidate.value,
                        baseline_influenced: baseline.influenced,
                        candidate_influenced: candidate.influenced,
                    });
                }
            }
            let latency = sample_started
                .elapsed()
                .as_micros()
                .min(u128::from(u64::MAX)) as u64;
            latencies.push(latency);
            latency_histogram[latency.max(1).ilog2() as usize] += 1;
            let processed = sample_index - self.config.sample_start + 1;
            if self.config.progress_interval > 0
                && (processed.is_multiple_of(self.config.progress_interval)
                    || sample_index + 1 == end)
            {
                progress(sample_index + 1, &accumulators);
            }
        }
        latencies.sort_unstable();
        let elapsed_seconds = started.elapsed().as_secs_f64();
        let evaluations = self
            .config
            .samples
            .saturating_mul(variant_names.len() as u64);
        let summaries = variant_names
            .iter()
            .zip(&accumulators)
            .map(|(name, accumulator)| summarize(name.clone(), accumulator))
            .collect();
        BatchReport {
            root_seed: self.config.root_seed,
            sample_start: self.config.sample_start,
            next_sample: end,
            elapsed_seconds,
            games_per_second: evaluations as f64 / elapsed_seconds.max(f64::MIN_POSITIVE),
            latency_p50_micros: quantile(&latencies, 0.50),
            latency_p95_micros: quantile(&latencies, 0.95),
            latency_p99_micros: quantile(&latencies, 0.99),
            latency_histogram,
            accumulators,
            summaries,
            discordances,
        }
    }
}

pub fn merge_batch_reports(
    variant_names: &[String],
    reports: Vec<BatchReport>,
    elapsed_seconds: f64,
    discordance_limit: usize,
) -> BatchReport {
    assert!(!reports.is_empty(), "at least one batch shard is required");
    let root_seed = reports[0].root_seed;
    let sample_start = reports
        .iter()
        .map(|report| report.sample_start)
        .min()
        .unwrap_or(0);
    let next_sample = reports
        .iter()
        .map(|report| report.next_sample)
        .max()
        .unwrap_or(sample_start);
    let mut accumulators = vec![VariantAccumulator::default(); variant_names.len()];
    let mut histogram = vec![0u64; 64];
    let mut discordances = Vec::new();
    for report in reports {
        assert_eq!(report.root_seed, root_seed);
        for (total, shard) in accumulators.iter_mut().zip(report.accumulators) {
            total.samples += shard.samples;
            total.value_sum += shard.value_sum;
            total.value_sum_squares += shard.value_sum_squares;
            total.delta_sum += shard.delta_sum;
            total.delta_sum_squares += shard.delta_sum_squares;
            total.successes += shard.successes;
            total.candidate_only += shard.candidate_only;
            total.baseline_only += shard.baseline_only;
            total.influenced += shard.influenced;
            total.influenced_delta_sum += shard.influenced_delta_sum;
        }
        for (total, count) in histogram.iter_mut().zip(report.latency_histogram) {
            *total += count;
        }
        discordances.extend(report.discordances);
    }
    discordances.sort_by_key(|record| (record.sample_index, record.variant_index));
    discordances.truncate(discordance_limit);
    let total_samples = accumulators.first().map_or(0, |item| item.samples);
    let summaries = variant_names
        .iter()
        .zip(&accumulators)
        .map(|(name, accumulator)| summarize(name.clone(), accumulator))
        .collect();
    BatchReport {
        root_seed,
        sample_start,
        next_sample,
        elapsed_seconds,
        games_per_second: total_samples as f64 * variant_names.len() as f64
            / elapsed_seconds.max(f64::MIN_POSITIVE),
        latency_p50_micros: histogram_quantile(&histogram, 0.50),
        latency_p95_micros: histogram_quantile(&histogram, 0.95),
        latency_p99_micros: histogram_quantile(&histogram, 0.99),
        latency_histogram: histogram,
        accumulators,
        summaries,
        discordances,
    }
}

pub fn slot_permutation(deck_len: usize, root_seed: u64, sample_index: u64) -> Vec<u8> {
    assert!(deck_len <= u8::MAX as usize + 1);
    let mut slots: Vec<u8> = (0..deck_len).map(|slot| slot as u8).collect();
    slots.shuffle(&mut ChaCha8Rng::from_seed(seed_bytes(
        root_seed,
        sample_index,
        0x534c_4f54_5045_524d,
    )));
    slots
}

pub fn independent_sample_selected(
    root_seed: u64,
    sample_index: u64,
    numerator: u64,
    denominator: u64,
) -> bool {
    denominator > 0
        && numerator.min(denominator)
            > sample_seed(root_seed ^ 0x434f_5252_4543_544e, sample_index) % denominator
}

fn summarize(name: String, accumulator: &VariantAccumulator) -> VariantSummary {
    let n = accumulator.samples as f64;
    let paired_delta = if n > 0.0 {
        accumulator.delta_sum / n
    } else {
        0.0
    };
    let delta_variance = if accumulator.samples > 1 {
        (accumulator.delta_sum_squares - accumulator.delta_sum * accumulator.delta_sum / n).max(0.0)
            / (n - 1.0)
    } else {
        0.0
    };
    VariantSummary {
        name,
        samples: accumulator.samples,
        mean: if n > 0.0 {
            accumulator.value_sum / n
        } else {
            0.0
        },
        success_rate: if n > 0.0 {
            accumulator.successes as f64 / n
        } else {
            0.0
        },
        paired_delta,
        paired_standard_error: if n > 0.0 {
            (delta_variance / n).sqrt()
        } else {
            0.0
        },
        candidate_only: accumulator.candidate_only,
        baseline_only: accumulator.baseline_only,
        mcnemar_exact_p: mcnemar_exact_p(accumulator.candidate_only, accumulator.baseline_only),
        influenced: accumulator.influenced,
        influenced_rate: if n > 0.0 {
            accumulator.influenced as f64 / n
        } else {
            0.0
        },
        influenced_mean_delta: if accumulator.influenced > 0 {
            accumulator.influenced_delta_sum / accumulator.influenced as f64
        } else {
            0.0
        },
    }
}

fn mcnemar_exact_p(candidate_only: u64, baseline_only: u64) -> f64 {
    let discordant = candidate_only + baseline_only;
    if discordant == 0 {
        return 1.0;
    }
    let tail = candidate_only.min(baseline_only);
    let n = discordant as f64;
    let mut probability = 0.0;
    for k in 0..=tail {
        let log_choose = (1..=k)
            .map(|index| ((discordant + 1 - index) as f64).ln() - (index as f64).ln())
            .sum::<f64>();
        probability += (log_choose - n * std::f64::consts::LN_2).exp();
    }
    (2.0 * probability).min(1.0)
}

fn quantile(sorted: &[u64], probability: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) as f64 * probability).round() as usize;
    sorted[index]
}

fn histogram_quantile(histogram: &[u64], probability: f64) -> u64 {
    let total: u64 = histogram.iter().sum();
    if total == 0 {
        return 0;
    }
    let target = (total as f64 * probability).ceil() as u64;
    let mut cumulative = 0;
    for (bucket, count) in histogram.iter().enumerate() {
        cumulative += count;
        if cumulative >= target {
            return 1u64.checked_shl(bucket as u32 + 1).unwrap_or(u64::MAX);
        }
    }
    u64::MAX
}

fn sample_seed(root_seed: u64, sample_index: u64) -> u64 {
    splitmix64(root_seed ^ sample_index.wrapping_mul(0x9e37_79b9_7f4a_7c15))
}

fn seed_bytes(root_seed: u64, sample_index: u64, domain: u64) -> [u8; 32] {
    let mut seed = [0; 32];
    let mut value = root_seed ^ domain ^ sample_index.rotate_left(17);
    for chunk in seed.chunks_exact_mut(8) {
        value = splitmix64(value);
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    seed
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_permutations_are_reproducible_and_sample_specific() {
        assert_eq!(slot_permutation(99, 7, 4), slot_permutation(99, 7, 4));
        assert_ne!(slot_permutation(99, 7, 4), slot_permutation(99, 7, 5));
    }

    #[test]
    fn paired_batch_reports_discordance_and_resume_boundary() {
        let config = BatchConfig {
            root_seed: 9,
            sample_start: 10,
            samples: 4,
            success_threshold: 0.5,
            discordance_limit: 10,
            progress_interval: 2,
        };
        let names = vec!["baseline".to_string(), "candidate".to_string()];
        let mut progress = Vec::new();
        let report = BatchEvaluator::new(config).run_with_progress(
            &names,
            |sample, _, variant| BatchEvaluation {
                value: f64::from((sample + variant as u64).is_multiple_of(2)),
                influenced: variant == 1,
            },
            |next, _| progress.push(next),
        );
        assert_eq!(report.next_sample, 14);
        assert_eq!(progress, vec![12, 14]);
        assert_eq!(report.summaries[1].candidate_only, 2);
        assert_eq!(report.summaries[1].baseline_only, 2);
        assert_eq!(report.discordances.len(), 4);
        assert_eq!(report.summaries[1].mcnemar_exact_p, 1.0);
    }

    #[test]
    fn correction_selection_does_not_depend_on_variant_results() {
        let selected: Vec<_> = (0..1_000)
            .filter(|sample| independent_sample_selected(11, *sample, 1, 10))
            .collect();
        assert!(selected.len() > 70 && selected.len() < 130);
        assert_eq!(selected, {
            (0..1_000)
                .filter(|sample| independent_sample_selected(11, *sample, 1, 10))
                .collect::<Vec<_>>()
        });
    }
}
