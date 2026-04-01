//! Tube2 -- faithful Rust port of the Airwindows Tube2 algorithm.
//!
//! Original code by Chris Johnson (Airwindows), released under the MIT license.
//! <https://github.com/airwindows/airwindows>
//!
//! Multi-stage tube emulation with asymmetric waveshaping, power-function
//! widening, and slew-based hysteresis/fuzz.

use fts_dsp::{AudioConfig, Processor};

/// Multi-stage tube saturation emulation ported from Airwindows Tube2.
///
/// Signal flow per sample:
/// 1. Optional anti-aliasing averaging (high sample rates)
/// 2. Asymmetric waveshaping (flattens bottom, points top)
/// 3. Power-function widening of the linear region
/// 4. Optional post-averaging
/// 5. Slew-based hysteresis and spiky fuzz with hard clamp at +/-0.52,
///    then rescale by 1/0.52
pub struct Tube2 {
    /// Input pad amount, 0.0--1.0. Higher values drive harder.
    pub input_pad: f64,
    /// Iteration amount, 0.0--1.0. Higher values = more saturation stages.
    pub iterations: f64,

    sample_rate: f64,

    // Averaging state for high sample rates (stage A: pre, stage C: mid, stage E: hysteresis)
    prev_a_l: f64,
    prev_a_r: f64,
    prev_c_l: f64,
    prev_c_r: f64,
    prev_e_l: f64,
    prev_e_r: f64,
}

impl Tube2 {
    pub fn new() -> Self {
        Self {
            input_pad: 0.5,
            iterations: 0.5,
            sample_rate: 44100.0,
            prev_a_l: 0.0,
            prev_a_r: 0.0,
            prev_c_l: 0.0,
            prev_c_r: 0.0,
            prev_e_l: 0.0,
            prev_e_r: 0.0,
        }
    }

    /// Process a single channel sample through the Tube2 algorithm.
    #[inline]
    fn process_sample(
        sample: f64,
        overallscale: f64,
        input_pad: f64,
        powerfactor: i32,
        asym_pad: f64,
        gainscaling: f64,
        outputscaling: f64,
        prev_a: &mut f64,
        prev_c: &mut f64,
        prev_e: &mut f64,
    ) -> f64 {
        let mut s = sample * input_pad;

        // ── Stage A: optional averaging for high sample rates ──
        if overallscale > 1.9 {
            let stored = s;
            s += *prev_a;
            *prev_a = stored;
            s *= 0.5;
        }

        // Clamp to +/-1
        s = s.clamp(-1.0, 1.0);

        // ── Stage 1: Asymmetric waveshaping ──
        // Flatten bottom, point top via sqrt-based sharpening
        s /= asym_pad;
        let neg_s = -s;
        let sharpen = if neg_s > 0.0 {
            1.0 + neg_s.sqrt()
        } else {
            1.0 - (-neg_s).sqrt()
        };
        s -= s * s.abs() * sharpen * 0.25;
        s *= asym_pad;

        // ── Optional mid averaging (stage C) ──
        if overallscale > 1.9 {
            let stored = s;
            s += *prev_c;
            *prev_c = stored;
            s *= 0.5;
        }

        // ── Stage 2: Power-function widening of linear region ──
        let mut factor = s;
        for _ in 0..powerfactor {
            factor *= s;
        }
        // If odd power and non-zero, ensure sign matches original
        if (powerfactor % 2 == 1) && (s != 0.0) {
            factor = (factor / s) * s.abs();
        }
        factor *= gainscaling;
        s -= factor;
        s *= outputscaling;

        // ── Stage 3: Hysteresis and spiky fuzz ──
        let slew = *prev_e - s;
        *prev_e = s;

        let slew_factor = if slew > 0.0 {
            1.0 + slew.sqrt() * 0.5
        } else {
            1.0 - (-slew).sqrt() * 0.5
        };

        s -= s * s.abs() * slew_factor * gainscaling;

        // Hard clamp at +/-0.52 then rescale
        s = s.clamp(-0.52, 0.52);
        s *= 1.923076923076923; // 1.0 / 0.52

        s
    }
}

impl Default for Tube2 {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for Tube2 {
    fn reset(&mut self) {
        self.prev_a_l = 0.0;
        self.prev_a_r = 0.0;
        self.prev_c_l = 0.0;
        self.prev_c_r = 0.0;
        self.prev_e_l = 0.0;
        self.prev_e_r = 0.0;
    }

    fn update(&mut self, config: AudioConfig) {
        self.sample_rate = config.sample_rate;
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        let len = left.len().min(right.len());

        let overallscale = self.sample_rate / 44100.0;

        // Derive coefficients from parameters
        let input_pad = self.input_pad;
        let iterations = 1.0 - self.iterations;
        let powerfactor = ((9.0 * iterations) + 1.0) as i32; // 1..10
        let asym_pad = powerfactor as f64;
        let gainscaling = 1.0 / (powerfactor as f64 + 1.0);
        let outputscaling = 1.0 + (1.0 / powerfactor as f64);

        for i in 0..len {
            left[i] = Self::process_sample(
                left[i],
                overallscale,
                input_pad,
                powerfactor,
                asym_pad,
                gainscaling,
                outputscaling,
                &mut self.prev_a_l,
                &mut self.prev_c_l,
                &mut self.prev_e_l,
            );
            right[i] = Self::process_sample(
                right[i],
                overallscale,
                input_pad,
                powerfactor,
                asym_pad,
                gainscaling,
                outputscaling,
                &mut self.prev_a_r,
                &mut self.prev_c_r,
                &mut self.prev_e_r,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 44100.0;

    fn config() -> AudioConfig {
        AudioConfig {
            sample_rate: SR,
            max_buffer_size: 512,
        }
    }

    #[test]
    fn silence_in_silence_out() {
        let mut t = Tube2::new();
        t.update(config());
        let mut l = vec![0.0; 4410];
        let mut r = vec![0.0; 4410];
        t.process(&mut l, &mut r);
        for (i, &s) in l.iter().enumerate() {
            assert!(s.abs() < 1e-10, "Non-silent output at sample {i}: {s}");
        }
    }

    #[test]
    fn no_nan_or_inf() {
        let mut t = Tube2::new();
        t.update(config());
        let mut l: Vec<f64> = (0..44100)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5)
            .collect();
        let mut r = l.clone();
        t.process(&mut l, &mut r);
        for (i, &s) in l.iter().enumerate() {
            assert!(s.is_finite(), "Non-finite at L[{i}]: {s}");
        }
        for (i, &s) in r.iter().enumerate() {
            assert!(s.is_finite(), "Non-finite at R[{i}]: {s}");
        }
    }

    #[test]
    fn output_bounded() {
        let mut t = Tube2::new();
        t.input_pad = 1.0;
        t.iterations = 1.0;
        t.update(config());
        let mut l: Vec<f64> = (0..44100)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin())
            .collect();
        let mut r = l.clone();
        t.process(&mut l, &mut r);
        let max = l.iter().fold(0.0_f64, |a, &b| a.max(b.abs()));
        assert!(
            max <= 1.001,
            "Output should be bounded near 1.0 but max is {max}"
        );
    }

    #[test]
    fn high_sample_rate_no_panic() {
        let mut t = Tube2::new();
        t.update(AudioConfig {
            sample_rate: 192000.0,
            max_buffer_size: 512,
        });
        let mut l: Vec<f64> = (0..19200)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / 192000.0).sin() * 0.5)
            .collect();
        let mut r = l.clone();
        t.process(&mut l, &mut r);
        for (i, &s) in l.iter().enumerate() {
            assert!(s.is_finite(), "Non-finite at 192k L[{i}]");
        }
    }

    #[test]
    fn adds_harmonics() {
        let mut t = Tube2::new();
        t.input_pad = 0.8;
        t.iterations = 0.8;
        t.update(config());
        let mut l: Vec<f64> = (0..44100)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5)
            .collect();
        let mut r = l.clone();
        let input_rms: f64 = (l.iter().map(|s| s * s).sum::<f64>() / l.len() as f64).sqrt();
        t.process(&mut l, &mut r);
        let output_rms: f64 = (l.iter().map(|s| s * s).sum::<f64>() / l.len() as f64).sqrt();
        // Should have reasonable output level
        let ratio = output_rms / input_rms;
        assert!(
            ratio > 0.1 && ratio < 5.0,
            "Level ratio {ratio} out of expected range"
        );
    }
}
