use serde::{Deserialize, Serialize};

use super::independent_sample_selected;

#[derive(Debug, Copy, Clone, Default)]
struct Moments {
    count: u64,
    mean: f64,
    m2: f64,
}

#[derive(Debug, Copy, Clone, Default)]
struct PairedMoments {
    x: Moments,
    y: Moments,
    co_moment: f64,
}

impl PairedMoments {
    fn push(&mut self, x: f64, y: f64) {
        let old_x_mean = self.x.mean;
        self.x.push(x);
        self.y.push(y);
        self.co_moment += (x - old_x_mean) * (y - self.y.mean);
    }

    fn covariance(self) -> f64 {
        if self.x.count < 2 {
            0.0
        } else {
            self.co_moment / (self.x.count - 1) as f64
        }
    }

    fn merge(&mut self, other: Self) {
        if other.x.count == 0 {
            return;
        }
        if self.x.count == 0 {
            *self = other;
            return;
        }
        let combined = self.x.count + other.x.count;
        let cross_weight = self.x.count as f64 * other.x.count as f64 / combined as f64;
        self.co_moment += other.co_moment
            + (other.x.mean - self.x.mean) * (other.y.mean - self.y.mean) * cross_weight;
        self.x.merge(other.x);
        self.y.merge(other.y);
    }
}

impl Moments {
    fn push(&mut self, value: f64) {
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        self.m2 += delta * (value - self.mean);
    }

    fn variance(self) -> f64 {
        if self.count < 2 {
            0.0
        } else {
            self.m2 / (self.count - 1) as f64
        }
    }

    fn merge(&mut self, other: Self) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            *self = other;
            return;
        }
        let combined = self.count + other.count;
        let delta = other.mean - self.mean;
        self.m2 +=
            other.m2 + delta * delta * self.count as f64 * other.count as f64 / combined as f64;
        self.mean += delta * other.count as f64 / combined as f64;
        self.count = combined;
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct MultiFidelityAccumulator {
    low: Moments,
    corrected_pairs: PairedMoments,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    pub mean: f64,
    pub standard_error: f64,
    pub low_mean: f64,
    pub correction_mean: f64,
    pub low_samples: u64,
    pub correction_samples: u64,
    pub low_correction_covariance: f64,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiFidelityConfig {
    pub root_seed: u64,
    pub sample_start: u64,
    pub samples: u64,
    pub correction_numerator: u64,
    pub correction_denominator: u64,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiFidelityReport {
    pub estimate: Option<Estimate>,
    pub next_sample: u64,
    pub high_evaluations: u64,
}

impl MultiFidelityAccumulator {
    pub fn push_low(&mut self, low_value: f64) {
        self.low.push(low_value);
    }

    pub fn push_correction(&mut self, low_value: f64, high_value: f64) {
        self.corrected_pairs.push(low_value, high_value - low_value);
    }

    pub fn estimate(self) -> Option<Estimate> {
        if self.low.count == 0 || self.corrected_pairs.y.count == 0 {
            return None;
        }
        let covariance = self.corrected_pairs.covariance();
        let variance = self.low.variance() / self.low.count as f64
            + self.corrected_pairs.y.variance() / self.corrected_pairs.y.count as f64
            + 2.0 * covariance / self.low.count as f64;
        Some(Estimate {
            mean: self.low.mean + self.corrected_pairs.y.mean,
            standard_error: variance.max(0.0).sqrt(),
            low_mean: self.low.mean,
            correction_mean: self.corrected_pairs.y.mean,
            low_samples: self.low.count,
            correction_samples: self.corrected_pairs.y.count,
            low_correction_covariance: covariance,
        })
    }

    pub fn merge(&mut self, other: Self) {
        self.low.merge(other.low);
        self.corrected_pairs.merge(other.corrected_pairs);
    }
}

pub fn evaluate_multifidelity<L, H>(
    config: MultiFidelityConfig,
    mut evaluate_low: L,
    mut evaluate_high: H,
) -> MultiFidelityReport
where
    L: FnMut(u64) -> f64,
    H: FnMut(u64) -> f64,
{
    let mut accumulator = MultiFidelityAccumulator::default();
    let end = config.sample_start.saturating_add(config.samples);
    let mut high_evaluations = 0;
    for sample_index in config.sample_start..end {
        let low = evaluate_low(sample_index);
        accumulator.push_low(low);
        if independent_sample_selected(
            config.root_seed,
            sample_index,
            config.correction_numerator,
            config.correction_denominator,
        ) {
            accumulator.push_correction(low, evaluate_high(sample_index));
            high_evaluations += 1;
        }
    }
    MultiFidelityReport {
        estimate: accumulator.estimate(),
        next_sample: end,
        high_evaluations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_uses_all_low_samples_and_only_independent_corrections() {
        let report = evaluate_multifidelity(
            MultiFidelityConfig {
                root_seed: 19,
                sample_start: 100,
                samples: 1_000,
                correction_numerator: 1,
                correction_denominator: 10,
            },
            |_| 0.25,
            |_| 0.50,
        );
        let estimate = report.estimate.expect("correction sample populated");
        assert_eq!(estimate.low_samples, 1_000);
        assert_eq!(estimate.correction_samples, report.high_evaluations);
        assert!((estimate.mean - 0.50).abs() < f64::EPSILON);
        assert_eq!(report.next_sample, 1_100);
    }

    #[test]
    fn standard_error_includes_overlap_covariance() {
        let report = evaluate_multifidelity(
            MultiFidelityConfig {
                root_seed: 3,
                sample_start: 0,
                samples: 100,
                correction_numerator: 1,
                correction_denominator: 1,
            },
            |index| (index % 2) as f64,
            |_| 0.5,
        );
        let estimate = report.estimate.expect("fully corrected estimate");
        assert!((estimate.mean - 0.5).abs() < f64::EPSILON);
        assert!(estimate.standard_error < 1e-9);
        assert!(estimate.low_correction_covariance < 0.0);
    }
}
