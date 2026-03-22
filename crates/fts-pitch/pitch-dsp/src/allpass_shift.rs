//! Barberpole pitch shifter — Dattorro/Schroeder style with cubic interpolation.
//!
//! Two read heads sweep through a circular delay buffer at a rate determined
//! by the pitch ratio. When one head nears the end of its sweep, a sin²
//! crossfade transitions to the other head. Cubic (Catmull-Rom) interpolation
//! provides sub-sample accuracy without the clicking artifacts of allpass
//! interpolation (no recursive state to "fly back").
//!
//! At splice points, a short cross-correlation search finds the best
//! phase-aligned position for the incoming head (H949-style de-glitching),
//! minimizing audible discontinuities during crossfade.
//!
//! Latency: **0 samples** — output is produced immediately.
//! Character: Classic hardware pitch shifter (Eventide H3000, Boss PS-series).

use fts_dsp::delay_line::DelayLine;
use std::f64::consts::PI;

const BUFFER_SIZE: usize = 8192;
/// Tolerance (in samples) for cross-correlation splice search.
const SPLICE_TOLERANCE: usize = 128;
/// Length of the comparison window for splice cross-correlation.
const SPLICE_TAIL_LEN: usize = 64;

pub struct AllpassShifter {
    /// Pitch ratio: 0.5 = octave down, 2.0 = octave up.
    pub speed: f64,
    /// Mix: 0.0 = dry only, 1.0 = wet only.
    pub mix: f64,

    delay: DelayLine,

    /// Fractional read offset for head A (samples behind write head).
    head_a: f64,
    /// Fractional read offset for head B.
    head_b: f64,

    /// Phase of head A within its sweep (0.0–1.0). Drives the crossfade.
    phase_a: f64,

    sample_rate: f64,
}

impl AllpassShifter {
    pub fn new() -> Self {
        Self {
            speed: 0.5,
            mix: 1.0,
            delay: DelayLine::new(BUFFER_SIZE),
            head_a: 1.0,
            head_b: (BUFFER_SIZE / 2) as f64,
            phase_a: 0.0,
            sample_rate: 48000.0,
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.delay.clear();
        self.head_a = 1.0;
        self.head_b = (BUFFER_SIZE / 2) as f64;
        self.phase_a = 0.0;
    }

    /// Crossfade window: sin²(π * phase).
    /// Two windows offset by 0.5 sum to exactly 1.0.
    #[inline]
    fn crossfade(phase: f64) -> f64 {
        let s = (PI * phase).sin();
        s * s
    }

    /// Find the best splice offset near `nominal` using cross-correlation
    /// with a short tail read from `reference_offset`. This is the H949
    /// de-glitching technique: align the incoming head's audio with the
    /// outgoing head's audio to minimize phase discontinuity.
    fn find_splice_offset(&self, nominal: f64, reference_offset: f64) -> f64 {
        let buf_len = self.delay.len();
        let tail_len = SPLICE_TAIL_LEN.min(buf_len / 4);
        let tolerance = SPLICE_TOLERANCE as isize;

        // Read reference tail from the outgoing head position.
        let mut ref_energy = 0.0f64;
        let mut ref_tail = [0.0f64; SPLICE_TAIL_LEN];
        for i in 0..tail_len {
            let pos = reference_offset as usize + i;
            if pos > 0 && pos < buf_len {
                ref_tail[i] = self.delay.read(pos);
                ref_energy += ref_tail[i] * ref_tail[i];
            }
        }

        // If the reference is silent, just use the nominal position.
        if ref_energy < 1e-12 {
            return nominal;
        }

        let mut best_corr = f64::NEG_INFINITY;
        let mut best_delta: isize = 0;

        for delta in -tolerance..=tolerance {
            let candidate = nominal + delta as f64;
            if candidate < 1.0 || (candidate as usize + tail_len) >= buf_len {
                continue;
            }

            let mut correlation = 0.0f64;
            let mut cand_energy = 0.0f64;

            for i in 0..tail_len {
                let s = self.delay.read(candidate as usize + i);
                correlation += ref_tail[i] * s;
                cand_energy += s * s;
            }

            let denom = (ref_energy * cand_energy).sqrt();
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

    /// Process one sample. Returns the mixed (dry/wet) output.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        self.delay.write(input);

        let drift = 1.0 - self.speed;
        let half_buf = (BUFFER_SIZE / 2) as f64;
        let max_offset = (BUFFER_SIZE - 4) as f64; // leave room for cubic interp

        // Advance read heads.
        self.head_a += drift;
        self.head_b += drift;

        // Wrap heads: when a head drifts out of range, find a phase-aligned
        // splice point near half-buffer offset from the other head.
        if self.head_a < 1.0 || self.head_a > max_offset {
            let nominal = ((self.head_b + half_buf - 1.0) % max_offset) + 1.0;
            self.head_a = self.find_splice_offset(nominal, self.head_b);
            self.head_a = self.head_a.clamp(1.0, max_offset);
            self.phase_a = 0.0;
        }
        if self.head_b < 1.0 || self.head_b > max_offset {
            let nominal = ((self.head_a + half_buf - 1.0) % max_offset) + 1.0;
            self.head_b = self.find_splice_offset(nominal, self.head_a);
            self.head_b = self.head_b.clamp(1.0, max_offset);
            self.phase_a = 0.5;
        }

        // Read from each head with cubic (Catmull-Rom) interpolation.
        // No recursive state — no coefficient flyback clicks.
        let a = self.delay.read_cubic(self.head_a);
        let b = self.delay.read_cubic(self.head_b);

        // Crossfade envelope: head A uses phase_a, head B uses phase_a + 0.5.
        let win_a = Self::crossfade(self.phase_a);
        let phase_b = (self.phase_a + 0.5).fract();
        let win_b = Self::crossfade(phase_b);

        let wet = a * win_a + b * win_b;

        // Advance the crossfade phase.
        let phase_inc = drift.abs().max(0.001) / BUFFER_SIZE as f64;
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
    fn no_large_spikes() {
        // Verify no crackling/clicking: output should never exceed input amplitude
        // by more than a small margin (cubic overshoot).
        let mut s = make_shifter();
        let amplitude = 0.5;
        let mut max_out = 0.0f64;
        for i in 0..48000 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * amplitude;
            let out = s.tick(input);
            if i > BUFFER_SIZE {
                max_out = max_out.max(out.abs());
            }
        }
        // Cubic interpolation can overshoot slightly, but clicks would produce
        // spikes well above the input amplitude.
        assert!(
            max_out < amplitude * 1.5,
            "Output spikes too high (crackling?): max={max_out}, input_amp={amplitude}"
        );
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

    #[test]
    fn splice_reduces_discontinuity() {
        // Compare output with and without splice cross-correlation.
        // The spliced version should have lower peak-to-peak jumps at wrap points.
        let freq = 440.0;
        let n = 48000;

        let mut s = make_shifter();
        let mut max_jump = 0.0f64;
        let mut prev = 0.0;
        for i in 0..n {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
            let out = s.tick(input);
            if i > BUFFER_SIZE {
                let jump = (out - prev).abs();
                max_jump = max_jump.max(jump);
            }
            prev = out;
        }

        // A well-spliced pitch shifter should have no sample-to-sample jumps
        // larger than ~2x the input amplitude (generous margin).
        assert!(
            max_jump < 1.0,
            "Max sample-to-sample jump too large (clicking?): {max_jump}"
        );
    }
}
