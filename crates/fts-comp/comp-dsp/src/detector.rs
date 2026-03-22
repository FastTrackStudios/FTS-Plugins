//! Linear-domain envelope detector with scaled time constants.
//!
//! Single-stage asymmetric 1-pole filter operating on |input| in the linear
//! domain, then converting to dB. This avoids zero-crossing artifacts inherent
//! in dB-domain detection while preserving natural signal dynamics.

use fts_dsp::db::{linear_to_db, DB_FLOOR};

/// Maximum number of stereo channels.
const MAX_CH: usize = 2;

/// Scaling factor for attack time constant.
/// Converts from user-facing "attack time" to 1-pole coefficient.
const ATTACK_SCALE: f64 = 1.8;

/// Scaling factor for release time constant.
const RELEASE_SCALE: f64 = 1.3;

/// Minimum release time in seconds to prevent zero-crossing collapse.
const MIN_RELEASE_S: f64 = 0.009;

// r[impl comp.chain.signal-flow]
/// Linear-domain envelope detector.
///
/// Operates directly on |input| in the linear domain with user-controlled
/// attack/release, then converts the smoothed envelope to dB. The linear
/// domain gives frequency-independent detection since the 1-pole filter
/// averages over zero crossings naturally.
pub struct Detector {
    /// Smoothed envelope (linear) per channel.
    env: [f64; MAX_CH],
    /// Previous output sample per channel (for feedback detection).
    prev_output: [f64; MAX_CH],

    // Coefficients (user-controlled, with scaling)
    attack_coeff: f64,
    release_coeff: f64,
    sample_rate: f64,
}

impl Detector {
    pub fn new() -> Self {
        Self {
            env: [0.0; MAX_CH],
            prev_output: [0.0; MAX_CH],
            attack_coeff: 0.0,
            release_coeff: 0.0,
            sample_rate: 48000.0,
        }
    }

    /// Update coefficients for new attack/release times or sample rate.
    ///
    /// `attack_s` and `release_s` are in seconds.
    pub fn set_params(&mut self, attack_s: f64, release_s: f64, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.attack_coeff = Self::coeff(attack_s, sample_rate, ATTACK_SCALE);
        self.release_coeff = Self::coeff(release_s.max(MIN_RELEASE_S), sample_rate, RELEASE_SCALE);
    }

    /// Compute 1-pole filter coefficient with scaling.
    #[inline]
    fn coeff(time_s: f64, sample_rate: f64, scale: f64) -> f64 {
        if time_s > 0.0 {
            (-scale / (sample_rate * time_s)).exp()
        } else {
            0.0
        }
    }

    /// Feed a sample into the detector and return the smoothed level in dB.
    ///
    /// `input_abs` is the absolute value of the input sample.
    /// `feedback` is the feedback amount (0.0 = pure feedforward, 1.0 = full feedback).
    /// `ch` is the channel index (0 or 1).
    #[inline]
    pub fn tick(&mut self, input_abs: f64, feedback: f64, ch: usize) -> f64 {
        let combined = input_abs + self.prev_output[ch].abs() * feedback;

        // Asymmetric 1-pole filter in linear domain
        let c = if combined > self.env[ch] {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.env[ch] = c * self.env[ch] + (1.0 - c) * combined;

        // Convert to dB
        linear_to_db(self.env[ch]).max(DB_FLOOR)
    }

    /// Store the output sample for feedback detection on next tick.
    #[inline]
    pub fn set_output(&mut self, output: f64, ch: usize) {
        self.prev_output[ch] = output;
    }

    /// Get the current smoothed level in dB.
    pub fn level_db(&self, ch: usize) -> f64 {
        linear_to_db(self.env[ch]).max(DB_FLOOR)
    }

    pub fn reset(&mut self) {
        self.env = [0.0; MAX_CH];
        self.prev_output = [0.0; MAX_CH];
    }
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}
