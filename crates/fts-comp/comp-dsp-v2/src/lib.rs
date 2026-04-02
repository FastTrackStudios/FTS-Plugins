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

pub use detector::Detector;
pub use gain_curve::GainCurve;
pub use smoother::GainReductionSmoother;

/// Main compressor combining detection, gain curve, and smoothing.
pub struct ProC3Compressor {
    detector: Detector,
    gain_curve: GainCurve,
    smoother: GainReductionSmoother,
    /// Parallel compression mix: 1.0 = 100% wet, 0.0 = 100% dry
    fold: f64,
}

impl ProC3Compressor {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            detector: Detector::new(),
            gain_curve: GainCurve::new(),
            smoother: GainReductionSmoother::new(sample_rate),
            fold: 1.0,
        }
    }

    /// Process a sample through the full signal chain.
    pub fn process(&mut self, input: f64, channel: usize) -> f64 {
        // Step 1: Detect level using exponential perceptual weighting
        let level_db = self.detector.detect_level(input.abs());

        // Step 2: Compute gain reduction from level via threshold/ratio/knee
        let gr_linear = self.gain_curve.compute_gr(level_db);

        // Step 3: Smooth GR with Hermite cubic (with change detection fallback)
        let gr_smoothed = self.smoother.smooth_gr(gr_linear, channel);

        // Step 4: Apply compression and mix with dry
        let compressed = input * gr_smoothed;
        let output = compressed * self.fold + input * (1.0 - self.fold);

        output
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
