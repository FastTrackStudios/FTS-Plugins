//! TapeDelay — tape echo with wow/flutter, feedback filtering, and saturation.
//!
//! Based on qdelay (tiagolr). Signal flow per channel:
//! Input → DelayLine (cubic read) → Feedback EQ → Saturation → Hard Limit → Write back
//!
//! Supports up to 3 read heads (RE-201 Space Echo style). All heads read from
//! the same delay buffer with shared wow/flutter modulation.

use fts_dsp::biquad::{Biquad, FilterType};
use fts_dsp::delay_line::DelayLine;
use fts_dsp::smoothing::ParamSmoother;
use fts_dsp::soft_clip::sin_clip;

use crate::modulation::{Flutter, Wow};

// RE-201 Space Echo head spacing ratios (relative to Head 1).
// From Cherry Audio Stardust 201 documentation.
pub const HEAD2_RATIO: f64 = 1.94;
pub const HEAD3_RATIO: f64 = 2.85;

// r[impl delay.tape.core]
/// Single-channel tape delay with modulation, feedback filtering, and saturation.
///
/// Supports up to 3 read heads (like the Roland RE-201 Space Echo).
/// Head 1 is at `time_ms`, Head 2 at 1.94×, Head 3 at 2.85×.
/// All heads share wow/flutter modulation (same tape transport).
/// Feedback is derived from the combined output of all active heads.
pub struct TapeDelay {
    // Parameters
    /// Delay time in milliseconds (base time for Head 1).
    pub time_ms: f64,
    /// Feedback amount (0.0 = no repeats, 1.0 = infinite).
    pub feedback: f64,
    /// Saturation drive (0.0 = clean, 1.0 = heavy).
    pub drive: f64,
    /// High-cut filter frequency in Hz (0 = disabled).
    pub hicut_freq: f64,
    /// Low-cut filter frequency in Hz (0 = disabled).
    pub locut_freq: f64,
    /// Filter Q.
    pub filter_q: f64,
    /// Wow depth (0.0–1.0).
    pub wow_depth: f64,
    /// Wow rate in Hz.
    pub wow_rate: f64,
    /// Wow drift amount (0.0–1.0).
    pub wow_drift: f64,
    /// Flutter depth (0.0–1.0).
    pub flutter_depth: f64,
    /// Flutter rate in Hz.
    pub flutter_rate: f64,

    // Multi-head (RE-201 style)
    /// Enable Head 1 (reads at base time_ms).
    pub head1_enabled: bool,
    /// Enable Head 2 (reads at HEAD2_RATIO × time_ms).
    pub head2_enabled: bool,
    /// Enable Head 3 (reads at HEAD3_RATIO × time_ms).
    pub head3_enabled: bool,
    /// Head 1 output level (0.0–1.0).
    pub head1_level: f64,
    /// Head 2 output level (0.0–1.0).
    pub head2_level: f64,
    /// Head 3 output level (0.0–1.0).
    pub head3_level: f64,

    // Internal state
    delay: DelayLine,
    wow: Wow,
    flutter: Flutter,
    hicut: Biquad,
    locut: Biquad,
    feedback_sample: f64,
    sample_rate: f64,
    smoother: ParamSmoother,
}

impl TapeDelay {
    /// Maximum delay time in seconds (must accommodate Head 3 at 2.85× base).
    const MAX_DELAY_S: f64 = 5.0;

    pub fn new() -> Self {
        Self {
            time_ms: 250.0,
            feedback: 0.4,
            drive: 0.0,
            hicut_freq: 8000.0,
            locut_freq: 0.0,
            filter_q: 0.707,
            wow_depth: 0.0,
            wow_rate: 0.5,
            wow_drift: 0.3,
            flutter_depth: 0.0,
            flutter_rate: 6.0,
            head1_enabled: true,
            head2_enabled: false,
            head3_enabled: false,
            head1_level: 1.0,
            head2_level: 1.0,
            head3_level: 1.0,
            delay: DelayLine::new(48000 * 5 + 1024),
            wow: Wow::new(),
            flutter: Flutter::new(),
            hicut: Biquad::new(),
            locut: Biquad::new(),
            feedback_sample: 0.0,
            sample_rate: 48000.0,
            smoother: ParamSmoother::new(0.0),
        }
    }

    // r[impl delay.tape.update]
    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        let max_len = (sample_rate * Self::MAX_DELAY_S) as usize + 1024;
        if self.delay.len() < max_len {
            self.delay = DelayLine::new(max_len);
        }

        self.wow.set_sample_rate(sample_rate);
        self.flutter.set_sample_rate(sample_rate);

        if self.hicut_freq > 0.0 {
            self.hicut.set(
                FilterType::Lowpass,
                self.hicut_freq,
                self.filter_q,
                sample_rate,
            );
        }
        if self.locut_freq > 0.0 {
            self.locut.set(
                FilterType::Highpass,
                self.locut_freq,
                self.filter_q,
                sample_rate,
            );
        }

        // Smooth delay time changes (~150ms time constant, from qdelay)
        self.smoother.set_time(0.15, sample_rate);

        let target = self.time_ms * 0.001 * sample_rate;
        if self.smoother.value() == 0.0 {
            self.smoother.set_immediate(target);
        }
    }

    // r[impl delay.tape.tick]
    // r[impl delay.tape.multihead]
    /// Process one sample. Returns the combined output of all active heads.
    ///
    /// Each head reads at its ratio × base time. All heads share the same
    /// wow/flutter modulation (physically correct — same tape transport).
    /// Feedback is derived from the combined output.
    pub fn tick(&mut self, input: f64, ch: usize) -> f64 {
        // Update modulation parameters
        self.wow.depth = self.wow_depth;
        self.wow.rate = self.wow_rate;
        self.wow.drift = self.wow_drift;
        self.flutter.depth = self.flutter_depth;
        self.flutter.rate = self.flutter_rate;

        // Smooth delay time (base time for all heads)
        let target_delay = self.time_ms * 0.001 * self.sample_rate;
        self.smoother.set_target(target_delay);
        let smooth_delay = self.smoother.tick();

        // Wow/flutter offset — shared across all heads (same tape transport)
        let wow_offset = self.wow.tick();
        let flutter_offset = self.flutter.tick();
        let mod_offset = wow_offset + flutter_offset;
        let max_read = self.delay.len() as f64 - 4.0;

        let mut output = 0.0;

        // Read Head 1 (at base time)
        if self.head1_enabled {
            let head1_delay = (smooth_delay + mod_offset).clamp(1.0, max_read);
            output += self.delay.read_cubic(head1_delay) * self.head1_level;
        }

        // Read Head 2 (at HEAD2_RATIO × base time)
        if self.head2_enabled {
            let head2_delay = (smooth_delay * HEAD2_RATIO + mod_offset).clamp(1.0, max_read);
            output += self.delay.read_cubic(head2_delay) * self.head2_level;
        }

        // Read Head 3 (at HEAD3_RATIO × base time)
        if self.head3_enabled {
            let head3_delay = (smooth_delay * HEAD3_RATIO + mod_offset).clamp(1.0, max_read);
            output += self.delay.read_cubic(head3_delay) * self.head3_level;
        }

        // Feedback path: combined output → filter → saturate → limit
        let mut fb = output * self.feedback;

        if self.hicut_freq > 0.0 {
            fb = self.hicut.tick(fb, ch);
        }
        if self.locut_freq > 0.0 {
            fb = self.locut.tick(fb, ch);
        }

        // Saturation in feedback path (cubic soft clip scaled by drive)
        if self.drive > 0.0 {
            let driven = fb * (1.0 + self.drive * 3.0);
            fb = sin_clip(driven);
        }

        // Hard limit feedback to prevent runaway
        fb = fb.clamp(-1.5, 1.5);

        // Write input + feedback to delay line
        self.delay.write(input + fb);
        self.feedback_sample = fb;

        output
    }

    pub fn last_feedback(&self) -> f64 {
        self.feedback_sample
    }

    pub fn reset(&mut self) {
        self.delay.clear();
        self.wow.reset();
        self.flutter.reset();
        self.hicut.reset();
        self.locut.reset();
        self.feedback_sample = 0.0;
        self.smoother.reset(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_delay() -> TapeDelay {
        let mut d = TapeDelay::new();
        d.time_ms = 100.0;
        d.feedback = 0.0;
        d.update(SR);
        d
    }

    #[test]
    fn silence_in_silence_out() {
        let mut d = make_delay();
        for _ in 0..48000 {
            let out = d.tick(0.0, 0);
            assert!(out.abs() < 1e-10);
        }
    }

    #[test]
    fn impulse_delayed() {
        let mut d = make_delay();
        let expected_delay = 4800; // 100ms at 48kHz

        let mut peak_pos = 0;
        let mut peak_val = 0.0;

        for i in 0..10000 {
            let input = if i == 0 { 1.0 } else { 0.0 };
            let out = d.tick(input, 0);
            if out.abs() > peak_val {
                peak_val = out.abs();
                peak_pos = i;
            }
        }

        assert!(
            (peak_pos as i64 - expected_delay as i64).unsigned_abs() < 10,
            "Peak at {peak_pos}, expected near {expected_delay}"
        );
        assert!(peak_val > 0.5, "Peak should be significant: {peak_val}");
    }

    #[test]
    fn feedback_creates_repeats() {
        let mut d = make_delay();
        d.feedback = 0.5;
        d.update(SR);

        d.tick(1.0, 0);

        let mut peaks = Vec::new();
        for i in 1..144000 {
            let out = d.tick(0.0, 0);
            if out.abs() > 0.05 && (peaks.is_empty() || i - peaks.last().unwrap() > 2000) {
                peaks.push(i);
            }
        }

        assert!(
            peaks.len() >= 3,
            "Should have multiple repeats with feedback: got {}",
            peaks.len()
        );
    }

    #[test]
    fn no_nan_with_all_features() {
        let mut d = TapeDelay::new();
        d.time_ms = 200.0;
        d.feedback = 0.7;
        d.drive = 0.8;
        d.hicut_freq = 5000.0;
        d.locut_freq = 100.0;
        d.wow_depth = 0.5;
        d.flutter_depth = 0.5;
        d.head2_enabled = true;
        d.head3_enabled = true;
        d.update(SR);

        for i in 0..96000 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = d.tick(input, 0);
            assert!(out.is_finite(), "NaN/Inf at sample {i}");
        }
    }

    #[test]
    fn saturation_limits_output() {
        let mut d = TapeDelay::new();
        d.time_ms = 10.0;
        d.feedback = 0.95;
        d.drive = 1.0;
        d.update(SR);

        for i in 0..48000 {
            let input = if i < 480 { 1.0 } else { 0.0 };
            let out = d.tick(input, 0);
            assert!(
                out.abs() < 3.0,
                "Output should be limited: {out} at sample {i}"
            );
        }
    }

    #[test]
    fn hicut_darkens_repeats() {
        let mut d_clean = make_delay();
        d_clean.feedback = 0.6;
        d_clean.update(SR);

        let mut d_dark = make_delay();
        d_dark.feedback = 0.6;
        d_dark.hicut_freq = 2000.0;
        d_dark.update(SR);

        let input: Vec<f64> = (0..200)
            .map(|i| (2.0 * PI * 10000.0 * i as f64 / SR).sin())
            .collect();

        for &s in &input {
            d_clean.tick(s, 0);
            d_dark.tick(s, 0);
        }

        let mut energy_clean = 0.0;
        let mut energy_dark = 0.0;
        for i in 0..20000 {
            let c = d_clean.tick(0.0, 0);
            let d = d_dark.tick(0.0, 0);
            if i > 13800 && i < 15200 {
                energy_clean += c * c;
                energy_dark += d * d;
            }
        }

        assert!(
            energy_dark < energy_clean * 0.99,
            "High-cut should reduce energy by 3rd repeat: clean={energy_clean:.6}, dark={energy_dark:.6}"
        );
    }

    #[test]
    fn smooth_time_change() {
        let mut d = make_delay();
        d.feedback = 0.0;
        d.update(SR);

        for _ in 0..4800 {
            d.tick(0.0, 0);
        }

        d.time_ms = 200.0;

        let mut prev: f64 = 0.0;
        let mut max_jump: f64 = 0.0;
        for i in 0..4800 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = d.tick(input, 0);
            let jump = (out - prev).abs();
            max_jump = max_jump.max(jump);
            prev = out;
        }
        assert!(
            max_jump < 1.0,
            "Time change should be smooth: max_jump={max_jump}"
        );
    }

    #[test]
    fn multihead_mode1_head3_only() {
        // Mode 1: Head 3 only — single tap at 2.85× base time
        let mut d = TapeDelay::new();
        d.time_ms = 100.0;
        d.feedback = 0.0;
        d.head1_enabled = false;
        d.head2_enabled = false;
        d.head3_enabled = true;
        d.update(SR);

        let mut outputs = Vec::with_capacity(20000);
        for i in 0..20000 {
            let input = if i == 0 { 1.0 } else { 0.0 };
            outputs.push(d.tick(input, 0));
        }

        // Head 3 at 100ms * 2.85 = 285ms = 13680 samples
        let head1_region = outputs[4700..4900]
            .iter()
            .map(|x| x.abs())
            .fold(0.0f64, f64::max);
        let head3_peak = outputs[13500..13900]
            .iter()
            .map(|x| x.abs())
            .fold(0.0f64, f64::max);

        assert!(
            head1_region < 0.01,
            "Head 1 should be silent in Mode 1: got {head1_region}"
        );
        assert!(
            head3_peak > 0.5,
            "Head 3 should produce peak near 13680: got {head3_peak}"
        );
    }

    #[test]
    fn multihead_mode2_head1_and_3() {
        // Mode 2: Heads 1 + 3
        let mut d = TapeDelay::new();
        d.time_ms = 100.0;
        d.feedback = 0.0;
        d.head1_enabled = true;
        d.head2_enabled = false;
        d.head3_enabled = true;
        d.update(SR);

        let mut outputs = Vec::with_capacity(20000);
        for i in 0..20000 {
            let input = if i == 0 { 1.0 } else { 0.0 };
            outputs.push(d.tick(input, 0));
        }

        let head1_peak = outputs[4700..4900]
            .iter()
            .map(|x| x.abs())
            .fold(0.0f64, f64::max);
        let head2_region = outputs[9200..9500]
            .iter()
            .map(|x| x.abs())
            .fold(0.0f64, f64::max);
        let head3_peak = outputs[13500..13900]
            .iter()
            .map(|x| x.abs())
            .fold(0.0f64, f64::max);

        assert!(
            head1_peak > 0.5,
            "Head 1 should be active: got {head1_peak}"
        );
        assert!(
            head2_region < 0.01,
            "Head 2 should be silent: got {head2_region}"
        );
        assert!(
            head3_peak > 0.5,
            "Head 3 should be active: got {head3_peak}"
        );
    }

    #[test]
    fn multihead_mode4_all_three() {
        // Mode 4: All three heads
        let mut d = TapeDelay::new();
        d.time_ms = 100.0;
        d.feedback = 0.0;
        d.head1_enabled = true;
        d.head2_enabled = true;
        d.head3_enabled = true;
        d.update(SR);

        let mut outputs = Vec::with_capacity(20000);
        for i in 0..20000 {
            let input = if i == 0 { 1.0 } else { 0.0 };
            outputs.push(d.tick(input, 0));
        }

        let head1_peak = outputs[4700..4900]
            .iter()
            .map(|x| x.abs())
            .fold(0.0f64, f64::max);
        let head2_peak = outputs[9200..9500]
            .iter()
            .map(|x| x.abs())
            .fold(0.0f64, f64::max);
        let head3_peak = outputs[13500..13900]
            .iter()
            .map(|x| x.abs())
            .fold(0.0f64, f64::max);

        assert!(head1_peak > 0.5, "Head 1: {head1_peak}");
        assert!(head2_peak > 0.5, "Head 2: {head2_peak}");
        assert!(head3_peak > 0.5, "Head 3: {head3_peak}");
    }

    #[test]
    fn multihead_no_nan() {
        let mut d = TapeDelay::new();
        d.time_ms = 500.0;
        d.feedback = 0.6;
        d.drive = 0.5;
        d.head2_enabled = true;
        d.head3_enabled = true;
        d.hicut_freq = 4000.0;
        d.wow_depth = 0.3;
        d.flutter_depth = 0.2;
        d.update(SR);

        for i in 0..96000 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.3;
            let out = d.tick(input, 0);
            assert!(out.is_finite(), "NaN/Inf at sample {i}");
            assert!(out.abs() < 10.0, "Runaway at sample {i}: {out}");
        }
    }
}
