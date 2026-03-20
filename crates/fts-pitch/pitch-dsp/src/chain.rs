//! PitchChain — top-level processor with algorithm selection.
//!
//! Wraps all four pitch shifting algorithms behind the `Processor` trait
//! with runtime algorithm switching. Pitch is controlled via a semitone
//! slider (−24 to +24).

use fts_dsp::{AudioConfig, Processor};
use serde::{Deserialize, Serialize};

use crate::divider::{DivideRatio, FreqDivider};
use crate::granular::GranularShifter;
use crate::pll::{PllOctave, PllTracker, SubWaveform};
use crate::psola::PsolaShifter;

/// Which pitch shifting algorithm to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Algorithm {
    /// Analog-style frequency divider. 0 latency, synthy character.
    /// Only supports exact octave shifts (−12 or −24 semitones).
    /// Other values are clamped to the nearest octave.
    FreqDivider,
    /// PLL tracking oscillator. 0 latency, warmer sub.
    /// Only supports exact octave shifts (−12 or −24 semitones).
    /// Other values are clamped to the nearest octave.
    Pll,
    /// Granular fixed-ratio. ~1024 sample latency, natural tone.
    /// Supports arbitrary semitone values.
    Granular,
    /// PSOLA pitch-synchronous. ~2048 sample latency, highest quality.
    /// Supports arbitrary semitone values.
    Psola,
}

/// Convert semitones to pitch ratio: `2^(semitones / 12)`.
pub fn semitones_to_ratio(semitones: f64) -> f64 {
    (2.0f64).powf(semitones / 12.0)
}

/// Convert pitch ratio to semitones: `12 * log2(ratio)`.
pub fn ratio_to_semitones(ratio: f64) -> f64 {
    12.0 * ratio.log2()
}

/// Top-level pitch shifter with algorithm selection.
pub struct PitchChain {
    /// Active algorithm.
    pub algorithm: Algorithm,
    /// Pitch shift in semitones. Range: −24.0 to +24.0.
    /// 0 = no shift, −12 = octave down, +12 = octave up.
    pub semitones: f64,
    /// Dry/wet mix (0.0–1.0).
    pub mix: f64,

    // -- PLL-specific settings --
    /// PLL sub-oscillator waveform.
    pub pll_waveform: SubWaveform,

    // -- Granular-specific settings --
    /// Grain size in samples.
    pub grain_size: usize,

    divider: FreqDivider,
    pll: PllTracker,
    granular: GranularShifter,
    psola: PsolaShifter,
}

impl PitchChain {
    pub fn new() -> Self {
        Self {
            algorithm: Algorithm::FreqDivider,
            semitones: -12.0,
            mix: 1.0,
            pll_waveform: SubWaveform::Saw,
            grain_size: 1024,
            divider: FreqDivider::new(),
            pll: PllTracker::new(),
            granular: GranularShifter::new(),
            psola: PsolaShifter::new(),
        }
    }

    /// Latency introduced by the currently selected algorithm.
    pub fn latency(&self) -> usize {
        match self.algorithm {
            Algorithm::FreqDivider => self.divider.latency(),
            Algorithm::Pll => self.pll.latency(),
            Algorithm::Granular => self.granular.latency(),
            Algorithm::Psola => self.psola.latency(),
        }
    }

    /// Map semitones to the nearest octave division for divider/PLL.
    /// Returns (DivideRatio/PllOctave, direction).
    /// Divider/PLL only support down-shifting by exact octaves.
    fn nearest_octave(&self) -> (bool, bool) {
        // is_oct2: true if closer to −24 than −12.
        // is_down: true if semitones < −6 (clearly wants a sub).
        let is_down = self.semitones < -6.0;
        let is_oct2 = self.semitones < -18.0;
        (is_down, is_oct2)
    }

    /// Sync algorithm-specific parameters before processing.
    fn sync_params(&mut self) {
        let ratio = semitones_to_ratio(self.semitones.clamp(-24.0, 24.0));
        let (is_down, is_oct2) = self.nearest_octave();

        // Divider: only supports octave down. Bypass if shifting up.
        self.divider.ratio = if is_oct2 {
            DivideRatio::Oct2
        } else {
            DivideRatio::Oct1
        };
        self.divider.mix = if is_down { self.mix } else { 0.0 };

        // PLL: only supports octave down. Bypass if shifting up.
        self.pll.waveform = self.pll_waveform;
        self.pll.octave = if is_oct2 {
            PllOctave::Oct2
        } else {
            PllOctave::Oct1
        };
        self.pll.mix = if is_down { self.mix } else { 0.0 };

        // Granular: arbitrary ratio.
        self.granular.speed = ratio;
        self.granular.mix = self.mix;
        self.granular.grain_size = self.grain_size;

        // PSOLA: arbitrary ratio.
        self.psola.speed = ratio;
        self.psola.mix = self.mix;
    }
}

impl Default for PitchChain {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for PitchChain {
    fn reset(&mut self) {
        self.divider.reset();
        self.pll.reset();
        self.granular.reset();
        self.psola.reset();
    }

    fn update(&mut self, config: AudioConfig) {
        self.divider.update(config.sample_rate);
        self.pll.update(config.sample_rate);
        self.granular.update(config.sample_rate);
        self.psola.update(config.sample_rate);
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        self.sync_params();

        match self.algorithm {
            Algorithm::FreqDivider => {
                for s in left.iter_mut() {
                    *s = self.divider.tick(*s);
                }
            }
            Algorithm::Pll => {
                for s in left.iter_mut() {
                    *s = self.pll.tick(*s);
                }
            }
            Algorithm::Granular => {
                for s in left.iter_mut() {
                    *s = self.granular.tick(*s);
                }
            }
            Algorithm::Psola => {
                for s in left.iter_mut() {
                    *s = self.psola.tick(*s);
                }
            }
        }

        // Copy mono result to right channel.
        right.copy_from_slice(left);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn config() -> AudioConfig {
        AudioConfig {
            sample_rate: SR,
            max_buffer_size: 512,
        }
    }

    fn sine_block(freq: f64, offset: usize, len: usize) -> Vec<f64> {
        (offset..offset + len)
            .map(|i| (2.0 * PI * freq * i as f64 / SR).sin() * 0.5)
            .collect()
    }

    #[test]
    fn semitone_to_ratio_conversions() {
        assert!((semitones_to_ratio(0.0) - 1.0).abs() < 1e-10);
        assert!((semitones_to_ratio(-12.0) - 0.5).abs() < 1e-10);
        assert!((semitones_to_ratio(12.0) - 2.0).abs() < 1e-10);
        assert!((semitones_to_ratio(-24.0) - 0.25).abs() < 1e-10);
        assert!((semitones_to_ratio(24.0) - 4.0).abs() < 1e-10);
        // -7 semitones (perfect fifth down)
        assert!((semitones_to_ratio(-7.0) - 0.6674199270850172).abs() < 1e-10);
    }

    #[test]
    fn ratio_to_semitone_roundtrip() {
        for st in [
            -24.0, -12.0, -7.0, -5.0, -1.0, 0.0, 1.0, 5.0, 7.0, 12.0, 24.0,
        ] {
            let rt = ratio_to_semitones(semitones_to_ratio(st));
            assert!(
                (rt - st).abs() < 1e-10,
                "Roundtrip failed for {st}: got {rt}"
            );
        }
    }

    #[test]
    fn all_algorithms_produce_output() {
        for algo in [
            Algorithm::FreqDivider,
            Algorithm::Pll,
            Algorithm::Granular,
            Algorithm::Psola,
        ] {
            let mut chain = PitchChain::new();
            chain.algorithm = algo;
            chain.semitones = -12.0;
            chain.mix = 1.0;
            chain.update(config());
            chain.reset();

            let mut energy = 0.0;
            let blocks = 100;
            let block_size = 512;

            for b in 0..blocks {
                let mut left = sine_block(220.0, b * block_size, block_size);
                let mut right = left.clone();
                chain.process(&mut left, &mut right);

                if b > 10 {
                    energy += left.iter().map(|s| s * s).sum::<f64>();
                }
            }

            assert!(
                energy > 0.1,
                "{algo:?} should produce output: energy={energy}"
            );
        }
    }

    #[test]
    fn granular_various_semitones() {
        // Test that different semitone values produce different output.
        let semitone_values = [-12.0, -7.0, -5.0, -1.0, 0.0, 1.0, 5.0, 7.0, 12.0];
        let mut outputs = Vec::new();

        for &st in &semitone_values {
            let mut chain = PitchChain::new();
            chain.algorithm = Algorithm::Granular;
            chain.semitones = st;
            chain.mix = 1.0;
            chain.update(config());

            let mut all = Vec::new();
            for b in 0..50 {
                let mut left = sine_block(220.0, b * 512, 512);
                let mut right = left.clone();
                chain.process(&mut left, &mut right);
                all.extend_from_slice(&left);
            }
            outputs.push(all);
        }

        // Each distinct semitone setting should produce distinct output.
        for i in 0..outputs.len() {
            for j in (i + 1)..outputs.len() {
                let diff: f64 = outputs[i]
                    .iter()
                    .zip(outputs[j].iter())
                    .map(|(a, b)| (a - b).abs())
                    .sum::<f64>()
                    / outputs[i].len() as f64;
                assert!(
                    diff > 0.0001,
                    "Semitones {} and {} should differ: avg_diff={diff}",
                    semitone_values[i],
                    semitone_values[j]
                );
            }
        }
    }

    #[test]
    fn psola_various_semitones() {
        for &st in &[-12.0, -7.0, -3.0, 0.0, 3.0, 7.0, 12.0] {
            let mut chain = PitchChain::new();
            chain.algorithm = Algorithm::Psola;
            chain.semitones = st;
            chain.mix = 1.0;
            chain.update(config());

            for b in 0..100 {
                let mut left = sine_block(220.0, b * 512, 512);
                let mut right = left.clone();
                chain.process(&mut left, &mut right);

                for (i, s) in left.iter().enumerate() {
                    assert!(
                        s.is_finite(),
                        "PSOLA NaN at semitones={st}, block {b} sample {i}"
                    );
                }
            }
        }
    }

    #[test]
    fn divider_bypasses_on_positive_semitones() {
        let mut chain = PitchChain::new();
        chain.algorithm = Algorithm::FreqDivider;
        chain.semitones = 5.0; // Upshift — divider can't do this.
        chain.mix = 1.0;
        chain.update(config());

        // With mix set to 1.0 but divider forced to mix=0 (bypass),
        // output should equal input (dry passthrough).
        let input = sine_block(440.0, 0, 512);
        let mut left = input.clone();
        let mut right = input.clone();
        chain.process(&mut left, &mut right);

        for (i, (out, inp)) in left.iter().zip(input.iter()).enumerate() {
            assert!(
                (out - inp).abs() < 1e-10,
                "Divider should bypass on upshift at sample {i}"
            );
        }
    }

    #[test]
    fn algorithm_switching() {
        let mut chain = PitchChain::new();
        chain.semitones = -12.0;
        chain.update(config());
        chain.reset();

        for (idx, algo) in [
            Algorithm::FreqDivider,
            Algorithm::Pll,
            Algorithm::Granular,
            Algorithm::Psola,
        ]
        .iter()
        .enumerate()
        {
            chain.algorithm = *algo;
            let mut left = sine_block(220.0, idx * 512, 512);
            let mut right = left.clone();
            chain.process(&mut left, &mut right);

            for (i, s) in left.iter().enumerate() {
                assert!(s.is_finite(), "{algo:?} produced NaN at sample {i}");
            }
        }
    }

    #[test]
    fn no_nan_all_algorithms() {
        for algo in [
            Algorithm::FreqDivider,
            Algorithm::Pll,
            Algorithm::Granular,
            Algorithm::Psola,
        ] {
            let mut chain = PitchChain::new();
            chain.algorithm = algo;
            chain.semitones = -12.0;
            chain.update(config());

            for b in 0..200 {
                let mut left = sine_block(82.0, b * 512, 512);
                let mut right = left.clone();
                chain.process(&mut left, &mut right);

                for (i, s) in left.iter().enumerate() {
                    assert!(s.is_finite(), "{algo:?} NaN at block {b} sample {i}");
                }
            }
        }
    }

    #[test]
    fn different_algorithms_differ() {
        let mut outputs: Vec<Vec<f64>> = Vec::new();

        for algo in [
            Algorithm::FreqDivider,
            Algorithm::Pll,
            Algorithm::Granular,
            Algorithm::Psola,
        ] {
            let mut chain = PitchChain::new();
            chain.algorithm = algo;
            chain.semitones = -12.0;
            chain.update(config());

            let mut all = Vec::new();
            for b in 0..50 {
                let mut left = sine_block(220.0, b * 512, 512);
                let mut right = left.clone();
                chain.process(&mut left, &mut right);
                all.extend_from_slice(&left);
            }
            outputs.push(all);
        }

        for i in 0..outputs.len() {
            for j in (i + 1)..outputs.len() {
                let diff: f64 = outputs[i]
                    .iter()
                    .zip(outputs[j].iter())
                    .map(|(a, b)| (a - b).abs())
                    .sum::<f64>()
                    / outputs[i].len() as f64;
                assert!(
                    diff > 0.001,
                    "Algorithms {i} and {j} should differ: avg_diff={diff}"
                );
            }
        }
    }

    #[test]
    fn mix_zero_passes_dry() {
        for algo in [
            Algorithm::FreqDivider,
            Algorithm::Pll,
            Algorithm::Granular,
            Algorithm::Psola,
        ] {
            let mut chain = PitchChain::new();
            chain.algorithm = algo;
            chain.semitones = -12.0;
            chain.mix = 0.0;
            chain.update(config());

            let input = sine_block(440.0, 0, 512);
            let mut left = input.clone();
            let mut right = input.clone();
            chain.process(&mut left, &mut right);

            for (i, (out, inp)) in left.iter().zip(input.iter()).enumerate() {
                assert!(
                    (out - inp).abs() < 1e-10,
                    "{algo:?} mix=0 should pass dry at sample {i}: diff={}",
                    (out - inp).abs()
                );
            }
        }
    }

    #[test]
    fn latency_values() {
        let mut chain = PitchChain::new();
        chain.update(config());

        chain.algorithm = Algorithm::FreqDivider;
        assert_eq!(chain.latency(), 0);

        chain.algorithm = Algorithm::Pll;
        assert_eq!(chain.latency(), 0);

        chain.algorithm = Algorithm::Granular;
        assert_eq!(chain.latency(), 1024);

        chain.algorithm = Algorithm::Psola;
        assert!(chain.latency() > 0);
    }
}
