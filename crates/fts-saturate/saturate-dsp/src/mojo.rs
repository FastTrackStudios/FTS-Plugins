//! Ports of the Airwindows Mojo and Dyno waveshaper algorithms.
//!
//! Original code by Chris Johnson (Airwindows), released under the MIT license.
//! <https://github.com/airwindows/airwindows>

use std::f64::consts::PI;

/// Mojo waveshaper. `drive` is 0.0--1.0.
///
/// Attempt at a faithful port of Airwindows Mojo. Applies a drive-dependent
/// gain, then a soft sine-based waveshaper that flattens gently before
/// wavefolding.
pub fn mojo(x: f64, drive: f64) -> f64 {
    let gain = 10.0_f64.powf((drive * 24.0 - 12.0) / 20.0);
    let sample = x * gain;

    let m = sample.abs().powf(0.25);
    if m > 0.0 {
        (sample * m * PI * 0.5).sin() / m * 0.987654321
    } else {
        0.0
    }
}

/// Dyno waveshaper. `drive` is 0.0--1.0.
///
/// Attempt at a faithful port of Airwindows Dyno. Applies a drive-dependent
/// gain, then a sine-based waveshaper that tries to raise peak energy.
pub fn dyno(x: f64, drive: f64) -> f64 {
    let gain = 10.0_f64.powf((drive * 24.0 - 12.0) / 20.0);
    let sample = x * gain;

    let d = sample.abs().powi(4);
    if d > 0.0 {
        (sample * d).sin() / d * 1.1654321
    } else {
        0.0
    }
}
