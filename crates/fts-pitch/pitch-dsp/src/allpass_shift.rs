//! Allpass interpolation pitch shifter — Dattorro/Schroeder "barberpole" style.
//!
//! Two read heads sweep through a circular delay buffer at a rate determined
//! by the pitch ratio. When one head nears the end of its sweep, a Hann-shaped
//! crossfade transitions to the other head. Allpass interpolation provides
//! sub-sample accuracy without the phase smearing of linear interpolation.
//!
//! Latency: **0 samples** — output is produced immediately from the current
//! write position minus a small read offset.
//!
//! Character: Classic hardware pitch shifter (Eventide H3000, Boss PS-series).

use std::f64::consts::PI;

const BUFFER_SIZE: usize = 4096;
/// Barberpole allpass-interpolated pitch shifter with zero latency.
pub struct AllpassShifter {
    /// Pitch ratio: 0.5 = octave down, 2.0 = octave up.
    pub speed: f64,
    /// Mix: 0.0 = dry only, 1.0 = wet only.
    pub mix: f64,

    buffer: [f64; BUFFER_SIZE],
    write_pos: usize,

    /// Fractional read offset for head A (samples behind write head).
    head_a: f64,
    /// Fractional read offset for head B.
    head_b: f64,

    /// Allpass state for head A.
    ap_state_a: f64,
    /// Allpass state for head B.
    ap_state_b: f64,
    /// Previous raw sample for head A allpass.
    ap_prev_a: f64,
    /// Previous raw sample for head B allpass.
    ap_prev_b: f64,

    /// Phase of head A within its sweep (0.0–1.0). Drives the crossfade.
    phase_a: f64,

    sample_rate: f64,
}

impl AllpassShifter {
    pub fn new() -> Self {
        Self {
            speed: 0.5,
            mix: 1.0,
            buffer: [0.0; BUFFER_SIZE],
            write_pos: 0,
            head_a: 1.0,
            head_b: (BUFFER_SIZE / 2) as f64,
            ap_state_a: 0.0,
            ap_state_b: 0.0,
            ap_prev_a: 0.0,
            ap_prev_b: 0.0,
            phase_a: 0.0,
            sample_rate: 48000.0,
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.buffer = [0.0; BUFFER_SIZE];
        self.write_pos = 0;
        self.head_a = 1.0;
        self.head_b = (BUFFER_SIZE / 2) as f64;
        self.ap_state_a = 0.0;
        self.ap_state_b = 0.0;
        self.ap_prev_a = 0.0;
        self.ap_prev_b = 0.0;
        self.phase_a = 0.0;
    }

    /// Read from the circular buffer at an integer offset behind write_pos.
    #[inline]
    fn read_buffer(&self, offset: usize) -> f64 {
        let idx = (self.write_pos + BUFFER_SIZE - offset) % BUFFER_SIZE;
        self.buffer[idx]
    }

    /// Allpass interpolation: reads at a fractional offset behind the write head.
    /// Returns the interpolated sample and updates the allpass state.
    #[inline]
    fn read_allpass(&self, offset: f64, ap_state: &mut f64, ap_prev: &mut f64) -> f64 {
        let offset_clamped = offset.max(1.0).min((BUFFER_SIZE - 2) as f64);
        let int_part = offset_clamped as usize;
        let frac = offset_clamped - int_part as f64;

        let x_n = self.read_buffer(int_part);
        let x_prev = self.read_buffer(int_part + 1);

        // First-order allpass: y[n] = x_prev + (x_n - y_prev) * frac
        // where frac is the fractional delay (coefficient).
        let coeff = (1.0 - frac) / (1.0 + frac);
        let y = x_prev + (x_n - *ap_state) * coeff;

        *ap_prev = x_n;
        *ap_state = y;
        y
    }

    /// Hann window value at phase [0, 1].
    #[inline]
    fn hann(phase: f64) -> f64 {
        0.5 * (1.0 - (PI * phase).cos())
    }

    /// Process one sample. Returns the mixed (dry/wet) output.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        // Write input into the circular buffer.
        self.buffer[self.write_pos] = input;
        self.write_pos = (self.write_pos + 1) % BUFFER_SIZE;

        let drift = 1.0 - self.speed;
        let half_buf = (BUFFER_SIZE / 2) as f64;
        let max_offset = (BUFFER_SIZE - 2) as f64;

        // Advance read heads.
        self.head_a += drift;
        self.head_b += drift;

        // Wrap heads: when a head drifts out of range, reset it to
        // half-buffer offset from the other head.
        if self.head_a < 1.0 || self.head_a > max_offset {
            self.head_a = ((self.head_b + half_buf - 1.0) % max_offset) + 1.0;
            self.ap_state_a = 0.0;
            self.ap_prev_a = 0.0;
            self.phase_a = 0.0;
        }
        if self.head_b < 1.0 || self.head_b > max_offset {
            self.head_b = ((self.head_a + half_buf - 1.0) % max_offset) + 1.0;
            self.ap_state_b = 0.0;
            self.ap_prev_b = 0.0;
            self.phase_a = 0.5;
        }

        // Read from each head with allpass interpolation.
        let mut ap_a = self.ap_state_a;
        let mut ap_prev_a = self.ap_prev_a;
        let mut ap_b = self.ap_state_b;
        let mut ap_prev_b = self.ap_prev_b;

        let a = self.read_allpass(self.head_a, &mut ap_a, &mut ap_prev_a);
        let b = self.read_allpass(self.head_b, &mut ap_b, &mut ap_prev_b);

        self.ap_state_a = ap_a;
        self.ap_prev_a = ap_prev_a;
        self.ap_state_b = ap_b;
        self.ap_prev_b = ap_prev_b;

        // Crossfade envelope: head A uses phase_a, head B uses phase_a + 0.5.
        let win_a = Self::hann(self.phase_a);
        let phase_b = (self.phase_a + 0.5).fract();
        let win_b = Self::hann(phase_b);

        let wet = a * win_a + b * win_b;

        // Advance the crossfade phase.
        let phase_inc = 1.0 / (BUFFER_SIZE as f64 / drift.abs().max(0.001));
        self.phase_a = (self.phase_a + phase_inc) % 1.0;

        input * (1.0 - self.mix) + wet * self.mix
    }

    /// Latency in samples. Always zero for this algorithm.
    pub fn latency(&self) -> usize {
        0
    }
}

impl Default for AllpassShifter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn make_shifter() -> AllpassShifter {
        let mut s = AllpassShifter::new();
        s.speed = 0.5;
        s.mix = 1.0;
        s.update(SR);
        s
    }

    #[test]
    fn silence_in_silence_out() {
        let mut s = make_shifter();
        for _ in 0..4800 {
            let out = s.tick(0.0);
            assert!(out.abs() < 1e-10, "Should be silent: {out}");
        }
    }

    #[test]
    fn produces_output_on_sine() {
        let mut s = make_shifter();
        let mut energy = 0.0;
        for i in 0..9600 {
            let input = (2.0 * PI * 220.0 * i as f64 / SR).sin() * 0.5;
            let out = s.tick(input);
            // Allow warmup: measure energy after the buffer has filled.
            if i > 4096 {
                energy += out * out;
            }
        }
        assert!(energy > 0.1, "Should produce output: energy={energy}");
    }

    #[test]
    fn no_nan() {
        let mut s = make_shifter();
        for i in 0..48000 {
            let input = (2.0 * PI * 82.0 * i as f64 / SR).sin() * 0.9;
            let out = s.tick(input);
            assert!(out.is_finite(), "NaN/Inf at sample {i}");
        }
    }

    #[test]
    fn different_speeds_differ() {
        let freq = 440.0;
        let n = 9600;

        let collect = |speed: f64| -> Vec<f64> {
            let mut s = make_shifter();
            s.speed = speed;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let x = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
                out.push(s.tick(x));
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
    fn dry_wet_mix() {
        let mut s = make_shifter();
        s.mix = 0.0;

        for i in 0..4800 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = s.tick(input);
            assert!(
                (out - input).abs() < 1e-10,
                "Mix=0 should pass dry at sample {i}: in={input} out={out}"
            );
        }
    }

    #[test]
    fn latency_is_zero() {
        let s = make_shifter();
        assert_eq!(s.latency(), 0);
    }
}
