//! FTS Rider — nih-plug entry point with full DSP bridge and Dioxus GUI.

use atomic_float::AtomicF32;
use fts_plugin_core::prelude::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use fts_dsp::{AudioConfig, Processor};
use rider_dsp::detector::DetectMode;
use rider_dsp::RiderChain;

mod editor;

// ── Shared UI State ──────────────────────────────────────────────────

/// Audio-thread → UI metering data.
pub struct RiderUiState {
    pub params: Arc<FtsRiderParams>,
    /// Current gain ride amount in dB (positive = boosting, negative = cutting).
    pub gain_db: AtomicF32,
    /// Peak input level in dB.
    pub input_peak_db: AtomicF32,
    /// Peak output level in dB.
    pub output_peak_db: AtomicF32,
    /// Detected level in dB.
    pub level_db: AtomicF32,
    /// Waveform history: input peaks (0.0–1.0 normalized), ring buffer.
    pub waveform_input: Box<[AtomicF32]>,
    /// Waveform history: gain ride (0.0=max cut, 0.5=unity, 1.0=max boost), ring buffer.
    pub waveform_gain: Box<[AtomicF32]>,
    /// Write position into waveform ring buffers.
    pub waveform_pos: AtomicF32,
}

/// Number of waveform history entries.
pub const WAVEFORM_LEN: usize = 200;

impl RiderUiState {
    fn new(params: Arc<FtsRiderParams>) -> Self {
        let waveform_input: Box<[AtomicF32]> = (0..WAVEFORM_LEN)
            .map(|_| AtomicF32::new(0.0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let waveform_gain: Box<[AtomicF32]> = (0..WAVEFORM_LEN)
            .map(|_| AtomicF32::new(0.5))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            params,
            gain_db: AtomicF32::new(0.0),
            input_peak_db: AtomicF32::new(-100.0),
            output_peak_db: AtomicF32::new(-100.0),
            level_db: AtomicF32::new(-100.0),
            waveform_input,
            waveform_gain,
            waveform_pos: AtomicF32::new(0.0),
        }
    }
}

// ── Parameters ───────────────────────────────────────────────────────

#[derive(Params)]
pub struct FtsRiderParams {
    #[id = "target"]
    pub target_db: FloatParam,

    #[id = "range"]
    pub range_db: FloatParam,

    #[id = "speed"]
    pub speed_ms: FloatParam,

    #[id = "gate"]
    pub gate_db: FloatParam,

    #[id = "sc_freq"]
    pub sc_freq: FloatParam,

    #[id = "detect_mode"]
    pub detect_mode: FloatParam,

    #[id = "sc_listen"]
    pub sc_listen: FloatParam,

    #[id = "output_gain"]
    pub output_gain_db: FloatParam,
}

impl Default for FtsRiderParams {
    fn default() -> Self {
        Self {
            target_db: FloatParam::new(
                "Target",
                -18.0,
                FloatRange::Linear {
                    min: -40.0,
                    max: -6.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            range_db: FloatParam::new(
                "Range",
                12.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            speed_ms: FloatParam::new(
                "Speed",
                50.0,
                FloatRange::Skewed {
                    min: 10.0,
                    max: 300.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            gate_db: FloatParam::new(
                "Gate",
                -50.0,
                FloatRange::Linear {
                    min: -70.0,
                    max: -20.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            sc_freq: FloatParam::new(
                "SC HPF",
                80.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 500.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            detect_mode: FloatParam::new(
                "Mode",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_step_size(1.0)
            .with_value_to_string(Arc::new(|v| {
                if v > 0.5 {
                    "LUFS".to_string()
                } else {
                    "RMS".to_string()
                }
            }))
            .with_string_to_value(Arc::new(|s| match s.trim().to_lowercase().as_str() {
                "lufs" | "k" | "1" => Some(1.0),
                "rms" | "0" => Some(0.0),
                _ => s.parse().ok(),
            })),

            sc_listen: FloatParam::new(
                "SC Listen",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_step_size(1.0)
            .with_value_to_string(Arc::new(|v| {
                if v > 0.5 {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }))
            .with_string_to_value(Arc::new(|s| match s.trim().to_lowercase().as_str() {
                "on" | "1" | "true" => Some(1.0),
                "off" | "0" | "false" => Some(0.0),
                _ => s.parse().ok(),
            })),

            output_gain_db: FloatParam::new(
                "Output",
                0.0,
                FloatRange::Linear {
                    min: -12.0,
                    max: 12.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
        }
    }
}

// ── Plugin ───────────────────────────────────────────────────────────

struct FtsRider {
    params: Arc<FtsRiderParams>,
    ui_state: Arc<RiderUiState>,
    editor_state: Arc<DioxusState>,
    chain: RiderChain,
    sample_rate: f64,
    /// Counter for waveform decimation.
    waveform_counter: usize,
    /// Samples between waveform writes (~50 updates/sec at 48kHz).
    waveform_interval: usize,
    /// Accumulated peak for current waveform interval.
    waveform_peak: f32,
    /// Accumulated gain for current waveform interval.
    waveform_gain_peak: f32,
}

impl Default for FtsRider {
    fn default() -> Self {
        let params = Arc::new(FtsRiderParams::default());
        let ui_state = Arc::new(RiderUiState::new(params.clone()));
        Self {
            params,
            ui_state,
            editor_state: DioxusState::new(|| (700, 480)),
            chain: RiderChain::new(),
            sample_rate: 48000.0,
            waveform_counter: 0,
            waveform_interval: 960, // ~50 Hz at 48kHz
            waveform_peak: 0.0,
            waveform_gain_peak: 0.0,
        }
    }
}

impl FtsRider {
    /// Sync nih-plug params → rider-dsp parameters.
    fn sync_params(&mut self) {
        let speed = self.params.speed_ms.value() as f64;
        self.chain.rider.attack_ms = speed * 0.4;
        self.chain.rider.release_ms = speed * 1.6;
        self.chain.rider.detector.window_ms = speed;

        self.chain.set_target_db(self.params.target_db.value() as f64);
        self.chain.set_range_db(self.params.range_db.value() as f64);
        self.chain
            .rider
            .activity_threshold_db = self.params.gate_db.value() as f64;

        let sc_freq = self.params.sc_freq.value() as f64;
        self.chain.set_sidechain_freq(sc_freq);

        let mode = if self.params.detect_mode.value() > 0.5 {
            DetectMode::KWeighted
        } else {
            DetectMode::Rms
        };
        self.chain.set_detect_mode(mode);

        self.chain.sc_listen = self.params.sc_listen.value() > 0.5;

        self.chain.rider.update(self.sample_rate);
    }
}

impl Plugin for FtsRider {
    const NAME: &'static str = "FTS Rider";
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
        self.chain.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: buffer_config.max_buffer_size as usize,
        });
        true
    }

    fn reset(&mut self) {
        self.chain.reset();
        self.waveform_counter = 0;
        self.waveform_peak = 0.0;
        self.waveform_gain_peak = 0.0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.sync_params();

        let output_gain_lin =
            fts_dsp::db::db_to_linear(self.params.output_gain_db.value() as f64);
        let range = self.params.range_db.value().max(0.01);

        for mut frame in buffer.iter_samples() {
            let mut channels = frame.iter_mut();
            let left_ref = channels.next().unwrap();
            let right_ref = channels.next().unwrap();

            let mut left = *left_ref as f64;
            let mut right = *right_ref as f64;

            // Track input peak
            let input_peak = left.abs().max(right.abs()) as f32;

            // Process through rider chain
            self.chain.process(&mut [left], &mut [right]);

            // Apply output gain
            left *= output_gain_lin;
            right *= output_gain_lin;

            *left_ref = left as f32;
            *right_ref = right as f32;

            // Track output peak
            let output_peak = left.abs().max(right.abs()) as f32;

            // Update metering atomics
            let gain = self.chain.gain_db() as f32;
            self.ui_state.gain_db.store(gain, Ordering::Relaxed);
            self.ui_state
                .level_db
                .store(self.chain.level_db() as f32, Ordering::Relaxed);

            // Exponential peak decay for smooth metering
            let prev_in = self.ui_state.input_peak_db.load(Ordering::Relaxed);
            let in_db = if input_peak > 0.0 {
                20.0 * input_peak.log10()
            } else {
                -100.0
            };
            let new_in = if in_db > prev_in {
                in_db
            } else {
                prev_in - 0.3
            };
            self.ui_state.input_peak_db.store(new_in, Ordering::Relaxed);

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

            // Waveform history accumulation
            self.waveform_peak = self.waveform_peak.max(input_peak);
            // Map gain_db to 0.0–1.0: 0.5 = unity, 0.0 = max cut, 1.0 = max boost
            let gain_norm = (gain / range + 0.5).clamp(0.0, 1.0);
            self.waveform_gain_peak = if (gain_norm - 0.5).abs() > (self.waveform_gain_peak - 0.5).abs() {
                gain_norm
            } else {
                self.waveform_gain_peak
            };

            self.waveform_counter += 1;
            if self.waveform_counter >= self.waveform_interval {
                let pos =
                    self.ui_state.waveform_pos.load(Ordering::Relaxed) as usize % WAVEFORM_LEN;
                self.ui_state.waveform_input[pos]
                    .store(self.waveform_peak.min(1.0), Ordering::Relaxed);
                self.ui_state.waveform_gain[pos]
                    .store(self.waveform_gain_peak, Ordering::Relaxed);
                self.ui_state
                    .waveform_pos
                    .store((pos + 1) as f32, Ordering::Relaxed);

                self.waveform_counter = 0;
                self.waveform_peak = 0.0;
                self.waveform_gain_peak = 0.5;
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsRider {
    const CLAP_ID: &'static str = "com.fasttrackstudio.rider";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Vocal rider with automatic level control");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Utility,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for FtsRider {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsRiderPlug0001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nih_export_clap!(FtsRider);
nih_export_vst3!(FtsRider);
