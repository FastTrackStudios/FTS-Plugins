//! Gate envelope — attack / hold / release shaper.
//!
//! Converts the binary open/close signal from the detector into a smooth
//! gain envelope with configurable attack, hold, and release times.

use std::f64::consts::PI;

/// Maximum stereo channels.
const MAX_CH: usize = 2;

/// Gate envelope states.
#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    /// Gate is closed, gain = floor.
    Closed,
    /// Gate is opening, gain ramping up.
    Attack,
    /// Gate is fully open, hold timer running.
    Hold,
    /// Gate is closing, gain ramping down.
    Release,
}

// r[impl gate.envelope.attack]
// r[impl gate.envelope.hold]
// r[impl gate.envelope.release]
// r[impl gate.envelope.range]
/// Gate envelope shaper with attack, hold, release, and range.
///
/// Takes the binary gate open/close state from the detector and produces
/// a smooth gain value (0.0 to 1.0) that modulates the audio signal.
pub struct GateEnvelope {
    state: [State; MAX_CH],
    /// Current gain value per channel (0.0 = fully closed, 1.0 = fully open).
    gain: [f64; MAX_CH],
    /// Hold timer (samples remaining) per channel.
    hold_counter: [u64; MAX_CH],

    // Cached parameters
    attack_inc: f64,   // Gain increment per sample during attack
    release_dec: f64,  // Gain decrement per sample during release
    hold_samples: u64, // Hold time in samples
    range_floor: f64,  // Minimum gain when gate is closed (0.0 = full mute)
}

impl GateEnvelope {
    pub fn new() -> Self {
        Self {
            state: [State::Closed; MAX_CH],
            gain: [0.0; MAX_CH],
            hold_counter: [0; MAX_CH],
            attack_inc: 1.0,
            release_dec: 0.001,
            hold_samples: 0,
            range_floor: 0.0,
        }
    }

    /// Update envelope parameters.
    ///
    /// - `attack_ms`: gate open time (0.01 to 100 ms)
    /// - `hold_ms`: hold time (0 to 2000 ms)
    /// - `release_ms`: gate close time (1 to 2000 ms)
    /// - `range_db`: gate depth in dB (0 = full mute, e.g., -40 = attenuate by 40dB)
    /// - `sample_rate`: current sample rate
    pub fn set_params(
        &mut self,
        attack_ms: f64,
        hold_ms: f64,
        release_ms: f64,
        range_db: f64,
        sample_rate: f64,
    ) {
        let attack_samples = (attack_ms * 0.001 * sample_rate).max(1.0);
        let release_samples = (release_ms * 0.001 * sample_rate).max(1.0);

        self.attack_inc = 1.0 / attack_samples;
        self.release_dec = 1.0 / release_samples;
        self.hold_samples = (hold_ms * 0.001 * sample_rate) as u64;

        // Range: 0dB means no attenuation (pass-through when "closed"),
        // -inf means full mute.
        self.range_floor = if range_db <= -100.0 {
            0.0
        } else {
            10.0_f64.powf(range_db / 20.0)
        };
    }

    /// Process one sample: given the detector's open/close decision,
    /// produce a smooth gain value.
    ///
    /// Returns gain in range [range_floor, 1.0].
    #[inline]
    pub fn tick(&mut self, gate_open: bool, ch: usize) -> f64 {
        match self.state[ch] {
            State::Closed => {
                if gate_open {
                    self.state[ch] = State::Attack;
                }
            }
            State::Attack => {
                self.gain[ch] += self.attack_inc;
                if self.gain[ch] >= 1.0 {
                    self.gain[ch] = 1.0;
                    self.state[ch] = State::Hold;
                    self.hold_counter[ch] = self.hold_samples;
                }
                if !gate_open && self.hold_samples == 0 {
                    self.state[ch] = State::Release;
                }
            }
            State::Hold => {
                self.gain[ch] = 1.0;
                if !gate_open {
                    if self.hold_counter[ch] > 0 {
                        self.hold_counter[ch] -= 1;
                    } else {
                        self.state[ch] = State::Release;
                    }
                } else {
                    // Signal came back — reset hold timer
                    self.hold_counter[ch] = self.hold_samples;
                }
            }
            State::Release => {
                self.gain[ch] -= self.release_dec;
                if self.gain[ch] <= 0.0 {
                    self.gain[ch] = 0.0;
                    self.state[ch] = State::Closed;
                }
                if gate_open {
                    self.state[ch] = State::Attack;
                }
            }
        }

        // Apply cos() shaping for smooth transitions
        let shaped = cos_shape(self.gain[ch]);

        // Scale between range_floor and 1.0
        self.range_floor + shaped * (1.0 - self.range_floor)
    }

    /// Get the current raw gain value (0-1, before shaping).
    pub fn raw_gain(&self, ch: usize) -> f64 {
        self.gain[ch]
    }

    pub fn reset(&mut self) {
        self.state = [State::Closed; MAX_CH];
        self.gain = [0.0; MAX_CH];
        self.hold_counter = [0; MAX_CH];
    }
}

impl Default for GateEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

/// Cosine-shaped curve for smooth gate transitions.
///
/// Maps linear 0..1 to a cos()-shaped 0..1 curve:
/// `0.5 - 0.5 * cos(x * PI)` — starts slow, accelerates, ends slow.
#[inline]
fn cos_shape(x: f64) -> f64 {
    0.5 - 0.5 * (x * PI).cos()
}
