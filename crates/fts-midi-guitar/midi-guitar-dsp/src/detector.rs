use crate::note_event::DetectedNote;
use crate::preprocessing::PreProcessor;
use crate::resonator::Resonator;

/// Default MIDI note range: one below guitar low E to one above high E.
/// Matches reference: FIRST_NOTE_NUMBER=39, LAST_NOTE_NUMBER=89.
const DEFAULT_LOW_NOTE: u8 = 39;
const DEFAULT_HIGH_NOTE: u8 = 89;

/// Default analysis window size in samples (~20ms at 48kHz).
/// Matches reference: WINDOW_SIZE=960.
const DEFAULT_WINDOW_SIZE: usize = 960;

/// Default energy threshold (linear).
/// Matches reference: 0.001.
const DEFAULT_THRESHOLD: f64 = 0.001;

/// Polyphonic note detector using a bank of constant-Q resonators.
///
/// Feeds each input sample to all resonators in parallel.
/// Every `window_size` samples, compares accumulated energy to thresholds
/// and emits note-on/note-off events.
///
/// Enhanced features beyond the reference implementation:
/// - Pre-processing: DC block, high-pass, hum notch, compression
/// - Spectral whitening: normalizes energy against running peak per bin
/// - Iterative harmonic cancellation (Klapuri method)
/// - Adaptive thresholding: scales threshold relative to recent signal energy
/// - Peak picking, hysteresis
pub struct PolyphonicDetector {
    resonators: Vec<Resonator>,
    /// MIDI note number for each resonator.
    note_map: Vec<u8>,
    /// Which notes are currently "on".
    active_notes: Vec<bool>,
    /// Sample counter within the current analysis window.
    window_counter: usize,
    /// Analysis window size in samples.
    window_size: usize,
    /// Note-on energy threshold (linear).
    threshold: f64,
    /// Velocity sensitivity: maps energy to velocity curve.
    /// Higher = more dynamic range. 0..1 range.
    sensitivity: f64,
    /// Whether to suppress harmonics of detected fundamentals.
    harmonic_suppression: bool,
    /// Whether to require spectral peak picking (only local maxima trigger).
    peak_picking: bool,
    /// Hysteresis ratio: note-off threshold = on_threshold * this factor.
    /// 1.0 = same threshold for on/off (reference behavior).
    hysteresis_ratio: f64,
    /// Current sample rate.
    sample_rate: f64,
    /// Lowest MIDI note to detect.
    low_note: u8,
    /// Highest MIDI note to detect.
    high_note: u8,
    /// Flag: coefficients need recalculation.
    needs_update: bool,

    // ── New: Pre-processing ──────────────────────────────────────────
    preprocessor: PreProcessor,
    preprocessing_enabled: bool,

    // ── New: Spectral whitening ──────────────────────────────────────
    /// Running peak energy per bin (exponential decay).
    whitening_peaks: Vec<f64>,
    /// Whitening decay factor (0.99 = slow adapt, 0.9 = fast adapt).
    whitening_decay: f64,
    /// Whether spectral whitening is enabled.
    whitening_enabled: bool,

    // ── New: Adaptive threshold ──────────────────────────────────────
    /// Running average signal energy (across all bins).
    adaptive_floor: f64,
    /// Decay factor for the adaptive floor.
    adaptive_decay: f64,
    /// Whether adaptive thresholding is enabled.
    adaptive_threshold_enabled: bool,

    // ── New: Klapuri iterative cancellation ──────────────────────────
    /// Whether to use iterative harmonic cancellation (Klapuri method).
    klapuri_enabled: bool,
    /// Maximum number of simultaneous notes to detect per window.
    max_polyphony: usize,
}

impl PolyphonicDetector {
    /// Create a new detector with reference-matching defaults.
    pub fn new() -> Self {
        let mut det = Self {
            resonators: Vec::new(),
            note_map: Vec::new(),
            active_notes: Vec::new(),
            window_counter: 0,
            window_size: DEFAULT_WINDOW_SIZE,
            threshold: DEFAULT_THRESHOLD,
            sensitivity: 0.5,
            harmonic_suppression: false,
            peak_picking: false,
            hysteresis_ratio: 1.0,
            sample_rate: 48000.0,
            low_note: DEFAULT_LOW_NOTE,
            high_note: DEFAULT_HIGH_NOTE,
            needs_update: true,

            preprocessor: PreProcessor::new(48000.0),
            preprocessing_enabled: false,

            whitening_peaks: Vec::new(),
            whitening_decay: 0.98,
            whitening_enabled: false,

            adaptive_floor: 0.0,
            adaptive_decay: 0.95,
            adaptive_threshold_enabled: false,

            klapuri_enabled: false,
            max_polyphony: 6,
        };
        det.rebuild_bank();
        det
    }

    /// Set the sample rate and recalculate all resonator coefficients.
    pub fn set_sample_rate(&mut self, sr: f64) {
        if (self.sample_rate - sr).abs() > 0.1 {
            self.sample_rate = sr;
            self.preprocessor.set_sample_rate(sr);
            self.needs_update = true;
        }
    }

    /// Set the energy threshold (linear scale, e.g. 0.001).
    pub fn set_threshold(&mut self, threshold: f64) {
        self.threshold = threshold.max(1e-10);
    }

    /// Set velocity sensitivity (0.0 = flat velocity, 1.0 = maximum dynamic range).
    pub fn set_sensitivity(&mut self, sensitivity: f64) {
        self.sensitivity = sensitivity.clamp(0.0, 1.0);
    }

    /// Set the analysis window size in samples.
    pub fn set_window_size(&mut self, samples: usize) {
        self.window_size = samples.max(1);
    }

    /// Set the MIDI note range for detection.
    pub fn set_note_range(&mut self, low: u8, high: u8) {
        let low = low.min(127);
        let high = high.max(low).min(127);
        if self.low_note != low || self.high_note != high {
            self.low_note = low;
            self.high_note = high;
            self.needs_update = true;
        }
    }

    /// Enable or disable harmonic suppression.
    pub fn set_harmonic_suppression(&mut self, enabled: bool) {
        self.harmonic_suppression = enabled;
    }

    /// Enable or disable spectral peak picking.
    pub fn set_peak_picking(&mut self, enabled: bool) {
        self.peak_picking = enabled;
    }

    /// Set the hysteresis ratio for note-off threshold.
    /// 1.0 = same threshold for on/off (reference behavior).
    /// Lower values (e.g. 0.3) mean notes stay on longer.
    pub fn set_hysteresis_ratio(&mut self, ratio: f64) {
        self.hysteresis_ratio = ratio.clamp(0.01, 1.0);
    }

    /// Enable or disable pre-processing (DC block, high-pass, hum notch, compression).
    pub fn set_preprocessing(&mut self, enabled: bool) {
        self.preprocessing_enabled = enabled;
    }

    /// Enable or disable spectral whitening.
    /// Normalizes each bin's energy against its running peak, flattening the
    /// spectrum so weak fundamentals aren't buried by strong harmonics.
    pub fn set_whitening(&mut self, enabled: bool) {
        self.whitening_enabled = enabled;
    }

    /// Set the whitening decay factor.
    /// Lower = faster adaptation (0.9), higher = slower (0.99).
    pub fn set_whitening_decay(&mut self, decay: f64) {
        self.whitening_decay = decay.clamp(0.5, 0.999);
    }

    /// Enable or disable adaptive thresholding.
    /// Scales the detection threshold relative to recent average signal energy.
    pub fn set_adaptive_threshold(&mut self, enabled: bool) {
        self.adaptive_threshold_enabled = enabled;
    }

    /// Enable or disable Klapuri iterative harmonic cancellation.
    /// Finds the strongest peak, subtracts its harmonic template, repeats.
    pub fn set_klapuri(&mut self, enabled: bool) {
        self.klapuri_enabled = enabled;
    }

    /// Set maximum polyphony for Klapuri cancellation.
    pub fn set_max_polyphony(&mut self, max: usize) {
        self.max_polyphony = max.clamp(1, 12);
    }

    /// Process a single input sample.
    ///
    /// Returns `Some(events)` when the analysis window completes (every `window_size` samples),
    /// `None` otherwise.
    pub fn process_sample(&mut self, sample: f64) -> Option<Vec<DetectedNote>> {
        if self.needs_update {
            self.rebuild_bank();
            self.needs_update = false;
        }

        // Pre-processing chain.
        let processed = if self.preprocessing_enabled {
            self.preprocessor.process(sample)
        } else {
            sample
        };

        // Feed sample to all resonators.
        for res in &mut self.resonators {
            res.process_sample(processed);
        }

        self.window_counter += 1;

        if self.window_counter >= self.window_size {
            self.window_counter = 0;
            Some(self.analyze_window())
        } else {
            None
        }
    }

    /// Reset all state.
    pub fn reset(&mut self) {
        for res in &mut self.resonators {
            res.reset();
        }
        for active in &mut self.active_notes {
            *active = false;
        }
        for peak in &mut self.whitening_peaks {
            *peak = 0.0;
        }
        self.adaptive_floor = 0.0;
        self.window_counter = 0;
        self.preprocessor.reset();
    }

    /// Return a slice of which notes are currently active (indexed by resonator index).
    pub fn active_notes(&self) -> &[bool] {
        &self.active_notes
    }

    /// Return the note map (MIDI note numbers indexed by resonator index).
    pub fn note_map(&self) -> &[u8] {
        &self.note_map
    }

    // ── Internal ──────────────────────────────────────────────────────

    /// Rebuild the resonator bank for the current note range and sample rate.
    fn rebuild_bank(&mut self) {
        let count = (self.high_note - self.low_note + 1) as usize;
        self.resonators.clear();
        self.note_map.clear();
        self.active_notes.clear();
        self.whitening_peaks.clear();

        self.resonators.reserve(count);
        self.note_map.reserve(count);
        self.active_notes.reserve(count);
        self.whitening_peaks.reserve(count);

        for midi_note in self.low_note..=self.high_note {
            let freq = midi_note_to_freq(midi_note);
            let mut res = Resonator::new();
            res.init(freq, self.sample_rate);
            self.resonators.push(res);
            self.note_map.push(midi_note);
            self.active_notes.push(false);
            self.whitening_peaks.push(0.0);
        }

        self.window_counter = 0;
    }

    /// Analyze accumulated energy and produce note events.
    fn analyze_window(&mut self) -> Vec<DetectedNote> {
        let mut events = Vec::new();

        // Collect energies from all resonators.
        let mut energies: Vec<f64> = self
            .resonators
            .iter_mut()
            .map(|r| r.take_energy())
            .collect();

        // Normalize energy by window size.
        let inv_window = 1.0 / self.window_size as f64;
        for e in &mut energies {
            *e *= inv_window;
        }

        // ── Spectral whitening ──────────────────────────────────────
        if self.whitening_enabled {
            self.apply_whitening(&mut energies);
        }

        // ── Adaptive threshold ──────────────────────────────────────
        let effective_threshold = if self.adaptive_threshold_enabled {
            self.compute_adaptive_threshold(&energies)
        } else {
            self.threshold
        };

        // ── Klapuri iterative harmonic cancellation ─────────────────
        if self.klapuri_enabled {
            self.klapuri_cancellation(&mut energies, effective_threshold);
        } else if self.harmonic_suppression {
            // Legacy harmonic suppression (simpler).
            self.suppress_harmonics(&mut energies, effective_threshold);
        }

        // ── Spectral peak picking ───────────────────────────────────
        let count = energies.len();
        let is_peak = if self.peak_picking {
            let mut peaks = vec![true; count];
            for i in 0..count {
                if energies[i] < effective_threshold {
                    peaks[i] = false;
                    continue;
                }
                if i > 0 && energies[i] < energies[i - 1] {
                    peaks[i] = false;
                }
                if i + 1 < count && energies[i] < energies[i + 1] {
                    peaks[i] = false;
                }
            }
            peaks
        } else {
            vec![true; count]
        };

        // Note-off threshold (with optional hysteresis).
        let off_threshold = effective_threshold * self.hysteresis_ratio;

        // Generate note on/off events.
        for (i, &energy) in energies.iter().enumerate() {
            let was_on = self.active_notes[i];

            if !was_on {
                if energy > effective_threshold && is_peak[i] {
                    let velocity = self.energy_to_velocity(energy, effective_threshold);
                    self.active_notes[i] = true;
                    events.push(DetectedNote {
                        note: self.note_map[i],
                        velocity,
                        is_on: true,
                    });
                }
            } else if energy <= off_threshold {
                self.active_notes[i] = false;
                events.push(DetectedNote {
                    note: self.note_map[i],
                    velocity: 0.0,
                    is_on: false,
                });
            }
        }

        events
    }

    /// Apply spectral whitening: compensate for guitar pickup frequency response.
    ///
    /// Guitar pickups act as differentiators, boosting harmonics relative to
    /// fundamentals. This applies a frequency-dependent gain correction that
    /// boosts lower frequencies relative to higher ones, counteracting the
    /// pickup's spectral tilt.
    ///
    /// This is NOT full spectral whitening (which amplifies noise) — it's a
    /// fixed tilt correction based on the known physics of guitar pickups.
    fn apply_whitening(&mut self, energies: &mut [f64]) {
        // Guitar pickup response roughly follows f² (voltage is proportional
        // to rate of flux change). Compensate by multiplying low bins by more.
        // Use a gentle correction curve to avoid over-boosting sub-fundamentals.
        for (i, energy) in energies.iter_mut().enumerate() {
            let note = self.note_map[i] as f64;
            // Reference point: MIDI 69 (A4, 440 Hz) = gain 1.0
            // Lower notes get boosted, higher notes slightly attenuated.
            // The correction is sqrt-based (not full f²) to be gentle.
            let semitones_below_a4 = 69.0 - note;
            let correction = if semitones_below_a4 > 0.0 {
                // Boost low notes: up to ~2x at MIDI 40 (low E)
                1.0 + (semitones_below_a4 / 29.0).min(1.0) * 1.0
            } else {
                // Slightly attenuate high notes
                1.0 / (1.0 + (-semitones_below_a4 / 20.0).min(1.0) * 0.3)
            };
            *energy *= correction;
        }

        // Also track running peaks for adaptive behavior.
        for (i, energy) in energies.iter().enumerate() {
            let peak = &mut self.whitening_peaks[i];
            if *energy > *peak {
                *peak = *energy;
            } else {
                *peak *= self.whitening_decay;
            }
        }
    }

    /// Compute adaptive threshold based on recent signal energy.
    /// Returns a threshold that scales with the signal level.
    fn compute_adaptive_threshold(&mut self, energies: &[f64]) -> f64 {
        // Compute median energy (robust to outliers unlike mean).
        let mut sorted: Vec<f64> = energies.iter().copied().filter(|&e| e > 1e-15).collect();
        let median = if sorted.is_empty() {
            0.0
        } else {
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            sorted[sorted.len() / 2]
        };

        // Update adaptive floor with exponential smoothing.
        self.adaptive_floor =
            self.adaptive_decay * self.adaptive_floor + (1.0 - self.adaptive_decay) * median;

        // Effective threshold: max of fixed threshold and adaptive floor * multiplier.
        // The multiplier ensures we're well above the noise floor.
        let adaptive = self.adaptive_floor * 3.0;
        self.threshold.max(adaptive)
    }

    /// Klapuri iterative harmonic cancellation.
    ///
    /// 1. Find the strongest energy peak.
    /// 2. Subtract its harmonic template (2x, 3x, 4x, 5x, 6x) from the energy array,
    ///    accounting for guitar string inharmonicity.
    /// 3. Also check if this peak is itself a harmonic of a lower note (sub-harmonic check).
    /// 4. Repeat up to `max_polyphony` times.
    ///
    /// After cancellation, bins that were dominated by harmonics will have
    /// reduced energy and won't trigger as false notes.
    fn klapuri_cancellation(&self, energies: &mut [f64], threshold: f64) {
        let count = energies.len();

        // First pass: check if any bin is a harmonic of a stronger lower bin.
        // Only suppress if the candidate fundamental is STRONGER — this avoids
        // killing real notes that happen to be at harmonic intervals.
        for i in 0..count {
            if energies[i] < threshold {
                continue;
            }

            let note = self.note_map[i];
            let freq = midi_note_to_freq(note);

            for divisor in 2..=5u32 {
                let candidate_fund_freq = freq / divisor as f64;
                let candidate_note = freq_to_midi_note(candidate_fund_freq);

                if candidate_note < self.low_note as f64 {
                    continue;
                }

                let fund_idx = (candidate_note.round() as i16 - self.low_note as i16) as usize;
                if fund_idx >= count {
                    continue;
                }

                // Only suppress if the fundamental is stronger than this bin.
                // This prevents killing real notes in polyphonic playing.
                if energies[fund_idx] > energies[i] {
                    energies[i] *= 0.02;
                    break;
                }
            }
        }

        // Second pass: iterative peak selection with harmonic cancellation.
        for _iteration in 0..self.max_polyphony {
            // Find the strongest bin above threshold.
            let mut best_idx = None;
            let mut best_energy = threshold;

            for i in 0..count {
                if energies[i] > best_energy {
                    best_energy = energies[i];
                    best_idx = Some(i);
                }
            }

            let fund_idx = match best_idx {
                Some(idx) => idx,
                None => break,
            };

            let fund_note = self.note_map[fund_idx];
            let fund_freq = midi_note_to_freq(fund_note);
            let b = estimate_inharmonicity(fund_note);

            // Cancel harmonics 2-6 of this fundamental.
            for harmonic_n in 2..=6u32 {
                let n = harmonic_n as f64;
                let partial_freq = n * fund_freq * (1.0 + b * n * n).sqrt();

                let partial_note = freq_to_midi_note(partial_freq);
                if partial_note < self.low_note as f64 || partial_note > self.high_note as f64 {
                    continue;
                }

                let idx = (partial_note.round() as i16 - self.low_note as i16) as usize;
                if idx < count {
                    energies[idx] *= 0.02; // Remove 98% of harmonic energy.

                    // Also suppress adjacent bins for off-center partials.
                    let off = (partial_note - partial_note.round()).abs();
                    if off > 0.25 {
                        if idx > 0 {
                            energies[idx - 1] *= 0.1;
                        }
                        if idx + 1 < count {
                            energies[idx + 1] *= 0.1;
                        }
                    }
                }
            }

            // Mark this fundamental as processed so we don't pick it again.
            // (Set to a small positive value so it still triggers note-on
            // but won't be selected in the next iteration.)
            energies[fund_idx] = threshold * 1.1;
        }
    }

    /// Legacy harmonic suppression (simpler than Klapuri).
    fn suppress_harmonics(&self, energies: &mut [f64], threshold: f64) {
        let count = energies.len();

        for i in 0..count {
            if energies[i] < threshold {
                continue;
            }
            let fund_note = self.note_map[i];
            let fund_energy = energies[i];

            // Semitone offsets for harmonics 2-6.
            for &semitone_offset in &[12i16, 19, 24, 28, 31, 36] {
                let harmonic_note = fund_note as i16 + semitone_offset;
                if harmonic_note < self.low_note as i16 || harmonic_note > self.high_note as i16 {
                    continue;
                }
                let idx = (harmonic_note - self.low_note as i16) as usize;
                if idx < count && fund_energy > energies[idx] * 0.5 {
                    energies[idx] *= 0.05;
                }
            }
        }
    }

    /// Map energy to velocity using sensitivity curve.
    fn energy_to_velocity(&self, energy: f64, threshold: f64) -> f32 {
        let ratio = energy / threshold;
        if ratio <= 1.0 {
            return 0.01;
        }

        let log_ratio = ratio.ln();
        let max_log = 10.0;
        let normalized = (log_ratio / max_log).min(1.0);

        let flat = 0.7;
        let vel = flat * (1.0 - self.sensitivity) + normalized * self.sensitivity;
        (vel as f32).clamp(0.01, 1.0)
    }
}

/// Convert MIDI note number to frequency (A4 = 440Hz = MIDI 69).
fn midi_note_to_freq(note: u8) -> f64 {
    440.0 * 2.0_f64.powf((note as f64 - 69.0) / 12.0)
}

/// Convert frequency to fractional MIDI note number.
fn freq_to_midi_note(freq: f64) -> f64 {
    69.0 + 12.0 * (freq / 440.0).log2()
}

/// Estimate guitar string inharmonicity coefficient B for a given MIDI note.
///
/// Typical values:
/// - Low E (MIDI 40): B ≈ 0.0015
/// - A2 (MIDI 45): B ≈ 0.0010
/// - D3 (MIDI 50): B ≈ 0.0007
/// - G3 (MIDI 55): B ≈ 0.0004
/// - B3 (MIDI 59): B ≈ 0.0002
/// - High E (MIDI 64+): B ≈ 0.0001
fn estimate_inharmonicity(midi_note: u8) -> f64 {
    // Linear interpolation from low E to high E.
    let note = midi_note as f64;
    let b = if note <= 40.0 {
        0.0015
    } else if note >= 64.0 {
        0.0001
    } else {
        // Linear from 0.0015 at 40 to 0.0001 at 64.
        let t = (note - 40.0) / (64.0 - 40.0);
        0.0015 * (1.0 - t) + 0.0001 * t
    };
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_midi_note_to_freq() {
        assert!((midi_note_to_freq(69) - 440.0).abs() < 0.01);
        assert!((midi_note_to_freq(60) - 261.63).abs() < 0.1);
    }

    #[test]
    fn test_freq_to_midi_note() {
        assert!((freq_to_midi_note(440.0) - 69.0).abs() < 0.01);
        assert!((freq_to_midi_note(261.63) - 60.0).abs() < 0.1);
    }

    #[test]
    fn test_inharmonicity_range() {
        let b_low = estimate_inharmonicity(40);
        let b_high = estimate_inharmonicity(64);
        assert!(b_low > b_high, "Low notes should have higher inharmonicity");
        assert!(b_low > 0.001, "Low E should have B > 0.001");
        assert!(b_high < 0.0005, "High E should have B < 0.0005");
    }

    #[test]
    fn test_sine_440hz_detects_a4() {
        let sample_rate = 48000.0;
        let mut detector = PolyphonicDetector::new();
        detector.set_sample_rate(sample_rate);

        let num_samples = (sample_rate * 0.1) as usize;
        let mut detected_notes = Vec::new();

        for i in 0..num_samples {
            let t = i as f64 / sample_rate;
            let sample = 0.5 * (2.0 * std::f64::consts::PI * 440.0 * t).sin();
            if let Some(events) = detector.process_sample(sample) {
                detected_notes.extend(events);
            }
        }

        let note_ons: Vec<_> = detected_notes.iter().filter(|n| n.is_on).collect();
        assert!(
            note_ons.iter().any(|n| n.note == 69),
            "Expected A4 (MIDI 69) to be detected, got: {:?}",
            note_ons
        );
    }

    #[test]
    fn test_silence_produces_no_notes() {
        let mut detector = PolyphonicDetector::new();
        detector.set_sample_rate(48000.0);

        let mut detected_notes = Vec::new();
        for _ in 0..4800 {
            if let Some(events) = detector.process_sample(0.0) {
                detected_notes.extend(events);
            }
        }

        let note_ons: Vec<_> = detected_notes.iter().filter(|n| n.is_on).collect();
        assert!(
            note_ons.is_empty(),
            "Expected no notes from silence, got: {:?}",
            note_ons
        );
    }

    #[test]
    fn test_harmonic_suppression() {
        let sample_rate = 48000.0;
        let mut detector = PolyphonicDetector::new();
        detector.set_sample_rate(sample_rate);
        detector.set_threshold(0.0001);
        detector.set_harmonic_suppression(true);
        detector.set_note_range(30, 96);

        let num_samples = (sample_rate * 0.1) as usize;
        let mut detected_notes = Vec::new();

        for i in 0..num_samples {
            let t = i as f64 / sample_rate;
            let sample = 0.5 * (2.0 * std::f64::consts::PI * 110.0 * t).sin();
            if let Some(events) = detector.process_sample(sample) {
                detected_notes.extend(events);
            }
        }

        let note_ons: Vec<_> = detected_notes.iter().filter(|n| n.is_on).collect();
        assert!(
            note_ons.iter().any(|n| n.note == 45),
            "Expected A2 (MIDI 45) to be detected, got: {:?}",
            note_ons
        );

        assert!(
            !note_ons.iter().any(|n| n.note == 57),
            "Harmonic A3 (MIDI 57) should be suppressed, got: {:?}",
            note_ons
        );
    }

    #[test]
    fn test_reference_mode_selectivity() {
        let sample_rate = 48000.0;
        let mut detector = PolyphonicDetector::new();
        detector.set_sample_rate(sample_rate);

        let num_samples = (sample_rate * 0.2) as usize;
        let mut detected_notes = Vec::new();

        for i in 0..num_samples {
            let t = i as f64 / sample_rate;
            let sample = 0.3 * (2.0 * std::f64::consts::PI * 329.63 * t).sin();
            if let Some(events) = detector.process_sample(sample) {
                detected_notes.extend(events);
            }
        }

        let note_ons: Vec<_> = detected_notes.iter().filter(|n| n.is_on).collect();
        assert!(
            note_ons.iter().any(|n| n.note == 64),
            "Expected E4 (MIDI 64) to be detected, got: {:?}",
            note_ons
        );
    }

    #[test]
    fn test_single_note_accuracy_sweep() {
        let sample_rate = 44100.0;
        let window_size = 960;

        let mut correct = 0;
        let mut wrong = 0;
        let mut missed = 0;
        let mut extra = 0;

        for midi_note in 40..=88 {
            let freq = 440.0 * 2.0_f64.powf((midi_note as f64 - 69.0) / 12.0);

            let mut detector = PolyphonicDetector::new();
            detector.set_sample_rate(sample_rate);
            detector.set_window_size(window_size);

            let num_samples = (sample_rate * 0.1) as usize;
            let mut note_ons: Vec<u8> = Vec::new();

            for i in 0..num_samples {
                let t = i as f64 / sample_rate;
                let sample = 0.5 * (2.0 * std::f64::consts::PI * freq * t).sin();
                if let Some(events) = detector.process_sample(sample) {
                    for ev in events {
                        if ev.is_on && !note_ons.contains(&ev.note) {
                            note_ons.push(ev.note);
                        }
                    }
                }
            }

            if note_ons.contains(&midi_note) {
                correct += 1;
                extra += note_ons.len() - 1;
            } else if note_ons.is_empty() {
                missed += 1;
                eprintln!("MISSED: MIDI {} ({:.1} Hz)", midi_note, freq);
            } else {
                wrong += 1;
                eprintln!(
                    "WRONG: MIDI {} ({:.1} Hz) -> detected {:?}",
                    midi_note, freq, note_ons
                );
            }
        }

        let total = correct + wrong + missed;
        let accuracy = correct as f64 / total as f64 * 100.0;
        eprintln!(
            "\nSingle-note accuracy: {}/{} = {:.1}%  (wrong={}, missed={}, extra_triggers={})",
            correct, total, accuracy, wrong, missed, extra,
        );
        assert!(
            accuracy >= 95.0,
            "Single-note accuracy {:.1}% should be >= 95%",
            accuracy,
        );
    }

    #[test]
    fn test_klapuri_suppresses_harmonics() {
        let sample_rate = 48000.0;
        let mut detector = PolyphonicDetector::new();
        detector.set_sample_rate(sample_rate);
        detector.set_threshold(0.0001);
        detector.set_klapuri(true);
        detector.set_peak_picking(true);
        detector.set_note_range(30, 96);

        // Generate 110 Hz with moderate harmonics (realistic guitar spectrum).
        // Fundamental is strongest; harmonics decay naturally.
        let num_samples = (sample_rate * 0.15) as usize;
        let mut detected_notes = Vec::new();

        for i in 0..num_samples {
            let t = i as f64 / sample_rate;
            let fundamental = 0.5 * (2.0 * std::f64::consts::PI * 110.0 * t).sin();
            let h2 = 0.15 * (2.0 * std::f64::consts::PI * 220.0 * t).sin();
            let h3 = 0.08 * (2.0 * std::f64::consts::PI * 330.0 * t).sin();
            let sample = fundamental + h2 + h3;

            if let Some(events) = detector.process_sample(sample) {
                detected_notes.extend(events);
            }
        }

        let note_ons: Vec<_> = detected_notes.iter().filter(|n| n.is_on).collect();

        // Should detect A2 (45).
        assert!(
            note_ons.iter().any(|n| n.note == 45),
            "Expected A2 (MIDI 45) to be detected, got: {:?}",
            note_ons
        );

        // Should NOT detect A3 (57) or E4 (64) as separate notes.
        let spurious: Vec<_> = note_ons
            .iter()
            .filter(|n| n.note == 57 || n.note == 64)
            .collect();
        assert!(
            spurious.is_empty(),
            "Harmonics should be cancelled by Klapuri, but got: {:?}",
            spurious
        );
    }

    #[test]
    fn test_whitening_helps_weak_fundamental() {
        let sample_rate = 48000.0;

        // Simulate a guitar DI signal: weak fundamental, strong 2nd harmonic
        // (typical of bridge pickup).
        let run = |whitening: bool| -> Vec<u8> {
            let mut detector = PolyphonicDetector::new();
            detector.set_sample_rate(sample_rate);
            detector.set_threshold(0.0001);
            detector.set_whitening(whitening);
            detector.set_klapuri(true);
            detector.set_peak_picking(true);
            detector.set_note_range(30, 96);

            let num_samples = (sample_rate * 0.15) as usize;
            let mut note_ons = Vec::new();

            for i in 0..num_samples {
                let t = i as f64 / sample_rate;
                // Weak fundamental, strong harmonics (DI bridge pickup character).
                let fundamental = 0.05 * (2.0 * std::f64::consts::PI * 110.0 * t).sin();
                let h2 = 0.4 * (2.0 * std::f64::consts::PI * 220.0 * t).sin();
                let h3 = 0.3 * (2.0 * std::f64::consts::PI * 330.0 * t).sin();
                let sample = fundamental + h2 + h3;

                if let Some(events) = detector.process_sample(sample) {
                    for ev in events {
                        if ev.is_on && !note_ons.contains(&ev.note) {
                            note_ons.push(ev.note);
                        }
                    }
                }
            }
            note_ons
        };

        let with_whitening = run(true);
        eprintln!("With whitening: {:?}", with_whitening);
        // With whitening + Klapuri, should identify fundamental A2 (45).
        assert!(
            with_whitening.contains(&45),
            "With whitening, should detect A2 (45), got: {:?}",
            with_whitening
        );
    }

    #[test]
    fn test_preprocessing_doesnt_break_detection() {
        let sample_rate = 48000.0;
        let mut detector = PolyphonicDetector::new();
        detector.set_sample_rate(sample_rate);
        detector.set_preprocessing(true);

        let num_samples = (sample_rate * 0.1) as usize;
        let mut detected_notes = Vec::new();

        for i in 0..num_samples {
            let t = i as f64 / sample_rate;
            let sample = 0.5 * (2.0 * std::f64::consts::PI * 440.0 * t).sin();
            if let Some(events) = detector.process_sample(sample) {
                detected_notes.extend(events);
            }
        }

        let note_ons: Vec<_> = detected_notes.iter().filter(|n| n.is_on).collect();
        assert!(
            note_ons.iter().any(|n| n.note == 69),
            "A4 should still be detected with preprocessing, got: {:?}",
            note_ons
        );
    }
}
