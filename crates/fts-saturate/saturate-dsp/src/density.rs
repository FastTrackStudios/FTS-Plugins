//! Faithful port of the Airwindows Density3 algorithm.
//!
//! Original code by Chris Johnson (Airwindows), released under the MIT license.
//! <https://github.com/airwindows/airwindows>
//!
//! Density applies a variable saturation curve driven by a Taylor-series
//! approximation of sine (for overdrive) or cosine (for "unsaturation"),
//! preceded by a one-pole IIR highpass filter to remove DC and low-frequency
//! content before the nonlinearity.

use std::f64::consts::FRAC_PI_2;

use fts_dsp::{AudioConfig, Processor};

/// Airwindows Density3 saturation processor (stereo, stateful).
///
/// # Parameters
///
/// | Field     | Range       | Description                                       |
/// |-----------|-------------|---------------------------------------------------|
/// | `density` | 0.0 -- 5.0  | Amount of saturation (maps from `A * 5.0`)        |
/// | `highpass` | 0.0 -- 1.0 | IIR highpass amount (`B`); cubed then scaled       |
/// | `output`  | 0.0 -- 1.0  | Output gain (`C`)                                 |
/// | `mix`     | 0.0 -- 1.0  | Dry/wet blend (`D`)                               |
pub struct Density {
    // --- parameters ---
    pub density: f64,
    pub highpass: f64,
    pub output: f64,
    pub mix: f64,

    // --- state ---
    iir_l: f64,
    iir_r: f64,
    sample_rate: f64,
}

impl Default for Density {
    fn default() -> Self {
        Self {
            density: 0.0,
            highpass: 0.0,
            output: 1.0,
            mix: 1.0,
            iir_l: 0.0,
            iir_r: 0.0,
            sample_rate: 44100.0,
        }
    }
}

impl Density {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Process a single sample through the Density algorithm.
///
/// `iir_state` is the per-channel IIR highpass accumulator.
#[inline]
fn process_sample(
    input: f64,
    iir_state: &mut f64,
    iir_amount: f64,
    density: f64,
    output: f64,
    mix: f64,
) -> f64 {
    let dry = input;

    // IIR highpass: subtract the lowpass component.
    *iir_state = (*iir_state * (1.0 - iir_amount)) + (input * iir_amount);
    let mut sample = input - *iir_state;

    let altered = if density > 1.0 {
        // Sine Taylor-series saturation.
        let clamped = (sample * density * FRAC_PI_2).clamp(-FRAC_PI_2, FRAC_PI_2);
        let x = clamped * clamped;
        let mut temp = clamped * x; // x^3
        let mut alt = clamped;
        alt -= temp / 6.0; // -x^3/3!
        temp *= x; // x^5
        alt += temp / 120.0; // +x^5/5!
        temp *= x; // x^7
        alt -= temp / 5040.0; // -x^7/7!
        temp *= x; // x^9
        alt += temp / 362880.0; // +x^9/9!
        temp *= x; // x^11
        alt -= temp / 39916800.0; // -x^11/11!
        alt
    } else {
        // Cosine-ish "unsaturation".
        let clamped = sample.clamp(-1.0, 1.0);
        let polarity = clamped;
        let x = sample * clamped;
        let mut temp = x;
        let mut alt = temp / 2.0; // x/2!
        temp *= x;
        alt -= temp / 24.0; // -x^2/4!
        temp *= x;
        alt += temp / 720.0; // +x^3/6!
        temp *= x;
        alt -= temp / 40320.0; // -x^4/8!
        temp *= x;
        alt += temp / 3628800.0; // +x^5/10!
        if polarity < 0.0 {
            -alt
        } else {
            alt
        }
    };

    // Blend between dry and altered based on density distance from 1.0.
    sample = if density > 2.0 {
        altered
    } else {
        let blend = (density - 1.0).abs();
        sample * (1.0 - blend) + altered * blend
    };

    // Dry/wet mix with output gain.
    dry * (1.0 - mix) + sample * output * mix
}

impl Processor for Density {
    fn reset(&mut self) {
        self.iir_l = 0.0;
        self.iir_r = 0.0;
    }

    fn update(&mut self, config: AudioConfig) {
        self.sample_rate = config.sample_rate;
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        let overallscale = self.sample_rate / 44100.0;
        let iir_amount = self.highpass.powi(3) / overallscale;

        for sample in left.iter_mut() {
            *sample = process_sample(
                *sample,
                &mut self.iir_l,
                iir_amount,
                self.density,
                self.output,
                self.mix,
            );
        }
        for sample in right.iter_mut() {
            *sample = process_sample(
                *sample,
                &mut self.iir_r,
                iir_amount,
                self.density,
                self.output,
                self.mix,
            );
        }
    }
}
