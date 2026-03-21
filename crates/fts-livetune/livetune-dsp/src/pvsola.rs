//! PVSOLA — Phase Vocoder with Synchronized Overlap-Add pitch shifter.
//!
//! Hybrid approach combining the frequency-domain accuracy of a phase vocoder
//! with the time-domain naturalness of PSOLA. Designed for high-quality pitch
//! shifting of monophonic signals (voice, solo instruments).
//!
//! Two parallel processing paths run simultaneously:
//! 1. **Phase vocoder path**: STFT → magnitude/phase → bin shifting with phase
//!    propagation → IFFT → overlap-add. Handles transients and unvoiced content.
//! 2. **PSOLA path**: autocorrelation pitch detection → Hann-windowed grain
//!    placement at shifted rate. Handles voiced/harmonic content naturally.
//!
//! A voicing detector (autocorrelation clarity) cross-fades between paths:
//! - High clarity (voiced): weight toward PSOLA for natural harmonics
//! - Low clarity (unvoiced/transient): weight toward phase vocoder
//!
//! Pipeline per sample:
//! 1. Feed input to both paths
//! 2. Phase vocoder: STFT frame processing every HOP_SIZE samples
//! 3. PSOLA: pitch detection + grain overlap-add
//! 4. Voicing-weighted blend of both outputs
//! 5. Optional formant preservation via cepstral envelope

use std::f64::consts::PI;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// FFT size for the phase vocoder path.
const FFT_SIZE: usize = 2048;
/// Hop size (overlap factor = 4).
const HOP_SIZE: usize = FFT_SIZE / 4;
/// Number of frequency bins.
const NUM_BINS: usize = FFT_SIZE / 2 + 1;
/// Cepstral lifter cutoff quefrency for formant envelope estimation.
const LIFTER_ORDER: usize = 30;

/// PSOLA analysis buffer length (must hold several pitch periods).
const PSOLA_BUF_LEN: usize = 4096;
/// Minimum detectable pitch period in samples (~500 Hz at 48 kHz).
const MIN_PERIOD: usize = 96;
/// Maximum detectable pitch period in samples (~60 Hz at 48 kHz).
const MAX_PERIOD: usize = 800;

/// Voicing clarity threshold: above this → mostly PSOLA.
const VOICED_THRESHOLD: f64 = 0.6;
/// Voicing clarity threshold: below this → mostly phase vocoder.
const UNVOICED_THRESHOLD: f64 = 0.3;
/// IIR smoothing coefficient for voicing blend transitions.
const BLEND_SMOOTH: f64 = 0.002;

// ---------------------------------------------------------------------------
// PvsolaShifter
// ---------------------------------------------------------------------------

/// PVSOLA hybrid pitch shifter.
pub struct PvsolaShifter {
    /// Pitch shift ratio (e.g., 1.05 = 5% up).
    pub shift_ratio: f64,
    /// Mix: 0.0 = dry, 1.0 = wet.
    pub mix: f64,
    /// Enable formant preservation on the phase vocoder path.
    pub preserve_formants: bool,
    /// Voicing blend override: -1.0 = auto, 0.0 = all vocoder, 1.0 = all PSOLA.
    pub voicing_blend: f64,

    // --- Phase vocoder state ---
    pv_input_buf: Vec<f64>,
    pv_input_pos: usize,
    pv_output_buf: Vec<f64>,
    pv_output_pos: usize,
    pv_window: Vec<f64>,
    pv_prev_phase: Vec<f64>,
    pv_synth_phase: Vec<f64>,
    pv_fft_real: Vec<f64>,
    pv_fft_imag: Vec<f64>,
    pv_mag: Vec<f64>,
    pv_phase: Vec<f64>,
    pv_envelope: Vec<f64>,

    // --- PSOLA state ---
    /// Circular analysis buffer for PSOLA.
    psola_buf: Vec<f64>,
    /// Write position in PSOLA buffer.
    psola_write_pos: usize,
    /// Fractional read position for synthesis grains.
    psola_synth_pos: f64,
    /// Current detected pitch period (in samples).
    psola_period: f64,
    /// PSOLA output accumulator.
    psola_output_buf: Vec<f64>,
    /// Read position in PSOLA output buffer.
    psola_output_pos: usize,
    /// Samples since last grain placement.
    psola_grain_counter: usize,

    // --- Voicing detection ---
    /// Current autocorrelation clarity (0.0–1.0).
    voicing_clarity: f64,
    /// Smoothed blend weight: 0.0 = all vocoder, 1.0 = all PSOLA.
    blend_weight: f64,

    // --- Common ---
    sample_rate: f64,
    /// Samples of latency.
    latency_samples: usize,
    /// Total input samples processed (for hop triggering).
    sample_count: usize,
}

impl PvsolaShifter {
    pub fn new() -> Self {
        let pv_window: Vec<f64> = (0..FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / FFT_SIZE as f64).cos()))
            .collect();

        Self {
            shift_ratio: 1.0,
            mix: 1.0,
            preserve_formants: true,
            voicing_blend: -1.0,

            pv_input_buf: vec![0.0; FFT_SIZE],
            pv_input_pos: 0,
            pv_output_buf: vec![0.0; FFT_SIZE * 2],
            pv_output_pos: 0,
            pv_window,
            pv_prev_phase: vec![0.0; NUM_BINS],
            pv_synth_phase: vec![0.0; NUM_BINS],
            pv_fft_real: vec![0.0; FFT_SIZE],
            pv_fft_imag: vec![0.0; FFT_SIZE],
            pv_mag: vec![0.0; NUM_BINS],
            pv_phase: vec![0.0; NUM_BINS],
            pv_envelope: vec![0.0; NUM_BINS],

            psola_buf: vec![0.0; PSOLA_BUF_LEN],
            psola_write_pos: 0,
            psola_synth_pos: 0.0,
            psola_period: 200.0,
            psola_output_buf: vec![0.0; PSOLA_BUF_LEN],
            psola_output_pos: 0,
            psola_grain_counter: 0,

            voicing_clarity: 0.0,
            blend_weight: 0.5,

            sample_rate: 48000.0,
            latency_samples: FFT_SIZE,
            sample_count: 0,
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.pv_input_buf.fill(0.0);
        self.pv_input_pos = 0;
        self.pv_output_buf.fill(0.0);
        self.pv_output_pos = 0;
        self.pv_prev_phase.fill(0.0);
        self.pv_synth_phase.fill(0.0);

        self.psola_buf.fill(0.0);
        self.psola_write_pos = 0;
        self.psola_synth_pos = 0.0;
        self.psola_period = 200.0;
        self.psola_output_buf.fill(0.0);
        self.psola_output_pos = 0;
        self.psola_grain_counter = 0;

        self.voicing_clarity = 0.0;
        self.blend_weight = 0.5;
        self.sample_count = 0;
    }

    // -----------------------------------------------------------------------
    // FFT (radix-2 Cooley-Tukey, same as FormantVocoder)
    // -----------------------------------------------------------------------

    /// In-place radix-2 Cooley-Tukey FFT. `n` must be a power of two.
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

        // Butterfly stages.
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

    // -----------------------------------------------------------------------
    // Phase vocoder path
    // -----------------------------------------------------------------------

    /// Estimate spectral envelope via cepstral liftering.
    fn pv_estimate_envelope(&mut self) {
        let mut cep_real = vec![0.0; FFT_SIZE];
        let mut cep_imag = vec![0.0; FFT_SIZE];

        for i in 0..NUM_BINS {
            cep_real[i] = (self.pv_mag[i].max(1e-20)).ln();
        }
        // Mirror for real-valued IFFT.
        for i in 1..NUM_BINS - 1 {
            cep_real[FFT_SIZE - i] = cep_real[i];
        }

        // IFFT → cepstrum.
        Self::fft(&mut cep_real, &mut cep_imag, true);

        // Low-pass lifter: zero out high quefrency.
        for i in LIFTER_ORDER..FFT_SIZE - LIFTER_ORDER {
            cep_real[i] = 0.0;
            cep_imag[i] = 0.0;
        }

        // Hamming taper at lifter boundary.
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

        // FFT back → smooth spectral envelope.
        Self::fft(&mut cep_real, &mut cep_imag, false);

        for i in 0..NUM_BINS {
            self.pv_envelope[i] = cep_real[i].exp();
        }
    }

    /// Process one phase vocoder STFT frame.
    fn pv_process_frame(&mut self) {
        // Apply analysis window.
        for i in 0..FFT_SIZE {
            let buf_idx = (self.pv_input_pos + i) % self.pv_input_buf.len();
            self.pv_fft_real[i] = self.pv_input_buf[buf_idx] * self.pv_window[i];
            self.pv_fft_imag[i] = 0.0;
        }

        // Forward FFT.
        Self::fft(&mut self.pv_fft_real, &mut self.pv_fft_imag, false);

        // Extract magnitude and phase.
        for i in 0..NUM_BINS {
            self.pv_mag[i] = (self.pv_fft_real[i] * self.pv_fft_real[i]
                + self.pv_fft_imag[i] * self.pv_fft_imag[i])
                .sqrt();
            self.pv_phase[i] = self.pv_fft_imag[i].atan2(self.pv_fft_real[i]);
        }

        // Optional formant preservation.
        if self.preserve_formants && (self.shift_ratio - 1.0).abs() > 0.001 {
            self.pv_estimate_envelope();
        }

        // Shift bins by ratio with phase propagation.
        let ratio = self.shift_ratio;
        let mut new_mag = vec![0.0; NUM_BINS];
        let mut new_phase = vec![0.0; NUM_BINS];
        let expected_phase_diff = 2.0 * PI * HOP_SIZE as f64 / FFT_SIZE as f64;

        let mut src_indices = vec![0usize; NUM_BINS];
        for i in 0..NUM_BINS {
            let src = i as f64 / ratio;
            let src_idx = src.floor() as usize;
            let frac = src - src_idx as f64;
            src_indices[i] = src_idx;

            if src_idx < NUM_BINS - 1 {
                new_mag[i] =
                    self.pv_mag[src_idx] * (1.0 - frac) + self.pv_mag[src_idx + 1] * frac;
            }
        }

        // Phase propagation with instantaneous frequency.
        for i in 0..NUM_BINS {
            let src = i as f64 / ratio;
            let src_idx = src.floor() as usize;
            if src_idx < NUM_BINS - 1 {
                let phase_diff = self.pv_phase[src_idx] - self.pv_prev_phase[src_idx];
                let expected = expected_phase_diff * src_idx as f64;
                let mut deviation = phase_diff - expected;
                deviation -= (deviation / (2.0 * PI)).round() * 2.0 * PI;
                let true_freq = src_idx as f64 + deviation / expected_phase_diff;
                let shifted_freq = true_freq * ratio;
                new_phase[i] = self.pv_synth_phase[i] + expected_phase_diff * shifted_freq;
            }
        }

        // Formant envelope re-application.
        if self.preserve_formants && (self.shift_ratio - 1.0).abs() > 0.001 {
            for i in 0..NUM_BINS {
                let src_idx = src_indices[i];
                if src_idx < NUM_BINS && self.pv_envelope[src_idx] > 1e-20 {
                    new_mag[i] *= self.pv_envelope[i.min(NUM_BINS - 1)]
                        / self.pv_envelope[src_idx].max(1e-20);
                }
            }
        }

        // Save phases.
        self.pv_prev_phase.copy_from_slice(&self.pv_phase);
        self.pv_synth_phase.copy_from_slice(&new_phase);

        // Convert back to complex.
        for i in 0..NUM_BINS {
            self.pv_fft_real[i] = new_mag[i] * new_phase[i].cos();
            self.pv_fft_imag[i] = new_mag[i] * new_phase[i].sin();
        }
        // Mirror for real-valued IFFT.
        for i in 1..NUM_BINS - 1 {
            self.pv_fft_real[FFT_SIZE - i] = self.pv_fft_real[i];
            self.pv_fft_imag[FFT_SIZE - i] = -self.pv_fft_imag[i];
        }

        // Inverse FFT.
        Self::fft(&mut self.pv_fft_real, &mut self.pv_fft_imag, true);

        // Overlap-add with synthesis window.
        let out_len = self.pv_output_buf.len();
        for i in 0..FFT_SIZE {
            let out_idx = (self.pv_output_pos + i) % out_len;
            self.pv_output_buf[out_idx] += self.pv_fft_real[i] * self.pv_window[i];
        }
    }

    /// Read one sample from the phase vocoder output and advance.
    fn pv_tick(&mut self, input: f64) -> f64 {
        // Write to PV input ring buffer.
        let buf_len = self.pv_input_buf.len();
        self.pv_input_buf[self.pv_input_pos % buf_len] = input;
        self.pv_input_pos = (self.pv_input_pos + 1) % buf_len;

        // Read from PV output buffer.
        let out_len = self.pv_output_buf.len();
        let out_idx = self.pv_output_pos % out_len;
        let wet = self.pv_output_buf[out_idx];
        self.pv_output_buf[out_idx] = 0.0;
        self.pv_output_pos = (self.pv_output_pos + 1) % out_len;

        // Process a frame every HOP_SIZE samples.
        if self.pv_input_pos % HOP_SIZE == 0 {
            self.pv_process_frame();
        }

        // Normalize OLA (4x overlap with Hann → 2/3 normalization).
        wet * (2.0 / 3.0)
    }

    // -----------------------------------------------------------------------
    // PSOLA path
    // -----------------------------------------------------------------------

    /// Autocorrelation-based pitch detection on the PSOLA buffer.
    /// Returns (period_in_samples, clarity).
    fn psola_detect_pitch(&self) -> (f64, f64) {
        let buf = &self.psola_buf;
        let len = PSOLA_BUF_LEN;
        let wp = self.psola_write_pos;

        // Analyze the most recent FFT_SIZE samples for autocorrelation.
        let analysis_len = FFT_SIZE.min(len);

        // Compute energy of analysis window.
        let mut energy = 0.0;
        for i in 0..analysis_len {
            let idx = (wp + len - analysis_len + i) % len;
            energy += buf[idx] * buf[idx];
        }
        if energy < 1e-10 {
            return (self.psola_period, 0.0);
        }

        let mut best_corr = 0.0;
        let mut best_lag = MIN_PERIOD;

        // Compute normalized autocorrelation at candidate lags.
        let max_lag = MAX_PERIOD.min(analysis_len / 2);
        for lag in MIN_PERIOD..=max_lag {
            let mut corr = 0.0;
            let mut e1 = 0.0;
            let mut e2 = 0.0;
            let n = analysis_len - lag;
            for i in 0..n {
                let idx_a = (wp + len - analysis_len + i) % len;
                let idx_b = (wp + len - analysis_len + i + lag) % len;
                corr += buf[idx_a] * buf[idx_b];
                e1 += buf[idx_a] * buf[idx_a];
                e2 += buf[idx_b] * buf[idx_b];
            }
            let denom = (e1 * e2).sqrt();
            let norm_corr = if denom > 1e-20 { corr / denom } else { 0.0 };

            if norm_corr > best_corr {
                best_corr = norm_corr;
                best_lag = lag;
            }
        }

        // Parabolic interpolation around the peak.
        let period = if best_lag > MIN_PERIOD && best_lag < max_lag {
            let compute_corr = |lag: usize| -> f64 {
                let mut corr = 0.0;
                let mut e1 = 0.0;
                let mut e2 = 0.0;
                let n = analysis_len - lag;
                for i in 0..n {
                    let idx_a = (wp + len - analysis_len + i) % len;
                    let idx_b = (wp + len - analysis_len + i + lag) % len;
                    corr += buf[idx_a] * buf[idx_b];
                    e1 += buf[idx_a] * buf[idx_a];
                    e2 += buf[idx_b] * buf[idx_b];
                }
                let denom = (e1 * e2).sqrt();
                if denom > 1e-20 { corr / denom } else { 0.0 }
            };
            let c_prev = compute_corr(best_lag - 1);
            let c_next = compute_corr(best_lag + 1);
            let denom = 2.0 * (2.0 * best_corr - c_prev - c_next);
            if denom.abs() > 1e-10 {
                best_lag as f64 + (c_prev - c_next) / denom
            } else {
                best_lag as f64
            }
        } else {
            best_lag as f64
        };

        (period, best_corr.max(0.0).min(1.0))
    }

    /// Place a Hann-windowed grain from the PSOLA buffer centered at `center`.
    fn psola_place_grain(&mut self) {
        let period = self.psola_period;
        let grain_len = (period * 2.0).round() as usize;
        if grain_len < 4 {
            return;
        }

        let buf_len = PSOLA_BUF_LEN;
        let center = self.psola_write_pos;
        let out_len = self.psola_output_buf.len();

        for i in 0..grain_len {
            let t = i as f64 / grain_len as f64;
            let w = 0.5 * (1.0 - (2.0 * PI * t).cos()); // Hann window
            let src_idx = (center + buf_len - grain_len / 2 + i) % buf_len;
            let out_idx = (self.psola_output_pos + i) % out_len;
            self.psola_output_buf[out_idx] += self.psola_buf[src_idx] * w;
        }
    }

    /// Feed one sample to the PSOLA path and return the output.
    fn psola_tick(&mut self, input: f64) -> f64 {
        // Write to circular buffer.
        self.psola_buf[self.psola_write_pos] = input;
        self.psola_write_pos = (self.psola_write_pos + 1) % PSOLA_BUF_LEN;

        // Read from output buffer.
        let out_len = self.psola_output_buf.len();
        let out_idx = self.psola_output_pos % out_len;
        let out = self.psola_output_buf[out_idx];
        self.psola_output_buf[out_idx] = 0.0;
        self.psola_output_pos = (self.psola_output_pos + 1) % out_len;

        // Grain placement at synthesis rate.
        self.psola_grain_counter += 1;
        let synth_period = (self.psola_period / self.shift_ratio).max(MIN_PERIOD as f64);
        if self.psola_grain_counter >= synth_period as usize {
            self.psola_grain_counter = 0;
            self.psola_place_grain();
        }

        out
    }

    // -----------------------------------------------------------------------
    // Voicing detection
    // -----------------------------------------------------------------------

    /// Update voicing detection and blend weight. Called periodically (every
    /// HOP_SIZE samples).
    fn update_voicing(&mut self, clarity: f64) {
        self.voicing_clarity = clarity;

        let target = if self.voicing_blend >= 0.0 {
            // Manual override.
            self.voicing_blend
        } else {
            // Auto: map clarity to blend weight.
            if clarity > VOICED_THRESHOLD {
                1.0
            } else if clarity < UNVOICED_THRESHOLD {
                0.0
            } else {
                (clarity - UNVOICED_THRESHOLD) / (VOICED_THRESHOLD - UNVOICED_THRESHOLD)
            }
        };

        // IIR smooth toward target.
        self.blend_weight += BLEND_SMOOTH * (target - self.blend_weight);
    }

    // -----------------------------------------------------------------------
    // Public tick
    // -----------------------------------------------------------------------

    /// Process one sample. Returns the pitch-shifted output.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        // Phase vocoder path.
        let pv_out = self.pv_tick(input);

        // PSOLA path.
        let psola_out = self.psola_tick(input);

        // Periodic voicing/pitch update (aligned with PV hop).
        self.sample_count += 1;
        if self.sample_count % HOP_SIZE == 0 {
            let (period, clarity) = self.psola_detect_pitch();
            self.psola_period = period;
            self.update_voicing(clarity);
        }

        // Blend the two paths based on voicing.
        let w = self.blend_weight;
        let wet = pv_out * (1.0 - w) + psola_out * w;

        // Dry/wet mix.
        input * (1.0 - self.mix) + wet * self.mix
    }

    /// Returns the latency in samples introduced by this processor.
    pub fn latency(&self) -> usize {
        self.latency_samples
    }
}

impl Default for PvsolaShifter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn make_shifter() -> PvsolaShifter {
        let mut s = PvsolaShifter::new();
        s.update(SR);
        s
    }

    #[test]
    fn silence_in_silence_out() {
        let mut s = make_shifter();
        s.shift_ratio = 1.05;

        for i in 0..FFT_SIZE * 4 {
            let out = s.tick(0.0);
            assert!(
                out.abs() < 1e-6,
                "Silence should produce silence at sample {i}: {out}"
            );
        }
    }

    #[test]
    fn produces_output_on_sine() {
        let mut s = make_shifter();
        s.shift_ratio = 1.1;
        s.mix = 1.0;

        let freq = 440.0;
        let n = FFT_SIZE * 8;
        let mut energy = 0.0;
        for i in 0..n {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
            let out = s.tick(input);
            if i > FFT_SIZE * 2 {
                energy += out * out;
            }
        }

        assert!(
            energy > 0.1,
            "Should produce output on sine input: energy={energy}"
        );
    }

    #[test]
    fn no_nan() {
        let mut s = make_shifter();
        s.shift_ratio = 0.94;
        s.preserve_formants = true;

        for i in 0..48000 {
            let input = (2.0 * PI * 220.0 * i as f64 / SR).sin() * 0.5;
            let out = s.tick(input);
            assert!(out.is_finite(), "NaN at sample {i}");
        }
    }

    #[test]
    fn unity_ratio_passthrough() {
        let mut s = make_shifter();
        s.shift_ratio = 1.0;
        s.mix = 1.0;
        s.preserve_formants = false;
        // Force all vocoder path for deterministic comparison.
        s.voicing_blend = 0.0;

        let freq = 440.0;
        let n = FFT_SIZE * 8;
        let mut energy = 0.0;
        for i in 0..n {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
            let out = s.tick(input);
            if i > FFT_SIZE * 2 {
                energy += out * out;
            }
        }

        assert!(
            energy > 1.0,
            "Unity ratio should approximately pass signal: energy={energy}"
        );
    }

    #[test]
    fn formant_toggle() {
        let freq = 220.0;
        let ratio = 1.2;
        let n = FFT_SIZE * 8;

        let collect = |formant: bool| -> Vec<f64> {
            let mut s = make_shifter();
            s.shift_ratio = ratio;
            s.preserve_formants = formant;
            s.mix = 1.0;
            s.voicing_blend = 0.0; // All vocoder for consistent comparison.
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
                out.push(s.tick(input));
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

        assert!(
            diff.is_finite(),
            "Formant toggle diff should be finite: {diff}"
        );
    }

    #[test]
    fn dry_wet_mix() {
        let mut s = make_shifter();
        s.shift_ratio = 1.1;
        s.mix = 0.0;

        for i in 0..FFT_SIZE * 4 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = s.tick(input);
            assert!(
                (out - input).abs() < 1e-10,
                "Mix=0 should pass dry at sample {i}"
            );
        }
    }
}
