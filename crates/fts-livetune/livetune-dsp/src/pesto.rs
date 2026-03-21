//! PESTO neural pitch detector.
//!
//! PESTO (Pitch Estimation with Self-supervised Transposition-equivariant
//! Objective) is a lightweight CNN-based pitch estimator that operates on
//! Constant-Q Transform (CQT) magnitude spectra.
//!
//! This module implements:
//! - Simplified CQT via windowed DFT at geometrically-spaced center
//!   frequencies (88 bins, A0–C8, 3 bins per semitone).
//! - Lazy ONNX model loading via the `ort` crate.
//! - Frame-buffered sample-by-sample interface matching the existing
//!   detector pattern (`tick` / `update` / `reset`).
//!
//! Without a loaded model file the detector always returns
//! [`PitchEstimate::unvoiced()`].

use std::f64::consts::TAU;

use crate::detector::PitchEstimate;

// ── Constants ────────────────────────────────────────────────────────────

/// Number of CQT bins (piano range A0–C8, 3 bins per semitone).
const CQT_BINS: usize = 88 * 3;
/// Bins per octave (12 semitones * 3 bins/semitone).
const BINS_PER_OCTAVE: usize = 36;
/// Quality factor Q = 1 / (2^(1/B) - 1) where B = bins per octave.
fn quality_factor() -> f64 {
    1.0 / (2.0_f64.powf(1.0 / BINS_PER_OCTAVE as f64) - 1.0)
}
/// Frame size in samples (at native sample rate).
const FRAME_SIZE: usize = 2048;
/// Hop size in samples.
const HOP_SIZE: usize = 512;
/// ONNX model input bins (one magnitude per semitone over the piano range).
const MODEL_INPUT_BINS: usize = 88;
/// MIDI note of the lowest CQT bin (A0 = 21).
const MIDI_LO: f64 = 21.0;

// ── Helpers ──────────────────────────────────────────────────────────────

/// Convert a MIDI note number to frequency in Hz.
#[inline]
fn midi_to_hz(midi: f64) -> f64 {
    440.0 * 2.0_f64.powf((midi - 69.0) / 12.0)
}

/// Hann window value at position `n` of length `len`.
#[inline]
fn hann(n: usize, len: usize) -> f64 {
    0.5 * (1.0 - (TAU * n as f64 / len as f64).cos())
}

// ── CQT state ────────────────────────────────────────────────────────────

/// Pre-computed per-bin CQT parameters.
struct CqtBin {
    /// Center frequency (Hz).
    freq: f64,
    /// Window length for this bin (capped to FRAME_SIZE).
    win_len: usize,
}

/// Pre-computed CQT kernel info for the current sample rate.
struct CqtState {
    bins: Vec<CqtBin>,
    sample_rate: f64,
}

impl CqtState {
    fn new(sample_rate: f64) -> Self {
        let q = quality_factor();
        let bins: Vec<CqtBin> = (0..CQT_BINS)
            .map(|k| {
                // MIDI note for this CQT bin (3 bins per semitone).
                let midi = MIDI_LO + k as f64 / 3.0;
                let freq = midi_to_hz(midi);
                let ideal_len = (q * sample_rate / freq).ceil() as usize;
                let win_len = ideal_len.min(FRAME_SIZE);
                CqtBin { freq, win_len }
            })
            .collect();
        Self { bins, sample_rate }
    }

    /// Compute the CQT magnitude spectrum from a frame of `FRAME_SIZE`
    /// samples.  Returns one magnitude per CQT bin.
    fn compute(&self, frame: &[f64]) -> Vec<f64> {
        debug_assert!(frame.len() >= FRAME_SIZE);
        let sr = self.sample_rate;

        self.bins
            .iter()
            .map(|bin| {
                let n = bin.win_len;
                // Use the most recent `n` samples of the frame.
                let offset = FRAME_SIZE - n;
                let mut re = 0.0;
                let mut im = 0.0;
                let phase_inc = -TAU * bin.freq / sr;
                for i in 0..n {
                    let w = hann(i, n);
                    let s = frame[offset + i] * w;
                    let phase = phase_inc * i as f64;
                    re += s * phase.cos();
                    im += s * phase.sin();
                }
                (re * re + im * im).sqrt() / n as f64
            })
            .collect()
    }

    /// Collapse `CQT_BINS` (3-per-semitone) down to `MODEL_INPUT_BINS` by
    /// taking the maximum within each semitone group of 3 bins.
    fn reduce_to_semitones(cqt: &[f64]) -> Vec<f32> {
        debug_assert_eq!(cqt.len(), CQT_BINS);
        (0..MODEL_INPUT_BINS)
            .map(|k| {
                let base = k * 3;
                let v = cqt[base].max(cqt[base + 1]).max(cqt[base + 2]);
                v as f32
            })
            .collect()
    }
}

// ── PESTO Detector ───────────────────────────────────────────────────────

/// PESTO neural pitch detector.
///
/// Buffers incoming audio sample-by-sample, computes a CQT every
/// [`HOP_SIZE`] samples, and runs an ONNX model (if loaded) to produce
/// a [`PitchEstimate`].
pub struct PestoDetector {
    /// Path to the ONNX model file (set before calling [`update`]).
    pub model_path: Option<String>,

    /// Lazily-initialised ONNX inference session.
    session: Option<ort::session::Session>,

    /// CQT pre-computation for the current sample rate.
    cqt: CqtState,

    /// Ring buffer holding the most recent `FRAME_SIZE` samples.
    buffer: Vec<f64>,
    /// Current write position in the ring buffer.
    write_pos: usize,
    /// Counter for hop-based frame triggering.
    hop_count: usize,
    /// Total samples fed (used to suppress output during initial fill).
    total_fed: usize,

    /// Current sample rate.
    sample_rate: f64,

    /// Most recent estimate (held between frames).
    last_estimate: PitchEstimate,
}

impl PestoDetector {
    /// Create a new detector. No ONNX model is loaded yet.
    pub fn new() -> Self {
        let sr = 48000.0;
        Self {
            model_path: None,
            session: None,
            cqt: CqtState::new(sr),
            buffer: vec![0.0; FRAME_SIZE],
            write_pos: 0,
            hop_count: 0,
            total_fed: 0,
            sample_rate: sr,
            last_estimate: PitchEstimate::unvoiced(),
        }
    }

    /// Update internal state for a new sample rate. Also attempts to load
    /// the ONNX model from `model_path` if not already loaded.
    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.cqt = CqtState::new(sample_rate);

        // Attempt lazy model load.
        if self.session.is_none() {
            if let Some(path) = &self.model_path {
                if let Ok(mut builder) = ort::session::Session::builder() {
                    if let Ok(session) = builder.commit_from_file(path) {
                        self.session = Some(session);
                    }
                }
            }
        }
    }

    /// Reset all internal state (buffers, counters, last estimate).
    /// Does *not* unload the ONNX session.
    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
        self.hop_count = 0;
        self.total_fed = 0;
        self.last_estimate = PitchEstimate::unvoiced();
    }

    /// Feed one sample and return the current pitch estimate.
    ///
    /// A new ONNX inference is triggered every [`HOP_SIZE`] samples.
    /// Between inferences the previous estimate is held.
    #[inline]
    pub fn tick(&mut self, input: f64) -> PitchEstimate {
        self.buffer[self.write_pos] = input;
        self.write_pos = (self.write_pos + 1) % FRAME_SIZE;
        self.hop_count += 1;
        self.total_fed += 1;

        if self.hop_count >= HOP_SIZE {
            self.hop_count = 0;
            self.last_estimate = self.analyze();
        }

        self.last_estimate
    }

    /// Analysis latency in samples (one full frame must be collected).
    pub fn latency(&self) -> usize {
        FRAME_SIZE
    }

    // ── internal ─────────────────────────────────────────────────────

    /// Build a contiguous frame from the ring buffer (oldest to newest).
    fn build_frame(&self) -> Vec<f64> {
        let mut frame = vec![0.0; FRAME_SIZE];
        for i in 0..FRAME_SIZE {
            frame[i] = self.buffer[(self.write_pos + i) % FRAME_SIZE];
        }
        frame
    }

    /// Run CQT + ONNX inference to produce a [`PitchEstimate`].
    fn analyze(&mut self) -> PitchEstimate {
        // Don't attempt analysis until we have a full frame.
        if self.total_fed < FRAME_SIZE {
            return PitchEstimate::unvoiced();
        }

        if self.session.is_none() {
            return PitchEstimate::unvoiced();
        }

        // Build frame and compute CQT (borrows self immutably).
        let frame = self.build_frame();
        let cqt_full = self.cqt.compute(&frame);
        let cqt_input = CqtState::reduce_to_semitones(&cqt_full);

        // Check for silence (all-zero CQT).
        let energy: f32 = cqt_input.iter().map(|v| v * v).sum();
        if energy < 1e-12 {
            return PitchEstimate::unvoiced();
        }

        // Build ONNX input tensor [1, 1, MODEL_INPUT_BINS] using ort's
        // Tensor::from_array with a (shape, data) tuple.
        let input_tensor = match ort::value::Tensor::from_array((
            vec![1usize, 1, MODEL_INPUT_BINS],
            cqt_input.into_boxed_slice(),
        )) {
            Ok(t) => t,
            Err(_) => return PitchEstimate::unvoiced(),
        };

        // Run inference (borrow session mutably now that CQT is done).
        let session = self.session.as_mut().unwrap();
        let outputs = match session.run(ort::inputs![input_tensor]) {
            Ok(o) => o,
            Err(_) => return PitchEstimate::unvoiced(),
        };

        // Interpret output.
        Self::interpret_output(&outputs)
    }

    /// Convert raw ONNX output into a `PitchEstimate`.
    ///
    /// Handles two common PESTO output formats:
    /// 1. A single continuous pitch value (scalar or \[1\]).
    /// 2. An activation vector of 128 bins (MIDI range 0-127).
    fn interpret_output(outputs: &ort::session::SessionOutputs<'_>) -> PitchEstimate {
        // Extract as f32 tensor from the first output.
        let (_shape, values) = match outputs[0].try_extract_tensor::<f32>() {
            Ok(pair) => pair,
            Err(_) => return PitchEstimate::unvoiced(),
        };

        if values.is_empty() {
            return PitchEstimate::unvoiced();
        }

        if values.len() == 1 || values.len() == 2 {
            // Scalar output: first value is MIDI pitch, optional second is
            // confidence.
            let midi = values[0] as f64;
            let confidence = if values.len() == 2 {
                (values[1] as f64).clamp(0.0, 1.0)
            } else {
                1.0
            };
            return Self::midi_to_estimate(midi, confidence);
        }

        // Multi-bin activation: find the peak and compute weighted average
        // around it for sub-bin precision.
        let (peak_idx, &peak_val) = values
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();

        if peak_val <= 0.0 {
            return PitchEstimate::unvoiced();
        }

        // Weighted average around peak (+/-2 bins).
        let lo = peak_idx.saturating_sub(2);
        let hi = (peak_idx + 2).min(values.len() - 1);
        let mut sum_w = 0.0_f64;
        let mut sum_wm = 0.0_f64;
        for i in lo..=hi {
            let w = values[i].max(0.0) as f64;
            sum_w += w;
            sum_wm += w * i as f64;
        }
        let midi = if sum_w > 1e-12 { sum_wm / sum_w } else { peak_idx as f64 };

        // Confidence: peak activation normalised against sum.
        let total: f64 = values.iter().map(|v| v.max(0.0) as f64).sum();
        let confidence = if total > 1e-12 {
            (peak_val as f64 / total).clamp(0.0, 1.0)
        } else {
            0.0
        };

        Self::midi_to_estimate(midi, confidence)
    }

    fn midi_to_estimate(midi: f64, confidence: f64) -> PitchEstimate {
        if !midi.is_finite() || midi <= 0.0 {
            return PitchEstimate::unvoiced();
        }
        let freq = 440.0 * 2.0_f64.powf((midi - 69.0) / 12.0);
        let semitones = midi - 69.0;
        PitchEstimate {
            freq_hz: freq,
            semitones,
            midi_note: midi,
            confidence,
        }
    }
}

impl Default for PestoDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_detector() -> PestoDetector {
        let mut d = PestoDetector::new();
        d.update(SR);
        d
    }

    fn generate_sine(freq: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f64 / SR).sin() * 0.8)
            .collect()
    }

    #[test]
    fn silence_returns_unvoiced() {
        let mut d = make_detector();
        for _ in 0..4800 {
            let est = d.tick(0.0);
            assert!(
                est.confidence < 0.01,
                "Silence should be unvoiced, got confidence={}",
                est.confidence
            );
            assert_eq!(est.freq_hz, 0.0);
        }
    }

    #[test]
    fn no_nan() {
        let mut d = make_detector();
        let signal = generate_sine(440.0, 48000);
        for &s in &signal {
            let est = d.tick(s);
            assert!(est.freq_hz.is_finite(), "freq_hz must be finite");
            assert!(est.semitones.is_finite(), "semitones must be finite");
            assert!(est.midi_note.is_finite(), "midi_note must be finite");
            assert!(est.confidence.is_finite(), "confidence must be finite");
        }
    }

    #[test]
    fn unvoiced_without_model() {
        let mut d = make_detector();
        // No model_path set, session is None.
        let signal = generate_sine(440.0, 8000);
        for &s in &signal {
            let est = d.tick(s);
            assert_eq!(
                est.freq_hz, 0.0,
                "Without a model, should always return unvoiced"
            );
            assert!(est.confidence < 0.01);
        }
    }

    #[test]
    fn cqt_bins_count() {
        let cqt = CqtState::new(SR);
        assert_eq!(cqt.bins.len(), CQT_BINS);
    }

    #[test]
    fn cqt_frequency_range() {
        let cqt = CqtState::new(SR);
        // First bin should be near A0 (27.5 Hz).
        assert!(
            (cqt.bins[0].freq - 27.5).abs() < 0.5,
            "First CQT bin should be ~A0, got {}",
            cqt.bins[0].freq
        );
        // Last bin should be near C8 (4186 Hz). With 3 sub-bins per
        // semitone the highest sub-bin overshoots C8 by ~2/3 semitone,
        // landing around 4350 Hz — that's expected.
        let last = &cqt.bins[CQT_BINS - 1];
        assert!(
            (last.freq - 4186.0).abs() < 200.0,
            "Last CQT bin should be near C8, got {}",
            last.freq
        );
    }

    #[test]
    fn reduce_to_semitones_shape() {
        let dummy = vec![1.0f64; CQT_BINS];
        let reduced = CqtState::reduce_to_semitones(&dummy);
        assert_eq!(reduced.len(), MODEL_INPUT_BINS);
    }

    #[test]
    fn midi_to_estimate_sanity() {
        let est = PestoDetector::midi_to_estimate(69.0, 0.9);
        assert!((est.freq_hz - 440.0).abs() < 0.01);
        assert!((est.midi_note - 69.0).abs() < 0.01);
        assert!((est.semitones).abs() < 0.01);
    }

    #[test]
    fn latency_value() {
        let d = make_detector();
        assert_eq!(d.latency(), FRAME_SIZE);
    }
}
