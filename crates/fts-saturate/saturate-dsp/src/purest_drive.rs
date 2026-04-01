//! PurestDrive — adaptive saturation.
//!
//! Faithful port of the Airwindows PurestDrive algorithm by Chris Johnson,
//! released under the MIT license.
//!
//! The algorithm uses the previous sample to adaptively blend between dry
//! and sine-saturated signal. When the previous sample was quiet or had
//! opposite polarity, less saturation is applied, preserving transients
//! and low-level detail.

use fts_dsp::{AudioConfig, Processor};

/// Adaptive saturation based on Airwindows PurestDrive.
///
/// Uses the previous sample to control how much sine-waveshaper saturation
/// is blended in, producing smooth, program-dependent distortion.
pub struct PurestDrive {
    /// Drive intensity (0.0 = clean, 1.0 = full saturation).
    pub intensity: f64,

    previous_l: f64,
    previous_r: f64,
}

impl PurestDrive {
    pub fn new() -> Self {
        Self {
            intensity: 0.0,
            previous_l: 0.0,
            previous_r: 0.0,
        }
    }
}

impl Default for PurestDrive {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for PurestDrive {
    fn reset(&mut self) {
        self.previous_l = 0.0;
        self.previous_r = 0.0;
    }

    fn update(&mut self, _config: AudioConfig) {
        // No sample-rate-dependent coefficients.
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        let intensity = self.intensity;

        for sample in left.iter_mut() {
            let dry = *sample;
            let saturated = dry.sin();
            let apply = ((self.previous_l + saturated).abs() / 2.0) * intensity;
            *sample = dry * (1.0 - apply) + saturated * apply;
            self.previous_l = dry.sin();
        }

        for sample in right.iter_mut() {
            let dry = *sample;
            let saturated = dry.sin();
            let apply = ((self.previous_r + saturated).abs() / 2.0) * intensity;
            *sample = dry * (1.0 - apply) + saturated * apply;
            self.previous_r = dry.sin();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_stays_silent() {
        let mut drive = PurestDrive::new();
        drive.intensity = 1.0;
        let mut left = [0.0; 64];
        let mut right = [0.0; 64];
        drive.process(&mut left, &mut right);
        assert!(left.iter().all(|&s| s == 0.0));
        assert!(right.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn zero_intensity_is_passthrough() {
        let mut drive = PurestDrive::new();
        drive.intensity = 0.0;
        let original: Vec<f64> = (0..64).map(|i| (i as f64 * 0.1).sin()).collect();
        let mut left: Vec<f64> = original.clone();
        let mut right = vec![0.0; 64];
        drive.process(&mut left, &mut right);
        for (a, b) in left.iter().zip(original.iter()) {
            assert!(
                (a - b).abs() < 1e-15,
                "expected passthrough at zero intensity"
            );
        }
    }

    #[test]
    fn full_intensity_applies_saturation() {
        let mut drive = PurestDrive::new();
        drive.intensity = 1.0;
        let mut left = [0.8; 8];
        let mut right = [0.0; 8];
        let original = left;
        drive.process(&mut left, &mut right);
        // After the first sample (where previous is 0), subsequent samples
        // should differ from the original due to saturation.
        assert!(
            left[1..]
                .iter()
                .zip(original[1..].iter())
                .any(|(a, b)| (a - b).abs() > 1e-6),
            "expected saturation to alter signal"
        );
    }
}
