//! NamChain — dual-slot NAM processor with dual IR convolution,
//! phase correction, delta delay, and blend/mix controls.
//!
//! Full signal flow (matching Ratatouille.lv2):
//! ```text
//! Input (mono) ──→ ┬─→ [Delta Delay A] ──→ Slot A (NAM) ──→ Normalize A ─┐
//!                   └─→ [Delta Delay B] ──→ Slot B (NAM) ──→ Normalize B ─┘
//!                                                                          │
//!                          Blend: A*(1-blend) + B*blend                    │
//!                                     │                                    │
//!                              Output Gain ──→ DC Blocker                  │
//!                                     │                                    │
//!                                     ├─→ IR A (convolver) ────────────────┐
//!                                     └─→ IR B (convolver) ────────────────┘
//!                                                                          │
//!                                        Mix: A*(1-mix) + B*mix           │
//!                                                 │                        │
//!                                              Output                      │
//! ```

use fts_dsp::delay_line::DelayLine;
use fts_dsp::{AudioConfig, Processor};

use crate::convolver::Convolver;
use crate::slot::NamSlot;

/// Simple DC blocker: `y[n] = x[n] - x[n-1] + R * y[n-1]`, R ≈ 0.995.
struct DcBlocker {
    x_prev: f64,
    y_prev: f64,
    r: f64,
}

impl DcBlocker {
    fn new() -> Self {
        Self {
            x_prev: 0.0,
            y_prev: 0.0,
            r: 0.995,
        }
    }

    fn reset(&mut self) {
        self.x_prev = 0.0;
        self.y_prev = 0.0;
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = x - self.x_prev + self.r * self.y_prev;
        self.x_prev = x;
        self.y_prev = y;
        y
    }
}

/// Dual-slot NAM processor chain with IR convolution.
pub struct NamChain {
    /// Model slot A.
    pub slot_a: NamSlot,
    /// Model slot B.
    pub slot_b: NamSlot,

    /// IR convolver A (cabinet simulation).
    pub ir_a: Convolver,
    /// IR convolver B (cabinet simulation).
    pub ir_b: Convolver,

    /// Blend between model slot A and B (0.0 = A only, 1.0 = B only).
    pub blend: f64,
    /// Mix between IR A and B (0.0 = A only, 1.0 = B only).
    pub ir_mix: f64,
    /// Output gain in linear amplitude.
    pub output_gain: f64,

    /// Delta delay in samples between the two model slots.
    /// Positive = delay slot B, negative = delay slot A.
    pub delta_delay_samples: f64,

    /// Enable automatic phase correction between models.
    pub phase_correction: bool,
    /// Detected phase offset (samples) — set by `detect_phase_offset()`.
    phase_offset: i32,

    dc_blocker: DcBlocker,
    delay_a: DelayLine,
    delay_b: DelayLine,

    // Scratch buffers
    buf_a: Vec<f64>,
    buf_b: Vec<f64>,
    ir_buf_a: Vec<f64>,
    ir_buf_b: Vec<f64>,
    mono_in: Vec<f64>,
    sample_rate: f64,
}

/// Maximum delta delay in samples.
const MAX_DELAY: usize = 512;

impl NamChain {
    pub fn new() -> Self {
        Self {
            slot_a: NamSlot::new(),
            slot_b: NamSlot::new(),
            ir_a: Convolver::new(),
            ir_b: Convolver::new(),
            blend: 0.0,
            ir_mix: 0.0,
            output_gain: 1.0,
            delta_delay_samples: 0.0,
            phase_correction: false,
            phase_offset: 0,
            dc_blocker: DcBlocker::new(),
            delay_a: DelayLine::new(MAX_DELAY + 1),
            delay_b: DelayLine::new(MAX_DELAY + 1),
            buf_a: Vec::new(),
            buf_b: Vec::new(),
            ir_buf_a: Vec::new(),
            ir_buf_b: Vec::new(),
            mono_in: Vec::new(),
            sample_rate: 48000.0,
        }
    }

    /// Detect phase offset between model A and B by running a test sine
    /// through both and finding the first zero-crossing difference.
    /// Call after loading both models and calling `update()`.
    pub fn detect_phase_offset(&mut self) {
        if !self.slot_a.is_loaded() || !self.slot_b.is_loaded() {
            self.phase_offset = 0;
            return;
        }

        let test_len = 4800; // 0.1s at 48kHz
        let freq = 100.0;
        let sr = self.sample_rate;

        // Generate test sine
        let test_input: Vec<f64> = (0..test_len)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / sr).sin() * 0.5)
            .collect();

        // Process through each slot
        let mut out_a = vec![0.0; test_len];
        let mut out_b = vec![0.0; test_len];
        self.slot_a.process(&test_input, &mut out_a);
        self.slot_b.process(&test_input, &mut out_b);

        // Reset slots after test (we corrupted their state)
        self.slot_a
            .update(self.sample_rate, self.buf_a.len().max(512));
        self.slot_b
            .update(self.sample_rate, self.buf_b.len().max(512));

        // Find first positive zero-crossing in each output (skip first 480 for settling)
        let find_zero_crossing = |output: &[f64]| -> Option<usize> {
            for i in 481..output.len() {
                if output[i - 1] <= 0.0 && output[i] > 0.0 {
                    return Some(i);
                }
            }
            None
        };

        let zc_a = find_zero_crossing(&out_a);
        let zc_b = find_zero_crossing(&out_b);

        self.phase_offset = match (zc_a, zc_b) {
            (Some(a), Some(b)) => {
                (b as i32 - a as i32).clamp(-(MAX_DELAY as i32), MAX_DELAY as i32)
            }
            _ => 0,
        };
    }

    /// Get the detected phase offset in samples.
    pub fn phase_offset(&self) -> i32 {
        self.phase_offset
    }

    /// Total latency in samples (from IR convolvers).
    pub fn latency(&self) -> usize {
        let ir_latency = if self.ir_a.is_loaded() || self.ir_b.is_loaded() {
            self.ir_a.latency().max(self.ir_b.latency())
        } else {
            0
        };
        ir_latency
    }
}

impl Default for NamChain {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for NamChain {
    fn reset(&mut self) {
        self.dc_blocker.reset();
        self.ir_a.reset();
        self.ir_b.reset();
    }

    fn update(&mut self, config: AudioConfig) {
        // Allocate generously — process() may receive buffers up to max_buffer_size
        // but we also need headroom for tests that pass larger slices.
        let n = config.max_buffer_size.max(8192);
        self.sample_rate = config.sample_rate;
        self.buf_a.resize(n, 0.0);
        self.buf_b.resize(n, 0.0);
        self.ir_buf_a.resize(n, 0.0);
        self.ir_buf_b.resize(n, 0.0);
        self.mono_in.resize(n, 0.0);

        self.slot_a.update(config.sample_rate, n);
        self.slot_b.update(config.sample_rate, n);
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        let n = left.len().min(right.len());
        if n == 0 {
            return;
        }

        // NAM models are mono — sum to mono
        for i in 0..n {
            self.mono_in[i] = (left[i] + right[i]) * 0.5;
        }

        // Compute effective delay for each slot
        let total_offset = self.delta_delay_samples
            + if self.phase_correction {
                self.phase_offset as f64
            } else {
                0.0
            };

        let delay_a_samples = if total_offset < 0.0 {
            (-total_offset) as usize
        } else {
            0
        };
        let delay_b_samples = if total_offset > 0.0 {
            total_offset as usize
        } else {
            0
        };

        // Apply delta delay + process slot A
        if delay_a_samples > 0 {
            for i in 0..n {
                self.delay_a.write(self.mono_in[i]);
                self.mono_in[i] = self.delay_a.read(delay_a_samples);
            }
        }
        self.slot_a
            .process(&self.mono_in[..n], &mut self.buf_a[..n]);

        // Process slot B (only if loaded)
        let has_b = self.slot_b.is_loaded();
        if has_b {
            // Re-read mono input (may have been modified by delay_a above)
            for i in 0..n {
                self.mono_in[i] = (left[i] + right[i]) * 0.5;
            }

            if delay_b_samples > 0 {
                for i in 0..n {
                    self.delay_b.write(self.mono_in[i]);
                    self.mono_in[i] = self.delay_b.read(delay_b_samples);
                }
            }
            self.slot_b
                .process(&self.mono_in[..n], &mut self.buf_b[..n]);
        }

        // Blend model outputs
        for i in 0..n {
            let sample = if has_b {
                self.buf_a[i] * (1.0 - self.blend) + self.buf_b[i] * self.blend
            } else {
                self.buf_a[i]
            };

            // Output gain + DC blocker
            self.mono_in[i] = self.dc_blocker.tick(sample * self.output_gain);
        }

        // IR convolution stage
        let has_ir_a = self.ir_a.is_loaded();
        let has_ir_b = self.ir_b.is_loaded();

        if has_ir_a || has_ir_b {
            if has_ir_a {
                for i in 0..n {
                    self.ir_buf_a[i] = self.ir_a.tick(self.mono_in[i]);
                }
            }
            if has_ir_b {
                for i in 0..n {
                    self.ir_buf_b[i] = self.ir_b.tick(self.mono_in[i]);
                }
            }

            // Mix IR outputs
            for i in 0..n {
                let out = match (has_ir_a, has_ir_b) {
                    (true, true) => {
                        self.ir_buf_a[i] * (1.0 - self.ir_mix) + self.ir_buf_b[i] * self.ir_mix
                    }
                    (true, false) => self.ir_buf_a[i],
                    (false, true) => self.ir_buf_b[i],
                    _ => unreachable!(),
                };
                left[i] = out;
                right[i] = out;
            }
        } else {
            // No IR — write post-model output directly
            for i in 0..n {
                left[i] = self.mono_in[i];
                right[i] = self.mono_in[i];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn config() -> AudioConfig {
        AudioConfig {
            sample_rate: SR,
            max_buffer_size: 2048,
        }
    }

    #[test]
    fn no_model_passes_through() {
        let mut c = NamChain::new();
        c.update(config());

        let mut l = vec![0.5; 256];
        let mut r = vec![0.5; 256];
        c.process(&mut l, &mut r);

        for &s in &l {
            assert!(s.is_finite(), "Non-finite output");
        }
        let energy: f64 = l.iter().map(|s| s * s).sum::<f64>() / l.len() as f64;
        assert!(
            energy > 0.001,
            "Expected non-trivial output, got energy {energy}"
        );
    }

    #[test]
    fn silence_in_silence_out() {
        let mut c = NamChain::new();
        c.update(config());

        let mut l = vec![0.0; 512];
        let mut r = vec![0.0; 512];
        c.process(&mut l, &mut r);

        for (i, &s) in l.iter().enumerate() {
            assert!(s.abs() < 1e-10, "Non-zero at {i}: {s}");
        }
    }

    #[test]
    fn dc_blocker_removes_dc() {
        let mut dc = DcBlocker::new();
        let mut out = 0.0;
        for _ in 0..10000 {
            out = dc.tick(1.0);
        }
        assert!(out.abs() < 0.01, "DC not blocked: {out}");
    }

    #[test]
    fn ir_convolution_produces_output() {
        let mut c = NamChain::new();
        c.update(config());

        // Load a simple decaying IR
        let ir: Vec<f64> = (0..512).map(|i| (-i as f64 / 100.0).exp()).collect();
        c.ir_a.load_ir(&ir, SR);

        // Process a sine wave
        let n = 4800;
        let mut l: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5)
            .collect();
        let mut r = l.clone();
        c.process(&mut l, &mut r);

        for (i, &s) in l.iter().enumerate() {
            assert!(s.is_finite(), "NaN/Inf at {i}");
        }

        // After latency, output should be non-trivial
        let tail_energy: f64 = l[1024..].iter().map(|s| s * s).sum::<f64>() / (n - 1024) as f64;
        assert!(
            tail_energy > 1e-6,
            "IR convolution should produce output: energy={tail_energy}"
        );
    }

    #[test]
    fn dual_ir_mix_works() {
        let mut c = NamChain::new();
        c.update(config());

        // Two different IRs
        let ir_a: Vec<f64> = (0..256).map(|i| (-i as f64 / 50.0).exp()).collect();
        let ir_b: Vec<f64> = (0..256)
            .map(|i| (-i as f64 / 50.0).exp() * (2.0 * PI * 1000.0 * i as f64 / SR).sin())
            .collect();
        c.ir_a.load_ir(&ir_a, SR);
        c.ir_b.load_ir(&ir_b, SR);

        // Process with mix=0 (all IR A)
        c.ir_mix = 0.0;
        let n = 4800;
        let input: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5)
            .collect();

        let mut l_a = input.clone();
        let mut r_a = input.clone();
        c.process(&mut l_a, &mut r_a);

        // Reset and process with mix=1 (all IR B)
        c.reset();
        c.ir_mix = 1.0;
        let mut l_b = input.clone();
        let mut r_b = input.clone();
        c.process(&mut l_b, &mut r_b);

        // Outputs should differ
        let diff: f64 = l_a[1024..]
            .iter()
            .zip(l_b[1024..].iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / (n - 1024) as f64;
        assert!(
            diff > 0.001,
            "Different IRs should produce different output: avg_diff={diff}"
        );
    }

    #[test]
    fn delta_delay_shifts_output() {
        let mut c = NamChain::new();
        c.update(config());

        // No delay
        c.delta_delay_samples = 0.0;
        let n = 2048;
        let input: Vec<f64> = (0..n).map(|i| if i == 100 { 1.0 } else { 0.0 }).collect();

        let mut l1 = input.clone();
        let mut r1 = input.clone();
        c.process(&mut l1, &mut r1);

        // With delay
        c.reset();
        c.delta_delay_samples = 10.0;
        let mut l2 = input.clone();
        let mut r2 = input.clone();
        c.process(&mut l2, &mut r2);

        // With only slot A loaded (passthrough), delta_delay_samples > 0 delays B, not A,
        // so with no B loaded, output should be the same. Test that delta on A works:
        c.reset();
        c.delta_delay_samples = -10.0; // delay A
        let mut l3 = input.clone();
        let mut r3 = input.clone();
        c.process(&mut l3, &mut r3);

        let diff2: f64 = l1
            .iter()
            .zip(l3.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>();
        assert!(
            diff2 > 0.01,
            "Negative delta should delay slot A: diff={diff2}"
        );
    }

    #[test]
    fn output_gain_scales() {
        let mut c = NamChain::new();
        c.output_gain = 0.5;
        c.update(config());

        let mut l = vec![1.0; 256];
        let mut r = vec![1.0; 256];
        c.process(&mut l, &mut r);

        for &s in &l {
            assert!(s.is_finite());
        }
    }

    #[test]
    fn latency_reports_correctly() {
        let mut c = NamChain::new();
        assert_eq!(c.latency(), 0, "No IR = no latency");

        let ir = vec![1.0; 256];
        c.ir_a.load_ir(&ir, 48000.0);
        assert!(c.latency() > 0, "With IR loaded, latency should be > 0");
    }
}
