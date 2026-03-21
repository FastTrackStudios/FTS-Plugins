//! FTS Trigger — 8-slot drum replacement plugin.
//!
//! Slate Trigger 2-inspired drum replacer with transient detection,
//! 8 sample slots, per-slot gain/pan/mute/solo, and velocity mapping.

use atomic_float::AtomicF32;
use fts_plugin_core::prelude::*;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use fts_dsp::AudioConfig;
use trigger_dsp::detector::{DetectAlgorithm, DetectMode};
use trigger_dsp::sampler::MixMode;
use trigger_dsp::velocity::VelocityCurve;

mod editor;
mod engine;
mod loader;

use engine::{TriggerEngine, NUM_SLOTS};
use loader::SampleLoadMessage;

// ── Constants ───────────────────────────────────────────────────────

pub const WAVEFORM_LEN: usize = 200;

// ── Shared UI State ─────────────────────────────────────────────────

/// Audio-thread → UI metering data.
pub struct TriggerUiState {
    pub params: Arc<FtsTriggerParams>,
    pub last_velocity: AtomicF32,
    pub detector_level_db: AtomicF32,
    pub triggered: AtomicF32,
    pub input_peak_db: AtomicF32,
    pub output_peak_db: AtomicF32,
    pub waveform_input: Box<[AtomicF32]>,
    pub waveform_triggers: Box<[AtomicF32]>,
    pub waveform_pos: AtomicF32,
    pub slot_playing: [AtomicF32; NUM_SLOTS],
    pub slot_peak_db: [AtomicF32; NUM_SLOTS],
    pub slot_names: [Mutex<String>; NUM_SLOTS],
    pub sample_tx: crossbeam_channel::Sender<SampleLoadMessage>,
    pub sample_rate: AtomicF32,
}

impl TriggerUiState {
    fn new(
        params: Arc<FtsTriggerParams>,
        tx: crossbeam_channel::Sender<SampleLoadMessage>,
    ) -> Self {
        let waveform_input: Box<[AtomicF32]> = (0..WAVEFORM_LEN)
            .map(|_| AtomicF32::new(0.0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let waveform_triggers: Box<[AtomicF32]> = (0..WAVEFORM_LEN)
            .map(|_| AtomicF32::new(0.0))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            params,
            last_velocity: AtomicF32::new(0.0),
            detector_level_db: AtomicF32::new(-100.0),
            triggered: AtomicF32::new(0.0),
            input_peak_db: AtomicF32::new(-100.0),
            output_peak_db: AtomicF32::new(-100.0),
            waveform_input,
            waveform_triggers,
            waveform_pos: AtomicF32::new(0.0),
            slot_playing: std::array::from_fn(|_| AtomicF32::new(0.0)),
            slot_peak_db: std::array::from_fn(|_| AtomicF32::new(-100.0)),
            slot_names: std::array::from_fn(|_| Mutex::new(String::new())),
            sample_tx: tx,
            sample_rate: AtomicF32::new(48000.0),
        }
    }
}

// ── Per-Slot Parameters ─────────────────────────────────────────────

#[derive(Params)]
pub struct SlotParams {
    #[id = "gain"]
    pub gain: FloatParam,

    #[id = "pan"]
    pub pan: FloatParam,

    #[id = "pitch"]
    pub pitch: FloatParam,

    #[id = "enabled"]
    pub enabled: FloatParam,

    #[id = "mute"]
    pub mute: FloatParam,

    #[id = "solo"]
    pub solo: FloatParam,
}

impl SlotParams {
    fn new(idx: usize) -> Self {
        Self {
            gain: FloatParam::new(
                &format!("S{} Gain", idx + 1),
                0.0,
                FloatRange::Linear {
                    min: -60.0,
                    max: 12.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            pan: FloatParam::new(
                &format!("S{} Pan", idx + 1),
                0.0,
                FloatRange::Linear {
                    min: -1.0,
                    max: 1.0,
                },
            )
            .with_value_to_string(Arc::new(|v| {
                if v < -0.01 {
                    format!("{:.0}L", -v * 100.0)
                } else if v > 0.01 {
                    format!("{:.0}R", v * 100.0)
                } else {
                    "C".to_string()
                }
            })),

            pitch: FloatParam::new(
                &format!("S{} Pitch", idx + 1),
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" st")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            enabled: FloatParam::new(
                &format!("S{} On", idx + 1),
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(Arc::new(|v| {
                if v > 0.5 { "On".to_string() } else { "Off".to_string() }
            }))
            .with_string_to_value(Arc::new(|s| match s.trim().to_lowercase().as_str() {
                "on" | "1" | "true" => Some(1.0),
                "off" | "0" | "false" => Some(0.0),
                _ => s.parse().ok(),
            })),

            mute: FloatParam::new(
                &format!("S{} Mute", idx + 1),
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(Arc::new(|v| {
                if v > 0.5 { "M".to_string() } else { "-".to_string() }
            }))
            .with_string_to_value(Arc::new(|s| match s.trim().to_lowercase().as_str() {
                "m" | "1" | "true" => Some(1.0),
                "-" | "0" | "false" => Some(0.0),
                _ => s.parse().ok(),
            })),

            solo: FloatParam::new(
                &format!("S{} Solo", idx + 1),
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(Arc::new(|v| {
                if v > 0.5 { "S".to_string() } else { "-".to_string() }
            }))
            .with_string_to_value(Arc::new(|s| match s.trim().to_lowercase().as_str() {
                "s" | "1" | "true" => Some(1.0),
                "-" | "0" | "false" => Some(0.0),
                _ => s.parse().ok(),
            })),
        }
    }
}

// ── Persisted Slot Paths ────────────────────────────────────────────

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct SlotPaths {
    pub paths: [Option<String>; NUM_SLOTS],
}

// ── Plugin Parameters ───────────────────────────────────────────────

#[derive(Params)]
pub struct FtsTriggerParams {
    // Detection
    #[id = "threshold"]
    pub threshold: FloatParam,

    #[id = "sensitivity"]
    pub sensitivity: FloatParam,

    #[id = "retrigger"]
    pub retrigger: FloatParam,

    #[id = "release_ratio"]
    pub release_ratio: FloatParam,

    #[id = "release_time"]
    pub release_time: FloatParam,

    #[id = "reactivity"]
    pub reactivity: FloatParam,

    #[id = "detect_mode"]
    pub detect_mode: IntParam,

    #[id = "detect_algorithm"]
    pub detect_algorithm: IntParam,

    // Sidechain
    #[id = "sc_hpf"]
    pub sc_hpf: FloatParam,

    #[id = "sc_lpf"]
    pub sc_lpf: FloatParam,

    #[id = "sc_listen"]
    pub sc_listen: FloatParam,

    // Velocity
    #[id = "dynamics"]
    pub dynamics: FloatParam,

    #[id = "vel_curve"]
    pub vel_curve: IntParam,

    // Mix/Output
    #[id = "mix_mode"]
    pub mix_mode: IntParam,

    #[id = "mix_amount"]
    pub mix_amount: FloatParam,

    #[id = "output_gain"]
    pub output_gain: FloatParam,

    // Per-slot (8 slots)
    #[nested(array, group = "Slot {}")]
    pub slots: [SlotParams; NUM_SLOTS],

    // Persisted sample paths
    #[persist = "slot_paths"]
    pub slot_paths: Arc<Mutex<SlotPaths>>,
}

impl Default for FtsTriggerParams {
    fn default() -> Self {
        Self {
            threshold: FloatParam::new(
                "Threshold",
                -30.0,
                FloatRange::Linear {
                    min: -60.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            sensitivity: FloatParam::new(
                "Sensitivity",
                1.0,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 10.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            retrigger: FloatParam::new(
                "Retrigger",
                10.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 200.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            release_ratio: FloatParam::new(
                "Hysteresis",
                0.5,
                FloatRange::Linear {
                    min: 0.1,
                    max: 0.9,
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            release_time: FloatParam::new(
                "Release",
                5.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 50.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            reactivity: FloatParam::new(
                "Reactivity",
                10.0,
                FloatRange::Skewed {
                    min: 0.5,
                    max: 250.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            detect_mode: IntParam::new("Mode", 0, IntRange::Linear { min: 0, max: 1 })
                .with_value_to_string(Arc::new(|v| match v {
                    0 => "Peak".to_string(),
                    1 => "RMS".to_string(),
                    _ => "Peak".to_string(),
                })),

            detect_algorithm: IntParam::new(
                "Algorithm",
                0,
                IntRange::Linear { min: 0, max: 6 },
            )
            .with_value_to_string(Arc::new(|v| match v {
                0 => "Live".to_string(),
                1 => "Flux".to_string(),
                2 => "SuperFlux".to_string(),
                3 => "HFC".to_string(),
                4 => "Complex".to_string(),
                5 => "RectComplex".to_string(),
                6 => "Mod KL".to_string(),
                _ => "Live".to_string(),
            })),

            // Sidechain
            sc_hpf: FloatParam::new(
                "SC HPF",
                0.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 1000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(Arc::new(|v| {
                if v < 1.0 {
                    "Off".to_string()
                } else {
                    format!("{:.0}", v)
                }
            })),

            sc_lpf: FloatParam::new(
                "SC LPF",
                0.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 20000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(Arc::new(|v| {
                if v < 1.0 {
                    "Off".to_string()
                } else if v >= 1000.0 {
                    format!("{:.1}k", v / 1000.0)
                } else {
                    format!("{:.0}", v)
                }
            })),

            sc_listen: FloatParam::new(
                "SC Listen",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(Arc::new(|v| {
                if v > 0.5 { "On".to_string() } else { "Off".to_string() }
            }))
            .with_string_to_value(Arc::new(|s| match s.trim().to_lowercase().as_str() {
                "on" | "1" | "true" => Some(1.0),
                "off" | "0" | "false" => Some(0.0),
                _ => s.parse().ok(),
            })),

            // Velocity
            dynamics: FloatParam::new(
                "Dynamics",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            vel_curve: IntParam::new("Curve", 0, IntRange::Linear { min: 0, max: 3 })
                .with_value_to_string(Arc::new(|v| match v {
                    0 => "Linear".to_string(),
                    1 => "Log".to_string(),
                    2 => "Exp".to_string(),
                    3 => "Fixed".to_string(),
                    _ => "Linear".to_string(),
                })),

            // Mix/Output
            mix_mode: IntParam::new("Mix", 0, IntRange::Linear { min: 0, max: 2 })
                .with_value_to_string(Arc::new(|v| match v {
                    0 => "Replace".to_string(),
                    1 => "Layer".to_string(),
                    2 => "Blend".to_string(),
                    _ => "Replace".to_string(),
                })),

            mix_amount: FloatParam::new(
                "Blend",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            output_gain: FloatParam::new(
                "Output",
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            // Slots
            slots: std::array::from_fn(|i| SlotParams::new(i)),

            // Persisted paths
            slot_paths: Arc::new(Mutex::new(SlotPaths::default())),
        }
    }
}

// ── Plugin ──────────────────────────────────────────────────────────

struct FtsTrigger {
    params: Arc<FtsTriggerParams>,
    ui_state: Arc<TriggerUiState>,
    editor_state: Arc<DioxusState>,
    engine: TriggerEngine,
    sample_rate: f64,
    sample_rx: crossbeam_channel::Receiver<SampleLoadMessage>,

    // Waveform decimation
    waveform_counter: usize,
    waveform_interval: usize,
    waveform_peak: f32,
    waveform_trigger_peak: f32,
}

impl Default for FtsTrigger {
    fn default() -> Self {
        let params = Arc::new(FtsTriggerParams::default());
        let (tx, rx) = crossbeam_channel::bounded(32);
        let ui_state = Arc::new(TriggerUiState::new(params.clone(), tx));
        Self {
            params,
            ui_state,
            editor_state: DioxusState::new(|| (1060, 640)),
            engine: TriggerEngine::new(),
            sample_rate: 48000.0,
            sample_rx: rx,
            waveform_counter: 0,
            waveform_interval: 960,
            waveform_peak: 0.0,
            waveform_trigger_peak: 0.0,
        }
    }
}

impl FtsTrigger {
    fn sync_params(&mut self) {
        let e = &mut self.engine;

        // Detection
        e.threshold_db = self.params.threshold.value() as f64;
        e.detect_time_ms = self.params.sensitivity.value() as f64;
        e.retrigger_ms = self.params.retrigger.value() as f64;
        e.release_ratio = self.params.release_ratio.value() as f64;
        e.release_time_ms = self.params.release_time.value() as f64;
        e.reactivity_ms = self.params.reactivity.value() as f64;
        e.detect_mode = match self.params.detect_mode.value() {
            1 => DetectMode::Rms,
            _ => DetectMode::Peak,
        };
        e.detect_algorithm = match self.params.detect_algorithm.value() {
            1 => DetectAlgorithm::SpectralFlux,
            2 => DetectAlgorithm::SuperFlux,
            3 => DetectAlgorithm::Hfc,
            4 => DetectAlgorithm::ComplexDomain,
            5 => DetectAlgorithm::RectifiedComplexDomain,
            6 => DetectAlgorithm::ModifiedKl,
            _ => DetectAlgorithm::PeakEnvelope,
        };

        // Sidechain
        e.sc_hpf_freq = self.params.sc_hpf.value() as f64;
        e.sc_lpf_freq = self.params.sc_lpf.value() as f64;
        e.sc_listen = self.params.sc_listen.value() > 0.5;

        // Velocity
        e.dynamics = self.params.dynamics.value() as f64;
        e.velocity_curve = match self.params.vel_curve.value() {
            1 => VelocityCurve::Logarithmic,
            2 => VelocityCurve::Exponential,
            3 => VelocityCurve::Fixed,
            _ => VelocityCurve::Linear,
        };

        // Mix/Output
        e.mix_mode = match self.params.mix_mode.value() {
            1 => MixMode::Layer,
            2 => MixMode::Blend,
            _ => MixMode::Replace,
        };
        e.mix_amount = self.params.mix_amount.value() as f64;
        e.output_gain = fts_dsp::db::db_to_linear(self.params.output_gain.value() as f64);

        // Per-slot
        for s in 0..NUM_SLOTS {
            let sp = &self.params.slots[s];
            e.slot_gain[s] = fts_dsp::db::db_to_linear(sp.gain.value() as f64);
            e.slot_pan[s] = sp.pan.value() as f64;
            e.slot_enabled[s] = sp.enabled.value() > 0.5;
            e.slot_mute[s] = sp.mute.value() > 0.5;
            e.slot_solo[s] = sp.solo.value() > 0.5;

            // Pitch: semitones to playback rate
            let pitch_st = sp.pitch.value() as f64;
            e.slot_pitch[s] = 2.0_f64.powf(pitch_st / 12.0);
            // Apply pitch to each slot's sampler layers
            for layer in &mut e.slots[s].layers {
                for sample in &mut layer.samples {
                    sample.playback_rate = e.slot_pitch[s];
                }
            }
        }

        e.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: 512,
        });
    }

    fn poll_loaded_samples(&mut self) {
        while let Ok(msg) = self.sample_rx.try_recv() {
            if msg.slot < NUM_SLOTS {
                self.engine.slots[msg.slot].set_single_sample(msg.sample);
                if let Ok(mut name) = self.ui_state.slot_names[msg.slot].lock() {
                    *name = msg.name;
                }
            }
        }
    }
}

impl Plugin for FtsTrigger {
    const NAME: &'static str = "FTS Trigger";
    const VENDOR: &'static str = "FastTrackStudio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        create_dioxus_editor_with_state(
            self.editor_state.clone(),
            self.ui_state.clone(),
            editor::App,
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate as f64;
        self.waveform_interval = (buffer_config.sample_rate as usize) / 50;
        self.ui_state
            .sample_rate
            .store(buffer_config.sample_rate, Ordering::Relaxed);

        self.engine.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: buffer_config.max_buffer_size as usize,
        });

        // Reload samples from persisted paths
        if let Ok(paths) = self.params.slot_paths.lock() {
            for (slot, path) in paths.paths.iter().enumerate() {
                if let Some(ref p) = path {
                    let tx = self.ui_state.sample_tx.clone();
                    let sr = self.sample_rate;
                    loader::load_sample_async(p.clone(), slot, sr, tx);
                }
            }
        }

        true
    }

    fn reset(&mut self) {
        self.engine.reset();
        self.waveform_counter = 0;
        self.waveform_peak = 0.0;
        self.waveform_trigger_peak = 0.0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Poll for loaded samples
        self.poll_loaded_samples();

        // Sync params
        self.sync_params();

        // Report latency to host (non-zero for spectral modes)
        let latency = self.engine.detector.latency_samples() as u32;
        _context.set_latency_samples(latency);

        // Process in blocks — convert f32 <-> f64
        for mut frame in buffer.iter_samples() {
            let mut channels = frame.iter_mut();
            let left_ref = channels.next().unwrap();
            let right_ref = channels.next().unwrap();

            let mut left = *left_ref as f64;
            let mut right = *right_ref as f64;

            let input_peak = left.abs().max(right.abs()) as f32;

            // Process one sample through the engine
            let mut left_buf = [left];
            let mut right_buf = [right];
            self.engine.process(&mut left_buf, &mut right_buf);
            left = left_buf[0];
            right = right_buf[0];

            *left_ref = left as f32;
            *right_ref = right as f32;

            let output_peak = left.abs().max(right.abs()) as f32;

            // Update metering atomics
            // Input peak with decay
            let prev_in = self.ui_state.input_peak_db.load(Ordering::Relaxed);
            let in_db = if input_peak > 0.0 {
                20.0 * input_peak.log10()
            } else {
                -100.0
            };
            let new_in = if in_db > prev_in { in_db } else { prev_in - 0.3 };
            self.ui_state
                .input_peak_db
                .store(new_in, Ordering::Relaxed);

            // Output peak with decay
            let prev_out = self.ui_state.output_peak_db.load(Ordering::Relaxed);
            let out_db = if output_peak > 0.0 {
                20.0 * output_peak.log10()
            } else {
                -100.0
            };
            let new_out = if out_db > prev_out {
                out_db
            } else {
                prev_out - 0.3
            };
            self.ui_state
                .output_peak_db
                .store(new_out, Ordering::Relaxed);

            // Detector level
            let det_db = self.engine.detector.level_db() as f32;
            self.ui_state
                .detector_level_db
                .store(det_db, Ordering::Relaxed);

            // Trigger flash
            if self.engine.triggered_this_block {
                self.ui_state.triggered.store(1.0, Ordering::Relaxed);
                self.ui_state
                    .last_velocity
                    .store(self.engine.last_velocity as f32, Ordering::Relaxed);
            }

            // Slot metering
            for s in 0..NUM_SLOTS {
                let peak = self.engine.slot_peak[s] as f32;
                let peak_db = if peak > 0.0 {
                    20.0 * peak.log10()
                } else {
                    -100.0
                };
                self.ui_state.slot_peak_db[s].store(peak_db, Ordering::Relaxed);
                self.ui_state.slot_playing[s].store(
                    if self.engine.slots[s].is_playing() {
                        1.0
                    } else {
                        0.0
                    },
                    Ordering::Relaxed,
                );
            }

            // Waveform history
            self.waveform_peak = self.waveform_peak.max(input_peak);
            if self.engine.triggered_this_block {
                self.waveform_trigger_peak = 1.0;
            }
            self.waveform_counter += 1;
            if self.waveform_counter >= self.waveform_interval {
                let pos =
                    self.ui_state.waveform_pos.load(Ordering::Relaxed) as usize % WAVEFORM_LEN;
                self.ui_state.waveform_input[pos]
                    .store(self.waveform_peak.min(1.0), Ordering::Relaxed);
                self.ui_state.waveform_triggers[pos]
                    .store(self.waveform_trigger_peak, Ordering::Relaxed);
                self.ui_state
                    .waveform_pos
                    .store((pos + 1) as f32, Ordering::Relaxed);

                self.waveform_counter = 0;
                self.waveform_peak = 0.0;
                self.waveform_trigger_peak = 0.0;
            }
        }

        // Decay trigger flash
        let prev = self.ui_state.triggered.load(Ordering::Relaxed);
        if prev > 0.0 {
            self.ui_state
                .triggered
                .store((prev - 0.02).max(0.0), Ordering::Relaxed);
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsTrigger {
    const CLAP_ID: &'static str = "com.fasttrackstudio.trigger";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Drum trigger with transient detection and sample playback");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Utility,
        ClapFeature::Drum,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for FtsTrigger {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsTrigPlugin001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Dynamics,
        Vst3SubCategory::Instrument,
    ];
}

nih_export_clap!(FtsTrigger);
nih_export_vst3!(FtsTrigger);
