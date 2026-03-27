//! Two-stage compressor detector: instant gain curve + GR smoothing.
//!
//! Architecture:
//! 1. Compute instantaneous level: 20*log10(|x|)
//! 2. Apply gain curve (threshold/ratio/knee) → raw GR in dB
//! 3. Transform raw GR to power domain (gr^p), smooth with asymmetric 1-pole
//! 4. Inverse transform smoothed value back to dB
//!
//! Smoothing in gr^p domain (p < 1) reduces Jensen's inequality bias
//! when attack > release, while maintaining threshold proportionality
//! (because (k*gr)^p smoothed and raised to 1/p still scales linearly with k).

use fts_dsp::db::{linear_to_db, DB_FLOOR};

/// Maximum number of stereo channels.
const MAX_CH: usize = 2;

/// Scaling factor for attack time constant.
const ATTACK_SCALE: f64 = 2.0;

/// Scaling factor for release time constant.
const RELEASE_SCALE: f64 = 2.0;

/// Minimum release time in seconds to prevent zero-crossing collapse.
/// Pro-C 3's normalized 0.0 release maps to ~9ms; this floor ensures
/// FTS-Comp's release floor matches Pro-C 3's minimum.
const MIN_RELEASE_S: f64 = 0.009;

/// Minimum attack time in seconds.
/// Set to Pro-C 3's minimum (0.01 ms) so instantaneous attack at near-zero
/// settings matches Pro-C 3's peak-hold behavior rather than clamping to 2ms.
const MIN_ATTACK_S: f64 = 0.00001;

/// Power exponent for GR smoothing domain.
/// p=1.0 = dB domain (baseline). p<1.0 reduces Jensen's bias.
/// p≈0.75 matches Pro-C 3's steady-state GR profile.
const SMOOTH_POWER: f64 = 0.80;

/// Two-stage detector: instant level → gain curve → smoothed GR.
pub struct Detector {
    /// Smoothed GR in transformed domain (gr^p) per channel.
    smooth_grp: [f64; MAX_CH],
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
            smooth_grp: [0.0; MAX_CH],
            prev_output: [0.0; MAX_CH],
            attack_coeff: 0.0,
            release_coeff: 0.0,
            sample_rate: 48000.0,
        }
    }

    /// Update coefficients for new attack/release times or sample rate.
    pub fn set_params(&mut self, attack_s: f64, release_s: f64, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.attack_coeff = Self::coeff(attack_s.max(MIN_ATTACK_S), sample_rate, ATTACK_SCALE);
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

    /// Smooth a raw GR value with asymmetric attack/release in power-dB domain.
    #[inline]
    pub fn smooth_gr(&mut self, raw_gr_db: f64, ch: usize) -> f64 {
        // Transform to power domain: gr^p
        let raw_grp = raw_gr_db.max(0.0).powf(SMOOTH_POWER);

        // Asymmetric smoothing in transformed domain
        let c = if raw_grp > self.smooth_grp[ch] {
            self.attack_coeff // GR increasing → attack
        } else {
            self.release_coeff // GR decreasing → release
        };
        self.smooth_grp[ch] = c * self.smooth_grp[ch] + (1.0 - c) * raw_grp;

        // Inverse transform: gr = smoothed^(1/p)
        self.smooth_grp[ch].max(0.0).powf(1.0 / SMOOTH_POWER)
    }

    /// Store the output sample for feedback detection on next tick.
    #[inline]
    pub fn set_output(&mut self, output: f64, ch: usize) {
        self.prev_output[ch] = output;
    }

    /// Get the current smoothed GR in dB.
    pub fn level_db(&self, ch: usize) -> f64 {
        self.smooth_grp[ch].max(0.0).powf(1.0 / SMOOTH_POWER)
    }

    pub fn reset(&mut self) {
        self.smooth_grp = [0.0; MAX_CH];
        self.prev_output = [0.0; MAX_CH];
    }
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}
