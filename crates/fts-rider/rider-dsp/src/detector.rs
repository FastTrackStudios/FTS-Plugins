//! Level detector — RMS and K-weighted (LUFS-style) measurement.
//!
//! Provides smooth level tracking for the rider's gain calculator.
//! Supports both RMS (flat frequency response) and K-weighted (perceptually
//! weighted, based on BS.1770 pre-filter + RLB) detection modes.

use fts_dsp::db::{db_to_linear, DB_FLOOR};
use fts_dsp::loudness::KWeightingFilter;

/// Detection mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetectMode {
    /// Flat RMS — equal weighting across all frequencies.
    Rms,
    /// K-weighted RMS (BS.1770 style) — perceptually weighted,
    /// better for vocal/dialog level tracking.
    KWeighted,
}

// r[impl rider.detector.rms]
// r[impl rider.detector.lufs]
/// Level detector with configurable window size and weighting.
///
/// Tracks the signal level using a sliding-window mean-square computation.
/// The window size controls responsiveness: shorter windows track faster
/// transients, longer windows provide smoother readings.
pub struct LevelDetector {
    /// Detection mode.
    pub mode: DetectMode,
    /// Window size in milliseconds (10-300).
    pub window_ms: f64,

    // K-weighting filters (stereo)
    k_filter_l: KWeightingFilter,
    k_filter_r: KWeightingFilter,

    // Ring buffer for windowed mean-square
    ring: Vec<f64>,
    ring_pos: usize,
    ring_sum: f64,

    /// Current detected level in dB.
    level_db: f64,

    sample_rate: f64,
}

impl LevelDetector {
    pub fn new() -> Self {
        Self {
            mode: DetectMode::KWeighted,
            window_ms: 50.0,
            k_filter_l: KWeightingFilter::new(),
            k_filter_r: KWeightingFilter::new(),
            ring: Vec::new(),
            ring_pos: 0,
            ring_sum: 0.0,
            level_db: DB_FLOOR,
            sample_rate: 48000.0,
        }
    }

    /// Update internal state after parameter changes.
    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.k_filter_l.update(sample_rate);
        self.k_filter_r.update(sample_rate);

        let window_samples = (self.window_ms * 0.001 * sample_rate).max(1.0) as usize;
        if self.ring.len() != window_samples {
            self.ring = vec![0.0; window_samples];
            self.ring_pos = 0;
            self.ring_sum = 0.0;
        }
    }

    /// Process a stereo sample pair and update the detected level.
    ///
    /// Returns the current level in dB.
    #[inline]
    pub fn tick(&mut self, left: f64, right: f64) -> f64 {
        let (l, r) = match self.mode {
            DetectMode::Rms => (left, right),
            DetectMode::KWeighted => (self.k_filter_l.tick(left), self.k_filter_r.tick(right)),
        };

        // Stereo mean-square
        let ms = l * l + r * r;

        // Update ring buffer
        if !self.ring.is_empty() {
            self.ring_sum -= self.ring[self.ring_pos];
            self.ring[self.ring_pos] = ms;
            self.ring_sum += ms;
            self.ring_pos = (self.ring_pos + 1) % self.ring.len();

            let mean_ms = (self.ring_sum / self.ring.len() as f64).max(0.0);
            self.level_db = if mean_ms > 0.0 {
                10.0 * mean_ms.log10()
            } else {
                DB_FLOOR
            };
        }

        self.level_db
    }

    /// Get the current detected level in dB.
    pub fn level_db(&self) -> f64 {
        self.level_db
    }

    /// Get the current detected level as linear gain.
    pub fn level_linear(&self) -> f64 {
        db_to_linear(self.level_db)
    }

    pub fn reset(&mut self) {
        self.k_filter_l.reset();
        self.k_filter_r.reset();
        self.ring.fill(0.0);
        self.ring_pos = 0;
        self.ring_sum = 0.0;
        self.level_db = DB_FLOOR;
    }
}

impl Default for LevelDetector {
    fn default() -> Self {
        Self::new()
    }
}
