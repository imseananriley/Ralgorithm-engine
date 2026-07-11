use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, Default)]
struct Moments {
    count: u64,
    mean: f64,
    m2: f64,
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
}

#[derive(Debug, Copy, Clone, Default)]
pub struct MultiFidelityAccumulator {
    low: Moments,
    correction: Moments,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    pub mean: f64,
    pub standard_error: f64,
    pub low_mean: f64,
    pub correction_mean: f64,
    pub low_samples: u64,
    pub correction_samples: u64,
}

impl MultiFidelityAccumulator {
    pub fn push_low(&mut self, low_value: f64) {
        self.low.push(low_value);
    }

    pub fn push_correction(&mut self, low_value: f64, high_value: f64) {
        self.correction.push(high_value - low_value);
    }

    pub fn estimate(self) -> Option<Estimate> {
        if self.low.count == 0 || self.correction.count == 0 {
            return None;
        }
        let variance = self.low.variance() / self.low.count as f64
            + self.correction.variance() / self.correction.count as f64;
        Some(Estimate {
            mean: self.low.mean + self.correction.mean,
            standard_error: variance.max(0.0).sqrt(),
            low_mean: self.low.mean,
            correction_mean: self.correction.mean,
            low_samples: self.low.count,
            correction_samples: self.correction.count,
        })
    }
}
