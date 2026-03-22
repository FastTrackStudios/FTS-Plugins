//! McLeod Pitch Method (MPM) pitch detector.
//!
//! Uses the Normalized Square Difference Function (NSDF) which is bounded
//! to [-1, +1] and has no systematic bias toward shorter lags, giving
//! excellent octave-error resistance for harmonic-rich signals.
//!
//! Reference: McLeod & Wyvill, "A Smarter Way to Find Pitch", ICMC 2005.

use std::f64::consts::TAU;

use crate::detector::PitchEstimate;

// ── Configuration ───────────────────────────────────────────────────────

/// First peak above this fraction of the highest peak is selected.
const DEFAULT_CUTOFF: f64 = 0.93;
/// Pre-filter: ignore peaks below this absolute NSDF value.
const SMALL_CUTOFF: f64 = 0.5;
/// Minimum detectable frequency (Hz).
const F_MIN: f64 = 60.0;
/// Maximum detectable frequency (Hz).
const F_MAX: f64 = 1000.0;

// ── SVF for pre-filtering ──────────────────────────────────────────────

struct Svf {
    ic1eq: f64,
    ic2eq: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    k: f64,
}

impl Svf {
    fn new() -> Self {
        Self {
            ic1eq: 0.0,
            ic2eq: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            k: 1.0,
        }
    }

    fn set_params(&mut self, freq: f64, q: f64, sample_rate: f64) {
        let g = (TAU * freq / sample_rate / 2.0).tan();
        self.k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    #[inline]
    fn tick_hp(&mut self, input: f64) -> f64 {
        let v3 = input - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        input - self.k * v1 - v2
    }

    #[inline]
    fn tick_lp(&mut self, input: f64) -> f64 {
        let v3 = input - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        v2
    }

    fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }
}

// ── MPM Detector ────────────────────────────────────────────────────────

/// McLeod Pitch Method detector using NSDF.
pub struct MpmDetector {
    /// Reference frequency for A4 (default 440.0).
    pub a_freq: f64,
    /// Silence gate in dB.
    pub gate_db: f64,

    sample_rate: f64,
    window_size: usize,
    hop_size: usize,

    // Ring buffer.
    buffer: Vec<f64>,
    write_pos: usize,
    hop_count: usize,

    // Scratch for NSDF computation.
    nsdf: Vec<f64>,
    /// Extracted analysis frame.
    frame: Vec<f64>,

    // Pre-filters.
    hp_filter: Svf,
    lp_filter: Svf,

    last_estimate: PitchEstimate,
}

impl MpmDetector {
    pub fn new() -> Self {
        Self {
            a_freq: 440.0,
            gate_db: -60.0,
            sample_rate: 48000.0,
            window_size: 1024,
            hop_size: 256,
            buffer: vec![0.0; 2048],
            write_pos: 0,
            hop_count: 0,
            nsdf: vec![0.0; 1024],
            frame: vec![0.0; 1024],
            hp_filter: Svf::new(),
            lp_filter: Svf::new(),
            last_estimate: PitchEstimate::unvoiced(),
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.window_size = if sample_rate >= 88200.0 { 2048 } else { 1024 };
        self.hop_size = self.window_size / 4;
        self.buffer = vec![0.0; self.window_size * 2];
        self.nsdf = vec![0.0; self.window_size];
        self.frame = vec![0.0; self.window_size];
        self.write_pos = 0;
        self.hop_count = 0;

        self.hp_filter.set_params(F_MIN, 0.5, sample_rate);
        self.lp_filter.set_params(F_MAX, 0.5, sample_rate);
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
        self.hop_count = 0;
        self.hp_filter.reset();
        self.lp_filter.reset();
        self.last_estimate = PitchEstimate::unvoiced();
    }

    #[inline]
    pub fn tick(&mut self, input: f64) -> PitchEstimate {
        let filtered = self.lp_filter.tick_lp(self.hp_filter.tick_hp(input));
        let buf_len = self.buffer.len();
        self.buffer[self.write_pos] = filtered;
        self.write_pos = (self.write_pos + 1) % buf_len;
        self.hop_count += 1;

        if self.hop_count >= self.hop_size {
            self.hop_count = 0;
            self.last_estimate = self.analyze();
        }

        self.last_estimate
    }

    pub fn current(&self) -> PitchEstimate {
        self.last_estimate
    }

    pub fn latency(&self) -> usize {
        self.window_size
    }

    // ── Analysis ────────────────────────────────────────────────────────

    fn analyze(&mut self) -> PitchEstimate {
        let w = self.window_size;
        let buf_len = self.buffer.len();

        // Extract frame from ring buffer.
        for i in 0..w {
            self.frame[i] = self.buffer[(self.write_pos + buf_len - w + i) % buf_len];
        }

        // Gate check.
        let mut power = 0.0f64;
        for &s in &self.frame[..w] {
            power += s * s;
        }
        let rms = (power / w as f64).sqrt();
        let db = if rms > 1e-20 {
            20.0 * rms.log10()
        } else {
            -120.0
        };
        if db < self.gate_db {
            return PitchEstimate::unvoiced();
        }

        // Compute NSDF.
        self.compute_nsdf();

        // Lag range.
        let min_lag = (self.sample_rate / F_MAX).floor() as usize;
        let max_lag = ((self.sample_rate / F_MIN).ceil() as usize).min(w - 1);
        if min_lag >= max_lag {
            return PitchEstimate::unvoiced();
        }

        // Find key maxima (one per positive lobe).
        let key_maxima = self.find_key_maxima(min_lag, max_lag);
        if key_maxima.is_empty() {
            return PitchEstimate::unvoiced();
        }

        // Find highest amplitude among key maxima.
        let highest_amp = key_maxima.iter().map(|&(_, a)| a).fold(0.0f64, f64::max);

        if highest_amp < SMALL_CUTOFF {
            return PitchEstimate::unvoiced();
        }

        // Select first peak above cutoff threshold.
        let actual_cutoff = DEFAULT_CUTOFF * highest_amp;

        for &(tau, amp) in &key_maxima {
            if amp < SMALL_CUTOFF {
                continue;
            }
            // Parabolic interpolation.
            let (refined_tau, refined_amp) = self.parabolic_interp(tau);

            if refined_amp >= actual_cutoff {
                let freq = self.sample_rate / refined_tau;
                if freq < F_MIN || freq > F_MAX {
                    continue;
                }

                let semitones = 12.0 * (freq / self.a_freq).log2();
                let midi_note = 69.0 + semitones;

                return PitchEstimate {
                    freq_hz: freq,
                    semitones,
                    midi_note,
                    confidence: highest_amp.clamp(0.0, 1.0),
                };
            }
        }

        PitchEstimate::unvoiced()
    }

    /// Compute the Normalized Square Difference Function.
    ///
    /// n'(tau) = 2 * r(tau) / m(tau)
    ///
    /// where r(tau) is the autocorrelation and m(tau) is the sum of energies.
    fn compute_nsdf(&mut self) {
        let w = self.window_size;
        let frame = &self.frame;

        // Compute autocorrelation r(tau) directly.
        // r(tau) = sum_{j=0}^{W-1-tau} x[j] * x[j+tau]
        // Also compute m(tau) = sum_{j=0}^{W-1-tau} x[j]^2 + sum_{j=0}^{W-1-tau} x[j+tau]^2

        // Precompute cumulative squared sums for efficient m(tau).
        // m(tau) = sum_{j=0}^{W-1-tau} x[j]^2 + sum_{j=tau}^{W-1} x[j]^2
        let mut sum_sq_full = 0.0f64;
        for j in 0..w {
            sum_sq_full += frame[j] * frame[j];
        }

        // m(0) = 2 * sum(x^2)
        let mut m_left = sum_sq_full; // sum_{j=0}^{W-1-tau} x[j]^2
        let mut m_right = sum_sq_full; // sum_{j=tau}^{W-1} x[j]^2

        self.nsdf[0] = 1.0; // n'(0) = 1 by definition

        for tau in 1..w {
            // Update m(tau): subtract one term from each side.
            m_left -= frame[w - tau] * frame[w - tau];
            m_right -= frame[tau - 1] * frame[tau - 1];
            let m_tau = m_left + m_right;

            // Compute autocorrelation at this lag.
            let mut r = 0.0f64;
            for j in 0..(w - tau) {
                r += frame[j] * frame[j + tau];
            }

            self.nsdf[tau] = if m_tau > 1e-20 { 2.0 * r / m_tau } else { 0.0 };
        }
    }

    /// Find key maxima: one per positive lobe of the NSDF.
    fn find_key_maxima(&self, min_lag: usize, max_lag: usize) -> Vec<(usize, f64)> {
        let mut maxima = Vec::new();
        let nsdf = &self.nsdf;
        let w = max_lag + 1;

        // Skip initial positive region (starts at nsdf[0]=1.0).
        let mut pos = 1;
        while pos < w / 3 && pos < w && nsdf[pos] > 0.0 {
            pos += 1;
        }
        // Skip through first negative region.
        while pos < w && nsdf[pos] <= 0.0 {
            pos += 1;
        }

        let mut cur_max_pos = 0usize;
        let mut cur_max_val = f64::NEG_INFINITY;

        while pos < w.saturating_sub(1) {
            // Local maximum check.
            if nsdf[pos] > nsdf[pos - 1] && nsdf[pos] >= nsdf[pos + 1] {
                if nsdf[pos] > cur_max_val {
                    cur_max_pos = pos;
                    cur_max_val = nsdf[pos];
                }
            }
            pos += 1;

            // Zero crossing — entering negative region.
            if pos < w && nsdf[pos] <= 0.0 {
                if cur_max_val > f64::NEG_INFINITY && cur_max_pos >= min_lag {
                    maxima.push((cur_max_pos, cur_max_val));
                }
                cur_max_pos = 0;
                cur_max_val = f64::NEG_INFINITY;

                // Skip through negative region.
                while pos < w && nsdf[pos] <= 0.0 {
                    pos += 1;
                }
            }
        }

        // Don't forget the last lobe.
        if cur_max_val > f64::NEG_INFINITY && cur_max_pos >= min_lag {
            maxima.push((cur_max_pos, cur_max_val));
        }

        maxima
    }

    /// Parabolic interpolation for sub-sample accuracy.
    /// Returns (refined_tau, refined_amplitude).
    fn parabolic_interp(&self, tau: usize) -> (f64, f64) {
        let w = self.window_size;
        if tau == 0 || tau >= w - 1 {
            return (tau as f64, self.nsdf[tau]);
        }

        let y0 = self.nsdf[tau - 1];
        let y1 = self.nsdf[tau];
        let y2 = self.nsdf[tau + 1];

        let a = (y0 + y2) / 2.0 - y1;
        let b = (y2 - y0) / 2.0;

        // If parabola opens upward, it's not a real peak.
        if a >= 0.0 {
            return (tau as f64, y1);
        }

        let delta_x = -b / (2.0 * a);
        let delta_y = y1 - b * b / (4.0 * a);

        (tau as f64 + delta_x, delta_y)
    }
}

impl Default for MpmDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_detector() -> MpmDetector {
        let mut d = MpmDetector::new();
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
            last.confidence > 0.5,
            "Should detect A4, confidence={}",
            last.confidence
        );
        assert!(
            (last.freq_hz - 440.0).abs() < 5.0,
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
            last.confidence > 0.3,
            "Should detect low E, confidence={}",
            last.confidence
        );
        assert!(
            (last.freq_hz - 82.41).abs() < 3.0,
            "Should detect ~82Hz, got {}",
            last.freq_hz
        );
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

        for i in 1..detected.len() {
            let ratio = detected[i] / detected[i - 1];
            assert!(
                (ratio - 2.0).abs() < 0.3,
                "Freq ratio should be ~2.0 between {} and {}, got {}",
                freqs[i - 1],
                freqs[i],
                ratio
            );
        }
    }

    #[test]
    fn high_confidence_for_pure_tone() {
        let mut d = make_detector();
        let signal = generate_sine(440.0, 48000);

        let mut last = PitchEstimate::unvoiced();
        for &s in &signal {
            last = d.tick(s);
        }

        assert!(
            last.confidence > 0.9,
            "Pure sine should have very high confidence, got {}",
            last.confidence
        );
    }
}
