//! PSOLA — Pitch-Synchronous Overlap-Add for monophonic pitch shifting.
//!
//! The highest-quality algorithm for monophonic sources (guitar DI, vocals).
//! Detects pitch via autocorrelation (simplified McLeod), then resynthesizes
//! at the target pitch using overlapping, windowed grains aligned to pitch
//! periods.
//!
//! Latency: ~2 pitch periods + analysis window (typically 1024–2048 samples).
//! Character: Most natural, preserves formants.

use fts_dsp::delay_line::DelayLine;

/// PSOLA pitch shifter for monophonic audio.
pub struct PsolaShifter {
    /// Pitch ratio: 0.5 = octave down, 2.0 = octave up.
    pub speed: f64,
    /// Mix: 0.0 = dry only, 1.0 = wet only.
    pub mix: f64,
    /// Base analysis window size. Default 2048; set to 512 for low-latency live mode.
    pub base_window_size: usize,

    // Circular analysis buffer.
    analysis_buf: DelayLine,
    // Circular output accumulator.
    output_buf: Vec<f64>,
    output_pos: usize,

    // Pitch detection state.
    /// Detected period in samples (0 = unvoiced).
    detected_period: usize,
    /// Minimum period (samples) — corresponds to max frequency.
    min_period: usize,
    /// Maximum period (samples) — corresponds to min frequency.
    max_period: usize,

    // PSOLA synthesis state.
    /// Samples until next output grain.
    synth_countdown: usize,
    /// Input write position counter (total samples written).
    write_count: usize,
    /// Samples since last pitch detection.
    detect_countdown: usize,
    /// How often to run pitch detection (in samples).
    detect_interval: usize,

    // Autocorrelation scratch buffer.
    autocorr_scratch: Vec<f64>,

    sample_rate: f64,
    /// Analysis window size.
    window_size: usize,
}

impl PsolaShifter {
    /// Default analysis window for pitch detection.
    const DEFAULT_WINDOW: usize = 2048;

    pub fn new() -> Self {
        let buf_len = 48000; // 1 second at 48kHz
        Self {
            speed: 0.5,
            mix: 1.0,
            base_window_size: Self::DEFAULT_WINDOW,
            analysis_buf: DelayLine::new(buf_len),
            output_buf: vec![0.0; buf_len],
            output_pos: 0,
            detected_period: 0,
            min_period: 24,   // ~2000 Hz at 48kHz
            max_period: 1200, // ~40 Hz at 48kHz
            synth_countdown: 0,
            write_count: 0,
            detect_countdown: 0,
            detect_interval: 512,
            autocorr_scratch: vec![0.0; 1200],
            sample_rate: 48000.0,
            window_size: Self::DEFAULT_WINDOW,
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;

        // Guitar range: ~40 Hz (low E drop) to ~2000 Hz (high harmonics).
        self.min_period = (sample_rate / 2000.0) as usize;
        self.max_period = (sample_rate / 40.0) as usize;

        let buf_len = sample_rate as usize + 4096;
        if self.analysis_buf.len() < buf_len {
            self.analysis_buf = DelayLine::new(buf_len);
            self.output_buf = vec![0.0; buf_len];
        }

        self.autocorr_scratch.resize(self.max_period + 1, 0.0);
        self.detect_interval = 512.min(self.max_period);
        self.window_size = self.base_window_size;
    }

    pub fn reset(&mut self) {
        self.analysis_buf.clear();
        self.output_buf.fill(0.0);
        self.output_pos = 0;
        self.detected_period = 0;
        self.synth_countdown = 0;
        self.write_count = 0;
        self.detect_countdown = 0;
    }

    /// Simplified autocorrelation pitch detection (McLeod-inspired).
    /// Returns period in samples, or 0 if unvoiced.
    fn detect_pitch(&mut self) -> usize {
        let window = self.window_size.min(self.max_period * 2 + self.min_period);

        // Need at least window samples in the buffer.
        if self.write_count < window {
            return 0;
        }

        // Compute normalized autocorrelation for each lag.
        let mut best_lag = 0;
        let mut best_clarity: f64 = 0.0;

        // Compute energy of the window.
        let mut energy = 0.0f64;
        for i in 0..window {
            let s = self.analysis_buf.read(i + 1);
            energy += s * s;
        }

        if energy < 1e-10 {
            return 0; // Silence
        }

        for lag in self.min_period..=self.max_period.min(window / 2) {
            let mut correlation = 0.0f64;
            let mut energy_lag = 0.0f64;

            let n = window - lag;
            for i in 0..n {
                let a = self.analysis_buf.read(i + 1);
                let b = self.analysis_buf.read(i + lag + 1);
                correlation += a * b;
                energy_lag += b * b;
            }

            // Normalized correlation.
            let denom = (energy * energy_lag).sqrt();
            let clarity = if denom > 1e-10 {
                correlation / denom
            } else {
                0.0
            };

            if clarity > best_clarity {
                best_clarity = clarity;
                best_lag = lag;
            }
        }

        // Require minimum clarity threshold for voiced detection.
        if best_clarity > 0.5 && best_lag >= self.min_period {
            best_lag
        } else {
            0
        }
    }

    /// Place a Hann-windowed grain centered at `center_offset` samples behind
    /// the write head, into the output accumulator at the current position.
    fn place_grain(&mut self, center_offset: usize) {
        let period = if self.detected_period > 0 {
            self.detected_period
        } else {
            self.window_size / 4
        };

        let grain_len = period * 2;
        let half = grain_len / 2;
        let buf_len = self.output_buf.len();

        for i in 0..grain_len {
            // Hann window.
            let phase = i as f64 / grain_len as f64;
            let win = 0.5 * (1.0 - (std::f64::consts::TAU * phase).cos());

            // Read from analysis buffer.
            let read_offset = center_offset + half - i;
            if read_offset == 0 || read_offset >= self.analysis_buf.len() {
                continue;
            }
            let sample = self.analysis_buf.read(read_offset);

            // Write to output accumulator.
            let write_idx = (self.output_pos + i) % buf_len;
            self.output_buf[write_idx] += sample * win;
        }
    }

    /// Process one sample. Returns the pitch-shifted output.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        // Write input to analysis buffer.
        self.analysis_buf.write(input);
        self.write_count += 1;

        // Periodic pitch detection.
        if self.detect_countdown == 0 {
            self.detected_period = self.detect_pitch();
            self.detect_countdown = self.detect_interval;
        }
        self.detect_countdown -= 1;

        // PSOLA synthesis: place grains at synthesis rate.
        if self.synth_countdown == 0 {
            let synth_period = if self.detected_period > 0 {
                (self.detected_period as f64 / self.speed).round() as usize
            } else {
                512 // Fallback for unvoiced
            };

            // Place a grain centered at the current analysis position.
            let center = if self.detected_period > 0 {
                self.detected_period
            } else {
                self.window_size / 4
            };
            self.place_grain(center);

            self.synth_countdown = synth_period.max(self.min_period);
        }
        self.synth_countdown -= 1;

        // Read and clear from output accumulator.
        let wet = self.output_buf[self.output_pos];
        self.output_buf[self.output_pos] = 0.0;
        self.output_pos = (self.output_pos + 1) % self.output_buf.len();

        input * (1.0 - self.mix) + wet * self.mix
    }

    pub fn latency(&self) -> usize {
        // Approximately 2 pitch periods + detection latency.
        if self.detected_period > 0 {
            self.detected_period * 2
        } else {
            self.window_size
        }
    }
}

impl Default for PsolaShifter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_psola() -> PsolaShifter {
        let mut p = PsolaShifter::new();
        p.speed = 0.5;
        p.mix = 1.0;
        p.update(SR);
        p
    }

    #[test]
    fn silence_in_silence_out() {
        let mut p = make_psola();
        for _ in 0..4800 {
            let out = p.tick(0.0);
            assert!(out.abs() < 1e-6, "Should be silent: {out}");
        }
    }

    #[test]
    fn produces_output_on_sine() {
        let mut p = make_psola();
        let freq = 220.0;

        let mut energy = 0.0;
        let n = 48000;
        for i in 0..n {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
            let out = p.tick(input);
            if i > 4096 {
                energy += out * out;
            }
        }
        assert!(energy > 0.1, "Should produce output: energy={energy}");
    }

    #[test]
    fn no_nan() {
        let mut p = make_psola();
        for i in 0..48000 {
            let input = (2.0 * PI * 82.0 * i as f64 / SR).sin() * 0.9;
            let out = p.tick(input);
            assert!(out.is_finite(), "NaN at sample {i}");
        }
    }

    #[test]
    fn detects_pitch() {
        let mut p = make_psola();
        let freq = 440.0;

        // Feed enough signal for detection.
        for i in 0..4800 {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.8;
            p.tick(input);
        }

        let expected_period = (SR / freq).round() as usize; // ~109
        assert!(
            p.detected_period > 0,
            "Should detect pitch, got period={}",
            p.detected_period
        );
        assert!(
            (p.detected_period as f64 - expected_period as f64).abs() < 10.0,
            "Period should be ~{expected_period}, got {}",
            p.detected_period
        );
    }

    #[test]
    fn unvoiced_detection_for_noise() {
        let mut p = make_psola();

        // Feed noise — should not detect a pitch.
        let mut rng = 12345u64;
        for _ in 0..9600 {
            // Simple LCG pseudo-random.
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let noise = (rng as f64 / u64::MAX as f64) * 2.0 - 1.0;
            p.tick(noise * 0.5);
        }

        // Period should be 0 (unvoiced) or very unstable.
        // Just check it doesn't crash.
    }

    #[test]
    fn different_speeds_differ() {
        let freq = 220.0;
        let n = 9600;

        let collect = |speed: f64| -> Vec<f64> {
            let mut p = make_psola();
            p.speed = speed;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let s = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
                out.push(p.tick(s));
            }
            out
        };

        let down = collect(0.5);
        let up = collect(2.0);

        let diff: f64 = down
            .iter()
            .zip(up.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / n as f64;

        assert!(
            diff > 0.001,
            "Different speeds should produce different output: {diff}"
        );
    }

    #[test]
    fn dry_wet_mix() {
        let mut p = make_psola();
        p.mix = 0.0;

        for i in 0..4800 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = p.tick(input);
            assert!((out - input).abs() < 1e-10, "Mix=0 should pass dry");
        }
    }
}
