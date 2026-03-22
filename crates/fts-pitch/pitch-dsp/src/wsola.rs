//! WSOLA — Waveform Similarity Overlap-Add pitch shifter.
//!
//! Dual read heads sweep through a circular delay buffer like the granular
//! shifter, but each grain boundary is aligned using cross-correlation to
//! minimize phase discontinuities. This produces smoother transitions than
//! naive granular shifting.
//!
//! Latency: `grain_size` samples (default 1024).
//! Character: Smooth, low-artefact, works well on complex material.

use fts_dsp::delay_line::DelayLine;

pub struct WsolaShifter {
    pub speed: f64,
    pub mix: f64,
    pub base_grain_size: usize,

    analysis_buf: DelayLine,

    grain_size: usize,
    tolerance: usize,

    // Dual read heads (like granular, but with cross-correlation alignment).
    offset_a: f64,
    offset_b: f64,
    grain_phase: f64,
    phase_inc: f64,

    // Previous grain tail for cross-correlation matching.
    prev_tail_a: Vec<f64>,
    prev_tail_b: Vec<f64>,

    write_count: usize,
    sample_rate: f64,
}

impl WsolaShifter {
    const DEFAULT_GRAIN: usize = 1024;
    const DEFAULT_TOLERANCE: usize = 128;
    const BUF_SECONDS: f64 = 2.0;

    pub fn new() -> Self {
        let grain_size = Self::DEFAULT_GRAIN;
        let tolerance = Self::DEFAULT_TOLERANCE;
        let buf_len = 48000 * 2 + grain_size + tolerance * 2;

        Self {
            speed: 0.5,
            mix: 1.0,
            base_grain_size: Self::DEFAULT_GRAIN,
            analysis_buf: DelayLine::new(buf_len),
            grain_size,
            tolerance,
            offset_a: grain_size as f64,
            offset_b: grain_size as f64 + grain_size as f64 * 0.5,
            grain_phase: 0.0,
            phase_inc: 1.0 / grain_size as f64,
            prev_tail_a: vec![0.0; grain_size / 4],
            prev_tail_b: vec![0.0; grain_size / 4],
            write_count: 0,
            sample_rate: 48000.0,
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.grain_size = ((self.base_grain_size as f64 * sample_rate) / 48000.0) as usize;
        if self.grain_size % 2 != 0 {
            self.grain_size += 1;
        }
        self.tolerance =
            ((Self::DEFAULT_TOLERANCE as f64 * sample_rate) / 48000.0).round() as usize;

        let buf_len =
            (sample_rate * Self::BUF_SECONDS) as usize + self.grain_size + self.tolerance * 2 + 64;
        if self.analysis_buf.len() < buf_len {
            self.analysis_buf = DelayLine::new(buf_len);
        }

        self.phase_inc = 1.0 / self.grain_size as f64;
        self.offset_a = self.grain_size as f64;
        self.offset_b = self.grain_size as f64 + self.grain_size as f64 * 0.5;

        let tail_len = self.grain_size / 4;
        self.prev_tail_a.resize(tail_len, 0.0);
        self.prev_tail_b.resize(tail_len, 0.0);
    }

    pub fn reset(&mut self) {
        self.analysis_buf.clear();
        self.offset_a = self.grain_size as f64;
        self.offset_b = self.grain_size as f64 + self.grain_size as f64 * 0.5;
        self.grain_phase = 0.0;
        self.prev_tail_a.fill(0.0);
        self.prev_tail_b.fill(0.0);
        self.write_count = 0;
    }

    /// sin^2(pi*phase) crossfade — sums to 1.0 with 0.5 offset.
    #[inline]
    fn crossfade(phase: f64) -> f64 {
        let s = (std::f64::consts::PI * phase).sin();
        s * s
    }

    /// Find best read offset near `nominal` using cross-correlation with `tail`.
    fn best_offset(&self, nominal: f64, tail: &[f64]) -> f64 {
        let tol = self.tolerance as isize;
        let tail_len = tail.len();
        let buf_len = self.analysis_buf.len();

        let mut energy_tail = 0.0f64;
        for s in tail {
            energy_tail += s * s;
        }
        if energy_tail < 1e-12 {
            return nominal;
        }

        let mut best_corr = f64::NEG_INFINITY;
        let mut best_delta: isize = 0;

        for delta in -tol..=tol {
            let candidate = nominal + delta as f64;
            if candidate < 1.0 || (candidate as usize + tail_len) >= buf_len {
                continue;
            }

            let mut correlation = 0.0f64;
            let mut energy_cand = 0.0f64;

            for i in 0..tail_len {
                let s = self.analysis_buf.read(candidate as usize + i);
                correlation += tail[i] * s;
                energy_cand += s * s;
            }

            let denom = (energy_tail * energy_cand).sqrt();
            let norm_corr = if denom > 1e-12 {
                correlation / denom
            } else {
                0.0
            };

            if norm_corr > best_corr {
                best_corr = norm_corr;
                best_delta = delta;
            }
        }

        nominal + best_delta as f64
    }

    /// Save a short tail of audio at `offset` for future cross-correlation.
    fn save_tail(&self, offset: f64, tail: &mut [f64]) {
        let buf_len = self.analysis_buf.len();
        for (i, t) in tail.iter_mut().enumerate() {
            let pos = offset as usize + i;
            if pos > 0 && pos < buf_len {
                *t = self.analysis_buf.read(pos);
            } else {
                *t = 0.0;
            }
        }
    }

    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        self.analysis_buf.write(input);
        self.write_count += 1;

        let max_offset = self.analysis_buf.len() as f64 - 4.0;
        let drift = 1.0 - self.speed;

        // Advance read heads by drift.
        self.offset_a += drift;
        self.offset_b += drift;

        // Advance phase.
        let prev_phase = self.grain_phase;
        self.grain_phase += self.phase_inc;

        // At grain boundary for A: find best alignment via cross-correlation.
        if self.grain_phase >= 1.0 {
            self.grain_phase -= 1.0;
            let target = self.grain_size as f64;
            let best = self.best_offset(target, &self.prev_tail_a);
            self.offset_a = best.clamp(1.0, max_offset);
            // Save a new tail from the aligned position.
            let mut tail = vec![0.0; self.prev_tail_a.len()];
            self.save_tail(self.offset_a, &mut tail);
            self.prev_tail_a = tail;
        }

        // At grain boundary for B.
        if prev_phase < 0.5 && self.grain_phase >= 0.5 {
            let target = self.grain_size as f64;
            let best = self.best_offset(target, &self.prev_tail_b);
            self.offset_b = best.clamp(1.0, max_offset);
            let mut tail = vec![0.0; self.prev_tail_b.len()];
            self.save_tail(self.offset_b, &mut tail);
            self.prev_tail_b = tail;
        }

        // Clamp offsets.
        self.offset_a = self.offset_a.clamp(1.0, max_offset);
        self.offset_b = self.offset_b.clamp(1.0, max_offset);

        // Read grains with cubic interpolation.
        let a = self.analysis_buf.read_cubic(self.offset_a);
        let b = self.analysis_buf.read_cubic(self.offset_b);

        // Crossfade.
        let win_a = Self::crossfade(self.grain_phase);
        let win_b = Self::crossfade((self.grain_phase + 0.5).fract());

        let wet = a * win_a + b * win_b;

        input * (1.0 - self.mix) + wet * self.mix
    }

    pub fn latency(&self) -> usize {
        self.grain_size
    }
}

impl Default for WsolaShifter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_wsola() -> WsolaShifter {
        let mut w = WsolaShifter::new();
        w.speed = 0.5;
        w.mix = 1.0;
        w.update(SR);
        w
    }

    #[test]
    fn silence_in_silence_out() {
        let mut w = make_wsola();
        for _ in 0..4800 {
            let out = w.tick(0.0);
            assert!(out.abs() < 1e-6, "Should be silent: {out}");
        }
    }

    #[test]
    fn produces_output_on_sine() {
        let mut w = make_wsola();
        let freq = 220.0;
        let mut energy = 0.0;
        let n = 48000;
        for i in 0..n {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
            let out = w.tick(input);
            if i > 4096 {
                energy += out * out;
            }
        }
        assert!(energy > 0.1, "Should produce output: energy={energy}");
    }

    #[test]
    fn no_nan() {
        let mut w = make_wsola();
        for i in 0..48000 {
            let input = (2.0 * PI * 82.0 * i as f64 / SR).sin() * 0.9;
            let out = w.tick(input);
            assert!(out.is_finite(), "NaN/Inf at sample {i}");
        }
    }

    #[test]
    fn different_speeds_differ() {
        let freq = 220.0;
        let n = 9600;
        let collect = |speed: f64| -> Vec<f64> {
            let mut w = make_wsola();
            w.speed = speed;
            (0..n)
                .map(|i| {
                    let s = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
                    w.tick(s)
                })
                .collect()
        };
        let down = collect(0.5);
        let up = collect(2.0);
        let diff: f64 = down
            .iter()
            .zip(up.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / n as f64;
        assert!(diff > 0.001, "Different speeds should differ: {diff}");
    }

    #[test]
    fn dry_wet_mix() {
        let mut w = make_wsola();
        w.mix = 0.0;
        for i in 0..4800 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = w.tick(input);
            assert!((out - input).abs() < 1e-10, "Mix=0 should pass dry");
        }
    }

    /// Measure pitch using YIN CMND (finds first local minimum below threshold).
    fn measure_pitch(signal: &[f64], sample_rate: f64) -> f64 {
        let min_period = (sample_rate / 2000.0) as usize;
        let max_period = (sample_rate / 30.0) as usize;
        let window = signal.len().min(max_period * 10);
        let max_lag = max_period.min(window / 2);
        if max_lag <= min_period {
            return 0.0;
        }
        let threshold = 0.2;
        let mut cmnd = vec![1.0f64; max_lag + 1];
        let mut running_sum = 0.0;
        for lag in 1..=max_lag {
            let mut diff = 0.0;
            let n = window - lag;
            for i in 0..n {
                let d = signal[i] - signal[i + lag];
                diff += d * d;
            }
            running_sum += diff;
            cmnd[lag] = if running_sum > 1e-12 {
                diff * lag as f64 / running_sum
            } else {
                0.0
            };
        }
        for lag in min_period..max_lag {
            if cmnd[lag] < threshold && cmnd[lag + 1] > cmnd[lag] {
                return sample_rate / lag as f64;
            }
        }
        let mut best_lag = min_period;
        let mut best_val = f64::MAX;
        for lag in min_period..=max_lag {
            if cmnd[lag] < best_val {
                best_val = cmnd[lag];
                best_lag = lag;
            }
        }
        if best_val < 0.5 {
            sample_rate / best_lag as f64
        } else {
            0.0
        }
    }

    #[test]
    fn octave_down_pitch_accuracy() {
        let mut w = make_wsola();
        w.speed = 0.5;
        w.update(SR);
        let freq = 440.0;
        let n = 96000;
        let mut output = Vec::with_capacity(n);
        for i in 0..n {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
            output.push(w.tick(input));
        }

        // Measure pitch using autocorrelation on the second half (after warmup).
        let start = n / 2;
        let measured_freq = measure_pitch(&output[start..], SR);
        let expected = freq * 0.5;
        let error_cents = 1200.0 * (measured_freq / expected).log2().abs();
        assert!(
            error_cents < 100.0,
            "Octave down should be ~{expected}Hz, got {measured_freq:.1}Hz ({error_cents:.0}c error)"
        );
    }
}
