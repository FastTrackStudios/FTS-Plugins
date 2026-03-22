//! FCPE (Fast Context-based Pitch Estimation) neural pitch detector.
//!
//! Uses an ONNX model to estimate F0 from mel-spectrogram frames.
//! Designed for real-time singing voice pitch detection.
//!
//! Pipeline: audio samples → Hann-windowed FFT → power spectrum → mel filterbank → log → ONNX → pitch
//!
//! Mel spectrogram parameters:
//! - FFT size: 2048, hop: 512
//! - 128 mel bins, frequency range 30–8000 Hz
//! - Triangular mel-spaced filterbank
//! - Log compression: ln(max(energy, 1e-5))

use crate::detector::PitchEstimate;
use std::f64::consts::PI;
use std::path::PathBuf;

// ── Constants ────────────────────────────────────────────────────────────

const FFT_SIZE: usize = 2048;
const HOP_SIZE: usize = 512;
const N_MELS: usize = 128;
const MEL_FMIN: f64 = 30.0;
const MEL_FMAX: f64 = 8000.0;
const LOG_FLOOR: f64 = 1e-5;

/// Number of pitch bins (CREPE-style: 360 bins, 20 cents each, C1–B7).
const N_PITCH_BINS: usize = 360;

/// Cents per bin.
const CENTS_PER_BIN: f64 = 20.0;

/// MIDI note of the lowest bin (C1 = 24).
const BASE_MIDI: f64 = 24.0;

/// Reference frequency for A4.
const A4_FREQ: f64 = 440.0;

// ── Mel conversion helpers ───────────────────────────────────────────────

/// Convert frequency in Hz to mel scale.
#[inline]
fn hz_to_mel(freq: f64) -> f64 {
    2595.0 * (1.0 + freq / 700.0).log10()
}

/// Convert mel value to frequency in Hz.
#[inline]
fn mel_to_hz(mel: f64) -> f64 {
    700.0 * (10.0f64.powf(mel / 2595.0) - 1.0)
}

// ── Mel Filterbank ───────────────────────────────────────────────────────

/// Precomputed mel filterbank matrix (N_MELS × fft_bins).
/// Stored as sparse: for each mel bin, store (start_index, weights).
struct MelFilterbank {
    /// For each mel bin: (start FFT bin index, triangular filter weights).
    filters: Vec<(usize, Vec<f64>)>,
}

impl MelFilterbank {
    fn new(sample_rate: f64) -> Self {
        let n_fft_bins = FFT_SIZE / 2 + 1;
        let mel_min = hz_to_mel(MEL_FMIN);
        let mel_max = hz_to_mel(MEL_FMAX);

        // N_MELS + 2 center frequencies (including edges).
        let n_points = N_MELS + 2;
        let mel_points: Vec<f64> = (0..n_points)
            .map(|i| mel_min + (mel_max - mel_min) * i as f64 / (n_points - 1) as f64)
            .collect();

        let hz_points: Vec<f64> = mel_points.iter().map(|&m| mel_to_hz(m)).collect();

        // Convert Hz to FFT bin indices (fractional).
        let bin_freqs: Vec<f64> = hz_points
            .iter()
            .map(|&f| f * FFT_SIZE as f64 / sample_rate)
            .collect();

        let mut filters = Vec::with_capacity(N_MELS);

        for m in 0..N_MELS {
            let left = bin_freqs[m];
            let center = bin_freqs[m + 1];
            let right = bin_freqs[m + 2];

            let start = left.floor() as usize;
            let end = (right.ceil() as usize).min(n_fft_bins - 1);

            if start >= end {
                filters.push((0, vec![]));
                continue;
            }

            let mut weights = Vec::with_capacity(end - start + 1);
            for k in start..=end {
                let freq = k as f64;
                let w = if freq <= left {
                    0.0
                } else if freq < center {
                    (freq - left) / (center - left).max(1e-10)
                } else if freq == center {
                    1.0
                } else if freq < right {
                    (right - freq) / (right - center).max(1e-10)
                } else {
                    0.0
                };
                weights.push(w.max(0.0));
            }

            filters.push((start, weights));
        }

        Self { filters }
    }

    /// Apply filterbank to a power spectrum, writing into `mel_out`.
    fn apply(&self, power_spectrum: &[f64], mel_out: &mut [f64; N_MELS]) {
        for (m, (start, weights)) in self.filters.iter().enumerate() {
            let mut sum = 0.0;
            for (i, &w) in weights.iter().enumerate() {
                let bin = start + i;
                if bin < power_spectrum.len() {
                    sum += w * power_spectrum[bin];
                }
            }
            mel_out[m] = sum;
        }
    }
}

// ── In-place real FFT (radix-2 DIT) ─────────────────────────────────────

/// Compute the magnitudes-squared of the FFT of a real signal.
/// `buf` must have length `FFT_SIZE`. Output written to `power_out` (length FFT_SIZE/2 + 1).
fn real_fft_power(buf: &[f64], power_out: &mut [f64]) {
    let n = buf.len();
    debug_assert!(n == FFT_SIZE);
    debug_assert!(n.is_power_of_two());

    let half = n / 2;

    // Pack real data into complex array of length N/2:
    // z[k] = buf[2k] + j*buf[2k+1]
    let mut re = vec![0.0f64; half];
    let mut im = vec![0.0f64; half];
    for k in 0..half {
        re[k] = buf[2 * k];
        im[k] = buf[2 * k + 1];
    }

    // In-place FFT on the half-length complex array.
    fft_in_place(&mut re, &mut im);

    // Unpack to get full N-point real FFT.
    // X[k] = 0.5*(Z[k] + Z*[N/2-k]) - 0.5j*W^k*(Z[k] - Z*[N/2-k])
    // where W = e^{-2*pi*j/N}
    let n_out = half + 1;
    debug_assert!(power_out.len() >= n_out);

    // DC and Nyquist.
    let x0_re = re[0] + im[0];
    let x_nyq_re = re[0] - im[0];
    power_out[0] = x0_re * x0_re;
    power_out[half] = x_nyq_re * x_nyq_re;

    for k in 1..half {
        let conj_idx = half - k;
        // Z[k]
        let zk_re = re[k];
        let zk_im = im[k];
        // Z*[N/2 - k]
        let zc_re = re[conj_idx];
        let zc_im = -im[conj_idx];

        // Even part: 0.5 * (Z[k] + Z*[N/2-k])
        let e_re = 0.5 * (zk_re + zc_re);
        let e_im = 0.5 * (zk_im + zc_im);

        // Odd part: 0.5 * (Z[k] - Z*[N/2-k])
        let o_re = 0.5 * (zk_re - zc_re);
        let o_im = 0.5 * (zk_im - zc_im);

        // Twiddle: W^k = cos(2*pi*k/N) - j*sin(2*pi*k/N)
        let angle = -2.0 * PI * k as f64 / n as f64;
        let tw_re = angle.cos();
        let tw_im = angle.sin();

        // -j * W^k * odd = -j * (tw_re + j*tw_im) * (o_re + j*o_im)
        //                 = (tw_im*o_re + tw_re*o_im) + j*(-tw_re*o_re + tw_im*o_im)
        // Wait, let me redo: -j*(a+jb) = b - ja
        // So: W^k * odd = (tw_re*o_re - tw_im*o_im) + j*(tw_re*o_im + tw_im*o_re)
        // Then -j * that = (tw_re*o_im + tw_im*o_re) + j*(-(tw_re*o_re - tw_im*o_im))
        let wo_re = tw_re * o_im + tw_im * o_re;
        let wo_im = -(tw_re * o_re - tw_im * o_im);

        let xk_re = e_re + wo_re;
        let xk_im = e_im + wo_im;

        power_out[k] = xk_re * xk_re + xk_im * xk_im;
    }
}

/// In-place radix-2 DIT FFT on complex data.
fn fft_in_place(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two());
    debug_assert_eq!(re.len(), im.len());

    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 0..n {
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
        let mut m = n >> 1;
        while m >= 1 && j >= m {
            j -= m;
            m >>= 1;
        }
        j += m;
    }

    // Butterfly stages.
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let angle_step = -2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..half {
                let angle = angle_step * k as f64;
                let tw_re = angle.cos();
                let tw_im = angle.sin();

                let a = start + k;
                let b = start + k + half;

                let t_re = tw_re * re[b] - tw_im * im[b];
                let t_im = tw_re * im[b] + tw_im * re[b];

                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
            }
        }
        len <<= 1;
    }
}

// ── Pitch bin conversion ─────────────────────────────────────────────────

/// Convert a pitch bin index (0..360) to frequency in Hz.
/// Bin 0 = C1, each bin = 20 cents.
#[inline]
fn bin_to_freq(bin: f64) -> f64 {
    let midi = BASE_MIDI + bin * CENTS_PER_BIN / 100.0;
    A4_FREQ * 2.0f64.powf((midi - 69.0) / 12.0)
}

/// Weighted average around a peak bin to refine frequency estimate.
fn weighted_peak_freq(activations: &[f32]) -> (f64, f64) {
    if activations.is_empty() {
        return (0.0, 0.0);
    }

    // Find peak bin.
    let mut peak_idx = 0;
    let mut peak_val = activations[0];
    for (i, &v) in activations.iter().enumerate() {
        if v > peak_val {
            peak_val = v;
            peak_idx = i;
        }
    }

    let confidence = peak_val as f64;
    if confidence < 0.1 {
        return (0.0, 0.0);
    }

    // Weighted average over a ±4-bin window around the peak.
    let radius = 4;
    let start = peak_idx.saturating_sub(radius);
    let end = (peak_idx + radius + 1).min(activations.len());

    let mut weighted_sum = 0.0f64;
    let mut weight_sum = 0.0f64;
    for i in start..end {
        let w = activations[i].max(0.0) as f64;
        weighted_sum += w * i as f64;
        weight_sum += w;
    }

    if weight_sum < 1e-10 {
        return (0.0, 0.0);
    }

    let refined_bin = weighted_sum / weight_sum;
    let freq = bin_to_freq(refined_bin);

    (freq, confidence)
}

// ── FCPE Detector ────────────────────────────────────────────────────────

/// FCPE neural pitch detector using an ONNX model.
///
/// Accumulates audio samples, computes mel spectrogram frames, and feeds
/// them to an ONNX model for pitch estimation. Falls back to unvoiced
/// output when no model is loaded.
pub struct FcpeDetector {
    /// Path to the ONNX model file.
    model_path: Option<PathBuf>,
    /// Lazily initialized ONNX session.
    session: Option<ort::session::Session>,

    /// Current sample rate.
    sample_rate: f64,
    /// Ring buffer for input samples.
    input_buffer: Vec<f64>,
    /// Write position in ring buffer.
    write_pos: usize,
    /// Samples accumulated since last hop.
    hop_count: usize,

    /// Hann window (precomputed, length FFT_SIZE).
    hann_window: Vec<f64>,
    /// Scratch buffer for windowed frame.
    windowed_frame: Vec<f64>,
    /// Power spectrum scratch (length FFT_SIZE/2 + 1).
    power_spectrum: Vec<f64>,
    /// Mel filterbank.
    mel_filterbank: Option<MelFilterbank>,
    /// Mel spectrogram frame (128 bins).
    mel_frame: [f64; N_MELS],

    /// Last pitch estimate (held between hops).
    last_estimate: PitchEstimate,
}

impl FcpeDetector {
    /// Create a new FCPE detector with no model loaded.
    pub fn new() -> Self {
        // Precompute Hann window.
        let hann_window: Vec<f64> = (0..FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / FFT_SIZE as f64).cos()))
            .collect();

        Self {
            model_path: None,
            session: None,
            sample_rate: 48000.0,
            input_buffer: vec![0.0; FFT_SIZE],
            write_pos: 0,
            hop_count: 0,
            hann_window,
            windowed_frame: vec![0.0; FFT_SIZE],
            power_spectrum: vec![0.0; FFT_SIZE / 2 + 1],
            mel_filterbank: None,
            mel_frame: [0.0; N_MELS],
            last_estimate: PitchEstimate::unvoiced(),
        }
    }

    /// Set the path to the ONNX model file.
    /// The model will be loaded lazily on the next `tick` call.
    pub fn set_model_path(&mut self, path: PathBuf) {
        self.model_path = Some(path);
        self.session = None; // Force re-load.
    }

    /// Update sample rate and rebuild mel filterbank.
    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.mel_filterbank = Some(MelFilterbank::new(sample_rate));
    }

    /// Reset all internal state.
    pub fn reset(&mut self) {
        self.input_buffer.fill(0.0);
        self.write_pos = 0;
        self.hop_count = 0;
        self.mel_frame = [0.0; N_MELS];
        self.last_estimate = PitchEstimate::unvoiced();
    }

    /// Feed one sample and return the current pitch estimate.
    /// A new ONNX inference runs every `HOP_SIZE` samples.
    #[inline]
    pub fn tick(&mut self, input: f64) -> PitchEstimate {
        // Write sample into ring buffer.
        self.input_buffer[self.write_pos] = input;
        self.write_pos = (self.write_pos + 1) % FFT_SIZE;
        self.hop_count += 1;

        if self.hop_count >= HOP_SIZE {
            self.hop_count = 0;
            self.last_estimate = self.analyze();
        }

        self.last_estimate
    }

    /// Analysis latency in samples (one full FFT frame).
    pub fn latency(&self) -> usize {
        FFT_SIZE
    }

    /// Attempt to lazily load the ONNX model.
    fn ensure_session(&mut self) -> bool {
        if self.session.is_some() {
            return true;
        }

        let path = match &self.model_path {
            Some(p) if p.exists() => p.clone(),
            _ => return false,
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

    /// Run mel spectrogram computation and ONNX inference.
    fn analyze(&mut self) -> PitchEstimate {
        // Compute mel spectrogram frame.
        self.compute_mel_frame();

        // If no model is available, return unvoiced.
        if !self.ensure_session() {
            return PitchEstimate::unvoiced();
        }

        // Run ONNX inference.
        match self.run_inference() {
            Some(est) => est,
            None => PitchEstimate::unvoiced(),
        }
    }

    /// Extract the current FFT frame from the ring buffer, apply Hann window,
    /// compute power spectrum, apply mel filterbank, and log-compress.
    fn compute_mel_frame(&mut self) {
        // Extract frame from ring buffer with Hann window.
        for i in 0..FFT_SIZE {
            let idx = (self.write_pos + i) % FFT_SIZE;
            self.windowed_frame[i] = self.input_buffer[idx] * self.hann_window[i];
        }

        // Compute power spectrum via FFT.
        real_fft_power(&self.windowed_frame, &mut self.power_spectrum);

        // Apply mel filterbank.
        if let Some(ref fb) = self.mel_filterbank {
            fb.apply(&self.power_spectrum, &mut self.mel_frame);
        }

        // Log compression.
        for m in self.mel_frame.iter_mut() {
            *m = m.max(LOG_FLOOR).ln();
        }
    }

    /// Run ONNX model inference and decode pitch.
    fn run_inference(&mut self) -> Option<PitchEstimate> {
        // Prepare input tensor: [1, 1, 128] (batch=1, T=1, mels=128).
        let mel_f32: Vec<f32> = self.mel_frame.iter().map(|&v| v as f32).collect();

        let input_tensor =
            ort::value::Tensor::from_array(([1i64, 1, N_MELS as i64], mel_f32)).ok()?;

        let session = self.session.as_mut()?;
        let outputs = session.run(ort::inputs![input_tensor]).ok()?;

        // Extract output tensor: returns (&Shape, &[f32]).
        let (_shape, activations) = outputs[0].try_extract_tensor::<f32>().ok()?;

        // If the output is per-frame pitch bins, decode via weighted peak.
        if activations.len() >= N_PITCH_BINS {
            // Take the first frame's activations.
            let frame_acts = &activations[..N_PITCH_BINS];
            let (freq, confidence) = weighted_peak_freq(frame_acts);

            if freq < 30.0 || confidence < 0.1 {
                return Some(PitchEstimate::unvoiced());
            }

            let semitones = 12.0 * (freq / A4_FREQ).log2();
            let midi_note = 69.0 + semitones;

            Some(PitchEstimate {
                freq_hz: freq,
                semitones,
                midi_note,
                confidence,
            })
        } else {
            None
        }
    }
}

impl Default for FcpeDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn make_detector() -> FcpeDetector {
        let mut d = FcpeDetector::new();
        d.update(SR);
        d
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
            assert!(
                est.freq_hz.abs() < 1e-10,
                "Silence should have zero freq, got {}",
                est.freq_hz
            );
        }
    }

    #[test]
    fn no_nan() {
        let mut d = make_detector();
        // Feed a sine wave — should not produce NaN even without a model.
        let freq = 440.0;
        for i in 0..48000 {
            let sample = (2.0 * PI * freq * i as f64 / SR).sin() * 0.8;
            let est = d.tick(sample);
            assert!(
                est.freq_hz.is_finite(),
                "freq_hz is not finite at sample {i}"
            );
            assert!(
                est.semitones.is_finite(),
                "semitones is not finite at sample {i}"
            );
            assert!(
                est.midi_note.is_finite(),
                "midi_note is not finite at sample {i}"
            );
            assert!(
                est.confidence.is_finite(),
                "confidence is not finite at sample {i}"
            );
        }
    }

    #[test]
    fn unvoiced_without_model() {
        let mut d = make_detector();
        // Without an ONNX model, every estimate should be unvoiced.
        let freq = 440.0;
        for i in 0..4800 {
            let sample = (2.0 * PI * freq * i as f64 / SR).sin() * 0.8;
            let est = d.tick(sample);
            assert!(
                est.confidence < 0.01,
                "Without model, confidence should be ~0, got {}",
                est.confidence
            );
        }
    }

    #[test]
    fn mel_conversion_roundtrip() {
        // Verify Hz → mel → Hz roundtrip.
        for &freq in &[30.0, 100.0, 440.0, 1000.0, 4000.0, 8000.0] {
            let mel = hz_to_mel(freq);
            let back = mel_to_hz(mel);
            assert!(
                (back - freq).abs() < 0.01,
                "Roundtrip failed for {freq}: got {back}"
            );
        }
    }

    #[test]
    fn bin_to_freq_c1() {
        // Bin 0 should be C1 (MIDI 24).
        let f = bin_to_freq(0.0);
        let expected = A4_FREQ * 2.0f64.powf((24.0 - 69.0) / 12.0); // ~32.7 Hz
        assert!(
            (f - expected).abs() < 0.1,
            "Bin 0 should be C1 (~32.7Hz), got {f}"
        );
    }

    #[test]
    fn mel_filterbank_shape() {
        let fb = MelFilterbank::new(48000.0);
        assert_eq!(fb.filters.len(), N_MELS);
        // Each filter should have non-empty weights (for reasonable sample rates).
        for (m, (_, weights)) in fb.filters.iter().enumerate() {
            assert!(
                !weights.is_empty(),
                "Mel filter {m} has no weights at 48kHz"
            );
        }
    }

    #[test]
    fn hann_window_endpoints() {
        let d = FcpeDetector::new();
        // Hann window should be ~0 at endpoints and ~1 at center.
        assert!(d.hann_window[0].abs() < 1e-10, "Hann start should be ~0");
        let mid = FFT_SIZE / 2;
        assert!(
            (d.hann_window[mid] - 1.0).abs() < 1e-10,
            "Hann center should be ~1"
        );
    }

    #[test]
    fn latency_is_fft_size() {
        let d = FcpeDetector::new();
        assert_eq!(d.latency(), FFT_SIZE);
    }
}
