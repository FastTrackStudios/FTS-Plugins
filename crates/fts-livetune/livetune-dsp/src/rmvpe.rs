//! RMVPE (Robust Model for Vocal Pitch Estimation) neural pitch detector.
//!
//! Uses a deep CNN on mel-spectrogram input with two output heads:
//! 1. Pitch activations: \[1, T, 360\] — 360 bins at 20 cents each (C1–B7)
//! 2. Voicing probability: \[1, T, 1\] — explicit voiced/unvoiced classification
//!
//! The ONNX model is lazily loaded from a file path. Without a model file the
//! detector returns `PitchEstimate::unvoiced()` for every sample.
//!
//! Internal processing runs at 16 kHz with 1024-sample FFT and 160-sample hop
//! (10 ms frames), using 128 log-mel bins spanning 30–8000 Hz.

use std::path::PathBuf;

use crate::detector::PitchEstimate;

// ── Constants ───────────────────────────────────────────────────────────

/// Internal sample rate for mel spectrogram computation.
const INTERNAL_SR: f64 = 16000.0;

/// FFT size for mel spectrogram.
const FFT_SIZE: usize = 1024;

/// Hop size in samples at 16 kHz (10 ms).
const HOP_SIZE: usize = 160;

/// Number of mel filter-bank bins.
const N_MELS: usize = 128;

/// Lower edge of mel filter bank (Hz).
const MEL_FMIN: f64 = 30.0;

/// Upper edge of mel filter bank (Hz).
const MEL_FMAX: f64 = 8000.0;

/// Number of pitch activation bins (20 cents each, C1–B7).
const N_PITCH_BINS: usize = 360;

/// Cents per pitch bin.
const CENTS_PER_BIN: f64 = 20.0;

/// MIDI note of the lowest bin (C1 = 24).
const BASE_MIDI: f64 = 24.0;

/// Minimum number of mel frames to accumulate before running inference.
const MIN_FRAMES: usize = 1;

/// Reference frequency for A4.
const A4_HZ: f64 = 440.0;

/// Floor value for log-mel computation.
const LOG_MEL_FLOOR: f64 = 1e-5;

// ── Mel filter bank ─────────────────────────────────────────────────────

/// Convert frequency in Hz to mel scale.
fn hz_to_mel(hz: f64) -> f64 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

/// Convert mel scale value to Hz.
fn mel_to_hz(mel: f64) -> f64 {
    700.0 * (10.0f64.powf(mel / 2595.0) - 1.0)
}

/// Precomputed triangular mel filter bank.
struct MelFilterBank {
    /// For each mel bin: (start_fft_bin, weights) where weights apply to
    /// consecutive FFT bins starting at start_fft_bin.
    filters: Vec<(usize, Vec<f64>)>,
}

impl MelFilterBank {
    fn new(n_fft: usize, sample_rate: f64, n_mels: usize, fmin: f64, fmax: f64) -> Self {
        let n_freqs = n_fft / 2 + 1;
        let mel_min = hz_to_mel(fmin);
        let mel_max = hz_to_mel(fmax);

        // n_mels + 2 center frequencies (including edges).
        let centers: Vec<f64> = (0..n_mels + 2)
            .map(|i| mel_to_hz(mel_min + (mel_max - mel_min) * i as f64 / (n_mels + 1) as f64))
            .collect();

        let fft_freqs: Vec<f64> = (0..n_freqs)
            .map(|i| i as f64 * sample_rate / n_fft as f64)
            .collect();

        let mut filters = Vec::with_capacity(n_mels);
        for m in 0..n_mels {
            let left = centers[m];
            let center = centers[m + 1];
            let right = centers[m + 2];

            let mut start = n_freqs;
            let mut weights = Vec::new();

            for (k, &freq) in fft_freqs.iter().enumerate() {
                let w = if freq >= left && freq <= center {
                    (freq - left) / (center - left).max(1e-10)
                } else if freq > center && freq <= right {
                    (right - freq) / (right - center).max(1e-10)
                } else {
                    0.0
                };

                if w > 0.0 {
                    if start == n_freqs {
                        start = k;
                    }
                    weights.push(w);
                } else if start < n_freqs {
                    // Past the right edge of this filter.
                    break;
                }
            }

            if start == n_freqs {
                start = 0;
            }
            filters.push((start, weights));
        }

        Self { filters }
    }

    /// Apply the filter bank to a power spectrum, producing `n_mels` values.
    fn apply(&self, power_spectrum: &[f64], out: &mut [f64]) {
        for (m, (start, weights)) in self.filters.iter().enumerate() {
            let mut sum = 0.0;
            for (i, &w) in weights.iter().enumerate() {
                let bin = start + i;
                if bin < power_spectrum.len() {
                    sum += w * power_spectrum[bin];
                }
            }
            out[m] = sum;
        }
    }
}

// ── Simple DFT (no FFI dependency) ──────────────────────────────────────

/// Compute the power spectrum of a real-valued windowed frame using a naive DFT.
/// Output length: n_fft / 2 + 1.
fn power_spectrum(frame: &[f64], n_fft: usize, out: &mut [f64]) {
    let n_out = n_fft / 2 + 1;
    debug_assert!(out.len() >= n_out);
    let n = frame.len().min(n_fft);

    for k in 0..n_out {
        let mut re = 0.0;
        let mut im = 0.0;
        let w = std::f64::consts::TAU * k as f64 / n_fft as f64;
        for (i, &x) in frame.iter().enumerate().take(n) {
            let angle = w * i as f64;
            re += x * angle.cos();
            im -= x * angle.sin();
        }
        out[k] = re * re + im * im;
    }
}

/// Hann window coefficients.
fn hann_window(size: usize) -> Vec<f64> {
    (0..size)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / size as f64;
            0.5 * (1.0 - t.cos())
        })
        .collect()
}

// ── Pitch decoding helpers ──────────────────────────────────────────────

/// Decode pitch activations (360 bins, 20 cents each starting at C1) into
/// frequency and confidence.  Returns `(freq_hz, peak_activation)`.
fn decode_pitch_activations(activations: &[f32]) -> (f64, f64) {
    if activations.len() < N_PITCH_BINS {
        return (0.0, 0.0);
    }

    let acts = &activations[..N_PITCH_BINS];

    // Find peak bin.
    let mut peak_val: f32 = -1.0;
    let mut peak_bin: usize = 0;
    for (b, &v) in acts.iter().enumerate() {
        if v > peak_val {
            peak_val = v;
            peak_bin = b;
        }
    }

    if peak_val <= 0.0 {
        return (0.0, 0.0);
    }

    // Weighted average around peak for sub-bin accuracy.
    let window = 4usize;
    let lo = peak_bin.saturating_sub(window);
    let hi = (peak_bin + window + 1).min(N_PITCH_BINS);
    let mut weighted_sum = 0.0f64;
    let mut weight_sum = 0.0f64;
    for b in lo..hi {
        let w = acts[b] as f64;
        if w > 0.0 {
            weighted_sum += w * b as f64;
            weight_sum += w;
        }
    }
    let refined_bin = if weight_sum > 0.0 {
        weighted_sum / weight_sum
    } else {
        peak_bin as f64
    };

    // Convert bin to frequency.
    // Bin 0 = C1 (MIDI 24), each bin = 20 cents.
    let midi = BASE_MIDI + refined_bin * CENTS_PER_BIN / 100.0;
    let freq = A4_HZ * 2.0f64.powf((midi - 69.0) / 12.0);

    (freq, peak_val as f64)
}

// ── RMVPE Detector ──────────────────────────────────────────────────────

/// RMVPE neural pitch detector.
///
/// Lazily loads an ONNX model from [`model_path`]. Without a model, all
/// estimates are unvoiced.
pub struct RmvpeDetector {
    /// Path to the ONNX model file.
    pub model_path: Option<PathBuf>,

    /// Reference A4 frequency (default 440 Hz).
    pub a_freq: f64,

    // ONNX session (lazy).
    session: Option<ort::session::Session>,
    session_init_attempted: bool,

    // Sample rate bookkeeping.
    native_sr: f64,

    // Resampler state: accumulate input at native SR, downsample to 16 kHz.
    resample_phase: f64,
    resample_ratio: f64,
    resample_prev: f64,

    // Input ring buffer at 16 kHz.
    input_buf: Vec<f64>,
    input_write: usize,
    samples_since_hop: usize,

    // Mel spectrogram.
    mel_bank: MelFilterBank,
    hann: Vec<f64>,
    fft_scratch: Vec<f64>,
    windowed_frame: Vec<f64>,
    mel_frame: Vec<f64>,

    // Accumulated mel frames for batched inference.
    mel_frames: Vec<Vec<f64>>,

    // Output.
    last_estimate: PitchEstimate,
}

impl RmvpeDetector {
    /// Create a new detector. Call [`update`] before processing audio.
    pub fn new() -> Self {
        let mel_bank = MelFilterBank::new(FFT_SIZE, INTERNAL_SR, N_MELS, MEL_FMIN, MEL_FMAX);
        let hann = hann_window(FFT_SIZE);

        Self {
            model_path: None,
            a_freq: A4_HZ,
            session: None,
            session_init_attempted: false,
            native_sr: 48000.0,
            resample_phase: 0.0,
            resample_ratio: 48000.0 / INTERNAL_SR,
            resample_prev: 0.0,
            input_buf: vec![0.0; FFT_SIZE],
            input_write: 0,
            samples_since_hop: 0,
            mel_bank,
            hann,
            fft_scratch: vec![0.0; FFT_SIZE / 2 + 1],
            windowed_frame: vec![0.0; FFT_SIZE],
            mel_frame: vec![0.0; N_MELS],
            mel_frames: Vec::new(),
            last_estimate: PitchEstimate::unvoiced(),
        }
    }

    /// Update the detector for a new sample rate.
    pub fn update(&mut self, sample_rate: f64) {
        self.native_sr = sample_rate;
        self.resample_ratio = sample_rate / INTERNAL_SR;
        self.reset();
    }

    /// Reset all internal state (keeps model loaded).
    pub fn reset(&mut self) {
        self.resample_phase = 0.0;
        self.resample_prev = 0.0;
        self.input_buf.fill(0.0);
        self.input_write = 0;
        self.samples_since_hop = 0;
        self.mel_frames.clear();
        self.last_estimate = PitchEstimate::unvoiced();
    }

    /// Feed one sample at the native sample rate and return the current pitch
    /// estimate. A new estimate is produced every hop (10 ms at 16 kHz).
    #[inline]
    pub fn tick(&mut self, input: f64) -> PitchEstimate {
        // --- Resample to 16 kHz via linear interpolation ---
        self.resample_phase += 1.0;
        while self.resample_phase >= self.resample_ratio {
            self.resample_phase -= self.resample_ratio;
            let frac = self.resample_phase / self.resample_ratio;
            let sample = self.resample_prev + frac * (input - self.resample_prev);
            self.push_internal_sample(sample);
        }
        self.resample_prev = input;

        self.last_estimate
    }

    /// Analysis latency in samples at the native sample rate.
    pub fn latency(&self) -> usize {
        // FFT_SIZE samples at 16 kHz, scaled to native SR.
        ((FFT_SIZE as f64) * self.resample_ratio).ceil() as usize
    }

    // ── Internal ────────────────────────────────────────────────────────

    /// Push one 16 kHz sample into the ring buffer and compute mel frames.
    fn push_internal_sample(&mut self, sample: f64) {
        let buf_len = self.input_buf.len();
        self.input_buf[self.input_write] = sample;
        self.input_write = (self.input_write + 1) % buf_len;
        self.samples_since_hop += 1;

        if self.samples_since_hop >= HOP_SIZE {
            self.samples_since_hop = 0;
            self.compute_mel_frame();
            self.run_inference_if_ready();
        }
    }

    /// Compute one mel spectrogram frame from the current ring buffer.
    fn compute_mel_frame(&mut self) {
        let buf_len = self.input_buf.len();

        // Extract windowed frame from ring buffer.
        for i in 0..FFT_SIZE {
            let idx = (self.input_write + buf_len - FFT_SIZE + i) % buf_len;
            self.windowed_frame[i] = self.input_buf[idx] * self.hann[i];
        }

        // Power spectrum.
        power_spectrum(&self.windowed_frame, FFT_SIZE, &mut self.fft_scratch);

        // Mel filter bank.
        self.mel_bank.apply(&self.fft_scratch, &mut self.mel_frame);

        // Log-mel.
        let mut frame = vec![0.0; N_MELS];
        for i in 0..N_MELS {
            frame[i] = self.mel_frame[i].max(LOG_MEL_FLOOR).ln();
        }

        self.mel_frames.push(frame);
    }

    /// Try to lazily initialise the ONNX session.
    fn ensure_session(&mut self) {
        if self.session.is_some() || self.session_init_attempted {
            return;
        }
        self.session_init_attempted = true;

        let path = match &self.model_path {
            Some(p) if p.exists() => p.clone(),
            _ => return,
        };

        match ort::session::Session::builder().and_then(|mut b| b.commit_from_file(&path)) {
            Ok(s) => {
                self.session = Some(s);
            }
            Err(e) => {
                eprintln!("RMVPE: failed to load ONNX model: {e}");
            }
        }
    }

    /// If we have accumulated enough mel frames, run the ONNX model.
    fn run_inference_if_ready(&mut self) {
        if self.mel_frames.len() < MIN_FRAMES {
            return;
        }

        self.ensure_session();

        if self.session.is_none() {
            // No model — discard frames (keep last to avoid unbounded growth).
            if self.mel_frames.len() > 1 {
                let last = self.mel_frames.pop().unwrap();
                self.mel_frames.clear();
                self.mel_frames.push(last);
            }
            self.last_estimate = PitchEstimate::unvoiced();
            return;
        }

        let n_frames = self.mel_frames.len();

        // Flatten mel frames into f32 for ONNX input [1, T, 128].
        let mel_f32: Vec<f32> = self
            .mel_frames
            .iter()
            .flat_map(|f| f.iter().map(|&v| v as f32))
            .collect();

        // Keep last frame for overlap, clear the rest.
        let last = self.mel_frames.pop().unwrap();
        self.mel_frames.clear();
        self.mel_frames.push(last);

        // Build ONNX input tensor using (shape, data) tuple.
        let input_tensor =
            match ort::value::Tensor::<f32>::from_array(([1usize, n_frames, N_MELS], mel_f32)) {
                Ok(t) => t,
                Err(_) => {
                    self.last_estimate = PitchEstimate::unvoiced();
                    return;
                }
            };

        let session = self.session.as_mut().unwrap();
        let outputs = match session.run(ort::inputs![input_tensor]) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("RMVPE inference error: {e}");
                self.last_estimate = PitchEstimate::unvoiced();
                return;
            }
        };

        // Output 0: pitch activations [1, T, 360].
        let (_pitch_shape, pitch_slice) = match outputs[0].try_extract_tensor::<f32>() {
            Ok(t) => t,
            Err(_) => {
                self.last_estimate = PitchEstimate::unvoiced();
                return;
            }
        };

        // Use the last frame's activations (offset into the flat slice).
        let frame_offset = if n_frames > 1 {
            (n_frames - 1) * N_PITCH_BINS
        } else {
            0
        };
        let acts = if frame_offset + N_PITCH_BINS <= pitch_slice.len() {
            &pitch_slice[frame_offset..frame_offset + N_PITCH_BINS]
        } else if pitch_slice.len() >= N_PITCH_BINS {
            &pitch_slice[pitch_slice.len() - N_PITCH_BINS..]
        } else {
            self.last_estimate = PitchEstimate::unvoiced();
            return;
        };

        let (freq, peak_activation) = decode_pitch_activations(acts);
        if freq < 30.0 || peak_activation < 0.01 {
            self.last_estimate = PitchEstimate::unvoiced();
            return;
        }

        // Output 1 (optional): voicing probability [1, T, 1].
        let voicing_prob = outputs
            .get("voicing")
            .and_then(|v| v.try_extract_tensor::<f32>().ok())
            .and_then(|(_shape, s)| {
                let idx = if n_frames > 1 { n_frames - 1 } else { 0 };
                s.get(idx).map(|&v| (v as f64).clamp(0.0, 1.0))
            })
            .unwrap_or_else(|| peak_activation.clamp(0.0, 1.0));

        let confidence = (voicing_prob * peak_activation).clamp(0.0, 1.0);
        if confidence < 0.01 {
            self.last_estimate = PitchEstimate::unvoiced();
            return;
        }

        let semitones = 12.0 * (freq / self.a_freq).log2();
        let midi_note = 69.0 + 12.0 * (freq / A4_HZ).log2();

        self.last_estimate = PitchEstimate {
            freq_hz: freq,
            semitones,
            midi_note,
            confidence,
        };
    }
}

impl Default for RmvpeDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn make_detector() -> RmvpeDetector {
        let mut d = RmvpeDetector::new();
        d.update(SR);
        d
    }

    #[test]
    fn silence_returns_unvoiced() {
        let mut d = make_detector();
        for _ in 0..48000 {
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
        let signal: Vec<f64> = (0..48000)
            .map(|i| (std::f64::consts::TAU * 220.0 * i as f64 / SR).sin() * 0.8)
            .collect();

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
        // No model_path set — every tick should return unvoiced.
        let signal: Vec<f64> = (0..48000)
            .map(|i| (std::f64::consts::TAU * 440.0 * i as f64 / SR).sin() * 0.8)
            .collect();

        for &s in &signal {
            let est = d.tick(s);
            assert!(
                est.confidence < 0.01,
                "Without model, should be unvoiced, got confidence={}",
                est.confidence
            );
        }
    }
}
