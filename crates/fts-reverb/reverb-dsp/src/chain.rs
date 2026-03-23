//! Reverb chain — top-level processor with algorithm dispatch,
//! pre/post processing, mix, and width.

use fts_dsp::biquad::{Biquad, FilterType};
use fts_dsp::delay_line::DelayLine;
use fts_dsp::{AudioConfig, Processor};

use crate::algorithm::{AlgorithmParams, AlgorithmType, ReverbAlgorithm};
use crate::algorithms;

/// Full reverb processing chain.
///
/// Signal flow: Input → Input HP/LP → Pre-Delay → Algorithm → Width → Mix
pub struct ReverbChain {
    // Algorithm
    algorithm: Box<dyn ReverbAlgorithm>,
    algorithm_type: AlgorithmType,
    variant: usize,

    // Pre-delay (up to 500ms)
    predelay: DelayLine,
    predelay_samples: usize,

    // Input conditioning
    input_hp: Biquad,
    input_lp: Biquad,

    // Algorithm params
    pub params: AlgorithmParams,

    // Global controls
    /// Pre-delay in milliseconds (0-500).
    pub predelay_ms: f64,
    /// Dry/wet mix (0.0 = fully dry, 1.0 = fully wet).
    pub mix: f64,
    /// Stereo width (0.0 = mono, 1.0 = normal, 2.0 = extra wide).
    pub width: f64,
    /// Input highpass frequency in Hz (20 = off).
    pub input_hp_freq: f64,
    /// Input lowpass frequency in Hz (20000 = off).
    pub input_lp_freq: f64,

    sample_rate: f64,
}

impl ReverbChain {
    pub fn new() -> Self {
        let sample_rate = 48000.0;
        let max_predelay = (sample_rate * 0.5) as usize; // 500ms

        Self {
            algorithm: algorithms::create(AlgorithmType::Room, 0, sample_rate),
            algorithm_type: AlgorithmType::Room,
            variant: 0,
            predelay: DelayLine::new(max_predelay + 1),
            predelay_samples: 0,
            input_hp: Biquad::new(),
            input_lp: Biquad::new(),
            params: AlgorithmParams::default(),
            predelay_ms: 0.0,
            mix: 0.5,
            width: 1.0,
            input_hp_freq: 20.0,
            input_lp_freq: 20000.0,
            sample_rate,
        }
    }

    /// Switch to a different algorithm type and/or variant. Resets algorithm state.
    pub fn set_algorithm(&mut self, algo: AlgorithmType) {
        self.set_algorithm_variant(algo, self.variant);
    }

    /// Switch to a specific algorithm type and variant.
    pub fn set_algorithm_variant(&mut self, algo: AlgorithmType, variant: usize) {
        let variant = variant.min(algo.variant_count().saturating_sub(1));
        if algo != self.algorithm_type || variant != self.variant {
            self.algorithm_type = algo;
            self.variant = variant;
            self.algorithm = algorithms::create(algo, variant, self.sample_rate);
            self.algorithm.set_params(&self.params);
        }
    }

    /// Set just the variant for the current algorithm type.
    pub fn set_variant(&mut self, variant: usize) {
        self.set_algorithm_variant(self.algorithm_type, variant);
    }

    /// Get the current algorithm type.
    pub fn algorithm_type(&self) -> AlgorithmType {
        self.algorithm_type
    }

    /// Get the current variant index.
    pub fn variant(&self) -> usize {
        self.variant
    }

    /// Update all algorithm parameters.
    pub fn update_params(&mut self) {
        self.algorithm.set_params(&self.params);
    }
}

impl Processor for ReverbChain {
    fn reset(&mut self) {
        self.algorithm.reset();
        self.predelay.clear();
        self.input_hp.reset();
        self.input_lp.reset();
    }

    fn update(&mut self, config: AudioConfig) {
        self.sample_rate = config.sample_rate;

        // Rebuild pre-delay if sample rate changed
        let max_predelay = (config.sample_rate * 0.5) as usize;
        self.predelay = DelayLine::new(max_predelay + 1);
        self.predelay_samples = (self.predelay_ms * 0.001 * config.sample_rate) as usize;

        // Update input filters
        self.input_hp.set(
            FilterType::Highpass,
            self.input_hp_freq.max(20.0),
            0.707,
            config.sample_rate,
        );
        self.input_lp.set(
            FilterType::Lowpass,
            self.input_lp_freq.min(20000.0),
            0.707,
            config.sample_rate,
        );

        // Update algorithm
        self.algorithm.set_sample_rate(config.sample_rate);
        self.algorithm.set_params(&self.params);
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        let n = left.len().min(right.len());

        // Update pre-delay samples (in case it changed)
        self.predelay_samples = (self.predelay_ms * 0.001 * self.sample_rate) as usize;

        for i in 0..n {
            let dry_l = left[i];
            let dry_r = right[i];

            // Input filtering
            let filt_l = self.input_lp.tick(self.input_hp.tick(dry_l, 0), 0);
            let filt_r = self.input_lp.tick(self.input_hp.tick(dry_r, 1), 1);

            // Pre-delay (mono summed for pre-delay, then split back)
            let (pd_l, pd_r) = if self.predelay_samples > 0 {
                self.predelay.write(filt_l);
                let delayed = self.predelay.read(self.predelay_samples);
                (delayed, filt_r) // Pre-delay on L, direct on R (or both)
            } else {
                (filt_l, filt_r)
            };

            // Algorithm processing (returns wet signal only)
            let (wet_l, wet_r) = self.algorithm.tick(pd_l, pd_r);

            // Stereo width (mid-side)
            let (final_l, final_r) = if (self.width - 1.0).abs() > 0.001 {
                let mid = (wet_l + wet_r) * 0.5;
                let side = (wet_l - wet_r) * 0.5;
                (mid + side * self.width, mid - side * self.width)
            } else {
                (wet_l, wet_r)
            };

            // Dry/wet mix
            left[i] = dry_l * (1.0 - self.mix) + final_l * self.mix;
            right[i] = dry_r * (1.0 - self.mix) + final_r * self.mix;
        }
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

    #[test]
    fn dry_wet_mix() {
        let mut c = ReverbChain::new();
        c.mix = 0.0;
        c.update(config());

        let mut l = vec![0.5; 512];
        let mut r = vec![0.5; 512];
        c.process(&mut l, &mut r);

        assert!((l[0] - 0.5).abs() < 1e-10, "Dry pass-through");
    }

    #[test]
    fn wet_produces_output() {
        let mut c = ReverbChain::new();
        c.mix = 1.0;
        c.update(config());

        // Send an impulse
        let n = 4800;
        let mut l: Vec<f64> = (0..n).map(|i| if i < 10 { 1.0 } else { 0.0 }).collect();
        let mut r = l.clone();

        c.process(&mut l, &mut r);

        // Wet signal should have energy after the impulse
        let late_energy: f64 = l[100..].iter().map(|x| x * x).sum();
        assert!(
            late_energy > 0.001,
            "Reverb should produce a tail: {late_energy}"
        );
    }

    #[test]
    fn all_algorithms_no_nan() {
        for &algo in AlgorithmType::ALL {
            for variant in 0..algo.variant_count() {
                let mut c = ReverbChain::new();
                c.set_algorithm_variant(algo, variant);
                c.mix = 1.0;
                c.update(config());

                let n = 4800; // 100ms
                let mut l: Vec<f64> = (0..n)
                    .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5)
                    .collect();
                let mut r = l.clone();

                c.process(&mut l, &mut r);

                let vname = algo.variant_name(variant);
                for (i, (&lv, &rv)) in l.iter().zip(r.iter()).enumerate() {
                    assert!(
                        lv.is_finite(),
                        "{} {}: L NaN/Inf at sample {i}: {lv}",
                        algo.name(),
                        vname,
                    );
                    assert!(
                        rv.is_finite(),
                        "{} {}: R NaN/Inf at sample {i}: {rv}",
                        algo.name(),
                        vname,
                    );
                }
            }
        }
    }

    #[test]
    fn algorithm_switching() {
        let mut c = ReverbChain::new();
        c.update(config());

        for &algo in AlgorithmType::ALL {
            for variant in 0..algo.variant_count() {
                c.set_algorithm_variant(algo, variant);
                assert_eq!(c.algorithm_type(), algo);
                assert_eq!(c.variant(), variant);

                let mut l = vec![0.1; 128];
                let mut r = vec![0.1; 128];
                c.process(&mut l, &mut r);
            }
        }
    }

    #[test]
    fn predelay_delays_signal() {
        let mut c = ReverbChain::new();
        c.mix = 1.0;
        c.predelay_ms = 10.0; // 10ms = 480 samples at 48kHz
        c.update(config());

        let n = 2400;
        let mut l: Vec<f64> = (0..n).map(|i| if i == 0 { 1.0 } else { 0.0 }).collect();
        let mut r = l.clone();

        c.process(&mut l, &mut r);

        // Signal should be near-zero for first ~480 samples on L
        let early_energy: f64 = l[..400].iter().map(|x| x * x).sum();
        let late_energy: f64 = l[400..].iter().map(|x| x * x).sum();

        // Late energy should be larger (delayed signal arrives)
        assert!(
            late_energy > early_energy,
            "Predelay should shift energy later: early={early_energy}, late={late_energy}"
        );
    }
}
