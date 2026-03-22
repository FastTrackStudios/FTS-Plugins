//! Envelope detection with exponential attack/release ballistics.
//!
//! Converts the instantaneous signal level into a smoothed envelope
//! suitable for gain reduction computation.

use fts_dsp::db::{linear_to_db, DB_FLOOR};

/// Maximum number of stereo channels.
const MAX_CH: usize = 2;

// r[impl comp.chain.signal-flow]
/// Envelope detector with asymmetric attack/release.
///
/// Uses exponential smoothing with separate attack and release coefficients.
/// Operates in the dB domain.
pub struct Detector {
    /// Smoothed signal level in dB per channel.
    slewed: [f64; MAX_CH],
    /// Previous output sample per channel (for feedback detection).
    prev_output: [f64; MAX_CH],
    /// Whether each channel has received its first non-silent sample.
    initialized: [bool; MAX_CH],

    // Cached coefficients
    attack_coeff: f64,
    release_coeff: f64,
    sample_rate: f64,
}

impl Detector {
    pub fn new() -> Self {
        Self {
            slewed: [DB_FLOOR; MAX_CH],
            prev_output: [0.0; MAX_CH],
            initialized: [false; MAX_CH],
            attack_coeff: 0.0,
            release_coeff: 0.0,
            sample_rate: 48000.0,
        }
    }

    /// Update coefficients for new attack/release times or sample rate.
    ///
    /// `attack_s` and `release_s` are in seconds. The displayed time represents
    /// the time to reach ~90% of the target (common convention for compressors),
    /// so the internal time constant is scaled accordingly.
    pub fn set_params(&mut self, attack_s: f64, release_s: f64, sample_rate: f64) {
        self.sample_rate = sample_rate;
        // Empirically-derived scaling factors that match Pro-C 3 Clean's
        // time constant definition. Attack uses ~1.8x (close to ln(10)/1.3)
        // and release uses ~1.3x.
        self.attack_coeff = Self::coeff_scaled(attack_s, sample_rate, 1.8);
        self.release_coeff = Self::coeff_scaled(release_s, sample_rate, 1.3);
    }

    /// Compute coefficient with a scaling factor on the time constant.
    #[inline]
    fn coeff_scaled(time_s: f64, sample_rate: f64, scale: f64) -> f64 {
        if time_s > 0.0 {
            (-scale / (sample_rate * time_s)).exp()
        } else {
            0.0
        }
    }

    /// Feed a sample into the detector and return the smoothed level in dB.
    ///
    /// `input` is the absolute value of the input sample.
    /// `feedback` is the feedback amount (0.0 = pure feedforward, 1.0 = full feedback).
    /// `ch` is the channel index (0 or 1).
    #[inline]
    pub fn tick(&mut self, input_abs: f64, feedback: f64, ch: usize) -> f64 {
        // Combine input with feedback from previous output
        let combined = input_abs + self.prev_output[ch].abs() * feedback;
        let input_db = linear_to_db(combined);

        // Clamp to sane range
        let input_db = input_db.clamp(DB_FLOOR, 6.0);

        // Snap to input on first non-silent sample to avoid slow ramp from DB_FLOOR
        if !self.initialized[ch] && input_db > DB_FLOOR {
            self.initialized[ch] = true;
            self.slewed[ch] = input_db;
            return self.slewed[ch];
        }

        // Asymmetric exponential smoothing
        if input_db > self.slewed[ch] {
            // Attack: signal rising
            self.slewed[ch] = self.attack_coeff * (self.slewed[ch] - input_db) + input_db;
        } else {
            // Release: signal falling
            self.slewed[ch] = self.release_coeff * (self.slewed[ch] - input_db) + input_db;
        }

        self.slewed[ch] = self.slewed[ch].clamp(DB_FLOOR, 1000.0);
        self.slewed[ch]
    }

    /// Store the output sample for feedback detection on next tick.
    #[inline]
    pub fn set_output(&mut self, output: f64, ch: usize) {
        self.prev_output[ch] = output;
    }

    /// Get the current smoothed level in dB.
    pub fn level_db(&self, ch: usize) -> f64 {
        self.slewed[ch]
    }

    pub fn reset(&mut self) {
        self.slewed = [DB_FLOOR; MAX_CH];
        self.prev_output = [0.0; MAX_CH];
        self.initialized = [false; MAX_CH];
    }
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}
