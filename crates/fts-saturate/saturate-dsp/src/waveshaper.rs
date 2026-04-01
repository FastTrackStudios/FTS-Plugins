//! Simple waveshaping saturation algorithms.
//!
//! Each function takes a sample and drive amount (0–1) and returns the shaped sample.
//! These are used for the non-tape styles where the saturation is primarily
//! a memoryless nonlinearity (no flutter, bias, head bump, etc.).

use std::f64::consts::PI;

// ── Waveshaping curves ─────────────────────────────────────────────

/// Soft saturation via tanh. Clean and musical.
#[inline]
pub fn tanh_sat(x: f64, drive: f64) -> f64 {
    let driven = x * (1.0 + drive * 7.0);
    driven.tanh()
}

/// Asymmetric tube-style saturation.
/// Positive half clips softer than negative — adds even harmonics.
#[inline]
pub fn tube_asym(x: f64, drive: f64) -> f64 {
    let d = 1.0 + drive * 7.0;
    let driven = x * d;
    if driven >= 0.0 {
        // Soft clip positive: x / (1 + x)
        let a = driven;
        a / (1.0 + a)
    } else {
        // Harder clip negative: tanh
        driven.tanh()
    }
}

/// Hard clip with slight rounding at the knee.
#[inline]
pub fn hard_clip(x: f64, drive: f64) -> f64 {
    let driven = x * (1.0 + drive * 7.0);
    driven.clamp(-1.0, 1.0)
}

/// Foldback distortion — signal folds back at ±1 threshold.
#[inline]
pub fn foldback(x: f64, drive: f64) -> f64 {
    let mut s = x * (1.0 + drive * 15.0);
    // Fold back repeatedly
    for _ in 0..4 {
        if s > 1.0 {
            s = 2.0 - s;
        } else if s < -1.0 {
            s = -2.0 - s;
        } else {
            break;
        }
    }
    s.clamp(-1.0, 1.0)
}

/// Rectify — full-wave rectification blended with original, then soft clipped.
#[inline]
pub fn rectify(x: f64, drive: f64) -> f64 {
    let rect = x.abs();
    let blend = x * (1.0 - drive) + rect * drive;
    let driven = blend * (1.0 + drive * 3.0);
    driven.tanh()
}

/// Bit crush — reduce effective bit depth.
#[inline]
pub fn bit_crush(x: f64, drive: f64) -> f64 {
    // drive 0 = 16 bit, drive 1 = ~2 bit
    let bits = 16.0 - drive * 14.0;
    let levels = 2.0_f64.powf(bits);
    let quantized = (x * levels).round() / levels;
    quantized
}

/// Smudge — sine waveshaper, creates odd harmonics with a soft, smeared character.
#[inline]
pub fn smudge(x: f64, drive: f64) -> f64 {
    let driven = x * (1.0 + drive * 3.0);
    (driven * PI * 0.5).sin()
}

/// Transformer — subtle asymmetric soft saturation with 2nd/3rd harmonic blend.
#[inline]
pub fn transformer(x: f64, drive: f64) -> f64 {
    let d = 1.0 + drive * 4.0;
    let driven = x * d;
    // Chebyshev-ish: blend of x, x^2 (even harmonics), x^3 (odd harmonics)
    let asym = drive * 0.3; // asymmetry amount
    let out = driven - asym * driven * driven - (driven * driven * driven) / 6.0;
    out.clamp(-1.2, 1.2) / 1.2
}

// ── Waveshaper processor ───────────────────────────────────────────

use crate::style::{Category, Style};
use fts_dsp::{AudioConfig, Processor};

/// Waveshaping curve selection — maps from style category+variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    TanhSubtle,
    TanhClean,
    TanhWarm,
    TubeSubtle,
    TubeClean,
    TubeWarm,
    TubeHot,
    TubeBroken,
    HardClipClean,
    HardClipCrunch,
    Transformer,
    TransformerWarm,
    TransformerColorful,
    Foldback,
    Rectify,
    BitCrush,
    Smudge,
}

impl Curve {
    /// Map a Style to a waveshaping curve.
    /// Only used for the FX category — other categories use dedicated backends.
    pub fn from_style(style: &Style) -> Self {
        match style.category {
            Category::FX => match style.variant {
                0 => Self::Smudge,
                1 => Self::Foldback,
                2 => Self::Rectify,
                _ => Self::BitCrush,
            },
            // Fallback — shouldn't be called for other categories
            _ => Self::TanhWarm,
        }
    }

    /// Apply the waveshaping curve to a sample.
    #[inline]
    pub fn apply(self, x: f64, drive: f64) -> f64 {
        match self {
            Self::TanhSubtle => tanh_sat(x, drive * 0.3),
            Self::TanhClean => tanh_sat(x, drive * 0.6),
            Self::TanhWarm => tanh_sat(x, drive),
            Self::TubeSubtle => tube_asym(x, drive * 0.25),
            Self::TubeClean => tube_asym(x, drive * 0.5),
            Self::TubeWarm => tube_asym(x, drive * 0.75),
            Self::TubeHot => tube_asym(x, drive),
            Self::TubeBroken => {
                // Double-stage: tube into hard clip
                let stage1 = tube_asym(x, drive);
                hard_clip(stage1, drive * 0.5)
            }
            Self::HardClipClean => {
                // Soft into hard clip blend
                let soft = tanh_sat(x, drive * 0.5);
                let hard = hard_clip(x, drive);
                soft * 0.6 + hard * 0.4
            }
            Self::HardClipCrunch => hard_clip(x, drive),
            Self::Transformer => transformer(x, drive * 0.4),
            Self::TransformerWarm => transformer(x, drive * 0.7),
            Self::TransformerColorful => transformer(x, drive),
            Self::Foldback => foldback(x, drive),
            Self::Rectify => rectify(x, drive),
            Self::BitCrush => bit_crush(x, drive),
            Self::Smudge => smudge(x, drive),
        }
    }
}

/// Generic waveshaper processor for non-tape styles.
pub struct WaveshaperProcessor {
    pub drive: f64,
    pub mix: f64,
    pub output_gain: f64,
    pub tone: f64,
    curve: Curve,
    // Simple 1-pole tone filter state
    tone_lp_l: f64,
    tone_lp_r: f64,
    sample_rate: f64,
}

impl WaveshaperProcessor {
    pub fn new() -> Self {
        Self {
            drive: 0.5,
            mix: 1.0,
            output_gain: 1.0,
            tone: 0.5,
            curve: Curve::TanhWarm,
            tone_lp_l: 0.0,
            tone_lp_r: 0.0,
            sample_rate: 44100.0,
        }
    }

    pub fn set_curve(&mut self, curve: Curve) {
        self.curve = curve;
    }
}

impl Default for WaveshaperProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for WaveshaperProcessor {
    fn reset(&mut self) {
        self.tone_lp_l = 0.0;
        self.tone_lp_r = 0.0;
    }

    fn update(&mut self, config: AudioConfig) {
        self.sample_rate = config.sample_rate;
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        let len = left.len().min(right.len());
        let drive = self.drive;
        let mix = self.mix;
        let output = self.output_gain;

        // Tone: 1-pole filter. tone < 0.5 = darker, > 0.5 = brighter
        // At 0.5, coefficient ~= 1.0 (bypass)
        let tone_freq = 1000.0 * (1.0 + (self.tone - 0.5) * 6.0).max(0.1);
        let tone_coeff = (2.0 * std::f64::consts::PI * tone_freq / self.sample_rate).min(1.0);

        for i in 0..len {
            let dry_l = left[i];
            let dry_r = right[i];

            // Waveshape
            let mut wet_l = self.curve.apply(dry_l, drive);
            let mut wet_r = self.curve.apply(dry_r, drive);

            // Tone filter (LP blend)
            if self.tone < 0.45 {
                // Darken: low-pass
                self.tone_lp_l += tone_coeff * (wet_l - self.tone_lp_l);
                self.tone_lp_r += tone_coeff * (wet_r - self.tone_lp_r);
                let dark_mix = 1.0 - (self.tone / 0.45);
                wet_l = wet_l * (1.0 - dark_mix) + self.tone_lp_l * dark_mix;
                wet_r = wet_r * (1.0 - dark_mix) + self.tone_lp_r * dark_mix;
            } else {
                // Track state even when not darkening
                self.tone_lp_l += tone_coeff * (wet_l - self.tone_lp_l);
                self.tone_lp_r += tone_coeff * (wet_r - self.tone_lp_r);
            }

            // Mix and output
            left[i] = (dry_l * (1.0 - mix) + wet_l * mix) * output;
            right[i] = (dry_r * (1.0 - mix) + wet_r * mix) * output;
        }
    }
}
