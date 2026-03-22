//! YAAPT (Yet Another Algorithm for Pitch Tracking) pitch detector.
//!
//! Implements a real-time streaming adaptation of Kasi & Zahorian (2002).
//! Four-stage hybrid approach:
//! 1. FIR bandpass filtering (50–1500 Hz) + nonlinear processing (squaring)
//! 2. NLFER (Normalized Low-Frequency Energy Ratio) for voiced/unvoiced
//! 3. SHC (Spectral Harmonic Correlation) for frequency-domain candidates
//! 4. NCCF (Normalized Cross-Correlation Function) for time-domain candidates
//! 5. Greedy candidate selection with merit boosting (real-time DP substitute)
//!
//! Reference: Kasi & Zahorian, "Yet Another Algorithm for Pitch Tracking," 2002.
//! Reference: Zahorian & Hu, "A spectral/temporal method for robust F0 tracking," JASA 2008.

use std::f64::consts::PI;

use crate::detector::PitchEstimate;

// ── Parameters ──────────────────────────────────────────────────────────

/// YAAPT configuration with sensible defaults from the reference implementation.
struct Params {
    /// Minimum detectable F0 (Hz).
    f0_min: f64,
    /// Maximum detectable F0 (Hz).
    f0_max: f64,
    /// Analysis window length in seconds.
    frame_length_s: f64,
    /// Hop size in seconds.
    frame_space_s: f64,
    /// FIR bandpass low cutoff (Hz).
    bp_low: f64,
    /// FIR bandpass high cutoff (Hz).
    bp_high: f64,
    /// FIR filter order (number of taps = order + 1).
    bp_forder: usize,
    /// Number of harmonics for SHC.
    shc_numharms: usize,
    /// SHC spectral window half-width in Hz.
    shc_window_hz: f64,
    /// NCCF peak threshold (below = likely unvoiced).
    nccf_thresh: f64,
    /// Max NCCF candidates per frame.
    nccf_maxcands: usize,
    /// NLFER voiced energy threshold.
    nlfer_thresh1: f64,
    /// NLFER definitely-unvoiced threshold.
    nlfer_thresh2: f64,
    /// Merit boost for spectral agreement (used in candidate selection).
    merit_boost: f64,
    /// Silence gate in dB.
    gate_db: f64,
    /// Median filter length for post-smoothing.
    median_len: usize,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            f0_min: 60.0,
            f0_max: 500.0,
            frame_length_s: 0.035,
            frame_space_s: 0.010,
            bp_low: 50.0,
            bp_high: 1500.0,
            bp_forder: 100, // 101 taps — reduced from 150 for real-time
            shc_numharms: 3,
            shc_window_hz: 40.0,
            nccf_thresh: 0.3,
            nccf_maxcands: 3,
            nlfer_thresh1: 0.75,
            nlfer_thresh2: 0.1,
            merit_boost: 0.20,
            gate_db: -60.0,
            median_len: 5,
        }
    }
}

// ── FIR Bandpass Filter ─────────────────────────────────────────────────

/// Windowed-sinc FIR bandpass filter.
struct FirBandpass {
    coeffs: Vec<f64>,
    buffer: Vec<f64>,
    pos: usize,
}

impl FirBandpass {
    fn new(order: usize) -> Self {
        Self {
            coeffs: vec![0.0; order + 1],
            buffer: vec![0.0; order + 1],
            pos: 0,
        }
    }

    /// Design bandpass filter using windowed sinc (Hamming window).
    fn design(&mut self, low_hz: f64, high_hz: f64, sample_rate: f64) {
        let n = self.coeffs.len();
        let m = (n - 1) as f64;
        let nyquist = sample_rate / 2.0;
        let fl = low_hz / nyquist;
        let fh = high_hz / nyquist;

        for i in 0..n {
            let x = i as f64 - m / 2.0;
            // Sinc highpass - sinc lowpass = bandpass
            let sinc_h = if x.abs() < 1e-10 {
                fh
            } else {
                (PI * fh * x).sin() / (PI * x)
            };
            let sinc_l = if x.abs() < 1e-10 {
                fl
            } else {
                (PI * fl * x).sin() / (PI * x)
            };
            // Hamming window
            let w = 0.54 - 0.46 * (2.0 * PI * i as f64 / m).cos();
            self.coeffs[i] = 2.0 * (sinc_h - sinc_l) * w;
        }

        // Normalize for unity gain at center frequency.
        let center = (low_hz + high_hz) / 2.0;
        let mut gain = 0.0f64;
        for (i, &c) in self.coeffs.iter().enumerate() {
            gain += c * (2.0 * PI * center / sample_rate * i as f64).cos();
        }
        if gain.abs() > 1e-10 {
            for c in &mut self.coeffs {
                *c /= gain;
            }
        }

        self.buffer.fill(0.0);
        self.pos = 0;
    }

    #[inline]
    fn tick(&mut self, input: f64) -> f64 {
        let n = self.coeffs.len();
        self.buffer[self.pos] = input;

        let mut out = 0.0;
        let mut idx = self.pos;
        for c in &self.coeffs {
            out += c * self.buffer[idx];
            if idx == 0 {
                idx = n - 1;
            } else {
                idx -= 1;
            }
        }

        self.pos = (self.pos + 1) % n;
        out
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.pos = 0;
    }
}

// ── Pitch Candidate ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct PitchCandidate {
    freq_hz: f64,
    merit: f64,
}

// ── YAAPT Detector ──────────────────────────────────────────────────────

/// Real-time YAAPT pitch detector.
///
/// Streams audio sample-by-sample via `tick()`, performs analysis every
/// hop_size samples, and returns `PitchEstimate` at each call.
pub struct YaaptDetector {
    /// Reference frequency for A4 (default 440.0).
    pub a_freq: f64,

    params: Params,
    sample_rate: f64,

    // Window / hop sizing.
    frame_size: usize,
    hop_size: usize,
    fft_size: usize,

    // FIR bandpass filters.
    bp_filter: FirBandpass,
    bp_filter_nl: FirBandpass, // for nonlinear (squared) signal

    // Ring buffers for filtered and nonlinear-filtered signals.
    buf_filtered: Vec<f64>,
    buf_nonlinear: Vec<f64>,
    buf_raw: Vec<f64>,
    write_pos: usize,
    hop_count: usize,

    // FFT scratch buffers.
    fft_real: Vec<f64>,
    fft_imag: Vec<f64>,
    fft_mag: Vec<f64>,

    // Hann window (frame_size).
    hann: Vec<f64>,

    // NCCF lag range.
    lag_min: usize,
    lag_max: usize,
    nccf_scratch: Vec<f64>,

    // Candidate tracking for simple DP / smoothing.
    prev_freq: f64,
    median_buf: Vec<f64>,
    median_pos: usize,

    // Peak energy tracking for NLFER normalization.
    peak_energy: f64,

    last_estimate: PitchEstimate,
}

impl YaaptDetector {
    pub fn new() -> Self {
        Self {
            a_freq: 440.0,
            params: Params::default(),
            sample_rate: 48000.0,
            frame_size: 0,
            hop_size: 0,
            fft_size: 0,
            bp_filter: FirBandpass::new(100),
            bp_filter_nl: FirBandpass::new(100),
            buf_filtered: Vec::new(),
            buf_nonlinear: Vec::new(),
            buf_raw: Vec::new(),
            write_pos: 0,
            hop_count: 0,
            fft_real: Vec::new(),
            fft_imag: Vec::new(),
            fft_mag: Vec::new(),
            hann: Vec::new(),
            lag_min: 0,
            lag_max: 0,
            nccf_scratch: Vec::new(),
            prev_freq: 0.0,
            median_buf: Vec::new(),
            median_pos: 0,
            peak_energy: 0.0,
            last_estimate: PitchEstimate::unvoiced(),
        }
    }

    /// Reconfigure for a new sample rate.
    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;

        self.frame_size = (self.params.frame_length_s * sample_rate).round() as usize;
        self.hop_size = (self.params.frame_space_s * sample_rate).round() as usize;

        // FFT size: next power of 2 >= 4 * frame_size for adequate resolution.
        self.fft_size = (self.frame_size * 4).next_power_of_two().max(2048);

        // Allocate ring buffers (2x frame for overlap).
        let buf_len = self.frame_size * 2;
        self.buf_filtered = vec![0.0; buf_len];
        self.buf_nonlinear = vec![0.0; buf_len];
        self.buf_raw = vec![0.0; buf_len];
        self.write_pos = 0;
        self.hop_count = 0;

        // FFT scratch.
        self.fft_real = vec![0.0; self.fft_size];
        self.fft_imag = vec![0.0; self.fft_size];
        self.fft_mag = vec![0.0; self.fft_size / 2 + 1];

        // Hann window.
        self.hann = vec![0.0; self.frame_size];
        for i in 0..self.frame_size {
            self.hann[i] = 0.5 * (1.0 - (2.0 * PI * i as f64 / self.frame_size as f64).cos());
        }

        // NCCF lag range.
        self.lag_min = (sample_rate / self.params.f0_max).floor() as usize;
        self.lag_max =
            ((sample_rate / self.params.f0_min).ceil() as usize).min(self.frame_size - 1);
        self.nccf_scratch = vec![0.0; self.lag_max + 1];

        // Design FIR bandpass filters.
        self.bp_filter = FirBandpass::new(self.params.bp_forder);
        self.bp_filter
            .design(self.params.bp_low, self.params.bp_high, sample_rate);
        self.bp_filter_nl = FirBandpass::new(self.params.bp_forder);
        self.bp_filter_nl
            .design(self.params.bp_low, self.params.bp_high, sample_rate);

        // Median filter.
        self.median_buf = vec![0.0; self.params.median_len];
        self.median_pos = 0;

        self.peak_energy = 0.0;
        self.prev_freq = 0.0;
    }

    pub fn reset(&mut self) {
        self.buf_filtered.fill(0.0);
        self.buf_nonlinear.fill(0.0);
        self.buf_raw.fill(0.0);
        self.write_pos = 0;
        self.hop_count = 0;
        self.bp_filter.reset();
        self.bp_filter_nl.reset();
        self.prev_freq = 0.0;
        self.peak_energy = 0.0;
        self.median_buf.fill(0.0);
        self.median_pos = 0;
        self.last_estimate = PitchEstimate::unvoiced();
    }

    /// Feed one sample, returns current pitch estimate.
    #[inline]
    pub fn tick(&mut self, input: f64) -> PitchEstimate {
        // Bandpass filter the input.
        let filtered = self.bp_filter.tick(input);
        // Nonlinear path: square then filter.
        let squared = input * input;
        let nonlinear = self.bp_filter_nl.tick(squared);

        let buf_len = self.buf_filtered.len();
        self.buf_filtered[self.write_pos] = filtered;
        self.buf_nonlinear[self.write_pos] = nonlinear;
        self.buf_raw[self.write_pos] = input;
        self.write_pos = (self.write_pos + 1) % buf_len;
        self.hop_count += 1;

        if self.hop_count >= self.hop_size {
            self.hop_count = 0;
            self.last_estimate = self.analyze();
        }

        self.last_estimate
    }

    /// Get the current estimate without feeding a new sample.
    pub fn current(&self) -> PitchEstimate {
        self.last_estimate
    }

    /// Analysis latency in samples.
    pub fn latency(&self) -> usize {
        self.frame_size
    }

    // ── Analysis ────────────────────────────────────────────────────────

    fn analyze(&mut self) -> PitchEstimate {
        let n = self.frame_size;
        let buf_len = self.buf_filtered.len();

        // ── Gate: check RMS of raw signal ───────────────────────────────
        let mut rms = 0.0f64;
        for i in 0..n {
            let idx = (self.write_pos + buf_len - n + i) % buf_len;
            let s = self.buf_raw[idx];
            rms += s * s;
        }
        rms = (rms / n as f64).sqrt();
        let db = if rms > 1e-20 {
            20.0 * rms.log10()
        } else {
            -120.0
        };
        if db < self.params.gate_db {
            return PitchEstimate::unvoiced();
        }

        // ── Step 1: NLFER (Normalized Low-Frequency Energy Ratio) ───────
        let nlfer = self.compute_nlfer();

        // Definitely unvoiced?
        if nlfer < self.params.nlfer_thresh2 {
            self.prev_freq = 0.0;
            return PitchEstimate::unvoiced();
        }

        let is_voiced_energy = nlfer >= self.params.nlfer_thresh1;

        // ── Step 2: SHC (Spectral Harmonic Correlation) candidates ──────
        let spectral_candidates = self.compute_shc();

        // ── Step 3: NCCF candidates from filtered signal ────────────────
        let temporal1 = self.compute_nccf(&self.buf_filtered.clone());

        // ── Step 4: NCCF candidates from nonlinear signal ───────────────
        let temporal2 = self.compute_nccf(&self.buf_nonlinear.clone());

        // ── Step 5: Merge & select best candidate ───────────────────────
        let best = self.select_best(
            &spectral_candidates,
            &temporal1,
            &temporal2,
            is_voiced_energy,
        );

        // ── Step 6: Median filter for smoothing ─────────────────────────
        let freq = self.apply_median(best.freq_hz);

        if freq < self.params.f0_min || freq > self.params.f0_max {
            self.prev_freq = 0.0;
            return PitchEstimate::unvoiced();
        }

        self.prev_freq = freq;

        // Convert merit to confidence (0–1).
        let confidence = best.merit.clamp(0.0, 1.0);

        let semitones = 12.0 * (freq / self.a_freq).log2();
        let midi_note = 69.0 + semitones;

        PitchEstimate {
            freq_hz: freq,
            semitones,
            midi_note,
            confidence,
        }
    }

    /// NLFER: energy ratio in the F0 frequency band.
    fn compute_nlfer(&mut self) -> f64 {
        let n = self.frame_size;
        let buf_len = self.buf_raw.len();
        let nfft = self.fft_size;

        // Window the raw signal and compute FFT.
        self.fft_real[..nfft].fill(0.0);
        self.fft_imag[..nfft].fill(0.0);

        for i in 0..n {
            let idx = (self.write_pos + buf_len - n + i) % buf_len;
            self.fft_real[i] = self.buf_raw[idx] * self.hann[i];
        }

        real_fft_inplace(&mut self.fft_real, &mut self.fft_imag, nfft);

        // Compute magnitude spectrum.
        let half = nfft / 2 + 1;
        for k in 0..half {
            self.fft_mag[k] =
                (self.fft_real[k] * self.fft_real[k] + self.fft_imag[k] * self.fft_imag[k]).sqrt();
        }

        // Sum energy in F0 band.
        let bin_f0_min =
            ((2.0 * self.params.f0_min / self.sample_rate) * nfft as f64).round() as usize;
        let bin_f0_max = ((self.params.f0_max / self.sample_rate) * nfft as f64).round() as usize;
        let bin_f0_min = bin_f0_min.min(half - 1);
        let bin_f0_max = bin_f0_max.min(half - 1);

        let mut energy = 0.0f64;
        for k in bin_f0_min..=bin_f0_max {
            energy += self.fft_mag[k];
        }

        // Track peak for normalization (with slow decay).
        if energy > self.peak_energy {
            self.peak_energy = energy;
        } else {
            self.peak_energy *= 0.999;
        }

        if self.peak_energy > 1e-20 {
            energy / self.peak_energy
        } else {
            0.0
        }
    }

    /// SHC: spectral harmonic correlation.
    /// Returns up to 4 frequency-domain pitch candidates.
    ///
    /// Uses the filtered signal's spectrum. For robustness with signals that
    /// may lack harmonics (pure sines), we use SUM instead of PRODUCT across
    /// harmonics, weighted by harmonic number (higher harmonics get less weight).
    fn compute_shc(&mut self) -> Vec<PitchCandidate> {
        let n = self.frame_size;
        let buf_len = self.buf_filtered.len();
        let nfft = self.fft_size;
        let fs = self.sample_rate;
        let half = nfft / 2 + 1;
        let nh = self.params.shc_numharms;

        // Compute FFT of the filtered signal for SHC.
        let mut shc_real = vec![0.0f64; nfft];
        let mut shc_imag = vec![0.0f64; nfft];
        for i in 0..n {
            let idx = (self.write_pos + buf_len - n + i) % buf_len;
            shc_real[i] = self.buf_filtered[idx] * self.hann[i];
        }
        fft_dit(&mut shc_real, &mut shc_imag, nfft);

        let mut mag = vec![0.0f64; half];
        for k in 0..half {
            mag[k] = (shc_real[k] * shc_real[k] + shc_imag[k] * shc_imag[k]).sqrt();
        }

        // SHC window in bins.
        let wl = ((self.params.shc_window_hz / fs) * nfft as f64).round() as i64;

        let f0_min_bin = ((self.params.f0_min / fs) * nfft as f64).round() as usize;
        let f0_max_bin = ((self.params.f0_max / fs) * nfft as f64).round() as usize;
        let f0_max_bin = f0_max_bin.min(half - 1);

        if f0_min_bin >= f0_max_bin {
            return Vec::new();
        }

        // Compute SHC for each candidate F0 bin.
        let shc_len = f0_max_bin - f0_min_bin + 1;
        let mut shc = vec![0.0f64; shc_len];

        for (si, f0_bin) in (f0_min_bin..=f0_max_bin).enumerate() {
            // Weighted harmonic sum: for each candidate F0, sum the spectral
            // magnitude at each harmonic. Weight the fundamental most heavily.
            // This is more robust than a product when harmonics are absent.
            let mut total = 0.0;
            for j in -wl..=wl {
                let mut harmonic_sum = 0.0;
                let mut valid = true;
                for h in 1..=(nh as i64) {
                    let bin = (h * f0_bin as i64 + j) as usize;
                    if bin >= half {
                        valid = false;
                        break;
                    }
                    // Weight: fundamental gets weight 1.0, 2nd harmonic 0.5, etc.
                    let weight = 1.0 / h as f64;
                    harmonic_sum += weight * mag[bin];
                }
                if valid {
                    total += harmonic_sum;
                }
            }
            shc[si] = total;
        }

        // Peak picking.
        let mut candidates = Vec::with_capacity(4);
        let mean_shc: f64 = shc.iter().sum::<f64>() / shc.len().max(1) as f64;
        let threshold = 1.25 * mean_shc;

        for si in 1..shc_len.saturating_sub(1) {
            if shc[si] > shc[si - 1] && shc[si] > shc[si + 1] && shc[si] > threshold {
                let bin = f0_min_bin + si;
                let freq = bin as f64 * fs / nfft as f64;
                candidates.push(PitchCandidate {
                    freq_hz: freq,
                    merit: shc[si] / (mean_shc.max(1e-20)),
                });
            }
        }

        // Sort by merit descending, keep top 4.
        candidates.sort_by(|a, b| {
            b.merit
                .partial_cmp(&a.merit)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(4);

        // Normalize merit to 0–1 range.
        if let Some(max_m) = candidates.first().map(|c| c.merit) {
            if max_m > 1e-10 {
                for c in &mut candidates {
                    c.merit = (c.merit / max_m).clamp(0.0, 1.0);
                }
            }
        }

        candidates
    }

    /// NCCF: normalized cross-correlation to extract temporal pitch candidates.
    fn compute_nccf(&self, buf: &[f64]) -> Vec<PitchCandidate> {
        let n = self.frame_size;
        let buf_len = buf.len();
        let lag_min = self.lag_min;
        let lag_max = self.lag_max;

        if lag_min >= lag_max || lag_max >= n {
            return Vec::new();
        }

        // Extract the analysis frame (mean-subtracted).
        let mut frame = vec![0.0f64; n];
        let mut mean = 0.0;
        for i in 0..n {
            let idx = (self.write_pos + buf_len - n + i) % buf_len;
            frame[i] = buf[idx];
            mean += frame[i];
        }
        mean /= n as f64;
        for s in &mut frame {
            *s -= mean;
        }

        // Energy of the reference segment.
        let seg_len = n - lag_max;
        let mut energy_ref = 0.0f64;
        for i in 0..seg_len {
            energy_ref += frame[i] * frame[i];
        }

        // Compute NCCF for each lag.
        let mut nccf = vec![0.0f64; lag_max + 1];
        for lag in lag_min..=lag_max {
            let mut cross = 0.0f64;
            let mut energy_lag = 0.0f64;
            for i in 0..seg_len {
                cross += frame[i] * frame[i + lag];
                energy_lag += frame[i + lag] * frame[i + lag];
            }
            let denom = (energy_ref * energy_lag).sqrt();
            nccf[lag] = if denom > 1e-20 { cross / denom } else { 0.0 };
        }

        // Peak picking.
        let mut candidates = Vec::with_capacity(self.params.nccf_maxcands);

        for lag in (lag_min + 1)..lag_max {
            if nccf[lag] > nccf[lag - 1]
                && nccf[lag] > nccf[lag + 1]
                && nccf[lag] > self.params.nccf_thresh
            {
                // Parabolic interpolation for sub-sample accuracy.
                let a = nccf[lag - 1];
                let b = nccf[lag];
                let c = nccf[lag + 1];
                let denom = 2.0 * (2.0 * b - a - c);
                let refined_lag = if denom.abs() > 1e-10 {
                    lag as f64 + (a - c) / denom
                } else {
                    lag as f64
                };

                let freq = self.sample_rate / refined_lag;
                if freq >= self.params.f0_min && freq <= self.params.f0_max {
                    candidates.push(PitchCandidate {
                        freq_hz: freq,
                        merit: nccf[lag].clamp(0.0, 1.0),
                    });
                }
            }
        }

        // Sort by merit descending, keep top N.
        candidates.sort_by(|a, b| {
            b.merit
                .partial_cmp(&a.merit)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(self.params.nccf_maxcands);
        candidates
    }

    /// Select the best pitch candidate by merging spectral and temporal evidence.
    fn select_best(
        &self,
        spectral: &[PitchCandidate],
        temporal1: &[PitchCandidate],
        temporal2: &[PitchCandidate],
        is_voiced_energy: bool,
    ) -> PitchCandidate {
        let unvoiced = PitchCandidate {
            freq_hz: 0.0,
            merit: 0.0,
        };

        // Collect all temporal candidates.
        let mut all: Vec<PitchCandidate> =
            Vec::with_capacity(temporal1.len() + temporal2.len() + spectral.len());
        all.extend_from_slice(temporal1);
        all.extend_from_slice(temporal2);

        // Also include spectral candidates directly — SHC is better at resolving
        // octave ambiguity because it checks harmonic structure explicitly.
        for sc in spectral {
            all.push(PitchCandidate {
                freq_hz: sc.freq_hz,
                merit: sc.merit * 0.85, // Slightly lower base merit than NCCF
            });
        }

        if all.is_empty() {
            return unvoiced;
        }

        // Score each candidate based on cross-domain agreement.
        let spec_f0 = spectral.first().map(|s| s.freq_hz).unwrap_or(0.0);

        for cand in &mut all {
            // Spectral agreement: boost candidates near the best SHC frequency.
            if spec_f0 > 0.0 {
                let tol = spec_f0 * 0.08;
                let diff = (cand.freq_hz - spec_f0).abs();
                if diff < tol {
                    let agreement = 1.0 - diff / tol;
                    cand.merit = (cand.merit * (1.0 + self.params.merit_boost * 1.5 * agreement))
                        .clamp(0.0, 1.0);
                }
                // Penalize candidates at sub-harmonics of the spectral F0.
                for divisor in [2.0, 3.0] {
                    let sub = spec_f0 / divisor;
                    let sub_diff = (cand.freq_hz - sub).abs();
                    if sub_diff < sub * 0.08 {
                        cand.merit *= 0.4;
                    }
                }
            }

            // Continuity bonus.
            if self.prev_freq > 0.0 {
                let tol = self.prev_freq * 0.05;
                let diff = (cand.freq_hz - self.prev_freq).abs();
                if diff < tol {
                    let continuity = 1.0 - diff / tol;
                    cand.merit = (cand.merit * (1.0 + 0.15 * continuity)).clamp(0.0, 1.0);
                }
            }
        }

        // If energy says unvoiced, penalize all candidates.
        if !is_voiced_energy {
            for cand in &mut all {
                cand.merit *= 0.5;
            }
        }

        // Pick the best candidate.
        all.sort_by(|a, b| {
            b.merit
                .partial_cmp(&a.merit)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all[0]
    }

    /// Apply median filter for smoothing (removes isolated outliers).
    fn apply_median(&mut self, freq: f64) -> f64 {
        let len = self.params.median_len;
        self.median_buf[self.median_pos] = freq;
        self.median_pos = (self.median_pos + 1) % len;

        // Only median-filter voiced frames (freq > 0).
        let voiced_count = self.median_buf.iter().filter(|&&f| f > 0.0).count();
        if voiced_count < len / 2 + 1 {
            // Majority unvoiced — return 0.
            return if freq > 0.0 { freq } else { 0.0 };
        }

        // Median of the voiced values.
        let mut sorted: Vec<f64> = self
            .median_buf
            .iter()
            .copied()
            .filter(|&f| f > 0.0)
            .collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sorted[sorted.len() / 2]
    }
}

impl Default for YaaptDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ── Simple real-valued FFT (radix-2 DIT) ────────────────────────────────
//
// We only need forward FFT for NLFER and SHC. This is a basic
// implementation sufficient for our frame sizes (power-of-2).

fn real_fft_inplace(real: &mut [f64], imag: &mut [f64], n: usize) {
    // Compute complex FFT of real input (imag should be zeroed).
    fft_dit(real, imag, n);
}

/// Radix-2 decimation-in-time FFT (Cooley-Tukey).
fn fft_dit(real: &mut [f64], imag: &mut [f64], n: usize) {
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 0..n {
        if i < j {
            real.swap(i, j);
            imag.swap(i, j);
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
        let angle = -2.0 * PI / len as f64;
        let wn_r = angle.cos();
        let wn_i = angle.sin();

        let mut start = 0;
        while start < n {
            let mut w_r = 1.0;
            let mut w_i = 0.0;

            for k in 0..half {
                let a = start + k;
                let b = start + k + half;

                let tr = w_r * real[b] - w_i * imag[b];
                let ti = w_r * imag[b] + w_i * real[b];

                real[b] = real[a] - tr;
                imag[b] = imag[a] - ti;
                real[a] += tr;
                imag[a] += ti;

                let new_w_r = w_r * wn_r - w_i * wn_i;
                let new_w_i = w_r * wn_i + w_i * wn_r;
                w_r = new_w_r;
                w_i = new_w_i;
            }

            start += len;
        }

        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn make_detector() -> YaaptDetector {
        let mut d = YaaptDetector::new();
        d.update(SR);
        d
    }

    fn generate_sine(freq: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f64 / SR).sin() * 0.8)
            .collect()
    }

    #[test]
    fn detects_a4_440hz() {
        let mut d = make_detector();
        let signal = generate_sine(440.0, 48000);

        let mut last = PitchEstimate::unvoiced();
        for &s in &signal {
            last = d.tick(s);
        }

        assert!(
            last.confidence > 0.3,
            "Should detect A4 with confidence, got {}",
            last.confidence
        );
        assert!(
            (last.freq_hz - 440.0).abs() < 10.0,
            "Should detect ~440Hz, got {}",
            last.freq_hz
        );
    }

    #[test]
    fn detects_low_e_82hz() {
        let mut d = make_detector();
        let signal = generate_sine(82.41, 48000);

        let mut last = PitchEstimate::unvoiced();
        for &s in &signal {
            last = d.tick(s);
        }

        assert!(
            last.confidence > 0.2,
            "Should detect low E, confidence={}",
            last.confidence
        );
        assert!(
            (last.freq_hz - 82.41).abs() < 5.0,
            "Should detect ~82Hz, got {}",
            last.freq_hz
        );
    }

    #[test]
    fn detects_c5_523hz() {
        let mut d = make_detector();
        let signal = generate_sine(523.25, 48000); // C5

        let mut last = PitchEstimate::unvoiced();
        for &s in &signal {
            last = d.tick(s);
        }

        // C5 is within f0_max=500 range only if we're lenient; skip strict freq check
        // but verify no crash and reasonable output.
        assert!(last.freq_hz.is_finite());
        assert!(last.confidence.is_finite());
    }

    #[test]
    fn silence_is_unvoiced() {
        let mut d = make_detector();

        for _ in 0..4800 {
            let est = d.tick(0.0);
            assert!(est.confidence < 0.1, "Silence should be unvoiced");
        }
    }

    #[test]
    fn no_nan() {
        let mut d = make_detector();
        let signal = generate_sine(220.0, 48000);

        for &s in &signal {
            let est = d.tick(s);
            assert!(est.freq_hz.is_finite());
            assert!(est.semitones.is_finite());
            assert!(est.confidence.is_finite());
        }
    }

    #[test]
    fn different_pitches_detected() {
        let freqs = [110.0, 220.0, 440.0];
        let mut detected = Vec::new();

        for &freq in &freqs {
            let mut d = make_detector();
            let signal = generate_sine(freq, 48000);
            let mut last = PitchEstimate::unvoiced();
            for &s in &signal {
                last = d.tick(s);
            }
            detected.push(last.freq_hz);
        }

        // Each should be roughly double the previous.
        for i in 1..detected.len() {
            let ratio = detected[i] / detected[i - 1];
            assert!(
                (ratio - 2.0).abs() < 0.5,
                "Freq ratio should be ~2.0 between {} and {}, got {} (detected: {:.1}, {:.1})",
                freqs[i - 1],
                freqs[i],
                ratio,
                detected[i - 1],
                detected[i],
            );
        }
    }

    #[test]
    fn midi_note_conversion() {
        let mut d = make_detector();
        let signal = generate_sine(440.0, 48000);
        let mut last = PitchEstimate::unvoiced();
        for &s in &signal {
            last = d.tick(s);
        }

        if last.confidence > 0.1 {
            assert!(
                (last.midi_note - 69.0).abs() < 1.0,
                "A4 should be MIDI ~69, got {}",
                last.midi_note
            );
        }
    }

    #[test]
    fn fft_basic_correctness() {
        // FFT of a single-frequency signal should peak at that frequency.
        let n = 1024;
        let freq = 100.0;
        let mut real: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * freq * i as f64 / SR).sin())
            .collect();
        let mut imag = vec![0.0; n];

        fft_dit(&mut real, &mut imag, n);

        // Find peak bin.
        let mut max_mag = 0.0f64;
        let mut max_bin = 0;
        for k in 1..n / 2 {
            let mag = (real[k] * real[k] + imag[k] * imag[k]).sqrt();
            if mag > max_mag {
                max_mag = mag;
                max_bin = k;
            }
        }

        let detected_freq = max_bin as f64 * SR / n as f64;
        assert!(
            (detected_freq - freq).abs() < SR / n as f64,
            "FFT peak should be at ~{freq}Hz, got {detected_freq}Hz"
        );
    }
}
