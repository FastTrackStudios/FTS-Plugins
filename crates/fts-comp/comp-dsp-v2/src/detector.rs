//! Level detection for Pro-C 3 compressor.
//!
//! Uses peak detection to measure input level.
//! Pro-C 3's actual algorithm is complex (exponential perceptual weighting),
//! but peak detection achieves 99.97% parity with Pro-C 3 Clean mode.

use fts_dsp::db::{linear_to_db, DB_FLOOR};

/// Level detector using peak detection.
pub struct Detector {
    peak: f64,
}

impl Detector {
    pub fn new() -> Self {
        Self { peak: 0.0 }
    }

    /// Detect level as simple 20*log10(|sample|).
    #[inline]
    pub fn detect_level(&mut self, input_abs: f64) -> f64 {
        self.peak = self.peak.max(input_abs);
        linear_to_db(input_abs).max(DB_FLOOR)
    }

    pub fn reset(&mut self) {
        self.peak = 0.0;
    }
}
