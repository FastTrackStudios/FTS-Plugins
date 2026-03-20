//! YIN pitch detector with SVF pre-filter and confidence output.
//!
//! Implements the YIN algorithm (de Cheveigné & Kawahara, 2002):
//! 1. Difference function
//! 2. Cumulative mean normalized difference
//! 3. Absolute threshold (first dip below threshold)
//! 4. Parabolic interpolation for sub-sample accuracy
//!
//! Pre-filters input with a bandpass (SVF high-pass + low-pass) to
//! reject out-of-range content before pitch estimation.

use std::f64::consts::TAU;

/// Result of a single pitch detection frame.
#[derive(Debug, Clone, Copy)]
pub struct PitchEstimate {
    /// Detected frequency in Hz (0.0 if unvoiced).
    pub freq_hz: f64,
    /// Detected pitch in semitones relative to A4 (0.0 = 440Hz).
    pub semitones: f64,
    /// MIDI note number (69.0 = A4).
    pub midi_note: f64,
    /// Confidence (0.0–1.0). Higher = more periodic/tonal.
    pub confidence: f64,
}

impl PitchEstimate {
    pub fn unvoiced() -> Self {
        Self {
            freq_hz: 0.0,
            semitones: 0.0,
            midi_note: 0.0,
            confidence: 0.0,
        }
    }
}

/// Simple 2nd-order SVF (state-variable filter) for pre-filtering.
struct Svf {
    ic1eq: f64,
    ic2eq: f64,
    a1: f64,
    a2: f64,
    a3: f64,
}

impl Svf {
    fn new() -> Self {
        Self {
            ic1eq: 0.0,
            ic2eq: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
        }
    }

    fn set_params(&mut self, freq: f64, q: f64, sample_rate: f64) {
        let g = (TAU * freq / sample_rate / 2.0).tan();
        let k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    /// Returns (low, band, high).
    #[inline]
    fn tick(&mut self, input: f64) -> (f64, f64, f64) {
        let v3 = input - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        (
            v2,
            v1,
            input - v1 / self.a1.max(1e-20) * (1.0 / self.a1.max(1e-20) - 1.0).max(0.0),
        )
    }

    /// High-pass output only.
    #[inline]
    fn tick_hp(&mut self, input: f64) -> f64 {
        let v3 = input - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        input - v1 * (1.0 / self.a2.max(1e-20)) * self.a2 - v2
    }

    /// Low-pass output only.
    #[inline]
    fn tick_lp(&mut self, input: f64) -> f64 {
        let (lp, _, _) = self.tick(input);
        lp
    }

    fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }
}

/// YIN pitch detector.
pub struct PitchDetector {
    /// Reference frequency for A4 (default 440.0).
    pub a_freq: f64,
    /// Minimum detectable frequency (Hz).
    pub min_freq: f64,
    /// Maximum detectable frequency (Hz).
    pub max_freq: f64,
    /// YIN threshold (lower = stricter, default 0.15).
    pub threshold: f64,
    /// Silence gate in dB (default -60).
    pub gate_db: f64,

    // Internal state.
    sample_rate: f64,
    /// Ring buffer for input samples.
    buffer: Vec<f64>,
    /// Write position in ring buffer.
    write_pos: usize,
    /// Samples accumulated since last analysis.
    hop_count: usize,
    /// Hop size (analysis interval in samples).
    hop_size: usize,
    /// Analysis window size (= buffer length / 2).
    window_size: usize,

    /// YIN difference function scratch.
    diff: Vec<f64>,
    /// Cumulative mean normalized difference scratch.
    cmndf: Vec<f64>,

    /// Pre-filter: high-pass to remove sub-bass.
    hp_filter: Svf,
    /// Pre-filter: low-pass to remove high harmonics.
    lp_filter: Svf,

    /// Last valid estimate (held during unvoiced frames).
    last_estimate: PitchEstimate,
}

impl PitchDetector {
    /// Default window size for analysis.
    const DEFAULT_WINDOW: usize = 1024;

    pub fn new() -> Self {
        let window = Self::DEFAULT_WINDOW;
        Self {
            a_freq: 440.0,
            min_freq: 60.0,
            max_freq: 1000.0,
            threshold: 0.15,
            gate_db: -60.0,
            sample_rate: 48000.0,
            buffer: vec![0.0; window * 2],
            write_pos: 0,
            hop_count: 0,
            hop_size: window / 4,
            window_size: window,
            diff: vec![0.0; window],
            cmndf: vec![0.0; window],
            hp_filter: Svf::new(),
            lp_filter: Svf::new(),
            last_estimate: PitchEstimate::unvoiced(),
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;

        // Window size: 2048 at >= 88.2kHz, 1024 otherwise (matching MXTune).
        self.window_size = if sample_rate >= 88200.0 { 2048 } else { 1024 };
        self.hop_size = self.window_size / 4;
        self.buffer.resize(self.window_size * 2, 0.0);
        self.diff.resize(self.window_size, 0.0);
        self.cmndf.resize(self.window_size, 0.0);

        // SVF pre-filters.
        self.hp_filter.set_params(self.min_freq, 0.5, sample_rate);
        self.lp_filter.set_params(self.max_freq, 0.5, sample_rate);
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
        self.hop_count = 0;
        self.hp_filter.reset();
        self.lp_filter.reset();
        self.last_estimate = PitchEstimate::unvoiced();
    }

    /// Feed one sample and optionally get a new pitch estimate.
    /// Returns the current estimate (updated every hop_size samples).
    #[inline]
    pub fn tick(&mut self, input: f64) -> PitchEstimate {
        // Pre-filter: bandpass.
        let filtered = self.lp_filter.tick_lp(self.hp_filter.tick_hp(input));

        // Write to ring buffer.
        self.buffer[self.write_pos] = filtered;
        self.write_pos = (self.write_pos + 1) % self.buffer.len();
        self.hop_count += 1;

        // Run analysis every hop_size samples.
        if self.hop_count >= self.hop_size {
            self.hop_count = 0;
            self.last_estimate = self.analyze();
        }

        self.last_estimate
    }

    /// Run YIN analysis on the current buffer contents.
    fn analyze(&mut self) -> PitchEstimate {
        let w = self.window_size;
        let buf_len = self.buffer.len();

        // Check gate: compute RMS of the analysis window.
        let mut rms = 0.0f64;
        for i in 0..w {
            let idx = (self.write_pos + buf_len - w + i) % buf_len;
            let s = self.buffer[idx];
            rms += s * s;
        }
        rms = (rms / w as f64).sqrt();
        let db = if rms > 1e-20 {
            20.0 * rms.log10()
        } else {
            -120.0
        };
        if db < self.gate_db {
            return PitchEstimate::unvoiced();
        }

        // Lag range in samples.
        let min_lag = (self.sample_rate / self.max_freq).floor() as usize;
        let max_lag = (self.sample_rate / self.min_freq).ceil() as usize;
        let max_lag = max_lag.min(w - 1);

        if min_lag >= max_lag || max_lag >= w {
            return PitchEstimate::unvoiced();
        }

        // Step 1: Difference function.
        // d(tau) = sum_{j=0}^{W-1-tau} (x[j] - x[j+tau])^2
        self.diff[0] = 0.0;
        for tau in 1..=max_lag {
            let mut sum = 0.0;
            for j in 0..(w - tau) {
                let idx_a = (self.write_pos + buf_len - w + j) % buf_len;
                let idx_b = (self.write_pos + buf_len - w + j + tau) % buf_len;
                let d = self.buffer[idx_a] - self.buffer[idx_b];
                sum += d * d;
            }
            self.diff[tau] = sum;
        }

        // Step 2: Cumulative mean normalized difference.
        self.cmndf[0] = 1.0;
        let mut running_sum = 0.0;
        for tau in 1..=max_lag {
            running_sum += self.diff[tau];
            self.cmndf[tau] = if running_sum > 1e-20 {
                self.diff[tau] * tau as f64 / running_sum
            } else {
                1.0
            };
        }

        // Step 3: Absolute threshold — find first dip below threshold.
        let mut best_tau = 0;
        for tau in min_lag..=max_lag {
            if self.cmndf[tau] < self.threshold {
                // Find the local minimum in this dip.
                best_tau = tau;
                while best_tau + 1 <= max_lag && self.cmndf[best_tau + 1] < self.cmndf[best_tau] {
                    best_tau += 1;
                }
                break;
            }
        }

        if best_tau == 0 {
            // No dip found — try global minimum as fallback.
            let mut min_val = f64::MAX;
            for tau in min_lag..=max_lag {
                if self.cmndf[tau] < min_val {
                    min_val = self.cmndf[tau];
                    best_tau = tau;
                }
            }
            // Only accept if reasonably low.
            if min_val > 0.5 {
                return PitchEstimate::unvoiced();
            }
        }

        // Step 4: Parabolic interpolation for sub-sample accuracy.
        let period = if best_tau > 0 && best_tau < max_lag {
            let a = self.cmndf[best_tau - 1];
            let b = self.cmndf[best_tau];
            let c = self.cmndf[best_tau + 1];
            let denom = 2.0 * (2.0 * b - a - c);
            if denom.abs() > 1e-20 {
                best_tau as f64 + (a - c) / denom
            } else {
                best_tau as f64
            }
        } else {
            best_tau as f64
        };

        if period < 1.0 {
            return PitchEstimate::unvoiced();
        }

        let freq = self.sample_rate / period;
        let confidence = 1.0 - self.cmndf[best_tau].min(1.0);

        // Convert to semitones relative to A4.
        let semitones = 12.0 * (freq / self.a_freq).log2();
        let midi_note = 69.0 + semitones;

        PitchEstimate {
            freq_hz: freq,
            semitones,
            midi_note,
            confidence,
        }
    }

    /// Get the current estimate without feeding a new sample.
    pub fn current(&self) -> PitchEstimate {
        self.last_estimate
    }

    /// Analysis latency in samples.
    pub fn latency(&self) -> usize {
        self.window_size
    }
}

impl Default for PitchDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_detector() -> PitchDetector {
        let mut d = PitchDetector::new();
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
            "Should detect A4 with confidence, got {}",
            last.confidence
        );
        assert!(
            (last.freq_hz - 440.0).abs() < 5.0,
            "Should detect ~440Hz, got {}",
            last.freq_hz
        );
        assert!(
            last.semitones.abs() < 0.5,
            "Semitones from A4 should be ~0, got {}",
            last.semitones
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
    fn different_pitches_detected_differently() {
        let freqs = [110.0, 220.0, 440.0, 880.0];
        let mut detected = Vec::new();

        for &freq in &freqs {
            let mut d = make_detector();
            let signal = generate_sine(freq, 24000);
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
                (ratio - 2.0).abs() < 0.3,
                "Freq ratio should be ~2.0 between {} and {}, got {}",
                freqs[i - 1],
                freqs[i],
                ratio
            );
        }
    }

    #[test]
    fn midi_note_conversion() {
        let mut d = make_detector();
        // A4 = 440Hz = MIDI 69.
        let signal = generate_sine(440.0, 24000);
        let mut last = PitchEstimate::unvoiced();
        for &s in &signal {
            last = d.tick(s);
        }

        assert!(
            (last.midi_note - 69.0).abs() < 0.5,
            "A4 should be MIDI 69, got {}",
            last.midi_note
        );
    }

    #[test]
    fn confidence_correlates_with_tonality() {
        let mut d_tonal = make_detector();
        let mut d_noise = make_detector();

        // Pure sine — high confidence.
        let sine = generate_sine(440.0, 24000);
        let mut last_tonal = PitchEstimate::unvoiced();
        for &s in &sine {
            last_tonal = d_tonal.tick(s);
        }

        // Noise — low confidence.
        let mut rng = 12345u64;
        let mut last_noise = PitchEstimate::unvoiced();
        for _ in 0..24000 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let noise = (rng as f64 / u64::MAX as f64) * 2.0 - 1.0;
            last_noise = d_noise.tick(noise * 0.5);
        }

        assert!(
            last_tonal.confidence > last_noise.confidence,
            "Tonal should have higher confidence ({}) than noise ({})",
            last_tonal.confidence,
            last_noise.confidence
        );
    }
}
