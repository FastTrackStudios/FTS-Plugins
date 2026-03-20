//! RiderChain: complete rider processing chain.
//!
//! Signal flow: Input → Sidechain HPF → Level Detector → Gain Calculator →
//! Smoothing → Gain Stage → Output.

use eq_dsp::filter_type::FilterStructure;
use eq_dsp::{Band, FilterType};
use fts_dsp::db::db_to_linear;
use fts_dsp::{AudioConfig, Processor};

use crate::detector::DetectMode;
use crate::rider::GainRider;

// r[impl rider.chain.signal-flow]
// r[impl rider.detector.sidechain]
/// Complete vocal rider processing chain.
///
/// Wraps [`GainRider`] with optional sidechain high-pass filtering
/// and implements the [`Processor`] trait for plug-in integration.
pub struct RiderChain {
    /// The gain rider engine.
    pub rider: GainRider,

    /// Sidechain HPF — filters the detection signal, not the output.
    sc_hpf_l: Band,
    sc_hpf_r: Band,

    /// Sidechain HPF frequency in Hz (0 = disabled).
    sc_hpf_freq: f64,

    /// When true, output the sidechain-filtered signal instead of riding.
    pub sc_listen: bool,

    config: AudioConfig,
}

fn make_sc_hpf() -> Band {
    let mut hpf = Band::new();
    hpf.filter_type = FilterType::Highpass;
    hpf.structure = FilterStructure::Tdf2;
    hpf.freq_hz = 80.0;
    hpf.q = 0.707;
    hpf.order = 2;
    hpf.enabled = false;
    hpf
}

impl RiderChain {
    pub fn new() -> Self {
        Self {
            rider: GainRider::new(),
            sc_hpf_l: make_sc_hpf(),
            sc_hpf_r: make_sc_hpf(),
            sc_hpf_freq: 0.0,
            sc_listen: false,
            config: AudioConfig {
                sample_rate: 48000.0,
                max_buffer_size: 512,
            },
        }
    }

    /// Set the sidechain HPF frequency. 0 disables the filter.
    pub fn set_sidechain_freq(&mut self, freq: f64) {
        self.sc_hpf_freq = freq;
        let enabled = freq > 0.0;
        self.sc_hpf_l.enabled = enabled;
        self.sc_hpf_r.enabled = enabled;
        if enabled {
            self.sc_hpf_l.freq_hz = freq;
            self.sc_hpf_r.freq_hz = freq;
            self.sc_hpf_l.update(self.config);
            self.sc_hpf_r.update(self.config);
        }
    }

    /// Set detection mode (RMS or K-Weighted).
    pub fn set_detect_mode(&mut self, mode: DetectMode) {
        self.rider.detector.mode = mode;
    }

    /// Set target level in dB.
    pub fn set_target_db(&mut self, db: f64) {
        self.rider.target_db = db;
    }

    /// Set gain range symmetrically (e.g., 12.0 means +/-12 dB).
    pub fn set_range_db(&mut self, range: f64) {
        self.rider.max_boost_db = range;
        self.rider.max_cut_db = range;
    }

    /// Get current gain in dB for metering.
    pub fn gain_db(&self) -> f64 {
        self.rider.gain_db()
    }

    /// Get current detected level in dB for metering.
    pub fn level_db(&self) -> f64 {
        self.rider.level_db()
    }
}

impl Default for RiderChain {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for RiderChain {
    fn reset(&mut self) {
        self.rider.reset();
        self.sc_hpf_l.reset();
        self.sc_hpf_r.reset();
    }

    fn update(&mut self, config: AudioConfig) {
        self.config = config;
        self.rider.update(config.sample_rate);
        if self.sc_hpf_l.enabled {
            self.sc_hpf_l.update(config);
            self.sc_hpf_r.update(config);
        }
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        let use_sc = self.sc_hpf_l.enabled;

        for i in 0..left.len().min(right.len()) {
            let (det_l, det_r) = if use_sc {
                (
                    self.sc_hpf_l.tick(left[i], 0),
                    self.sc_hpf_r.tick(right[i], 1),
                )
            } else {
                (left[i], right[i])
            };

            if self.sc_listen {
                left[i] = det_l;
                right[i] = det_r;
                continue;
            }

            // Feed detection signal to rider, get gain
            let gain_db = self.rider.tick(det_l, det_r);
            let gain_lin = db_to_linear(gain_db);

            // Apply gain to original (unfiltered) signal
            left[i] *= gain_lin;
            right[i] *= gain_lin;
        }
    }
}
