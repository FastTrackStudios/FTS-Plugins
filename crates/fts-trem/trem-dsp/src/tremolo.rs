//! Core tremolo engine — amplitude modulation via modulator output.
//!
//! Supports harmonic tremolo (split into low/high bands, modulate
//! out of phase) and standard tremolo.

use fts_dsp::biquad::{Biquad, FilterType};

/// Tremolo shape applied to the raw modulator output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TremShape {
    /// Use the modulator output directly (pattern/LFO shaped).
    Pattern,
    /// Sine wave at the modulator rate.
    Sine,
    /// Triangle wave.
    Triangle,
    /// Square wave with adjustable pulse width.
    Square,
}

/// Tremolo mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TremMode {
    /// Standard tremolo — same modulation on both channels.
    Mono,
    /// Stereo tremolo — L/R modulated with phase offset.
    Stereo,
    /// Harmonic tremolo — low and high bands modulated out of phase.
    Harmonic,
}

/// Single-channel tremolo processor.
///
/// Takes a modulation value (0..1) and applies it as amplitude modulation.
pub struct Tremolo {
    /// Modulation depth (0..1). 0 = no effect, 1 = full depth.
    pub depth: f64,
    /// Tremolo mode.
    pub mode: TremMode,
    /// Crossover frequency for harmonic tremolo (Hz).
    pub crossover_freq: f64,

    // Harmonic tremolo filters
    lp: Biquad,
    hp: Biquad,

    sample_rate: f64,
}

impl Tremolo {
    pub fn new() -> Self {
        Self {
            depth: 0.5,
            mode: TremMode::Mono,
            crossover_freq: 800.0,
            lp: Biquad::new(),
            hp: Biquad::new(),
            sample_rate: 48000.0,
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.lp
            .set(FilterType::Lowpass, self.crossover_freq, 0.707, sample_rate);
        self.hp.set(
            FilterType::Highpass,
            self.crossover_freq,
            0.707,
            sample_rate,
        );
    }

    /// Apply tremolo to a sample given the modulation value (0..1).
    ///
    /// `mod_val` is the primary modulation, `mod_val_inv` is the inverted
    /// modulation for harmonic tremolo's complementary band.
    #[inline]
    pub fn tick(&mut self, sample: f64, mod_val: f64, ch: usize) -> f64 {
        // Convert 0..1 modulation to gain: at depth=1, gain ranges 0..1
        // at depth=0, gain is always 1 (no modulation)
        let gain = 1.0 - self.depth * (1.0 - mod_val);
        let gain_inv = 1.0 - self.depth * mod_val;

        match self.mode {
            TremMode::Mono | TremMode::Stereo => sample * gain,
            TremMode::Harmonic => {
                let low = self.lp.tick(sample, ch);
                let high = self.hp.tick(sample, ch);
                low * gain + high * gain_inv
            }
        }
    }

    pub fn reset(&mut self) {
        self.lp.reset();
        self.hp.reset();
    }
}

impl Default for Tremolo {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    #[test]
    fn no_depth_passes_through() {
        let mut t = Tremolo::new();
        t.depth = 0.0;
        t.update(SR);

        let input = 0.75;
        let out = t.tick(input, 0.5, 0);
        assert!(
            (out - input).abs() < 1e-10,
            "Zero depth should pass through: {out}"
        );
    }

    #[test]
    fn full_depth_at_zero_mod_silences() {
        let mut t = Tremolo::new();
        t.depth = 1.0;
        t.update(SR);

        let out = t.tick(1.0, 0.0, 0);
        assert!(
            out.abs() < 1e-10,
            "Full depth at mod=0 should silence: {out}"
        );
    }

    #[test]
    fn full_depth_at_one_mod_passes() {
        let mut t = Tremolo::new();
        t.depth = 1.0;
        t.update(SR);

        let out = t.tick(1.0, 1.0, 0);
        assert!(
            (out - 1.0).abs() < 1e-10,
            "Full depth at mod=1 should pass: {out}"
        );
    }

    #[test]
    fn harmonic_tremolo_produces_output() {
        let mut t = Tremolo::new();
        t.depth = 1.0;
        t.mode = TremMode::Harmonic;
        t.crossover_freq = 800.0;
        t.update(SR);

        let mut has_output = false;
        for i in 0..4800 {
            let s = (2.0 * PI * 440.0 * i as f64 / SR).sin();
            let mod_val = (2.0 * PI * 5.0 * i as f64 / SR).sin() * 0.5 + 0.5;
            let out = t.tick(s, mod_val, 0);
            if out.abs() > 0.01 {
                has_output = true;
            }
            assert!(out.is_finite(), "NaN at sample {i}");
        }
        assert!(has_output, "Harmonic tremolo should produce output");
    }

    #[test]
    fn modulation_range() {
        let mut t = Tremolo::new();
        t.depth = 0.5;
        t.update(SR);

        // At mod=0.5, gain = 1 - 0.5 * (1 - 0.5) = 0.75
        let out = t.tick(1.0, 0.5, 0);
        assert!((out - 0.75).abs() < 1e-10, "Half depth at mid mod: {out}");
    }
}
