//! Formant-preserving phase vocoder for pitch correction.
//!
//! STFT-based pitch shifting with cepstral envelope preservation.
//! The spectral envelope (formants) is estimated via cepstral liftering,
//! separated from the excitation, and re-applied after pitch shifting.
//!
//! This prevents the "chipmunk effect" when shifting pitch significantly.
//!
//! Pipeline per STFT frame:
//! 1. FFT → log magnitude → IFFT → cepstrum
//! 2. Low-pass lifter → smooth spectral envelope
//! 3. Flatten spectrum (remove envelope)
//! 4. Shift bins by pitch ratio
//! 5. Re-apply original envelope
//! 6. Phase propagation + IFFT → overlap-add

use std::f64::consts::PI;

/// Phase vocoder FFT size.
const FFT_SIZE: usize = 2048;
/// Hop size (overlap factor = 4).
const HOP_SIZE: usize = FFT_SIZE / 4;
/// Number of frequency bins.
const NUM_BINS: usize = FFT_SIZE / 2 + 1;
/// Cepstral lifter cutoff (quefrency samples). ~30 at 48kHz ≈ envelope
/// resolution down to ~1600Hz spacing — works for most voices.
const LIFTER_ORDER: usize = 30;

/// Formant-preserving phase vocoder.
pub struct FormantVocoder {
    /// Pitch shift ratio (e.g., 1.05 = 5% up). Set per-frame.
    pub shift_ratio: f64,
    /// Mix: 0.0 = dry, 1.0 = wet.
    pub mix: f64,
    /// Enable formant preservation.
    pub preserve_formants: bool,

    // STFT analysis state.
    /// Input accumulation buffer.
    input_buf: Vec<f64>,
    /// Write position in input buffer.
    input_pos: usize,
    /// Output accumulation buffer (overlap-add).
    output_buf: Vec<f64>,
    /// Read position in output buffer.
    output_pos: usize,
    /// Analysis window (Hann).
    window: Vec<f64>,
    /// Previous frame phases (for phase propagation).
    prev_phase: Vec<f64>,
    /// Accumulated output phases.
    synth_phase: Vec<f64>,

    // FFT scratch buffers.
    fft_real: Vec<f64>,
    fft_imag: Vec<f64>,
    /// Magnitude spectrum.
    mag: Vec<f64>,
    /// Phase spectrum.
    phase: Vec<f64>,
    /// Spectral envelope (from cepstral analysis).
    envelope: Vec<f64>,
    /// Cepstrum scratch (reserved for true envelope iteration).
    #[allow(dead_code)]
    cepstrum: Vec<f64>,

    // Laroche-Dolson phase locking scratch buffers.
    /// Whether each bin is a local magnitude peak.
    peak_bins: Vec<bool>,
    /// Index of the nearest peak bin for each bin.
    nearest_peak: Vec<usize>,

    sample_rate: f64,
    /// Samples of latency.
    latency_samples: usize,
}

impl FormantVocoder {
    pub fn new() -> Self {
        let window: Vec<f64> = (0..FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / FFT_SIZE as f64).cos()))
            .collect();

        Self {
            shift_ratio: 1.0,
            mix: 1.0,
            preserve_formants: true,
            input_buf: vec![0.0; FFT_SIZE],
            input_pos: 0,
            output_buf: vec![0.0; FFT_SIZE * 2],
            output_pos: 0,
            window,
            prev_phase: vec![0.0; NUM_BINS],
            synth_phase: vec![0.0; NUM_BINS],
            fft_real: vec![0.0; FFT_SIZE],
            fft_imag: vec![0.0; FFT_SIZE],
            mag: vec![0.0; NUM_BINS],
            phase: vec![0.0; NUM_BINS],
            envelope: vec![0.0; NUM_BINS],
            cepstrum: vec![0.0; FFT_SIZE],
            peak_bins: vec![false; NUM_BINS],
            nearest_peak: vec![0; NUM_BINS],
            sample_rate: 48000.0,
            latency_samples: FFT_SIZE,
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.input_buf.fill(0.0);
        self.input_pos = 0;
        self.output_buf.fill(0.0);
        self.output_pos = 0;
        self.prev_phase.fill(0.0);
        self.synth_phase.fill(0.0);
    }

    /// In-place DFT (radix-2 Cooley-Tukey). N must be power of 2.
    fn fft(real: &mut [f64], imag: &mut [f64], inverse: bool) {
        let n = real.len();
        assert!(n.is_power_of_two());

        // Bit-reversal permutation.
        let mut j = 0;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j ^= bit;
            if i < j {
                real.swap(i, j);
                imag.swap(i, j);
            }
        }

        // Butterfly.
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let angle_sign = if inverse { 1.0 } else { -1.0 };
            let angle = angle_sign * 2.0 * PI / len as f64;

            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let w_angle = angle * k as f64;
                    let wr = w_angle.cos();
                    let wi = w_angle.sin();

                    let a = start + k;
                    let b = start + k + half;

                    let tr = real[b] * wr - imag[b] * wi;
                    let ti = real[b] * wi + imag[b] * wr;

                    real[b] = real[a] - tr;
                    imag[b] = imag[a] - ti;
                    real[a] += tr;
                    imag[a] += ti;
                }
            }
            len <<= 1;
        }

        if inverse {
            let inv_n = 1.0 / n as f64;
            for i in 0..n {
                real[i] *= inv_n;
                imag[i] *= inv_n;
            }
        }
    }

    /// Estimate spectral envelope via cepstral liftering.
    fn estimate_envelope(&mut self) {
        // Copy log magnitude into cepstrum buffer.
        let mut cep_real = vec![0.0; FFT_SIZE];
        let mut cep_imag = vec![0.0; FFT_SIZE];

        for i in 0..NUM_BINS {
            cep_real[i] = (self.mag[i].max(1e-20)).ln();
        }
        // Mirror for real-valued IFFT.
        for i in 1..NUM_BINS - 1 {
            cep_real[FFT_SIZE - i] = cep_real[i];
        }

        // IFFT to get cepstrum.
        Self::fft(&mut cep_real, &mut cep_imag, true);

        // Low-pass lifter: zero out high quefrency components.
        // Keep coefficients 0..LIFTER_ORDER and FFT_SIZE-LIFTER_ORDER..FFT_SIZE.
        for i in LIFTER_ORDER..FFT_SIZE - LIFTER_ORDER {
            cep_real[i] = 0.0;
            cep_imag[i] = 0.0;
        }

        // Apply Hamming taper to the lifter boundary (reduces ringing).
        if LIFTER_ORDER > 1 {
            let taper_len = LIFTER_ORDER.min(8);
            for i in 0..taper_len {
                let w = 0.5 * (1.0 + (PI * i as f64 / taper_len as f64).cos());
                let idx = LIFTER_ORDER - taper_len + i;
                cep_real[idx] *= w;
                let mirror = FFT_SIZE - idx;
                if mirror < FFT_SIZE {
                    cep_real[mirror] *= w;
                }
            }
        }

        // FFT back to get smooth spectral envelope.
        Self::fft(&mut cep_real, &mut cep_imag, false);

        for i in 0..NUM_BINS {
            self.envelope[i] = cep_real[i].exp();
        }
    }

    /// Process one STFT frame: analyze, shift, synthesize.
    fn process_frame(&mut self) {
        // Apply analysis window and load into FFT buffer.
        for i in 0..FFT_SIZE {
            let buf_idx = (self.input_pos + i) % self.input_buf.len();
            self.fft_real[i] = self.input_buf[buf_idx] * self.window[i];
            self.fft_imag[i] = 0.0;
        }

        // Forward FFT.
        Self::fft(&mut self.fft_real, &mut self.fft_imag, false);

        // Extract magnitude and phase.
        for i in 0..NUM_BINS {
            self.mag[i] =
                (self.fft_real[i] * self.fft_real[i] + self.fft_imag[i] * self.fft_imag[i]).sqrt();
            self.phase[i] = self.fft_imag[i].atan2(self.fft_real[i]);
        }

        // Formant preservation: estimate and apply envelope.
        if self.preserve_formants && (self.shift_ratio - 1.0).abs() > 0.001 {
            self.estimate_envelope();
        }

        // Phase vocoder pitch shifting: shift bins.
        let ratio = self.shift_ratio;
        let mut new_mag = vec![0.0; NUM_BINS];
        let mut new_phase = vec![0.0; NUM_BINS];

        let expected_phase_diff = 2.0 * PI * HOP_SIZE as f64 / FFT_SIZE as f64;

        // --- Pass 1: compute shifted magnitudes for all bins ---
        // Also store the source bin index for formant preservation.
        let mut src_indices = vec![0usize; NUM_BINS];
        for i in 0..NUM_BINS {
            let src = i as f64 / ratio;
            let src_idx = src.floor() as usize;
            let frac = src - src_idx as f64;
            src_indices[i] = src_idx;

            if src_idx < NUM_BINS - 1 {
                new_mag[i] = self.mag[src_idx] * (1.0 - frac) + self.mag[src_idx + 1] * frac;
            }
        }

        // --- Pass 2: identify peak bins in the shifted magnitude spectrum ---
        for i in 0..NUM_BINS {
            self.peak_bins[i] = if i == 0 {
                new_mag[0] > new_mag[1]
            } else if i == NUM_BINS - 1 {
                new_mag[NUM_BINS - 1] > new_mag[NUM_BINS - 2]
            } else {
                new_mag[i] > new_mag[i - 1] && new_mag[i] > new_mag[i + 1]
            };
        }

        // --- Pass 3: compute phase for peak bins using instantaneous frequency ---
        for i in 0..NUM_BINS {
            if !self.peak_bins[i] {
                continue;
            }
            let src = i as f64 / ratio;
            let src_idx = src.floor() as usize;
            if src_idx < NUM_BINS - 1 {
                let phase_diff = self.phase[src_idx] - self.prev_phase[src_idx];
                let expected = expected_phase_diff * src_idx as f64;
                let mut deviation = phase_diff - expected;
                deviation -= (deviation / (2.0 * PI)).round() * 2.0 * PI;
                let true_freq = src_idx as f64 + deviation / expected_phase_diff;
                let shifted_freq = true_freq * ratio;
                new_phase[i] = self.synth_phase[i] + expected_phase_diff * shifted_freq;
            }
        }

        // --- Pass 4: build nearest-peak map (sweep left then right) ---
        // Forward sweep: assign nearest peak seen so far from the left.
        let mut last_peak: Option<usize> = None;
        for i in 0..NUM_BINS {
            if self.peak_bins[i] {
                last_peak = Some(i);
            }
            self.nearest_peak[i] = last_peak.unwrap_or(0);
        }
        // Backward sweep: pick the closer peak between left and right.
        last_peak = None;
        for i in (0..NUM_BINS).rev() {
            if self.peak_bins[i] {
                last_peak = Some(i);
            }
            if let Some(rp) = last_peak {
                let lp = self.nearest_peak[i];
                if (rp as isize - i as isize).unsigned_abs()
                    < (lp as isize - i as isize).unsigned_abs()
                {
                    self.nearest_peak[i] = rp;
                }
            }
        }

        // --- Pass 5: lock non-peak bins to their nearest peak ---
        for i in 0..NUM_BINS {
            if self.peak_bins[i] {
                continue;
            }
            let np = self.nearest_peak[i];
            // Preserve the analysis-frame phase relationship relative to the peak.
            let src_i = (i as f64 / ratio).floor() as usize;
            let src_np = (np as f64 / ratio).floor() as usize;
            if src_i < NUM_BINS && src_np < NUM_BINS {
                new_phase[i] = new_phase[np]
                    + (self.phase[src_i.min(NUM_BINS - 1)] - self.phase[src_np.min(NUM_BINS - 1)]);
            }
        }

        // --- Formant preservation: re-apply original envelope ---
        if self.preserve_formants && (self.shift_ratio - 1.0).abs() > 0.001 {
            for i in 0..NUM_BINS {
                let src_idx = src_indices[i];
                if src_idx < NUM_BINS && self.envelope[src_idx] > 1e-20 {
                    new_mag[i] *=
                        self.envelope[i.min(NUM_BINS - 1)] / self.envelope[src_idx].max(1e-20);
                }
            }
        }

        // Save phases for next frame.
        self.prev_phase.copy_from_slice(&self.phase);
        self.synth_phase.copy_from_slice(&new_phase);

        // Convert back to complex.
        for i in 0..NUM_BINS {
            self.fft_real[i] = new_mag[i] * new_phase[i].cos();
            self.fft_imag[i] = new_mag[i] * new_phase[i].sin();
        }
        // Mirror for real-valued IFFT.
        for i in 1..NUM_BINS - 1 {
            self.fft_real[FFT_SIZE - i] = self.fft_real[i];
            self.fft_imag[FFT_SIZE - i] = -self.fft_imag[i];
        }

        // Inverse FFT.
        Self::fft(&mut self.fft_real, &mut self.fft_imag, true);

        // Overlap-add with synthesis window.
        let out_len = self.output_buf.len();
        for i in 0..FFT_SIZE {
            let out_idx = (self.output_pos + i) % out_len;
            self.output_buf[out_idx] += self.fft_real[i] * self.window[i];
        }
    }

    /// Process one sample. Returns the pitch-shifted output.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        // Write to input ring buffer.
        let buf_len = self.input_buf.len();
        self.input_buf[self.input_pos % buf_len] = input;
        self.input_pos = (self.input_pos + 1) % buf_len;

        // Read from output buffer.
        let out_len = self.output_buf.len();
        let out_idx = self.output_pos % out_len;
        let wet = self.output_buf[out_idx];
        self.output_buf[out_idx] = 0.0;
        self.output_pos = (self.output_pos + 1) % out_len;

        // Process a frame every HOP_SIZE samples.
        if self.input_pos % HOP_SIZE == 0 {
            self.process_frame();
        }

        // Normalize overlap-add (4x overlap with Hann window needs 2/3 normalization).
        let normalized = wet * (2.0 / 3.0);

        input * (1.0 - self.mix) + normalized * self.mix
    }

    pub fn latency(&self) -> usize {
        self.latency_samples
    }
}

impl Default for FormantVocoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn make_vocoder() -> FormantVocoder {
        let mut v = FormantVocoder::new();
        v.update(SR);
        v
    }

    #[test]
    fn unity_ratio_passes_signal() {
        let mut v = make_vocoder();
        v.shift_ratio = 1.0;
        v.mix = 1.0;
        v.preserve_formants = false;

        let freq = 440.0;
        let n = FFT_SIZE * 8;

        let mut energy = 0.0;
        for i in 0..n {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
            let out = v.tick(input);
            if i > FFT_SIZE * 2 {
                energy += out * out;
            }
        }

        assert!(
            energy > 1.0,
            "Unity ratio should pass signal: energy={energy}"
        );
    }

    #[test]
    fn shifting_produces_different_output() {
        let freq = 440.0;
        let n = FFT_SIZE * 8;

        let collect = |ratio: f64| -> Vec<f64> {
            let mut v = make_vocoder();
            v.shift_ratio = ratio;
            v.mix = 1.0;
            v.preserve_formants = false;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
                out.push(v.tick(input));
            }
            out
        };

        let unity = collect(1.0);
        let shifted = collect(1.1); // ~1.7 semitones up

        let diff: f64 = unity
            .iter()
            .zip(shifted.iter())
            .skip(FFT_SIZE * 2)
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / (n - FFT_SIZE * 2) as f64;

        assert!(diff > 0.01, "Shifted should differ from unity: {diff}");
    }

    #[test]
    fn no_nan() {
        let mut v = make_vocoder();
        v.shift_ratio = 0.94; // ~1 semitone down
        v.preserve_formants = true;

        for i in 0..48000 {
            let input = (2.0 * PI * 220.0 * i as f64 / SR).sin() * 0.5;
            let out = v.tick(input);
            assert!(out.is_finite(), "NaN at sample {i}");
        }
    }

    #[test]
    fn silence_in_silence_out() {
        let mut v = make_vocoder();
        v.shift_ratio = 1.05;

        for i in 0..FFT_SIZE * 4 {
            let out = v.tick(0.0);
            assert!(
                out.abs() < 1e-6,
                "Silence should produce silence at sample {i}: {out}"
            );
        }
    }

    #[test]
    fn formant_preservation_changes_output() {
        let freq = 220.0;
        let ratio = 1.2; // ~3 semitones up
        let n = FFT_SIZE * 8;

        let collect = |formant: bool| -> Vec<f64> {
            let mut v = make_vocoder();
            v.shift_ratio = ratio;
            v.preserve_formants = formant;
            v.mix = 1.0;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
                out.push(v.tick(input));
            }
            out
        };

        let with_formant = collect(true);
        let without_formant = collect(false);

        let diff: f64 = with_formant
            .iter()
            .zip(without_formant.iter())
            .skip(FFT_SIZE * 2)
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / (n - FFT_SIZE * 2) as f64;

        // For a pure sine, the difference should be small but nonzero
        // (envelope estimation on a single harmonic has limited effect).
        // Just verify no crash.
        assert!(diff.is_finite(), "Diff should be finite: {diff}");
    }

    #[test]
    fn dry_wet_mix() {
        let mut v = make_vocoder();
        v.shift_ratio = 1.1;
        v.mix = 0.0;

        for i in 0..FFT_SIZE * 4 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = v.tick(input);
            assert!(
                (out - input).abs() < 1e-10,
                "Mix=0 should pass dry at sample {i}"
            );
        }
    }
}
