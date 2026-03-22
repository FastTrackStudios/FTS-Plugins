//! Delay chain — composes tape delay, pitch delay, ducking, and diffusion
//! into a full stereo processor.

use fts_dsp::{AudioConfig, Processor};

use crate::modulation::{Diffuser, DuckingFollower};
use crate::tape_delay::TapeDelay;

// r[impl delay.chain.signal-flow]
/// Stereo mode for the delay chain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StereoMode {
    /// Independent L/R delays.
    Stereo,
    /// Cross-feed: L output feeds R input and vice versa.
    PingPong,
    /// Mono delay duplicated to both channels.
    Mono,
}

/// RE-201–style tape head configuration.
///
/// Controls which of the 3 playback heads are active.
/// Head 1 is at base delay time, Head 2 at 1.94×, Head 3 at 2.85×
/// (matching RE-201 physical head spacing).
///
/// Modes match the Neural DSP Archetype: Mateus Asato Echo module.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeadMode {
    /// Head 3 only — single repetition at longest interval.
    Mode1,
    /// Heads 1 + 3 — dual repetition (short + long).
    Mode2,
    /// Heads 2 + 3 — syncopated dual repetition.
    Mode3,
    /// All three heads — three-repetition pattern.
    Mode4,
}

/// Full stereo delay processing chain.
///
/// Signal flow: Input → Tape Delay (L/R) → Diffusion → Duck → Stereo Width → Mix
pub struct DelayChain {
    // Delay engines
    pub delay_l: TapeDelay,
    pub delay_r: TapeDelay,

    // Global parameters
    /// Dry/wet mix (0.0 = fully dry, 1.0 = fully wet).
    pub mix: f64,
    /// Stereo mode.
    pub stereo_mode: StereoMode,
    /// Stereo width (0.0 = mono, 1.0 = normal, 2.0 = extra wide).
    pub width: f64,
    /// Tape head configuration (RE-201 style).
    pub head_mode: HeadMode,
    /// Enable diffusion.
    pub diffusion_enabled: bool,
    /// Diffusion size (0.0–1.0).
    pub diffusion_size: f64,
    /// Diffusion smear / feedback (0.0–1.0).
    pub diffusion_smear: f64,
    /// Enable ducking.
    pub ducking_enabled: bool,
    /// Ping-pong feedback (used when stereo_mode == PingPong).
    pub pingpong_feedback: f64,

    // Internal
    diffuser_l: Diffuser,
    diffuser_r: Diffuser,
    pub ducker: DuckingFollower,
    sample_rate: f64,
}

impl DelayChain {
    pub fn new() -> Self {
        Self {
            delay_l: TapeDelay::new(),
            delay_r: TapeDelay::new(),
            mix: 0.5,
            stereo_mode: StereoMode::Stereo,
            width: 1.0,
            head_mode: HeadMode::Mode1,
            diffusion_enabled: false,
            diffusion_size: 0.5,
            diffusion_smear: 0.5,
            ducking_enabled: false,
            pingpong_feedback: 0.5,
            diffuser_l: Diffuser::new(48000.0, false),
            diffuser_r: Diffuser::new(48000.0, true),
            ducker: DuckingFollower::new(),
            sample_rate: 48000.0,
        }
    }
}

impl Processor for DelayChain {
    fn reset(&mut self) {
        self.delay_l.reset();
        self.delay_r.reset();
        self.diffuser_l.reset();
        self.diffuser_r.reset();
        self.ducker.reset();
    }

    fn update(&mut self, config: AudioConfig) {
        self.sample_rate = config.sample_rate;

        // Configure tape heads based on head mode
        let (h1, h2, h3) = match self.head_mode {
            HeadMode::Mode1 => (false, false, true), // Head 3 only
            HeadMode::Mode2 => (true, false, true),  // Head 1 + 3
            HeadMode::Mode3 => (false, true, true),  // Head 2 + 3
            HeadMode::Mode4 => (true, true, true),   // All heads
        };
        self.delay_l.head1_enabled = h1;
        self.delay_l.head2_enabled = h2;
        self.delay_l.head3_enabled = h3;
        self.delay_r.head1_enabled = h1;
        self.delay_r.head2_enabled = h2;
        self.delay_r.head3_enabled = h3;

        self.delay_l.update(config.sample_rate);
        self.delay_r.update(config.sample_rate);

        self.diffuser_l = Diffuser::new(config.sample_rate, false);
        self.diffuser_r = Diffuser::new(config.sample_rate, true);
        self.diffuser_l.size = self.diffusion_size;
        self.diffuser_l.smear = self.diffusion_smear;
        self.diffuser_r.size = self.diffusion_size;
        self.diffuser_r.smear = self.diffusion_smear;
        self.diffuser_l.update(config.sample_rate, false);
        self.diffuser_r.update(config.sample_rate, true);

        self.ducker.set_sample_rate(config.sample_rate);
        self.ducker.update_coeffs();
    }

    // r[impl delay.chain.process]
    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        let n = left.len().min(right.len());

        for i in 0..n {
            let dry_l = left[i];
            let dry_r = right[i];

            // Compute delay input based on stereo mode
            let (in_l, in_r) = match self.stereo_mode {
                StereoMode::Stereo => (dry_l, dry_r),
                StereoMode::Mono => {
                    let mono = (dry_l + dry_r) * 0.5;
                    (mono, mono)
                }
                StereoMode::PingPong => {
                    // In ping-pong, we feed mono to L and cross-feed from outputs
                    let mono = (dry_l + dry_r) * 0.5;
                    let fb_r = self.delay_r.last_feedback();
                    let fb_l = self.delay_l.last_feedback();
                    (
                        mono + fb_r * self.pingpong_feedback,
                        fb_l * self.pingpong_feedback,
                    )
                }
            };

            let mut wet_l = self.delay_l.tick(in_l, 0);
            let mut wet_r = self.delay_r.tick(in_r, 1);

            // Diffusion
            if self.diffusion_enabled {
                wet_l = self.diffuser_l.tick(wet_l);
                wet_r = self.diffuser_r.tick(wet_r);
            }

            // Ducking
            if self.ducking_enabled {
                let input_level = (dry_l.abs() + dry_r.abs()) * 0.5;
                let duck_gain = self.ducker.tick(input_level);
                wet_l *= duck_gain;
                wet_r *= duck_gain;
            }

            // Stereo width (mid-side)
            if (self.width - 1.0).abs() > 0.001 {
                let mid = (wet_l + wet_r) * 0.5;
                let side = (wet_l - wet_r) * 0.5;
                wet_l = mid + side * self.width;
                wet_r = mid - side * self.width;
            }

            // Mix dry/wet
            left[i] = dry_l * (1.0 - self.mix) + wet_l * self.mix;
            right[i] = dry_r * (1.0 - self.mix) + wet_r * self.mix;
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
            max_buffer_size: 512,
        }
    }

    fn make_chain() -> DelayChain {
        let mut c = DelayChain::new();
        c.delay_l.time_ms = 100.0;
        c.delay_r.time_ms = 100.0;
        c.delay_l.feedback = 0.0;
        c.delay_r.feedback = 0.0;
        c.mix = 1.0;
        c.update(config());
        c
    }

    #[test]
    fn dry_wet_mix() {
        let mut c = make_chain();
        c.mix = 0.0; // Fully dry

        let mut l = vec![0.5; 512];
        let mut r = vec![0.5; 512];
        c.process(&mut l, &mut r);

        // Should be unchanged
        assert!((l[0] - 0.5).abs() < 1e-10);
    }

    #[test]
    fn stereo_mode_works() {
        let mut c = make_chain();
        c.stereo_mode = StereoMode::Mono;
        c.delay_l.time_ms = 50.0;
        c.delay_r.time_ms = 50.0;
        c.update(config());

        // Send signal only to left
        let n = 9600;
        let mut l: Vec<f64> = (0..n).map(|i| if i < 100 { 1.0 } else { 0.0 }).collect();
        let mut r = vec![0.0; n];

        c.process(&mut l, &mut r);

        // In mono mode, right should also get output
        let r_energy: f64 = r.iter().map(|x| x * x).sum();
        assert!(r_energy > 0.01, "Mono mode should output to both channels");
    }

    #[test]
    fn no_nan_full_chain() {
        let mut c = DelayChain::new();
        c.delay_l.time_ms = 200.0;
        c.delay_r.time_ms = 250.0;
        c.delay_l.feedback = 0.6;
        c.delay_r.feedback = 0.6;
        c.delay_l.drive = 0.5;
        c.delay_r.drive = 0.5;
        c.delay_l.hicut_freq = 5000.0;
        c.delay_r.hicut_freq = 5000.0;
        c.delay_l.wow_depth = 0.3;
        c.delay_r.wow_depth = 0.3;
        c.delay_l.flutter_depth = 0.3;
        c.delay_r.flutter_depth = 0.3;
        c.diffusion_enabled = true;
        c.ducking_enabled = true;
        c.ducker.amount = 0.5;
        c.ducker.threshold = 0.1;
        c.mix = 0.5;
        c.update(config());

        let n = 48000;
        let mut l: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5)
            .collect();
        let mut r = l.clone();

        c.process(&mut l, &mut r);

        for (i, (&lv, &rv)) in l.iter().zip(r.iter()).enumerate() {
            assert!(lv.is_finite(), "L NaN at {i}");
            assert!(rv.is_finite(), "R NaN at {i}");
        }
    }

    #[test]
    fn ping_pong_produces_output_both_channels() {
        let mut c = DelayChain::new();
        c.delay_l.time_ms = 50.0;
        c.delay_r.time_ms = 50.0;
        c.delay_l.feedback = 0.3;
        c.delay_r.feedback = 0.3;
        c.stereo_mode = StereoMode::PingPong;
        c.pingpong_feedback = 0.6;
        c.mix = 1.0;
        c.update(config());

        // Send a burst in left channel
        let n = 19200;
        let mut l: Vec<f64> = (0..n).map(|i| if i < 100 { 0.8 } else { 0.0 }).collect();
        let mut r = vec![0.0; n];

        c.process(&mut l, &mut r);

        // Both channels should have output (ping-pong cross-feeds)
        let l_energy: f64 = l.iter().map(|x| x * x).sum();
        let r_energy: f64 = r.iter().map(|x| x * x).sum();

        assert!(l_energy > 0.01, "L should have output: {l_energy}");
        assert!(
            r_energy > 0.001,
            "R should have output from ping-pong cross-feed: {r_energy}"
        );
    }

    #[test]
    fn diffusion_smears_impulse() {
        let mut c_clean = make_chain();
        c_clean.diffusion_enabled = false;
        c_clean.update(config());

        let mut c_diff = make_chain();
        c_diff.diffusion_enabled = true;
        c_diff.diffusion_smear = 0.7;
        c_diff.diffusion_size = 0.5;
        c_diff.update(config());

        let n = 19200;
        let impulse: Vec<f64> = (0..n).map(|i| if i == 0 { 1.0 } else { 0.0 }).collect();

        let mut l_clean = impulse.clone();
        let mut r_clean = impulse.clone();
        c_clean.process(&mut l_clean, &mut r_clean);

        let mut l_diff = impulse.clone();
        let mut r_diff = impulse.clone();
        c_diff.process(&mut l_diff, &mut r_diff);

        // Count how many samples are above threshold — diffusion should spread energy
        let count_clean = l_clean.iter().filter(|x| x.abs() > 0.01).count();
        let count_diff = l_diff.iter().filter(|x| x.abs() > 0.01).count();

        assert!(
            count_diff > count_clean,
            "Diffusion should spread the impulse: clean={count_clean}, diff={count_diff}"
        );
    }

    #[test]
    fn width_control() {
        let mut c = make_chain();
        c.delay_l.time_ms = 50.0;
        c.delay_r.time_ms = 75.0; // Different times for L/R
        c.width = 2.0; // Extra wide
        c.update(config());

        let n = 9600;
        let mut l: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5)
            .collect();
        let mut r = l.clone();

        c.process(&mut l, &mut r);

        // With different delay times and width>1, L and R should differ
        let diff: f64 = l
            .iter()
            .zip(r.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / n as f64;
        assert!(
            diff > 0.001,
            "Width should create stereo difference: avg_diff={diff}"
        );
    }
}
