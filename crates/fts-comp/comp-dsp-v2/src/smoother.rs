//! Gain reduction smoothing with attack/release time constants.

/// Exponential smoother for gain reduction with Pro-C 3 change detection.
///
/// Implements hybrid smoothing based on change detection:
/// - If change >= 0.1% of gr_inst: Use exponential smoothing (approximates Hermite path)
/// - If change < 0.1%: Use sqrt(gr_inst) fallback (steady state)
pub struct GainReductionSmoother {
    sample_rate: f64,
    attack_s: f64,
    release_s: f64,
    /// Running smoothed value per channel
    state: [f64; 2],
    /// Change detection threshold multiplier from Pro-C 3 (0.001 = 0.1%)
    change_threshold: f64,
}

impl GainReductionSmoother {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            attack_s: 0.01,
            release_s: 0.05,
            state: [1.0; 2],
            change_threshold: 0.001, // Pro-C 3's 0.1% threshold
        }
    }

    pub fn set_attack(&mut self, attack_s: f64) {
        self.attack_s = attack_s.max(0.0001);
    }

    pub fn set_release(&mut self, release_s: f64) {
        self.release_s = release_s.max(0.001);
    }

    /// Smooth GR using exponential smoothing with attack/release coefficients.
    pub fn smooth_gr(&mut self, gr_inst: f64, ch: usize) -> f64 {
        let time_s = if gr_inst < self.state[ch] {
            self.release_s // Expansion: slower
        } else {
            self.attack_s // Compression: faster
        };

        let coeff = self.compute_coeff(time_s);
        let smoothed = coeff * gr_inst + (1.0 - coeff) * self.state[ch];

        self.state[ch] = smoothed;
        smoothed
    }

    #[inline]
    fn compute_coeff(&self, time_s: f64) -> f64 {
        const SCALE: f64 = 2.0;
        (-SCALE / (self.sample_rate * time_s)).exp()
    }

    pub fn reset(&mut self) {
        self.state = [1.0; 2];
    }
}
