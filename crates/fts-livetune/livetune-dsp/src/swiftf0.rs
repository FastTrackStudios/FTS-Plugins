//! SwiftF0 — lightweight neural F0 estimator for real-time pitch detection.
//!
//! Uses a compact MobileNet-style depthwise separable convolution architecture
//! on short 512-sample frames at 16 kHz for ultra-low-latency inference via
//! ONNX Runtime.
//!
//! Architecture:
//! - Input: raw audio frame, 512 samples at 16 kHz
//! - ONNX input shape: \[1, 512\] (batch, samples)
//! - Output: \[1, 128\] — pitch activations over MIDI note range 0–127
//! - Each bin = 1 semitone; peak bin → MIDI note
//! - Weighted average around peak for sub-semitone accuracy
//! - Confidence from peak activation (post-softmax)
//! - Hop = 256 samples at 16 kHz (50 % overlap)

use crate::detector::PitchEstimate;

/// Target sample rate for SwiftF0 model input.
const TARGET_SR: f64 = 16_000.0;

/// Frame size expected by the ONNX model (samples at 16 kHz).
const FRAME_SIZE: usize = 512;

/// Hop size at 16 kHz (50 % overlap).
const HOP_SIZE: usize = 256;

/// Number of output pitch bins (MIDI 0–127).
const NUM_BINS: usize = 128;

/// Lightweight, low-latency neural F0 estimator.
///
/// Wraps an ONNX model that maps 512-sample 16 kHz audio frames to 128
/// pitch-class activations.  When no model file is provided the detector
/// gracefully degrades and returns [`PitchEstimate::unvoiced`].
pub struct SwiftF0Detector {
    /// Path to the ONNX model file.
    model_path: Option<String>,
    /// Lazily initialised ONNX inference session.
    session: Option<ort::session::Session>,

    /// Native sample rate.
    sample_rate: f64,
    /// Resampling ratio: `TARGET_SR / sample_rate`.
    resample_ratio: f64,

    /// 16 kHz frame buffer for model input.
    frame_buf: Vec<f64>,
    /// Write position in `frame_buf`.
    frame_pos: usize,
    /// Counter for hop-based triggering (at 16 kHz rate).
    hop_count: usize,
    /// Fractional resampler phase.
    resample_phase: f64,
    /// Previous input sample for linear interpolation.
    prev_sample: f64,

    /// Most recent estimate (held between hops).
    last_estimate: PitchEstimate,
}

impl SwiftF0Detector {
    /// Create a new detector.  Call [`update`](Self::update) before use.
    pub fn new() -> Self {
        Self {
            model_path: None,
            session: None,
            sample_rate: 48_000.0,
            resample_ratio: TARGET_SR / 48_000.0,
            frame_buf: vec![0.0; FRAME_SIZE],
            frame_pos: 0,
            hop_count: 0,
            resample_phase: 0.0,
            prev_sample: 0.0,
            last_estimate: PitchEstimate::unvoiced(),
        }
    }

    /// Set the path to the ONNX model file.
    ///
    /// The session is lazily created on the first inference attempt.
    pub fn set_model_path(&mut self, path: &str) {
        self.model_path = Some(path.to_owned());
        self.session = None; // force re-init
    }

    /// Update internal state for a new sample rate.
    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.resample_ratio = TARGET_SR / sample_rate;
    }

    /// Reset all internal buffers and state.
    pub fn reset(&mut self) {
        self.frame_buf.fill(0.0);
        self.frame_pos = 0;
        self.hop_count = 0;
        self.resample_phase = 0.0;
        self.prev_sample = 0.0;
        self.last_estimate = PitchEstimate::unvoiced();
    }

    /// Feed one native-rate sample and return the current pitch estimate.
    ///
    /// Internally resamples to 16 kHz via linear interpolation, buffers a
    /// 512-sample frame, and runs inference every 256 resampled samples.
    #[inline]
    pub fn tick(&mut self, input: f64) -> PitchEstimate {
        // --- Linear-interpolation resampler (native SR → 16 kHz) ---
        self.resample_phase += self.resample_ratio;
        while self.resample_phase >= 1.0 {
            self.resample_phase -= 1.0;
            // Linear interp between prev and current.
            let t = self.resample_phase; // fractional overshoot
            let sample = self.prev_sample * t + input * (1.0 - t);
            self.push_resampled(sample);
        }
        self.prev_sample = input;

        self.last_estimate
    }

    /// Analysis latency in native-rate samples.
    ///
    /// The model needs one full 512-sample frame at 16 kHz before producing
    /// its first estimate.
    pub fn latency(&self) -> usize {
        // Convert 16 kHz frame size to native sample count.
        ((FRAME_SIZE as f64) / self.resample_ratio).ceil() as usize
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Push one 16 kHz sample into the frame buffer and trigger inference at
    /// each hop boundary.
    fn push_resampled(&mut self, sample: f64) {
        self.frame_buf[self.frame_pos] = sample;
        self.frame_pos = (self.frame_pos + 1) % FRAME_SIZE;
        self.hop_count += 1;

        if self.hop_count >= HOP_SIZE {
            self.hop_count = 0;
            self.last_estimate = self.run_inference();
        }
    }

    /// Ensure the ONNX session is loaded (lazy init).  Returns `true` if a
    /// session is available.
    fn ensure_session(&mut self) -> bool {
        if self.session.is_some() {
            return true;
        }
        let path = match &self.model_path {
            Some(p) => p.clone(),
            None => return false,
        };
        match ort::session::Session::builder()
            .and_then(|mut builder| builder.commit_from_file(&path))
        {
            Ok(session) => {
                self.session = Some(session);
                true
            }
            Err(_) => false,
        }
    }

    /// Run inference on the current frame buffer.
    fn run_inference(&mut self) -> PitchEstimate {
        if !self.ensure_session() {
            return PitchEstimate::unvoiced();
        }

        // Build a contiguous 512-sample array ordered correctly despite the
        // ring-buffer write position.
        let mut ordered = vec![0.0f32; FRAME_SIZE];
        for i in 0..FRAME_SIZE {
            let idx = (self.frame_pos + i) % FRAME_SIZE;
            ordered[i] = self.frame_buf[idx] as f32;
        }

        // Construct the ONNX input tensor [1, 512].
        let input_tensor =
            match ort::value::Tensor::from_array((vec![1i64, FRAME_SIZE as i64], ordered)) {
                Ok(t) => t,
                Err(_) => return PitchEstimate::unvoiced(),
            };

        let session = self.session.as_mut().unwrap();

        let outputs = match session.run(ort::inputs![input_tensor]) {
            Ok(o) => o,
            Err(_) => return PitchEstimate::unvoiced(),
        };

        // Extract first output tensor → [1, 128] f32.
        let output_value = &outputs[0];
        let tensor = match output_value.try_extract_tensor::<f32>() {
            Ok(t) => t,
            Err(_) => return PitchEstimate::unvoiced(),
        };

        let (_shape, data) = tensor;
        let activations: Vec<f32> = data.iter().copied().collect();
        if activations.len() < NUM_BINS {
            return PitchEstimate::unvoiced();
        }

        Self::decode_activations(&activations[..NUM_BINS])
    }

    /// Decode 128 pitch activations into a [`PitchEstimate`].
    fn decode_activations(activations: &[f32]) -> PitchEstimate {
        debug_assert!(activations.len() == NUM_BINS);

        // Softmax for normalised probabilities.
        let max_val = activations
            .iter()
            .cloned()
            .fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = activations.iter().map(|&a| (a - max_val).exp()).collect();
        let sum: f32 = exps.iter().sum();
        if sum <= 0.0 {
            return PitchEstimate::unvoiced();
        }
        let probs: Vec<f32> = exps.iter().map(|e| e / sum).collect();

        // Peak bin.
        let (peak_idx, &peak_prob) = probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();

        let confidence = peak_prob as f64;

        // Weighted average around peak (±2 bins) for sub-semitone accuracy.
        let lo = peak_idx.saturating_sub(2);
        let hi = (peak_idx + 2).min(NUM_BINS - 1);
        let mut weighted_sum = 0.0f64;
        let mut weight_total = 0.0f64;
        for i in lo..=hi {
            let w = probs[i] as f64;
            weighted_sum += i as f64 * w;
            weight_total += w;
        }
        let midi_note = if weight_total > 0.0 {
            weighted_sum / weight_total
        } else {
            peak_idx as f64
        };

        let semitones = midi_note - 69.0;
        let freq_hz = 440.0 * 2.0_f64.powf(semitones / 12.0);

        PitchEstimate {
            freq_hz,
            semitones,
            midi_note,
            confidence,
        }
    }
}

impl Default for SwiftF0Detector {
    fn default() -> Self {
        Self::new()
    }
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    fn make_detector() -> SwiftF0Detector {
        let mut d = SwiftF0Detector::new();
        d.update(SR);
        d
    }

    #[test]
    fn silence_returns_unvoiced() {
        let mut d = make_detector();
        // Feed enough silence to trigger several hops.
        for _ in 0..48_000 {
            let est = d.tick(0.0);
            assert!(
                est.confidence < 0.01,
                "Silence should be unvoiced, confidence = {}",
                est.confidence,
            );
            assert_eq!(est.freq_hz, 0.0);
        }
    }

    #[test]
    fn no_nan() {
        let mut d = make_detector();
        let pi2 = std::f64::consts::PI * 2.0;
        // Feed a sine wave through the detector (without a model, inference
        // returns unvoiced, but nothing should be NaN).
        for i in 0..48_000 {
            let sample = (pi2 * 440.0 * i as f64 / SR).sin() * 0.8;
            let est = d.tick(sample);
            assert!(est.freq_hz.is_finite(), "freq_hz is not finite");
            assert!(est.semitones.is_finite(), "semitones is not finite");
            assert!(est.midi_note.is_finite(), "midi_note is not finite");
            assert!(est.confidence.is_finite(), "confidence is not finite");
        }
    }

    #[test]
    fn unvoiced_without_model() {
        let mut d = make_detector();
        // No model path set — every estimate should be unvoiced.
        for i in 0..4_800 {
            let sample = (std::f64::consts::TAU * 440.0 * i as f64 / SR).sin();
            let est = d.tick(sample);
            assert_eq!(est.freq_hz, 0.0, "Should be unvoiced without model");
            assert_eq!(est.confidence, 0.0);
        }
    }
}
