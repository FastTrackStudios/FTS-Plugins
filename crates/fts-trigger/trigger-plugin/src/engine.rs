//! TriggerEngine — 8-slot sample trigger engine.
//!
//! Wraps the trigger-dsp detection subsystem (detector + velocity + sidechain filters)
//! with 8 independent `Sampler` instances. All slots fire from a single transient
//! detector, with per-slot gain, pan, mute, and solo.

use eq_dsp::band::Band;
use eq_dsp::filter_type::{FilterStructure, FilterType};
use fts_dsp::db::db_to_linear;
use fts_dsp::AudioConfig;
use trigger_dsp::detector::{DetectAlgorithm, DetectMode, TriggerDetector};
use trigger_dsp::sampler::{MixMode, Sampler};
use trigger_dsp::velocity::{VelocityCurve, VelocityMapper};

/// Number of sample slots.
pub const NUM_SLOTS: usize = 8;

/// 8-slot drum trigger engine.
pub struct TriggerEngine {
    // Detection subsystem
    pub detector: TriggerDetector,
    pub velocity: VelocityMapper,
    sc_hpf: Band,
    sc_lpf: Band,

    // 8 sample slots
    pub slots: [Sampler; NUM_SLOTS],
    pub slot_gain: [f64; NUM_SLOTS],
    pub slot_pan: [f64; NUM_SLOTS],
    pub slot_enabled: [bool; NUM_SLOTS],
    pub slot_mute: [bool; NUM_SLOTS],
    pub slot_solo: [bool; NUM_SLOTS],
    pub slot_pitch: [f64; NUM_SLOTS],

    // Global params (synced from plugin)
    pub threshold_db: f64,
    pub release_ratio: f64,
    pub detect_time_ms: f64,
    pub release_time_ms: f64,
    pub retrigger_ms: f64,
    pub reactivity_ms: f64,
    pub detect_mode: DetectMode,
    pub detect_algorithm: DetectAlgorithm,
    pub dynamics: f64,
    pub velocity_curve: VelocityCurve,
    pub mix_mode: MixMode,
    pub mix_amount: f64,
    pub output_gain: f64,
    pub sc_hpf_freq: f64,
    pub sc_lpf_freq: f64,
    pub sc_listen: bool,

    // Metering
    pub last_velocity: f64,
    pub triggered_this_block: bool,
    pub slot_peak: [f64; NUM_SLOTS],

    config: AudioConfig,
}

impl TriggerEngine {
    pub fn new() -> Self {
        let mut sc_hpf = Band::new();
        sc_hpf.filter_type = FilterType::Highpass;
        sc_hpf.structure = FilterStructure::Tdf2;
        sc_hpf.freq_hz = 100.0;
        sc_hpf.q = 0.707;
        sc_hpf.order = 2;
        sc_hpf.enabled = false;

        let mut sc_lpf = Band::new();
        sc_lpf.filter_type = FilterType::Lowpass;
        sc_lpf.structure = FilterStructure::Tdf2;
        sc_lpf.freq_hz = 10000.0;
        sc_lpf.q = 0.707;
        sc_lpf.order = 2;
        sc_lpf.enabled = false;

        Self {
            detector: TriggerDetector::new(),
            velocity: VelocityMapper::new(),
            sc_hpf,
            sc_lpf,
            slots: std::array::from_fn(|_| Sampler::new()),
            slot_gain: [1.0; NUM_SLOTS],
            slot_pan: [0.0; NUM_SLOTS],
            slot_enabled: [true; NUM_SLOTS],
            slot_mute: [false; NUM_SLOTS],
            slot_solo: [false; NUM_SLOTS],
            slot_pitch: [1.0; NUM_SLOTS],
            threshold_db: -30.0,
            release_ratio: 0.5,
            detect_time_ms: 1.0,
            release_time_ms: 5.0,
            retrigger_ms: 10.0,
            reactivity_ms: 10.0,
            detect_mode: DetectMode::Peak,
            detect_algorithm: DetectAlgorithm::PeakEnvelope,
            dynamics: 0.5,
            velocity_curve: VelocityCurve::Linear,
            mix_mode: MixMode::Replace,
            mix_amount: 1.0,
            output_gain: 1.0,
            sc_hpf_freq: 0.0,
            sc_lpf_freq: 0.0,
            sc_listen: false,
            last_velocity: 0.0,
            triggered_this_block: false,
            slot_peak: [0.0; NUM_SLOTS],
            config: AudioConfig {
                sample_rate: 48000.0,
                max_buffer_size: 512,
            },
        }
    }

    pub fn update(&mut self, config: AudioConfig) {
        self.config = config;

        // Detector
        self.detector.detect_threshold_db = self.threshold_db;
        self.detector.release_ratio = self.release_ratio;
        self.detector.detect_time_ms = self.detect_time_ms;
        self.detector.release_time_ms = self.release_time_ms;
        self.detector.retrigger_ms = self.retrigger_ms;
        self.detector.reactivity_ms = self.reactivity_ms;
        self.detector.mode = self.detect_mode;
        self.detector.algorithm = self.detect_algorithm;
        self.detector.update(config.sample_rate);

        // Velocity
        self.velocity.dynamics = self.dynamics;
        self.velocity.curve = self.velocity_curve;

        // Samplers
        for slot in &mut self.slots {
            slot.set_sample_rate(config.sample_rate);
            // Each slot uses Replace mode internally — engine handles mix
            slot.mix_mode = MixMode::Replace;
            slot.mix_amount = 1.0;
        }

        // Sidechain filters
        if self.sc_hpf_freq > 0.0 {
            self.sc_hpf.enabled = true;
            self.sc_hpf.freq_hz = self.sc_hpf_freq;
            self.sc_hpf.update(config);
        } else {
            self.sc_hpf.enabled = false;
        }
        if self.sc_lpf_freq > 0.0 {
            self.sc_lpf.enabled = true;
            self.sc_lpf.freq_hz = self.sc_lpf_freq;
            self.sc_lpf.update(config);
        } else {
            self.sc_lpf.enabled = false;
        }
    }

    pub fn reset(&mut self) {
        self.detector.reset();
        for slot in &mut self.slots {
            slot.reset();
        }
        self.sc_hpf.reset();
        self.sc_lpf.reset();
        self.last_velocity = 0.0;
        self.triggered_this_block = false;
        self.slot_peak = [0.0; NUM_SLOTS];
    }

    /// Process a stereo buffer in-place.
    pub fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        self.triggered_this_block = false;
        self.slot_peak = [0.0; NUM_SLOTS];
        let detect_threshold_gain = db_to_linear(self.threshold_db);
        let any_solo = self.slot_solo.iter().any(|&s| s);

        for i in 0..left.len() {
            let dry_l = left[i];
            let dry_r = right[i];

            // Sidechain path: filter for detection
            let mut sc_l = left[i];
            let mut sc_r = right[i];

            if self.sc_hpf.enabled {
                sc_l = self.sc_hpf.tick(sc_l, 0);
                sc_r = self.sc_hpf.tick(sc_r, 1);
            }
            if self.sc_lpf.enabled {
                sc_l = self.sc_lpf.tick(sc_l, 0);
                sc_r = self.sc_lpf.tick(sc_r, 1);
            }

            // Sidechain listen mode
            if self.sc_listen {
                left[i] = sc_l;
                right[i] = sc_r;
                continue;
            }

            // Detection (mono sum)
            let sc_mono = (sc_l + sc_r) * 0.5;

            // Check for trigger
            if let Some(peak_level) = self.detector.tick(sc_mono) {
                let vel = self.velocity.map(peak_level, detect_threshold_gain);
                self.last_velocity = vel;
                self.triggered_this_block = true;

                // Fire all eligible slots
                for s in 0..NUM_SLOTS {
                    if !self.slot_enabled[s] || self.slot_mute[s] {
                        continue;
                    }
                    if any_solo && !self.slot_solo[s] {
                        continue;
                    }
                    self.slots[s].trigger(vel);
                }
            }

            // Sum all slot outputs with per-slot gain + pan
            let mut wet_l = 0.0;
            let mut wet_r = 0.0;

            for s in 0..NUM_SLOTS {
                if !self.slot_enabled[s] || self.slot_mute[s] {
                    // Still tick to advance voice positions
                    let _ = self.slots[s].tick(0.0, 0.0);
                    continue;
                }
                if any_solo && !self.slot_solo[s] {
                    let _ = self.slots[s].tick(0.0, 0.0);
                    continue;
                }

                let (sl, sr) = self.slots[s].tick(0.0, 0.0);
                let gain = self.slot_gain[s];

                // Constant-power pan
                let pan = self.slot_pan[s].clamp(-1.0, 1.0);
                let pan_angle = (pan + 1.0) * 0.25 * std::f64::consts::PI;
                let pan_l = pan_angle.cos();
                let pan_r = pan_angle.sin();

                let out_l = sl * gain * pan_l;
                let out_r = sr * gain * pan_r;

                wet_l += out_l;
                wet_r += out_r;

                // Per-slot peak metering
                let peak = out_l.abs().max(out_r.abs());
                if peak > self.slot_peak[s] {
                    self.slot_peak[s] = peak;
                }
            }

            // Mix wet/dry
            let (out_l, out_r) = match self.mix_mode {
                MixMode::Replace => (wet_l, wet_r),
                MixMode::Layer => (dry_l + wet_l, dry_r + wet_r),
                MixMode::Blend => {
                    let wet = self.mix_amount;
                    let dry = 1.0 - wet;
                    (dry_l * dry + wet_l * wet, dry_r * dry + wet_r * wet)
                }
            };

            // Output gain
            left[i] = out_l * self.output_gain;
            right[i] = out_r * self.output_gain;
        }
    }
}

impl Default for TriggerEngine {
    fn default() -> Self {
        Self::new()
    }
}
