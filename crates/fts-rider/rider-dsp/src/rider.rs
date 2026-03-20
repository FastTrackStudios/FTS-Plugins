//! Gain calculation engine — computes gain adjustments to ride signal toward target.
//!
//! The rider measures the current signal level and smoothly adjusts gain to bring
//! it toward a target loudness. Attack/release smoothing ensures natural-sounding
//! rides. Voice activity detection prevents boosting silence or bleed.

use fts_dsp::db::db_to_linear;
use fts_dsp::envelope::EnvelopeFollower;

use crate::detector::LevelDetector;

// r[impl rider.gain.target]
// r[impl rider.gain.range]
// r[impl rider.gain.smoothing]
// r[impl rider.vocal.activity]
/// Vocal rider gain engine.
///
/// Tracks the input level via [`LevelDetector`] and computes a smooth gain
/// adjustment toward the target level, constrained by a configurable range.
pub struct GainRider {
    /// Level detector for the main input.
    pub detector: LevelDetector,

    // ── Parameters ──────────────────────────────────────────────────────
    /// Target level in dB (e.g., -18.0 for typical vocal).
    pub target_db: f64,
    /// Maximum boost in dB (positive, e.g., 12.0).
    pub max_boost_db: f64,
    /// Maximum cut in dB (positive value representing downward range, e.g., 12.0).
    pub max_cut_db: f64,
    /// Attack time in milliseconds (how fast gain increases).
    pub attack_ms: f64,
    /// Release time in milliseconds (how fast gain decreases).
    pub release_ms: f64,
    /// Voice activity threshold in dB — below this, gain freezes.
    pub activity_threshold_db: f64,

    // ── Internal state ──────────────────────────────────────────────────
    /// Current smoothed gain in dB.
    gain_db: f64,
    /// Attack coefficient (one-pole).
    attack_coeff: f64,
    /// Release coefficient (one-pole).
    release_coeff: f64,

    sample_rate: f64,
}

impl GainRider {
    pub fn new() -> Self {
        Self {
            detector: LevelDetector::new(),
            target_db: -18.0,
            max_boost_db: 12.0,
            max_cut_db: 12.0,
            attack_ms: 20.0,
            release_ms: 80.0,
            activity_threshold_db: -50.0,
            gain_db: 0.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            sample_rate: 48000.0,
        }
    }

    /// Recalculate coefficients after parameter or sample rate changes.
    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.detector.update(sample_rate);

        // One-pole smoothing coefficients
        self.attack_coeff = EnvelopeFollower::coeff(self.attack_ms * 0.001, sample_rate);
        self.release_coeff = EnvelopeFollower::coeff(self.release_ms * 0.001, sample_rate);
    }

    /// Process one stereo sample pair. Returns the gain in dB to apply.
    #[inline]
    pub fn tick(&mut self, left: f64, right: f64) -> f64 {
        let level_db = self.detector.tick(left, right);

        // Voice activity gate: if below threshold, freeze gain
        if level_db < self.activity_threshold_db {
            return self.gain_db;
        }

        // Desired gain = target - current level
        let desired_db = self.target_db - level_db;

        // Clamp to range
        let clamped_db = desired_db.clamp(-self.max_cut_db, self.max_boost_db);

        // Smooth with attack/release — attack when gain is increasing (boosting more),
        // release when gain is decreasing (cutting more or reducing boost).
        let coeff = if clamped_db > self.gain_db {
            self.attack_coeff
        } else {
            self.release_coeff
        };

        self.gain_db = coeff * self.gain_db + (1.0 - coeff) * clamped_db;

        self.gain_db
    }

    /// Get the current gain in dB.
    pub fn gain_db(&self) -> f64 {
        self.gain_db
    }

    /// Get the current gain as a linear multiplier.
    pub fn gain_linear(&self) -> f64 {
        db_to_linear(self.gain_db)
    }

    /// Get the current detected level in dB.
    pub fn level_db(&self) -> f64 {
        self.detector.level_db()
    }

    pub fn reset(&mut self) {
        self.detector.reset();
        self.gain_db = 0.0;
    }
}

impl Default for GainRider {
    fn default() -> Self {
        Self::new()
    }
}
