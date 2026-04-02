//! Pro-C 3 Clean compressor implementation from reverse engineering.
//!
//! This is a 1:1 implementation based on Ghidra analysis of Pro-C 3.clap,
//! with all 16 core functions renamed and documented.
//!
//! Signal flow:
//! 1. detect_level_exponential_perceptual: level = exp(|sample| * 0.1151)
//! 2. compute_gain_curve_from_level: apply threshold/ratio/knee
//! 3. smooth_gr_with_hermite_cubic: Hermite cubic with change detection
//!    - If change detected: full Hermite polynomial
//!    - Else: fallback sqrt smoothing
//! 4. Output compressed audio

pub mod detector;
pub mod gain_curve;
pub mod smoother;
pub mod chain;

pub use detector::Detector;
pub use gain_curve::GainCurve;
pub use smoother::GainReductionSmoother;
pub use chain::CompChain;

/// Main compressor combining detection, gain curve, and smoothing.
pub struct ProC3Compressor {
    detector: Detector,
    gain_curve: GainCurve,
    smoother: GainReductionSmoother,
    sample_rate: f64,
    last_gr_db: [f64; 2],

    // Parameters exposed for plugin interface compatibility
    pub threshold_db: f64,
    pub ratio: f64,
    pub attack_ms: f64,
    pub release_ms: f64,
    pub knee_db: f64,
    pub fold: f64,
    pub input_gain_db: f64,
    pub output_gain_db: f64,
    pub range_db: f64,
    pub hold_ms: f64,

    // Advanced features (mostly stubs for compatibility)
    pub auto_makeup: bool,
    pub feedback: f64,
    pub channel_link: f64,
    pub inertia: f64,
    pub inertia_decay: f64,
    pub ceiling: f64,
}

impl ProC3Compressor {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            detector: Detector::new(),
            gain_curve: GainCurve::new(),
            smoother: GainReductionSmoother::new(sample_rate),
            sample_rate,
            last_gr_db: [0.0; 2],
            threshold_db: 0.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 50.0,
            knee_db: 2.0,
            fold: 1.0,
            input_gain_db: 0.0,
            output_gain_db: 0.0,
            range_db: 60.0,
            hold_ms: 0.0,
            auto_makeup: false,
            feedback: 0.0,
            channel_link: 1.0,
            inertia: 0.0,
            inertia_decay: 0.0,
            ceiling: 0.0,
        }
    }

    /// Process a sample through the full signal chain.
    pub fn process(&mut self, input: f64, channel: usize) -> f64 {
        // Apply input gain
        let input_linear = input * fts_dsp::db::db_to_linear(self.input_gain_db);

        // Step 1: Detect level
        let level_db = self.detector.detect_level(input_linear.abs());

        // Step 2: Compute gain reduction from level via threshold/ratio/knee
        // Note: threshold offset is included in the gain curve calculation
        let gr_linear = self.gain_curve.compute_gr(level_db);

        // Step 3: Smooth GR
        let gr_smoothed = self.smoother.smooth_gr(gr_linear, channel);

        // Step 4: Apply to audio
        let mut output = input_linear * gr_smoothed;

        // Apply output gain and soft ceiling
        let output_gain = fts_dsp::db::db_to_linear(self.output_gain_db);
        output *= output_gain;

        if self.ceiling > 0.0 {
            output = (output / self.ceiling).tanh() * self.ceiling;
        }

        // Apply fold (parallel compression mix)
        let compressed = output;
        output = compressed * self.fold + input_linear * (1.0 - self.fold);

        // Track GR for metering
        self.last_gr_db[channel] = fts_dsp::db::linear_to_db(gr_smoothed).max(0.0);

        output
    }

    /// Update to new sample rate (called on format change)
    pub fn update(&mut self, sample_rate: f64) {
        if (sample_rate - self.sample_rate).abs() > 0.1 {
            self.sample_rate = sample_rate;
            self.smoother = GainReductionSmoother::new(sample_rate);
            // Re-apply current attack/release
            self.smoother.set_attack(self.attack_ms / 1000.0);
            self.smoother.set_release(self.release_ms / 1000.0);
        }
    }

    /// Get current gain reduction in dB for metering
    pub fn gain_reduction_db(&self) -> f64 {
        self.last_gr_db[0].max(self.last_gr_db[1])
    }

    /// Set threshold in dB
    pub fn set_threshold(&mut self, threshold_db: f64) {
        self.gain_curve.threshold_db = threshold_db;
    }

    /// Set ratio (e.g., 4.0 = 4:1)
    pub fn set_ratio(&mut self, ratio: f64) {
        self.gain_curve.ratio = ratio;
    }

    /// Set knee width in dB
    pub fn set_knee(&mut self, knee_db: f64) {
        self.gain_curve.knee_db = knee_db;
    }

    /// Set attack time in seconds
    pub fn set_attack(&mut self, attack_s: f64) {
        self.smoother.set_attack(attack_s);
    }

    /// Set release time in seconds
    pub fn set_release(&mut self, release_s: f64) {
        self.smoother.set_release(release_s);
    }

    /// Set parallel compression fold parameter
    pub fn set_fold(&mut self, fold: f64) {
        self.fold = fold.clamp(0.0, 1.0);
    }

    /// Reset internal state
    pub fn reset(&mut self) {
        self.detector.reset();
        self.smoother.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quiet_signal_passes_through() {
        let mut comp = ProC3Compressor::new(48000.0);
        comp.set_threshold(0.0); // 0 dB threshold

        let quiet_input = 0.001; // Very quiet
        let output = comp.process(quiet_input, 0);

        // Quiet signal should pass through mostly unchanged
        assert!((output - quiet_input).abs() < 0.0001);
    }

    #[test]
    fn test_loud_signal_is_compressed() {
        let mut comp = ProC3Compressor::new(48000.0);
        comp.set_threshold(-18.0); // -18 dB threshold
        comp.set_ratio(4.0); // 4:1 ratio

        let loud_input = 0.5; // ~-6 dB
        let output = comp.process(loud_input, 0);

        // Loud signal should be compressed (reduced in amplitude)
        assert!(output.abs() < loud_input.abs());
    }
}
