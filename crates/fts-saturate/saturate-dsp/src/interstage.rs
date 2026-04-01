//! Interstage — transformer coupling emulation.
//!
//! Faithful port of the Airwindows Interstage algorithm by Chris Johnson,
//! released under the MIT license.
//!
//! The algorithm emulates the subtle frequency-shaping and slew-limiting
//! behavior of transformer-coupled analog stages. It uses alternating IIR
//! lowpass filters (flipped each sample) with slew limiting against the
//! lowpassed reference, producing gentle high-frequency rolloff and soft
//! saturation character with zero user parameters in the original.
//!
//! We add an `intensity` parameter (0.0–1.0) that scales the slew-limit
//! threshold: at 0.0 the effect is very gentle, at 1.0 it matches the
//! original Airwindows behavior.

use fts_dsp::{AudioConfig, Processor};

/// Golden-ratio-derived constant used in the original algorithm (~1/phi^2).
const PHI_INV_SQ: f64 = 0.381966011250105;

/// Per-channel filter state for the Interstage algorithm.
#[derive(Clone, Debug)]
struct ChannelState {
    iir_a: f64,
    iir_b: f64,
    iir_c: f64,
    iir_d: f64,
    iir_e: f64,
    iir_f: f64,
    last_sample: f64,
}

impl ChannelState {
    fn new() -> Self {
        Self {
            iir_a: 0.0,
            iir_b: 0.0,
            iir_c: 0.0,
            iir_d: 0.0,
            iir_e: 0.0,
            iir_f: 0.0,
            last_sample: 0.0,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

/// Transformer coupling emulation based on Airwindows Interstage.
///
/// Uses alternating IIR lowpass filters with slew limiting to emulate
/// the subtle high-frequency rolloff and soft saturation of
/// transformer-coupled analog stages.
pub struct Interstage {
    /// Effect intensity (0.0 = very gentle, 1.0 = original Airwindows threshold).
    pub intensity: f64,

    left: ChannelState,
    right: ChannelState,
    flip: bool,
    sample_rate: f64,
}

impl Interstage {
    pub fn new() -> Self {
        Self {
            intensity: 1.0,
            left: ChannelState::new(),
            right: ChannelState::new(),
            flip: false,
            sample_rate: 44100.0,
        }
    }

    /// Process a single channel sample through the Interstage algorithm.
    #[inline]
    fn process_sample(
        ch: &mut ChannelState,
        input: f64,
        flip: bool,
        first_stage: f64,
        iir_amount: f64,
        threshold: f64,
    ) -> f64 {
        let dry = input;

        // Start with averaging against last sample.
        let mut sample = (input + ch.last_sample) * 0.5;

        if flip {
            ch.iir_a = ch.iir_a * (1.0 - first_stage) + sample * first_stage;
            sample = ch.iir_a;
            ch.iir_c = ch.iir_c * (1.0 - iir_amount) + sample * iir_amount;
            sample = ch.iir_c;
            ch.iir_e = ch.iir_e * (1.0 - iir_amount) + sample * iir_amount;
            sample = ch.iir_e;

            // Make highpass.
            sample = dry - sample;

            // Slew limit against lowpassed reference point.
            if sample - ch.iir_a > threshold {
                sample = ch.iir_a + threshold;
            }
            if sample - ch.iir_a < -threshold {
                sample = ch.iir_a - threshold;
            }
        } else {
            ch.iir_b = ch.iir_b * (1.0 - first_stage) + sample * first_stage;
            sample = ch.iir_b;
            ch.iir_d = ch.iir_d * (1.0 - iir_amount) + sample * iir_amount;
            sample = ch.iir_d;
            ch.iir_f = ch.iir_f * (1.0 - iir_amount) + sample * iir_amount;
            sample = ch.iir_f;

            // Make highpass.
            sample = dry - sample;

            // Slew limit against lowpassed reference point.
            if sample - ch.iir_b > threshold {
                sample = ch.iir_b + threshold;
            }
            if sample - ch.iir_b < -threshold {
                sample = ch.iir_b - threshold;
            }
        }

        ch.last_sample = sample;
        sample
    }
}

impl Default for Interstage {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for Interstage {
    fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
        self.flip = false;
    }

    fn update(&mut self, config: AudioConfig) {
        self.sample_rate = config.sample_rate;
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        let overallscale = self.sample_rate / 44100.0;
        let first_stage = PHI_INV_SQ / overallscale;
        let iir_amount = 0.00295 / overallscale;
        let threshold = PHI_INV_SQ * (0.2 + 0.8 * self.intensity);

        for i in 0..left.len() {
            left[i] = Self::process_sample(
                &mut self.left,
                left[i],
                self.flip,
                first_stage,
                iir_amount,
                threshold,
            );
            right[i] = Self::process_sample(
                &mut self.right,
                right[i],
                self.flip,
                first_stage,
                iir_amount,
                threshold,
            );
            self.flip = !self.flip;
        }
    }
}
