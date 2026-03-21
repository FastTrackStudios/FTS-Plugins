//! CREPE neural pitch detector (Convolutional Representation for Pitch Estimation).
//!
//! Uses a CNN on raw audio waveform frames (1024 samples at 16 kHz) to produce
//! a 360-bin pitch activation vector spanning C1-B7 (20 cents per bin).
//! The ONNX model is lazily loaded from a configurable path; if no model is
//! present, `tick()` returns `PitchEstimate::unvoiced()`.
//!
//! Reference: Kim, Salamon, Li & Bello, "CREPE: A Convolutional Representation
//! for Pitch Estimation", ICASSP 2018.

use crate::detector::PitchEstimate;

// ── Constants ────────────────────────────────────────────────────────────

/// CREPE operates at 16 kHz.
const CREPE_SR: f64 = 16000.0;

/// Number of input samples per CREPE frame.
const FRAME_SIZE: usize = 1024;

/// Hop size at 16 kHz (75% overlap -> hop = 256).
const HOP_SIZE: usize = 256;

/// Number of output pitch bins (360 bins, 20 cents each, 6 octaves C1-B7).
const NUM_BINS: usize = 360;

/// Reference frequency for A4.
const A4_HZ: f64 = 440.0;

// ── Helpers ──────────────────────────────────────────────────────────────

/// Convert a (possibly fractional) CREPE bin index to Hz.
/// Bin 0 = C1 ~ 32.70 Hz. Each bin is 20 cents.
/// `freq = 10.0 * 2^(bin / 60.0)`
#[inline]
fn bin_to_hz(bin: f64) -> f64 {
    10.0 * (2.0_f64).powf(bin / 60.0)
}

/// Weighted-average bin index around the peak for sub-bin accuracy.
fn weighted_peak(activations: &[f32; NUM_BINS]) -> (f64, f64) {
    // Find raw peak.
    let mut peak_idx = 0usize;
    let mut peak_val = activations[0];
    for (i, &v) in activations.iter().enumerate() {
        if v > peak_val {
            peak_val = v;
            peak_idx = i;
        }
    }

    if peak_val <= 0.0 {
        return (0.0, 0.0);
    }

    // Weighted average over a window of +/-4 bins around the peak.
    let radius = 4usize;
    let lo = peak_idx.saturating_sub(radius);
    let hi = (peak_idx + radius).min(NUM_BINS - 1);

    let mut sum_w = 0.0f64;
    let mut sum_wb = 0.0f64;
    for i in lo..=hi {
        let w = activations[i].max(0.0) as f64;
        sum_w += w;
        sum_wb += w * i as f64;
    }

    let refined_bin = if sum_w > 1e-12 {
        sum_wb / sum_w
    } else {
        peak_idx as f64
    };

    (refined_bin, peak_val as f64)
}

// ── Detector ─────────────────────────────────────────────────────────────

/// CREPE-based neural pitch detector backed by an ONNX Runtime session.
pub struct CrepeDetector {
    /// Path to the ONNX model file.
    model_path: Option<String>,
    /// Lazily-initialised ONNX session.
    session: Option<ort::session::Session>,

    /// Native sample rate of the host.
    sample_rate: f64,

    /// Previous native-rate sample (for linear interpolation during resampling).
    prev_native: f64,
    /// Fractional phase accumulator for resampling (native -> 16 kHz).
    /// Counts fractional native samples; when >= 1.0 we need a new native sample.
    resample_phase: f64,
    /// Resampling ratio: native_sr / 16000.
    resample_ratio: f64,

    /// Resampled 16 kHz frame buffer (length = FRAME_SIZE).
    frame_buf: [f32; FRAME_SIZE],
    /// Write position inside `frame_buf`.
    frame_pos: usize,

    /// Last pitch estimate (held between analysis frames).
    last_estimate: PitchEstimate,
}

impl CrepeDetector {
    pub fn new() -> Self {
        Self {
            model_path: None,
            session: None,
            sample_rate: 48000.0,
            prev_native: 0.0,
            resample_phase: 0.0,
            resample_ratio: 48000.0 / CREPE_SR,
            frame_buf: [0.0f32; FRAME_SIZE],
            frame_pos: 0,
            last_estimate: PitchEstimate::unvoiced(),
        }
    }

    /// Set the path to the CREPE ONNX model. Clears any previously loaded session.
    pub fn set_model_path(&mut self, path: &str) {
        self.model_path = Some(path.to_owned());
        self.session = None;
    }

    /// Update native sample rate and reconfigure internal buffers.
    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.resample_ratio = sample_rate / CREPE_SR;
    }

    /// Reset all internal state.
    pub fn reset(&mut self) {
        self.prev_native = 0.0;
        self.resample_phase = 0.0;
        self.frame_buf = [0.0f32; FRAME_SIZE];
        self.frame_pos = 0;
        self.last_estimate = PitchEstimate::unvoiced();
    }

    /// Feed one native-rate sample and return the current pitch estimate.
    #[inline]
    pub fn tick(&mut self, input: f64) -> PitchEstimate {
        // Lazy-load the ONNX model on first tick (if path is set and session absent).
        if self.session.is_none() && self.model_path.is_some() {
            self.try_load_model();
        }

        // If still no session, short-circuit.
        if self.session.is_none() {
            return PitchEstimate::unvoiced();
        }

        // Resample from native SR -> 16 kHz using linear interpolation.
        // Each native sample advances the resampler phase by 1.0.
        // We emit a 16 kHz sample every `resample_ratio` native samples.
        let cur = input;
        let prev = self.prev_native;
        self.prev_native = cur;

        self.resample_phase += 1.0;
        while self.resample_phase >= self.resample_ratio {
            self.resample_phase -= self.resample_ratio;
            // Fractional position within [prev, cur].
            // phase is how far past the emit point we are; the emit point
            // was at (resample_ratio - remaining_after_subtract) native samples ago.
            let t = 1.0 - self.resample_phase / self.resample_ratio.max(1e-12);
            let sample = (prev + t * (cur - prev)) as f32;
            self.push_16k_sample(sample);
        }

        self.last_estimate
    }

    /// Analysis latency in native-rate samples.
    pub fn latency(&self) -> usize {
        // FRAME_SIZE samples at 16 kHz, scaled to native rate.
        (FRAME_SIZE as f64 * self.resample_ratio).ceil() as usize
    }

    // ── Private ──────────────────────────────────────────────────────────

    fn try_load_model(&mut self) {
        if let Some(ref path) = self.model_path {
            match ort::session::Session::builder()
                .and_then(|mut b: ort::session::builder::SessionBuilder| {
                    b.commit_from_file(path)
                })
            {
                Ok(sess) => {
                    self.session = Some(sess);
                }
                Err(_) => {
                    // Model not available - remain in fallback mode.
                }
            }
        }
    }

    /// Push a single 16 kHz sample into the frame buffer and run analysis
    /// when the frame is full.
    fn push_16k_sample(&mut self, sample: f32) {
        self.frame_buf[self.frame_pos] = sample;
        self.frame_pos += 1;

        if self.frame_pos >= FRAME_SIZE {
            // Frame is full - run inference.
            self.last_estimate = self.run_inference();

            // Shift buffer by HOP_SIZE (retain 75% overlap).
            let keep = FRAME_SIZE - HOP_SIZE;
            self.frame_buf.copy_within(HOP_SIZE..FRAME_SIZE, 0);
            for i in keep..FRAME_SIZE {
                self.frame_buf[i] = 0.0;
            }
            self.frame_pos = keep;
        }
    }

    /// Run the CREPE ONNX model on the current frame buffer.
    fn run_inference(&mut self) -> PitchEstimate {
        let session = match &mut self.session {
            Some(s) => s,
            None => return PitchEstimate::unvoiced(),
        };

        // Normalise the frame (zero-mean, unit-variance).
        let mut normalised = [0.0f32; FRAME_SIZE];
        let mean: f32 =
            self.frame_buf.iter().copied().sum::<f32>() / FRAME_SIZE as f32;
        let var: f32 = self
            .frame_buf
            .iter()
            .map(|&s| (s - mean) * (s - mean))
            .sum::<f32>()
            / FRAME_SIZE as f32;
        let std_dev = var.sqrt().max(1e-8);
        for (i, &s) in self.frame_buf.iter().enumerate() {
            normalised[i] = (s - mean) / std_dev;
        }

        // Build input tensor [1, 1024].
        let input_tensor = match ort::value::Tensor::from_array((
            vec![1usize, FRAME_SIZE],
            normalised.to_vec().into_boxed_slice(),
        )) {
            Ok(t) => t,
            Err(_) => return PitchEstimate::unvoiced(),
        };

        let outputs = match session.run(ort::inputs![input_tensor]) {
            Ok(o) => o,
            Err(_) => return PitchEstimate::unvoiced(),
        };

        // Extract the first output tensor.
        let (_shape, values) = match outputs[0].try_extract_tensor::<f32>() {
            Ok(pair) => pair,
            Err(_) => return PitchEstimate::unvoiced(),
        };

        if values.len() < NUM_BINS {
            return PitchEstimate::unvoiced();
        }

        let mut activations = [0.0f32; NUM_BINS];
        activations.copy_from_slice(&values[..NUM_BINS]);

        // Compute weighted peak.
        let (refined_bin, confidence) = weighted_peak(&activations);
        if confidence < 1e-4 {
            return PitchEstimate::unvoiced();
        }

        let freq = bin_to_hz(refined_bin);
        let semitones = 12.0 * (freq / A4_HZ).log2();
        let midi_note = 69.0 + semitones;

        PitchEstimate {
            freq_hz: freq,
            semitones,
            midi_note,
            confidence,
        }
    }
}

impl Default for CrepeDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_detector() -> CrepeDetector {
        let mut d = CrepeDetector::new();
        d.update(SR);
        d
    }

    #[test]
    fn silence_in_silence_out() {
        let mut d = make_detector();
        // No model loaded -> always unvoiced, confidence == 0.
        for _ in 0..4800 {
            let est = d.tick(0.0);
            assert!(
                est.confidence < 0.01,
                "Silence should be unvoiced, confidence={}",
                est.confidence
            );
        }
    }

    #[test]
    fn no_nan() {
        let mut d = make_detector();
        // Without a model, feeding a sine should still produce finite values.
        let signal: Vec<f64> = (0..4800)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.8)
            .collect();
        for &s in &signal {
            let est = d.tick(s);
            assert!(est.freq_hz.is_finite(), "freq_hz is not finite");
            assert!(est.semitones.is_finite(), "semitones is not finite");
            assert!(est.midi_note.is_finite(), "midi_note is not finite");
            assert!(est.confidence.is_finite(), "confidence is not finite");
        }
    }

    #[test]
    fn produces_unvoiced_without_model() {
        let mut d = make_detector();
        // No model path set -> always unvoiced.
        let signal: Vec<f64> = (0..9600)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.8)
            .collect();
        for &s in &signal {
            let est = d.tick(s);
            assert_eq!(
                est.freq_hz, 0.0,
                "Without model should be unvoiced, got freq={}",
                est.freq_hz
            );
            assert_eq!(est.confidence, 0.0);
        }
    }
}
