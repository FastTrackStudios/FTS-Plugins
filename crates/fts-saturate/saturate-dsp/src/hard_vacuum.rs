//! HardVacuum — multi-stage tube amp emulation with slew-dependent asymmetric distortion.
//!
//! Faithful port of the Airwindows HardVacuum algorithm by Chris Johnson,
//! released under the MIT license.
//! <https://github.com/airwindows/airwindows>
//!
//! The algorithm computes a slew-based skew from sample-to-sample differences,
//! then runs through a multi-stage sine-waveshaper loop with asymmetric
//! positive/negative drive controlled by the warmth parameter. This produces
//! rich even-harmonic tube-like distortion whose character changes with the
//! signal's slew rate.

use fts_dsp::{AudioConfig, Processor};

const HALF_PI: f64 = std::f64::consts::FRAC_PI_2; // 1.57079633
const PI: f64 = std::f64::consts::PI; // 3.1415926
const DRIVE_SCALE: f64 = 1.557079633; // pi/2 * ~0.991, per original

/// Multi-stage tube amp emulation based on Airwindows HardVacuum.
///
/// Uses slew-dependent asymmetric distortion to model tube amplifier
/// nonlinearities. Higher `multistage` values cascade more distortion
/// stages; `warmth` introduces even-harmonic asymmetry; `aura` controls
/// the slew-dependent character.
pub struct HardVacuum {
    /// Drive / number of distortion stages (0.0--1.0, maps to 0--2+ stages).
    pub multistage: f64,
    /// Even-harmonic asymmetry amount (0.0--1.0).
    pub warmth: f64,
    /// Slew-dependent distortion character (0.0--1.0).
    pub aura: f64,
    /// Output level (0.0--1.0).
    pub output: f64,
    /// Dry/wet mix (0.0 = fully dry, 1.0 = fully wet).
    pub mix: f64,

    last_sample_l: f64,
    last_sample_r: f64,
}

impl HardVacuum {
    pub fn new() -> Self {
        Self {
            multistage: 0.5,
            warmth: 0.5,
            aura: 0.5,
            output: 1.0,
            mix: 1.0,
            last_sample_l: 0.0,
            last_sample_r: 0.0,
        }
    }

    /// Process a single channel sample given its last-sample state.
    /// Returns `(output_sample, new_last_sample)`.
    #[inline]
    fn process_sample(
        input: f64,
        last_sample: f64,
        multistage: f64,
        warmth: f64,
        inv_warmth: f64,
        aura_scaled: f64,
        out: f64,
        wet: f64,
    ) -> (f64, f64) {
        let dry = input;
        let mut sample = input;

        // Compute slew-based skew
        let slew = sample - last_sample;
        let bridge = slew.abs().min(PI).sin();
        let mut skew = if slew > 0.0 {
            bridge * aura_scaled
        } else {
            -bridge * aura_scaled
        };
        skew *= sample;
        skew *= DRIVE_SCALE;

        // Multi-stage distortion loop
        let mut countdown = multistage;
        while countdown > 0.0 {
            let drive = if countdown > 1.0 {
                DRIVE_SCALE
            } else {
                countdown * (1.0 + (0.557079633 * inv_warmth))
            };
            let positive = drive - warmth;
            let negative = drive + warmth;

            let mut bridge_rect = sample.abs();
            bridge_rect += skew;
            bridge_rect = bridge_rect.min(HALF_PI).sin();
            bridge_rect *= drive;
            bridge_rect += skew;
            bridge_rect = bridge_rect.min(HALF_PI).sin();

            if sample > 0.0 {
                sample = (sample * (1.0 - positive + skew)) + (bridge_rect * (positive + skew));
            } else {
                sample = (sample * (1.0 - negative + skew)) - (bridge_rect * (negative + skew));
            }

            countdown -= 1.0;
        }

        if out != 1.0 {
            sample *= out;
        }
        if wet != 1.0 {
            sample = (sample * wet) + (dry * (1.0 - wet));
        }

        (sample, input)
    }
}

impl Default for HardVacuum {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for HardVacuum {
    fn reset(&mut self) {
        self.last_sample_l = 0.0;
        self.last_sample_r = 0.0;
    }

    fn update(&mut self, _config: AudioConfig) {
        // No sample-rate-dependent coefficients.
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        // Map parameters exactly as the original algorithm.
        let mut multistage = self.multistage * 2.0;
        if multistage > 1.0 {
            multistage *= multistage;
        }
        let warmth_scaled = self.warmth / HALF_PI;
        let inv_warmth = 1.0 - self.warmth;
        let aura_scaled = self.aura * PI;
        let out = self.output;
        let wet = self.mix;

        for sample in left.iter_mut() {
            let (result, new_last) = Self::process_sample(
                *sample,
                self.last_sample_l,
                multistage,
                warmth_scaled,
                inv_warmth,
                aura_scaled,
                out,
                wet,
            );
            *sample = result;
            self.last_sample_l = new_last;
        }

        for sample in right.iter_mut() {
            let (result, new_last) = Self::process_sample(
                *sample,
                self.last_sample_r,
                multistage,
                warmth_scaled,
                inv_warmth,
                aura_scaled,
                out,
                wet,
            );
            *sample = result;
            self.last_sample_r = new_last;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_stays_silent() {
        let mut hv = HardVacuum::new();
        let mut left = [0.0; 64];
        let mut right = [0.0; 64];
        hv.process(&mut left, &mut right);
        assert!(left.iter().all(|&s| s == 0.0));
        assert!(right.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn zero_mix_is_passthrough() {
        let mut hv = HardVacuum::new();
        hv.mix = 0.0;
        let original: Vec<f64> = (0..64).map(|i| (i as f64 * 0.1).sin() * 0.5).collect();
        let mut left = original.clone();
        let mut right = original.clone();
        hv.process(&mut left, &mut right);
        for (a, b) in left.iter().zip(original.iter()) {
            assert!((a - b).abs() < 1e-12, "expected passthrough at zero mix");
        }
    }

    #[test]
    fn full_drive_applies_distortion() {
        let mut hv = HardVacuum::new();
        hv.multistage = 1.0;
        hv.warmth = 0.5;
        hv.aura = 0.5;
        let original: Vec<f64> = (0..64).map(|i| (i as f64 * 0.1).sin() * 0.8).collect();
        let mut left = original.clone();
        let mut right = vec![0.0; 64];
        hv.process(&mut left, &mut right);
        assert!(
            left.iter()
                .zip(original.iter())
                .any(|(a, b)| (a - b).abs() > 1e-6),
            "expected distortion to alter signal"
        );
    }

    #[test]
    fn reset_clears_state() {
        let mut hv = HardVacuum::new();
        let mut left = [0.5; 16];
        let mut right = [0.5; 16];
        hv.process(&mut left, &mut right);
        assert!(hv.last_sample_l != 0.0);
        hv.reset();
        assert_eq!(hv.last_sample_l, 0.0);
        assert_eq!(hv.last_sample_r, 0.0);
    }

    #[test]
    fn stereo_channels_are_independent() {
        let mut hv = HardVacuum::new();
        hv.multistage = 0.8;
        hv.aura = 0.7;
        let mut left: Vec<f64> = (0..32).map(|i| (i as f64 * 0.2).sin() * 0.6).collect();
        let mut right = vec![0.0; 32];
        hv.process(&mut left, &mut right);
        // Right channel (silence) should remain silent.
        assert!(right.iter().all(|&s| s == 0.0));
        // Left channel should be modified.
        assert!(left.iter().any(|&s| s != 0.0));
    }
}
