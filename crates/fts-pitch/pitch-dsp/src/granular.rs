//! Granular pitch shifter — fixed-ratio time-domain grain overlap.
//!
//! Two read heads sweep through a circular buffer at `speed` rate relative
//! to the write head. Grains are crossfaded with a Hann window to prevent
//! clicks. No pitch detection required — works on any signal.
//!
//! Latency: `grain_size` samples (default 1024).
//! Character: Natural, slight chorus-like artifacts at extreme shifts.

use fts_dsp::delay_line::DelayLine;

/// Granular pitch shifter with dual crossfading grains.
pub struct GranularShifter {
    /// Pitch ratio: 0.5 = octave down, 2.0 = octave up.
    pub speed: f64,
    /// Mix: 0.0 = dry only, 1.0 = wet only.
    pub mix: f64,
    /// Grain size in samples.
    pub grain_size: usize,

    delay: DelayLine,
    /// Read offset for grain A (samples behind write head).
    offset_a: f64,
    /// Read offset for grain B (offset by half a grain).
    offset_b: f64,
    /// Phase within the current grain (0.0–1.0).
    grain_phase: f64,
    /// Phase increment per sample.
    phase_inc: f64,

    sample_rate: f64,
}

impl GranularShifter {
    /// Maximum buffer length in seconds.
    const MAX_BUF_S: f64 = 1.0;

    pub fn new() -> Self {
        let buf_len = 48000 + 4096;
        Self {
            speed: 0.5,
            mix: 1.0,
            grain_size: 1024,
            delay: DelayLine::new(buf_len),
            offset_a: 1024.0,
            offset_b: 1024.0 + 512.0,
            grain_phase: 0.0,
            phase_inc: 1.0 / 1024.0,
            sample_rate: 48000.0,
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        let max_len = (sample_rate * Self::MAX_BUF_S) as usize + 4096;
        if self.delay.len() < max_len {
            self.delay = DelayLine::new(max_len);
        }
        self.phase_inc = 1.0 / self.grain_size as f64;
        self.offset_a = self.grain_size as f64;
        self.offset_b = self.grain_size as f64 + self.grain_size as f64 * 0.5;
    }

    pub fn reset(&mut self) {
        self.delay.clear();
        self.offset_a = self.grain_size as f64;
        self.offset_b = self.grain_size as f64 + self.grain_size as f64 * 0.5;
        self.grain_phase = 0.0;
    }

    /// Hann window value at phase [0, 1].
    #[inline]
    fn hann(phase: f64) -> f64 {
        0.5 * (1.0 - (std::f64::consts::TAU * phase).cos())
    }

    /// Process one sample. Returns the pitch-shifted output.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        self.delay.write(input);

        let max_offset = self.delay.len() as f64 - 4.0;
        let drift = 1.0 - self.speed;

        // Advance read heads.
        self.offset_a += drift;
        self.offset_b += drift;

        // Wrap offsets back to target region when they drift too far.
        let target = self.grain_size as f64;
        if self.offset_a < 1.0 || self.offset_a > max_offset {
            self.offset_a = target;
        }
        if self.offset_b < 1.0 || self.offset_b > max_offset {
            self.offset_b = target + self.grain_size as f64 * 0.5;
        }

        // Read grains with cubic interpolation.
        let a = self.delay.read_cubic(self.offset_a.clamp(1.0, max_offset));
        let b = self.delay.read_cubic(self.offset_b.clamp(1.0, max_offset));

        // Windowed crossfade: grain A uses phase, grain B uses phase + 0.5.
        let win_a = Self::hann(self.grain_phase);
        let phase_b = (self.grain_phase + 0.5).fract();
        let win_b = Self::hann(phase_b);

        let wet = a * win_a + b * win_b;

        // Advance grain phase.
        self.grain_phase += self.phase_inc;
        if self.grain_phase >= 1.0 {
            self.grain_phase -= 1.0;
            // Reset grain A to target offset at grain boundary.
            self.offset_a = target;
        }
        // Reset grain B at half-grain boundary.
        if self.grain_phase >= 0.5 && (self.grain_phase - self.phase_inc) < 0.5 {
            self.offset_b = target + self.grain_size as f64 * 0.5;
        }

        input * (1.0 - self.mix) + wet * self.mix
    }

    pub fn latency(&self) -> usize {
        self.grain_size
    }
}

impl Default for GranularShifter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_granular() -> GranularShifter {
        let mut g = GranularShifter::new();
        g.speed = 0.5;
        g.mix = 1.0;
        g.grain_size = 1024;
        g.update(SR);
        g
    }

    #[test]
    fn latency_equals_grain_size() {
        let g = make_granular();
        assert_eq!(g.latency(), 1024);
    }

    #[test]
    fn silence_in_silence_out() {
        let mut g = make_granular();
        for _ in 0..4800 {
            let out = g.tick(0.0);
            assert!(out.abs() < 1e-10, "Should be silent: {out}");
        }
    }

    #[test]
    fn produces_output() {
        let mut g = make_granular();

        // Feed sine, check that output has energy after latency.
        let mut energy = 0.0;
        for i in 0..9600 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = g.tick(input);
            if i > 2048 {
                energy += out * out;
            }
        }
        assert!(energy > 1.0, "Should produce output: energy={energy}");
    }

    #[test]
    fn no_nan() {
        let mut g = make_granular();
        for i in 0..48000 {
            let input = (2.0 * PI * 82.0 * i as f64 / SR).sin() * 0.9;
            let out = g.tick(input);
            assert!(out.is_finite(), "NaN at sample {i}");
        }
    }

    #[test]
    fn different_speeds_differ() {
        let freq = 440.0;
        let n = 9600;

        let collect = |speed: f64| -> Vec<f64> {
            let mut g = make_granular();
            g.speed = speed;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let s = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
                out.push(g.tick(s));
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
            diff > 0.01,
            "Different speeds should produce different output: {diff}"
        );
    }

    #[test]
    fn unity_speed_approximates_passthrough() {
        let mut g = make_granular();
        g.speed = 1.0;
        g.update(SR);

        let freq = 440.0;
        let n = 9600;

        let mut max_out = 0.0f64;
        for i in 0..n {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
            let out = g.tick(input);
            if i > 2048 {
                max_out = max_out.max(out.abs());
            }
        }

        // At speed=1.0, output should still have signal.
        assert!(
            max_out > 0.1,
            "Unity speed should pass signal: max={max_out}"
        );
    }

    #[test]
    fn dry_wet_mix() {
        let mut g = make_granular();
        g.mix = 0.0;

        for i in 0..4800 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = g.tick(input);
            assert!((out - input).abs() < 1e-10, "Mix=0 should pass dry");
        }
    }
}
