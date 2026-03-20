//! Gate detector — envelope follower with hysteresis and zero-crossing awareness.
//!
//! Determines whether the gate should be open or closed based on the
//! sidechain signal level, with separate open/close thresholds to
//! prevent chatter near the boundary.

use fts_dsp::db::{linear_to_db, DB_FLOOR};
use fts_dsp::envelope::EnvelopeFollower;

/// Maximum stereo channels.
const MAX_CH: usize = 2;

// r[impl gate.detector.hysteresis]
// r[impl gate.detector.zero-crossing]
/// Gate detector with hysteresis and zero-crossing snapping.
///
/// Tracks whether the gate should be open or closed per channel.
/// Uses separate open and close thresholds (hysteresis) and prefers
/// state changes at zero crossings to minimize click artifacts.
pub struct GateDetector {
    /// Current detected level in dB per channel.
    level_db: [f64; MAX_CH],
    /// Whether the gate is currently open per channel.
    pub is_open: [bool; MAX_CH],
    /// Previous sample sign per channel (for zero-crossing detection).
    prev_sign: [bool; MAX_CH],
    /// Samples since the desired state change (for zero-crossing timeout).
    defer_count: [u32; MAX_CH],

    // Smoothing coefficients for level detection
    attack_coeff: f64,
    release_coeff: f64,
}

/// Maximum samples to defer a state change waiting for a zero crossing.
/// At 48kHz this is ~1ms — long enough to catch most zero crossings,
/// short enough to not noticeably delay gate response.
const MAX_DEFER_SAMPLES: u32 = 48;

impl GateDetector {
    pub fn new() -> Self {
        Self {
            level_db: [DB_FLOOR; MAX_CH],
            is_open: [false; MAX_CH],
            prev_sign: [false; MAX_CH],
            defer_count: [0; MAX_CH],
            attack_coeff: 0.0,
            release_coeff: 0.0,
        }
    }

    /// Update detection smoothing coefficients.
    ///
    /// Uses fast detection times (1ms attack, 10ms release) for responsive
    /// level tracking. The gate's own attack/hold/release are in the envelope.
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.attack_coeff = EnvelopeFollower::coeff(0.001, sample_rate);
        self.release_coeff = EnvelopeFollower::coeff(0.010, sample_rate);
    }

    /// Feed a sample and update gate open/close state.
    ///
    /// Returns `true` if the gate should be open.
    ///
    /// # Parameters
    /// - `sample`: the sidechain signal sample (may be filtered)
    /// - `open_threshold_db`: level above which the gate opens
    /// - `close_threshold_db`: level below which the gate closes (should be <= open)
    /// - `ch`: channel index
    #[inline]
    pub fn tick(
        &mut self,
        sample: f64,
        open_threshold_db: f64,
        close_threshold_db: f64,
        ch: usize,
    ) -> bool {
        let input_abs = sample.abs();
        let input_db = linear_to_db(input_abs);

        // Smooth the level
        if input_db > self.level_db[ch] {
            self.level_db[ch] = self.attack_coeff * (self.level_db[ch] - input_db) + input_db;
        } else {
            self.level_db[ch] = self.release_coeff * (self.level_db[ch] - input_db) + input_db;
        }

        // Zero-crossing detection
        let current_sign = sample >= 0.0;
        let at_zero_crossing = current_sign != self.prev_sign[ch];
        self.prev_sign[ch] = current_sign;

        // Hysteresis state machine
        let want_open = if self.is_open[ch] {
            self.level_db[ch] >= close_threshold_db
        } else {
            self.level_db[ch] >= open_threshold_db
        };

        // Prefer state changes at zero crossings to minimize clicks
        if want_open != self.is_open[ch] {
            self.defer_count[ch] += 1;
            if at_zero_crossing || input_abs < 1e-6 || self.defer_count[ch] >= MAX_DEFER_SAMPLES {
                self.is_open[ch] = want_open;
                self.defer_count[ch] = 0;
            }
        } else {
            self.defer_count[ch] = 0;
        }

        self.is_open[ch]
    }

    /// Get the current detected level in dB.
    pub fn level_db(&self, ch: usize) -> f64 {
        self.level_db[ch]
    }

    pub fn reset(&mut self) {
        self.level_db = [DB_FLOOR; MAX_CH];
        self.is_open = [false; MAX_CH];
        self.prev_sign = [false; MAX_CH];
        self.defer_count = [0; MAX_CH];
    }
}

impl Default for GateDetector {
    fn default() -> Self {
        Self::new()
    }
}
