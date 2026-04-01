//! FTS MIDI Guitar DSP — Polyphonic pitch detection using constant-Q resonators.
//!
//! Converts monophonic or polyphonic guitar audio into MIDI note events.
//! Uses a bank of first-order IIR complex bandpass filters (one per semitone)
//! with energy-based note detection, hysteresis, and harmonic suppression.
//!
//! Enhanced features for guitar DI signals:
//! - Pre-processing: DC block, high-pass, hum notch, compression
//! - Spectral whitening: normalizes per-bin energy against running peak
//! - Klapuri iterative harmonic cancellation with inharmonicity model
//! - Adaptive thresholding relative to signal energy

pub mod detector;
pub mod note_event;
pub mod preprocessing;
pub mod resonator;

pub use detector::PolyphonicDetector;
pub use note_event::DetectedNote;
pub use preprocessing::PreProcessor;
pub use resonator::Resonator;

/// Top-level detector with a simplified API.
pub struct MidiGuitarDetector {
    detector: PolyphonicDetector,
}

impl MidiGuitarDetector {
    pub fn new() -> Self {
        Self {
            detector: PolyphonicDetector::new(),
        }
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.detector.set_sample_rate(sr);
    }

    pub fn set_threshold(&mut self, threshold: f64) {
        self.detector.set_threshold(threshold);
    }

    pub fn set_sensitivity(&mut self, sensitivity: f64) {
        self.detector.set_sensitivity(sensitivity);
    }

    pub fn set_window_size(&mut self, samples: usize) {
        self.detector.set_window_size(samples);
    }

    pub fn set_note_range(&mut self, low: u8, high: u8) {
        self.detector.set_note_range(low, high);
    }

    pub fn set_harmonic_suppression(&mut self, enabled: bool) {
        self.detector.set_harmonic_suppression(enabled);
    }

    pub fn set_peak_picking(&mut self, enabled: bool) {
        self.detector.set_peak_picking(enabled);
    }

    pub fn set_hysteresis_ratio(&mut self, ratio: f64) {
        self.detector.set_hysteresis_ratio(ratio);
    }

    pub fn set_preprocessing(&mut self, enabled: bool) {
        self.detector.set_preprocessing(enabled);
    }

    pub fn set_whitening(&mut self, enabled: bool) {
        self.detector.set_whitening(enabled);
    }

    pub fn set_whitening_decay(&mut self, decay: f64) {
        self.detector.set_whitening_decay(decay);
    }

    pub fn set_adaptive_threshold(&mut self, enabled: bool) {
        self.detector.set_adaptive_threshold(enabled);
    }

    pub fn set_klapuri(&mut self, enabled: bool) {
        self.detector.set_klapuri(enabled);
    }

    pub fn set_max_polyphony(&mut self, max: usize) {
        self.detector.set_max_polyphony(max);
    }

    /// Process a single audio sample.
    ///
    /// Returns `Some(events)` every `window_size` samples when the analysis
    /// window completes, `None` otherwise.
    pub fn process_sample(&mut self, sample: f64) -> Option<Vec<DetectedNote>> {
        self.detector.process_sample(sample)
    }

    /// Reset all internal state.
    pub fn reset(&mut self) {
        self.detector.reset();
    }

    /// Access the underlying detector for advanced queries (e.g. active notes).
    pub fn inner(&self) -> &PolyphonicDetector {
        &self.detector
    }
}

impl Default for MidiGuitarDetector {
    fn default() -> Self {
        Self::new()
    }
}
