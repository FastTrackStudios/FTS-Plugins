//! Gain curve computation: threshold, ratio, knee with attack/release coefficients.
//!
//! Extracted from Pro-C 3 binary (compute_attack_release_coeffs @ 180109b60).

/// Gain reduction curve processor with attack/release coefficients.
pub struct GainCurve {
    pub threshold_db: f64,
    pub ratio: f64,
    pub knee_db: f64,
    pub range_db: f64,

    // Attack/release coefficients (linear domain)
    pub attack_coeff: f64,
    pub release_coeff: f64,
    pub other_coeff: f64,

    // Sample rate dependency
    sample_rate: f64,
    attack_ms: f64,
    release_ms: f64,
}

impl GainCurve {
    pub fn new(sample_rate: f64) -> Self {
        let mut curve = Self {
            threshold_db: 0.0,
            ratio: 4.0,
            knee_db: 2.0,
            range_db: 60.0,
            attack_coeff: 0.01,
            release_coeff: 0.05,
            other_coeff: 0.1,
            sample_rate,
            attack_ms: 10.0,
            release_ms: 50.0,
        };
        curve.update_coefficients();
        curve
    }

    /// Update coefficients when attack/release times change.
    pub fn update_coefficients(&mut self) {
        // Simplified coefficient computation from attack/release times
        // In the real binary, this comes from compute_attack_release_coeffs
        self.attack_coeff = (-2.0 / (self.sample_rate * self.attack_ms / 1000.0)).exp();
        self.release_coeff = (-2.0 / (self.sample_rate * self.release_ms / 1000.0)).exp();
        self.other_coeff = self.release_coeff; // Placeholder
    }

    /// Set attack time in milliseconds.
    pub fn set_attack_ms(&mut self, attack_ms: f64) {
        self.attack_ms = attack_ms.max(0.1);
        self.update_coefficients();
    }

    /// Set release time in milliseconds.
    pub fn set_release_ms(&mut self, release_ms: f64) {
        self.release_ms = release_ms.max(0.1);
        self.update_coefficients();
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
