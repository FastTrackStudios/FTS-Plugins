//! Adaptive resonant decay — measures each drum hit's natural resonance
//! and dynamically adjusts gate hold/release to match.
//!
//! After an onset, captures an FFT snapshot to estimate the resonant frequency,
//! then tracks the decay envelope of the resonant component via a narrow bandpass.

use fts_dsp::biquad::{Biquad, FilterType};
use fts_dsp::envelope::EnvelopeFollower;
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

/// FFT size for resonance estimation (1024 at 48kHz ≈ 21ms window, 46.9 Hz/bin).
const FFT_SIZE: usize = 1024;

/// States for the decay tracker.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DecayState {
    /// Waiting for onset.
    Idle,
    /// Filling onset buffer for FFT analysis.
    Capturing,
    /// Measuring decay rate from resonant bandpass.
    Measuring,
}

/// Adaptive resonant decay tracker.
pub struct AdaptiveDecayTracker {
    // FFT infrastructure
    onset_buf: Vec<f64>,
    onset_buf_pos: usize,
    window: Vec<f64>,

    // Resonant frequency tracking
    resonant_freq: f64,
    bandpass: Biquad,
    pub resonance_q: f64,

    // Decay measurement
    decay_env: EnvelopeFollower,
    onset_amplitude: f64,
    samples_since_onset: u64,
    measured_decay_rate: f64, // dB per ms (negative)

    // Output: computed adaptive hold/release
    pub computed_hold_ms: f64,
    pub computed_release_ms: f64,

    // Configuration
    pub enabled: bool,
    pub decay_sensitivity: f64, // 0.0 = max hold, 1.0 = tight follow
    pub min_release_ms: f64,
    pub max_release_ms: f64,
    /// Frequency search range override. When (0, 0), uses drum-class-based defaults.
    pub search_range: (f64, f64),

    // State
    state: DecayState,
    sample_rate: f64,
    capture_target: usize,
}

impl AdaptiveDecayTracker {
    pub fn new() -> Self {
        // Hanning window
        let window: Vec<f64> = (0..FFT_SIZE)
            .map(|i| {
                0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / (FFT_SIZE - 1) as f64).cos())
            })
            .collect();

        Self {
            onset_buf: vec![0.0; FFT_SIZE],
            onset_buf_pos: 0,
            window,
            resonant_freq: 0.0,
            bandpass: Biquad::new(),
            resonance_q: 8.0,
            decay_env: EnvelopeFollower::new(0.0),
            onset_amplitude: 0.0,
            samples_since_onset: 0,
            measured_decay_rate: 0.0,
            computed_hold_ms: 50.0,
            computed_release_ms: 100.0,
            enabled: false,
            decay_sensitivity: 0.5,
            min_release_ms: 10.0,
            max_release_ms: 1000.0,
            search_range: (0.0, 0.0),
            state: DecayState::Idle,
            sample_rate: 48000.0,
            capture_target: FFT_SIZE,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        // Decay envelope: instant attack, 200ms release for smooth tracking
        self.decay_env.set_times_ms(0.01, 200.0, sample_rate);
    }

    /// Signal that an onset has been detected. Begins capture for FFT analysis.
    ///
    /// `drum_class` is used to determine the frequency search range.
    pub fn on_onset(&mut self, drum_class: super::classifier::DrumClass) {
        if !self.enabled {
            return;
        }

        // Set frequency search range based on drum type
        if self.search_range == (0.0, 0.0) {
            use super::classifier::DrumClass;
            let (lo, hi) = match drum_class {
                DrumClass::Kick => (30.0, 200.0),
                DrumClass::Snare => (100.0, 500.0),
                DrumClass::Tom => (60.0, 400.0),
                DrumClass::HiHat => (3000.0, 15000.0),
                DrumClass::Unknown => (30.0, 500.0), // wide search
            };
            self.search_range = (lo, hi);
        }

        self.state = DecayState::Capturing;
        self.onset_buf_pos = 0;
        self.onset_amplitude = 0.0;
        self.samples_since_onset = 0;
        self.measured_decay_rate = 0.0;
    }

    /// Process one mono sample. Call after onset detection.
    ///
    /// Returns `true` if adaptive parameters were updated this sample.
    pub fn tick(&mut self, sample: f64) -> bool {
        if !self.enabled || self.state == DecayState::Idle {
            return false;
        }

        match self.state {
            DecayState::Idle => false,

            DecayState::Capturing => {
                // Fill onset buffer
                self.onset_buf[self.onset_buf_pos] = sample;
                self.onset_buf_pos += 1;

                if self.onset_buf_pos >= self.capture_target {
                    // Run FFT to estimate resonant frequency
                    self.estimate_resonance();
                    self.state = DecayState::Measuring;
                    self.samples_since_onset = 0;
                }
                false
            }

            DecayState::Measuring => {
                self.samples_since_onset += 1;

                // Track resonant component through narrow bandpass
                let resonant = self.bandpass.tick(sample, 0);
                let abs_res = resonant.abs();

                // Update decay envelope
                let env = self.decay_env.tick(abs_res);

                // Capture peak in first 5ms
                let five_ms = (self.sample_rate * 0.005) as u64;
                if self.samples_since_onset < five_ms {
                    if env > self.onset_amplitude {
                        self.onset_amplitude = env;
                    }
                }

                // After 20ms, start measuring decay rate
                let twenty_ms = (self.sample_rate * 0.020) as u64;
                if self.samples_since_onset > twenty_ms && self.onset_amplitude > 1e-8 {
                    let ratio = env / self.onset_amplitude;
                    let current_db = if ratio > 1e-10 {
                        20.0 * ratio.log10()
                    } else {
                        -100.0
                    };
                    let elapsed_ms = self.samples_since_onset as f64 / self.sample_rate * 1000.0;

                    if elapsed_ms > 0.0 {
                        self.measured_decay_rate = current_db / elapsed_ms;
                    }

                    // Once we have a stable measurement (~50ms after onset), compute
                    let fifty_ms = (self.sample_rate * 0.050) as u64;
                    if self.samples_since_onset > fifty_ms {
                        self.compute_adaptive_times();
                        self.state = DecayState::Idle;
                        return true;
                    }
                }

                // Timeout after 500ms — use whatever we have
                let timeout = (self.sample_rate * 0.5) as u64;
                if self.samples_since_onset > timeout {
                    self.compute_adaptive_times();
                    self.state = DecayState::Idle;
                    return true;
                }

                false
            }
        }
    }

    /// Estimate the resonant frequency from the onset buffer via FFT.
    fn estimate_resonance(&mut self) {
        // Apply window and convert to complex
        let mut fft_input: Vec<Complex<f64>> = self
            .onset_buf
            .iter()
            .enumerate()
            .map(|(i, &s)| Complex::new(s * self.window[i], 0.0))
            .collect();

        // Run FFT
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        fft.process(&mut fft_input);

        // Compute magnitudes
        let magnitudes: Vec<f64> = fft_input.iter().map(|c| c.norm()).collect();

        // Search for peak in the expected frequency range
        let (search_lo, search_hi) = self.search_range;
        let bin_lo = ((search_lo * FFT_SIZE as f64 / self.sample_rate) as usize).max(1);
        let bin_hi =
            ((search_hi * FFT_SIZE as f64 / self.sample_rate) as usize).min(FFT_SIZE / 2 - 1);

        if bin_lo >= bin_hi {
            self.resonant_freq = (search_lo + search_hi) * 0.5;
            self.setup_bandpass();
            return;
        }

        // Find peak bin
        let mut peak_bin = bin_lo;
        let mut peak_mag = 0.0;
        for bin in bin_lo..=bin_hi {
            if magnitudes[bin] > peak_mag {
                peak_mag = magnitudes[bin];
                peak_bin = bin;
            }
        }

        // Parabolic interpolation for sub-bin accuracy
        let refined = if peak_bin > bin_lo && peak_bin < bin_hi {
            let alpha = magnitudes[peak_bin - 1];
            let beta = magnitudes[peak_bin];
            let gamma = magnitudes[peak_bin + 1];
            let denom = alpha - 2.0 * beta + gamma;
            if denom.abs() > 1e-10 {
                peak_bin as f64 + 0.5 * (alpha - gamma) / denom
            } else {
                peak_bin as f64
            }
        } else {
            peak_bin as f64
        };

        self.resonant_freq = refined * self.sample_rate / FFT_SIZE as f64;
        self.setup_bandpass();
    }

    /// Configure the bandpass filter at the detected resonant frequency.
    fn setup_bandpass(&mut self) {
        let freq = self.resonant_freq.clamp(20.0, self.sample_rate * 0.45);
        self.bandpass.set(
            FilterType::Bandpass,
            freq,
            self.resonance_q,
            self.sample_rate,
        );
        self.bandpass.reset();
        self.decay_env.reset(0.0);
    }

    /// Compute adaptive hold and release times from the measured decay rate.
    fn compute_adaptive_times(&mut self) {
        // Estimate T60 (time for -60dB decay)
        let t60_ms = if self.measured_decay_rate < -0.01 {
            (-60.0 / self.measured_decay_rate).clamp(20.0, 3000.0)
        } else {
            500.0 // fallback: moderate decay
        };

        // Sensitivity: 0 = use full T60 as hold, 1 = tight follow (30% of T60)
        let hold_fraction = 1.0 - self.decay_sensitivity * 0.7; // 1.0 to 0.3
        let release_fraction = 0.3 + self.decay_sensitivity * 0.2; // 0.3 to 0.5

        let natural_hold = t60_ms * hold_fraction;
        let natural_release = t60_ms * release_fraction;

        self.computed_hold_ms = natural_hold.clamp(0.0, 2000.0);
        self.computed_release_ms = natural_release.clamp(self.min_release_ms, self.max_release_ms);
    }

    /// Get the estimated resonant frequency (Hz). Zero if not yet measured.
    pub fn resonant_freq(&self) -> f64 {
        self.resonant_freq
    }

    /// Whether the tracker is currently measuring a decay.
    pub fn is_active(&self) -> bool {
        self.state != DecayState::Idle
    }

    pub fn reset(&mut self) {
        self.state = DecayState::Idle;
        self.onset_buf_pos = 0;
        self.onset_amplitude = 0.0;
        self.samples_since_onset = 0;
        self.measured_decay_rate = 0.0;
        self.resonant_freq = 0.0;
        self.bandpass.reset();
        self.decay_env.reset(0.0);
        self.search_range = (0.0, 0.0);
    }
}

impl Default for AdaptiveDecayTracker {
    fn default() -> Self {
        Self::new()
    }
}
