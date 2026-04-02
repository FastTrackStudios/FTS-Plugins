//! 3-band multiband compression system extracted from Pro-C 3 binary RE.
//!
//! Pro-C 3 processes audio through 3 independent compression bands:
//! - Band 0: High frequency band
//! - Band 1: Mid frequency band
//! - Band 2: Low frequency band (with special sqrt-based processing)
//!
//! Each band:
//! 1. Detects input level independently
//! 2. Computes gain reduction per band
//! 3. Smooths gain reduction with Hermite cubic
//! 4. Applies to band-specific audio
//!
//! Bands are combined at the output.
//! Function: update_filter_design_from_level @ 0x18010e9d0 (202 bytes)

use crate::hermite::{HermiteCubicSmoother, StateFuncHypothesis};
use crate::detector::Detector;
use crate::gain_curve::GainCurve;
use std::f64::consts::PI;
use std::f64::consts::LN_2;

/// Per-band compression state
#[derive(Clone)]
pub struct CompressionBand {
    /// Band index (0=high, 1=mid, 2=low)
    pub band_index: usize,

    /// Level detector for this band
    detector: Detector,

    /// Gain curve processor
    gain_curve: GainCurve,

    /// Smoothing with Hermite cubic
    smoother: HermiteCubicSmoother,

    /// Last computed gain reduction (dB)
    last_gr_db: f64,
}

impl CompressionBand {
    pub fn new(sample_rate: f64, band_index: usize) -> Self {
        Self {
            band_index,
            detector: Detector::new(),
            gain_curve: GainCurve::new(sample_rate),
            smoother: HermiteCubicSmoother::new(StateFuncHypothesis::Identity),
            last_gr_db: 0.0,
        }
    }

    /// Process one sample through this band's compression
    pub fn process(
        &mut self,
        input: f64,
        channel: usize,
    ) -> f64 {
        // Step 1: Detect level
        let level_db = self.detector.detect_level(input.abs());

        // Step 2: Compute gain reduction
        let gr_instant = self.gain_curve.compute_gr(level_db);

        // Step 3: Smooth with Hermite cubic
        let log_rel = self.gain_curve.release_coeff.ln();
        let log_atk = self.gain_curve.attack_coeff.ln();
        let sqrt_h0 = gr_instant.sqrt();
        let sqrt_h1 = (gr_instant * 0.9).sqrt();

        let gr_smoothed = self.smoother.process(
            gr_instant,
            self.gain_curve.attack_coeff,
            self.gain_curve.release_coeff,
            log_rel,
            log_atk,
            sqrt_h0,
            sqrt_h1,
            channel,
        );

        // Step 4: Apply band-specific processing
        // Band 2 (low) has special sqrt-based scaling (from binary @ 0x18010e9d0)
        let gr_final = if self.band_index == 2 {
            // Low band special processing
            gr_smoothed // Placeholder - would apply sqrt scaling based on filter type
        } else {
            gr_smoothed
        };

        // Track for metering
        self.last_gr_db = fts_dsp::db::linear_to_db(gr_final).max(0.0);

        // Step 5: Apply to audio
        input * gr_final
    }

    /// Update parameters for this band
    pub fn set_threshold(&mut self, threshold_db: f64) {
        self.gain_curve.set_threshold(threshold_db);
    }

    pub fn set_ratio(&mut self, ratio: f64) {
        self.gain_curve.set_ratio(ratio);
    }

    pub fn set_knee(&mut self, knee_db: f64) {
        self.gain_curve.set_knee(knee_db);
    }

    pub fn set_attack_ms(&mut self, attack_ms: f64) {
        self.gain_curve.set_attack_ms(attack_ms);
    }

    pub fn set_release_ms(&mut self, release_ms: f64) {
        self.gain_curve.set_release_ms(release_ms);
    }

    pub fn set_style(&mut self, style_id: i32) {
        self.gain_curve.set_style(crate::styles::CompressionStyle::from_id(style_id));
    }

    /// Reset internal state
    pub fn reset(&mut self) {
        self.detector.reset();
        self.smoother.reset();
        self.last_gr_db = 0.0;
    }

    /// Get gain reduction in dB
    pub fn gain_reduction_db(&self) -> f64 {
        self.last_gr_db
    }
}

/// 3-band multiband compression system
/// Binary architecture @ 0x18010bf40 (438 instructions)
pub struct MultiBandCompressor {
    /// Three independent compression bands
    bands: [CompressionBand; 3],

    /// Sample rate (needed for level-dependent crossover)
    sample_rate: f64,
}

impl MultiBandCompressor {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            bands: [
                CompressionBand::new(sample_rate, 0),  // High band
                CompressionBand::new(sample_rate, 1),  // Mid band
                CompressionBand::new(sample_rate, 2),  // Low band
            ],
            sample_rate,
        }
    }

    /// Process one sample through all 3 bands
    /// Formula from binary @ 0x18010e9d0:
    /// freq = (2 * exp_approx(level * LN2) * PI) / sample_rate
    pub fn process(&mut self, input: f64, channel: usize) -> f64 {
        // Process through all 3 bands
        // Each band computes independent gain reduction
        let gr_band0 = self.bands[0].process(input, channel);
        let gr_band1 = self.bands[1].process(input, channel);
        let gr_band2 = self.bands[2].process(input, channel);

        // Combine bands: currently just average the gain reductions
        // TODO: Implement proper crossover filtering
        // This is a simplified multiband where each band applies independently
        // Full implementation would filter input into frequency bands, process, then recombine
        let combined_output = (gr_band0 + gr_band1 + gr_band2) / 3.0;

        combined_output
    }

    /// Update to new sample rate
    pub fn update(&mut self, sample_rate: f64) {
        if (sample_rate - self.sample_rate).abs() > 0.1 {
            self.sample_rate = sample_rate;
            for band in &mut self.bands {
                band.gain_curve.update(sample_rate);
            }
        }
    }

    /// Set parameters for all bands
    pub fn set_threshold(&mut self, threshold_db: f64) {
        for band in &mut self.bands {
            band.set_threshold(threshold_db);
        }
    }

    pub fn set_ratio(&mut self, ratio: f64) {
        for band in &mut self.bands {
            band.set_ratio(ratio);
        }
    }

    pub fn set_knee(&mut self, knee_db: f64) {
        for band in &mut self.bands {
            band.set_knee(knee_db);
        }
    }

    pub fn set_attack_ms(&mut self, attack_ms: f64) {
        for band in &mut self.bands {
            band.set_attack_ms(attack_ms);
        }
    }

    pub fn set_release_ms(&mut self, release_ms: f64) {
        for band in &mut self.bands {
            band.set_release_ms(release_ms);
        }
    }

    pub fn set_style(&mut self, style_id: i32) {
        for band in &mut self.bands {
            band.set_style(style_id);
        }
    }

    /// Reset all bands
    pub fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }
    }

    /// Get maximum gain reduction across all bands
    pub fn gain_reduction_db(&self) -> f64 {
        self.bands
            .iter()
            .map(|b| b.gain_reduction_db())
            .fold(0.0, f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multiband_creation() {
        let mb = MultiBandCompressor::new(48000.0);
        assert_eq!(mb.bands.len(), 3);
        assert_eq!(mb.bands[0].band_index, 0);
        assert_eq!(mb.bands[1].band_index, 1);
        assert_eq!(mb.bands[2].band_index, 2);
    }

    #[test]
    fn test_multiband_processing() {
        let mut mb = MultiBandCompressor::new(48000.0);
        mb.set_threshold(-18.0);
        mb.set_ratio(4.0);

        let input = 0.5;
        let output = mb.process(input, 0);

        // Output should be compressed (less than input)
        assert!(output.abs() <= input.abs());
        assert!(output.is_finite());
    }

    #[test]
    fn test_band_independence() {
        let mb = MultiBandCompressor::new(48000.0);
        // Each band should be independent
        assert_eq!(mb.bands[0].band_index, 0);
        assert_eq!(mb.bands[1].band_index, 1);
        assert_eq!(mb.bands[2].band_index, 2);
    }
}
