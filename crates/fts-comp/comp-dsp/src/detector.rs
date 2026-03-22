//! Two-stage compressor detector: instant gain curve + GR smoothing.
//!
//! Architecture (DAFX textbook style):
//! 1. Compute instantaneous level: 20*log10(|x|)
//! 2. Apply gain curve (threshold/ratio/knee) → raw GR
//! 3. Smooth the raw GR with asymmetric 1-pole (attack/release)
//!
//! This produces natural peak-weighted detection because the gain curve
//! is nonlinear (soft knee). Averaging the nonlinear GR over a waveform
//! cycle gives a result between peak and RMS detection, with the exact
//! weighting controlled by the knee shape.

use fts_dsp::db::{linear_to_db, DB_FLOOR};

/// Maximum number of stereo channels.
const MAX_CH: usize = 2;

/// Scaling factor for attack time constant.
const ATTACK_SCALE: f64 = 2.0;

/// Scaling factor for release time constant.
const RELEASE_SCALE: f64 = 2.0;

/// Minimum release time in seconds to prevent zero-crossing collapse.
const MIN_RELEASE_S: f64 = 0.009;

/// Two-stage detector: instant level → gain curve → smoothed GR.
pub struct Detector {
    /// Smoothed GR (dB, positive = reducing) per channel.
    smooth_gr: [f64; MAX_CH],
    /// Previous output sample per channel (for feedback detection).
    prev_output: [f64; MAX_CH],

    // Coefficients
    attack_coeff: f64,
    release_coeff: f64,
    sample_rate: f64,
}

impl Detector {
    pub fn new() -> Self {
        Self {
            smooth_gr: [0.0; MAX_CH],
            prev_output: [0.0; MAX_CH],
            attack_coeff: 0.0,
            release_coeff: 0.0,
            sample_rate: 48000.0,
        }
    }

    /// Update coefficients for new attack/release times or sample rate.
    pub fn set_params(&mut self, attack_s: f64, release_s: f64, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.attack_coeff = Self::coeff(attack_s, sample_rate, ATTACK_SCALE);
        self.release_coeff = Self::coeff(release_s.max(MIN_RELEASE_S), sample_rate, RELEASE_SCALE);
    }

    #[inline]
    fn coeff(time_s: f64, sample_rate: f64, scale: f64) -> f64 {
        if time_s > 0.0 {
            (-scale / (sample_rate * time_s)).exp()
        } else {
            0.0
        }
    }

    /// Feed a sample and return the instantaneous level in dB.
    #[inline]
    pub fn tick(&mut self, input_abs: f64, feedback: f64, ch: usize) -> f64 {
        let combined = input_abs + self.prev_output[ch].abs() * feedback;
        linear_to_db(combined).max(DB_FLOOR)
    }

    /// Smooth a raw GR value with asymmetric attack/release.
    ///
    /// Attack = GR increasing (more compression, louder input).
    /// Release = GR decreasing (less compression, quieter input).
    #[inline]
    pub fn smooth_gr(&mut self, raw_gr: f64, ch: usize) -> f64 {
        let c = if raw_gr > self.smooth_gr[ch] {
            self.attack_coeff // GR increasing → attack
        } else {
            self.release_coeff // GR decreasing → release
        };
        self.smooth_gr[ch] = c * self.smooth_gr[ch] + (1.0 - c) * raw_gr;
        self.smooth_gr[ch]
    }

    /// Store the output sample for feedback detection on next tick.
    #[inline]
    pub fn set_output(&mut self, output: f64, ch: usize) {
        self.prev_output[ch] = output;
    }

    /// Get the current smoothed GR in dB.
    pub fn level_db(&self, ch: usize) -> f64 {
        self.smooth_gr[ch]
    }

    pub fn reset(&mut self) {
        self.smooth_gr = [0.0; MAX_CH];
        self.prev_output = [0.0; MAX_CH];
    }
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}
