//! Bitstream Autocorrelation (BSAC) pitch detector.
//!
//! Ultra-low-latency pitch detection using 1-bit signal quantization
//! and XOR + popcount for fast autocorrelation computation.
//!
//! The signal is converted to a binary stream (sign bits), then
//! autocorrelation is computed by XOR-ing bitstream segments and
//! counting matching bits. This is ~64x faster than float ACF
//! since a single u64 XOR processes 64 samples simultaneously.
//!
//! Reference: Van Vleck & Middleton (arcsine law for clipped ACF);
//! cycfi Q library (production BSAC implementation).

use std::f64::consts::TAU;

use crate::detector::PitchEstimate;

// ── Configuration ───────────────────────────────────────────────────────

/// Minimum detectable frequency (Hz).
const F_MIN: f64 = 60.0;
/// Maximum detectable frequency (Hz).
const F_MAX: f64 = 1000.0;
/// Minimum periodicity (correlation) for a valid detection.
/// Lower than typical ACF thresholds because 1-bit quantization
/// introduces correlation loss, especially for lower frequencies
/// where the fractional period doesn't align with word boundaries.
const MIN_PERIODICITY: f64 = 0.5;

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
        Self { ic1eq: 0.0, ic2eq: 0.0, a1: 0.0, a2: 0.0, a3: 0.0, k: 1.0 }
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

// ── Bitstream ACF Detector ──────────────────────────────────────────────

/// Bitstream autocorrelation pitch detector.
///
/// Quantizes the signal to 1 bit (sign), then uses XOR+popcount
/// for extremely fast autocorrelation. Each u64 word processes
/// 64 samples in a single XOR operation.
pub struct BitstreamDetector {
    /// Reference frequency for A4 (default 440.0).
    pub a_freq: f64,
    /// Silence gate in dB.
    pub gate_db: f64,

    sample_rate: f64,
    /// Total buffer size in samples (must be power of 2 * 64).
    buf_samples: usize,
    /// Analysis hop size.
    hop_size: usize,

    // Pre-filters.
    hp_filter: Svf,
    lp_filter: Svf,

    // Sample buffer (for sub-sample interpolation via zero crossings).
    sample_buf: Vec<f64>,
    sample_pos: usize,

    // Bitstream storage: packed u64 words.
    words: Vec<u64>,
    /// Current bit position within the bitstream.
    bit_pos: usize,

    // Lag range.
    min_lag: usize,
    max_lag: usize,

    hop_count: usize,

    // Previous estimate for hysteresis.
    prev_sign: bool,

    last_estimate: PitchEstimate,
}

impl BitstreamDetector {
    pub fn new() -> Self {
        Self {
            a_freq: 440.0,
            gate_db: -60.0,
            sample_rate: 48000.0,
            buf_samples: 2048,
            hop_size: 256,
            hp_filter: Svf::new(),
            lp_filter: Svf::new(),
            sample_buf: Vec::new(),
            sample_pos: 0,
            words: Vec::new(),
            bit_pos: 0,
            min_lag: 0,
            max_lag: 0,
            hop_count: 0,
            prev_sign: false,
            last_estimate: PitchEstimate::unvoiced(),
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;

        let max_period = (sample_rate / F_MIN).ceil() as usize;
        // Buffer must hold at least 3 full periods, rounded up to multiple of 64.
        self.buf_samples = ((max_period * 3 + 63) / 64 * 64).next_power_of_two();
        self.hop_size = self.buf_samples / 8;

        let n_words = self.buf_samples / 64;
        self.words = vec![0u64; n_words];
        self.sample_buf = vec![0.0; self.buf_samples];
        self.sample_pos = 0;
        self.bit_pos = 0;

        self.min_lag = (sample_rate / F_MAX).floor() as usize;
        self.max_lag = (sample_rate / F_MIN).ceil() as usize;
        self.max_lag = self.max_lag.min(self.buf_samples / 2 - 1);

        self.hp_filter.set_params(F_MIN, 0.5, sample_rate);
        self.lp_filter.set_params(F_MAX, 0.5, sample_rate);
        self.hop_count = 0;
        self.prev_sign = false;
    }

    pub fn reset(&mut self) {
        self.words.fill(0);
        self.sample_buf.fill(0.0);
        self.sample_pos = 0;
        self.bit_pos = 0;
        self.hop_count = 0;
        self.prev_sign = false;
        self.hp_filter.reset();
        self.lp_filter.reset();
        self.last_estimate = PitchEstimate::unvoiced();
    }

    #[inline]
    pub fn tick(&mut self, input: f64) -> PitchEstimate {
        let filtered = self.lp_filter.tick_lp(self.hp_filter.tick_hp(input));

        // Store filtered sample for zero-crossing interpolation.
        self.sample_buf[self.sample_pos] = filtered;
        self.sample_pos = (self.sample_pos + 1) % self.buf_samples;

        // Convert to 1-bit with hysteresis.
        let sign = if filtered > 0.01 {
            true
        } else if filtered < -0.01 {
            false
        } else {
            self.prev_sign
        };
        self.prev_sign = sign;

        // Set bit in packed word.
        let word_idx = self.bit_pos / 64;
        let bit_idx = self.bit_pos % 64;
        if sign {
            self.words[word_idx] |= 1u64 << bit_idx;
        } else {
            self.words[word_idx] &= !(1u64 << bit_idx);
        }
        self.bit_pos = (self.bit_pos + 1) % self.buf_samples;

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
        self.buf_samples / 2
    }

    // ── Analysis ────────────────────────────────────────────────────────

    fn analyze(&self) -> PitchEstimate {
        // Gate check: compute RMS of recent samples.
        let n = self.buf_samples / 2;
        let mut rms = 0.0f64;
        for i in 0..n {
            let idx = (self.sample_pos + self.buf_samples - n + i) % self.buf_samples;
            let s = self.sample_buf[idx];
            rms += s * s;
        }
        rms = (rms / n as f64).sqrt();
        let db = if rms > 1e-20 { 20.0 * rms.log10() } else { -120.0 };
        if db < self.gate_db {
            return PitchEstimate::unvoiced();
        }

        // Compute bitstream ACF for all lags in range.
        let n_words = self.words.len();
        let half_words = n_words / 2;

        let mut best_lag = 0usize;
        let mut best_periodicity = 0.0f64;

        for lag in self.min_lag..=self.max_lag {
            let periodicity = self.bitstream_acf(lag, half_words);
            if periodicity > best_periodicity {
                best_periodicity = periodicity;
                best_lag = lag;
            }
        }

        if best_periodicity < MIN_PERIODICITY {
            return PitchEstimate::unvoiced();
        }

        // Check for sub-harmonics (octave error correction).
        // If half the period also shows strong correlation, use it.
        if best_lag >= self.min_lag * 2 {
            let half_lag = best_lag / 2;
            if half_lag >= self.min_lag {
                let half_per = self.bitstream_acf(half_lag, half_words);
                if half_per > best_periodicity * 0.85 {
                    best_lag = half_lag;
                    best_periodicity = half_per;
                }
            }
        }

        // Sub-sample refinement via zero-crossing interpolation.
        let refined_lag = self.refine_via_zero_crossings(best_lag);

        if refined_lag < 1.0 {
            return PitchEstimate::unvoiced();
        }

        let freq = self.sample_rate / refined_lag;
        if freq < F_MIN || freq > F_MAX {
            return PitchEstimate::unvoiced();
        }

        let semitones = 12.0 * (freq / self.a_freq).log2();
        let midi_note = 69.0 + semitones;

        PitchEstimate {
            freq_hz: freq,
            semitones,
            midi_note,
            confidence: best_periodicity.clamp(0.0, 1.0),
        }
    }

    /// Compute bitstream ACF at a given lag.
    /// Returns periodicity in [0, 1] where 1 = perfect correlation.
    #[inline]
    fn bitstream_acf(&self, lag: usize, half_words: usize) -> f64 {
        let n_words = self.words.len();
        let word_offset = lag / 64;
        let bit_shift = lag % 64;

        // Start from the most recent half of the bitstream.
        let start = (self.bit_pos / 64 + n_words - half_words) % n_words;

        let mut mismatch: u32 = 0;
        let total_bits = half_words * 64;

        if bit_shift == 0 {
            // Aligned case — fast path.
            for i in 0..half_words {
                let idx_a = (start + i) % n_words;
                let idx_b = (start + i + word_offset) % n_words;
                mismatch += (self.words[idx_a] ^ self.words[idx_b]).count_ones();
            }
        } else {
            // Unaligned case — shift and combine adjacent words.
            let shift2 = 64 - bit_shift;
            for i in 0..half_words {
                let idx_a = (start + i) % n_words;
                let idx_b1 = (start + i + word_offset) % n_words;
                let idx_b2 = (start + i + word_offset + 1) % n_words;
                let shifted = (self.words[idx_b1] >> bit_shift)
                    | (self.words[idx_b2] << shift2);
                mismatch += (self.words[idx_a] ^ shifted).count_ones();
            }
        }

        // Convert mismatch count to periodicity.
        1.0 - (2.0 * mismatch as f64 / total_bits as f64)
    }

    /// Refine the lag estimate using zero-crossing interpolation.
    fn refine_via_zero_crossings(&self, lag: usize) -> f64 {
        let n = self.buf_samples;

        // Find the most recent zero crossing near sample_pos - lag.
        // This gives sub-sample accuracy.
        let search_start = (self.sample_pos + n - lag - 2) % n;

        // Find a positive-going zero crossing.
        for offset in 0..4 {
            let idx = (search_start + offset) % n;
            let next_idx = (idx + 1) % n;
            let prev = self.sample_buf[idx];
            let curr = self.sample_buf[next_idx];

            if prev <= 0.0 && curr > 0.0 {
                // Linear interpolation for fractional sample.
                let frac = -prev / (curr - prev + 1e-20);
                let exact_pos = (idx as f64 + frac) % n as f64;

                // Now find the corresponding zero crossing near sample_pos.
                let target_start = (self.sample_pos + n - 2) % n;
                for offset2 in 0..4 {
                    let idx2 = (target_start + offset2) % n;
                    let next_idx2 = (idx2 + 1) % n;
                    let prev2 = self.sample_buf[idx2];
                    let curr2 = self.sample_buf[next_idx2];

                    if prev2 <= 0.0 && curr2 > 0.0 {
                        let frac2 = -prev2 / (curr2 - prev2 + 1e-20);
                        let exact_pos2 = (idx2 as f64 + frac2) % n as f64;

                        // Compute the distance.
                        let mut dist = exact_pos2 - exact_pos;
                        if dist < 0.0 {
                            dist += n as f64;
                        }

                        // Sanity check: should be close to the original lag.
                        if (dist - lag as f64).abs() < 5.0 {
                            return dist;
                        }
                    }
                }
            }
        }

        // Fallback to integer lag.
        lag as f64
    }
}

impl Default for BitstreamDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_detector() -> BitstreamDetector {
        let mut d = BitstreamDetector::new();
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
            last.confidence > 0.3,
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
            let signal = generate_sine(freq, 96000); // 2 seconds for filter settling
            let mut last = PitchEstimate::unvoiced();
            for &s in &signal {
                last = d.tick(s);
            }
            detected.push(last.freq_hz);
        }

        for i in 1..detected.len() {
            let ratio = detected[i] / detected[i - 1];
            assert!(
                (ratio - 2.0).abs() < 0.5,
                "Freq ratio should be ~2.0 between {} and {}, got {} (detected {:.1}, {:.1})",
                freqs[i - 1],
                freqs[i],
                ratio,
                detected[i - 1],
                detected[i],
            );
        }
    }
}
