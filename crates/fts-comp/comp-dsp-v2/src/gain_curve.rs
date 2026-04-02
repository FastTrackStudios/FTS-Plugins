//! Gain curve computation: threshold, ratio, knee.

/// Gain reduction curve processor.
pub struct GainCurve {
    pub threshold_db: f64,
    pub ratio: f64,
    pub knee_db: f64,
    pub range_db: f64,
}

impl GainCurve {
    pub fn new() -> Self {
        Self {
            threshold_db: 0.0,
            ratio: 4.0,
            knee_db: 2.0,
            range_db: 60.0,
        }
    }

    /// Compute gain reduction (linear) from detected level (dB).
    /// Returns linear GR where 1.0 = no reduction, 0.5 = -6 dB reduction.
    pub fn compute_gr(&self, level_db: f64) -> f64 {
        let thresh = self.threshold_db;
        let half_knee = self.knee_db / 2.0;

        if level_db < thresh - half_knee {
            1.0 // No compression
        } else if level_db > thresh + half_knee {
            // Full compression above knee
            // GR is computed as: excess * (1 - 1/ratio)
            // This gives positive dB reduction amount
            let excess = level_db - thresh;
            let gr_db_amount = excess * (1.0 - 1.0 / self.ratio);
            // Convert to linear gain (negative dB = linear < 1.0)
            db_to_linear(-gr_db_amount.min(self.range_db))
        } else {
            // Soft knee transition
            let x = (level_db - (thresh - half_knee)) / self.knee_db;
            let knee_factor = x * x;
            let excess = level_db - thresh;
            let gr_db_amount = excess * (1.0 - 1.0 / self.ratio) * knee_factor;
            db_to_linear(-gr_db_amount)
        }
    }
}

#[inline]
fn db_to_linear(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}
