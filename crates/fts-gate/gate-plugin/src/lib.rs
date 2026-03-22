//! FTS Gate — nih-plug entry point with full DSP bridge and Dioxus GUI.
//!
//! Features: timbre-aware drum classification, adaptive resonant decay,
//! and phase-locked multi-instance alignment.

use atomic_float::AtomicF32;
use fts_plugin_core::prelude::*;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use fts_dsp::{AudioConfig, Processor};
use gate_dsp::classifier::DrumClass;
use gate_dsp::GateChain;

mod editor;

// ── Shared UI State ──────────────────────────────────────────────────

pub struct GateUiState {
    pub params: Arc<FtsGateParams>,
    pub gate_gain: AtomicF32,
    pub input_peak_db: AtomicF32,
    pub output_peak_db: AtomicF32,
    pub drum_class: AtomicU8,
    pub adaptive_hold_ms: AtomicF32,
    pub adaptive_release_ms: AtomicF32,
    pub resonant_freq: AtomicF32,
}

impl GateUiState {
    fn new(params: Arc<FtsGateParams>) -> Self {
        Self {
            params,
            gate_gain: AtomicF32::new(0.0),
            input_peak_db: AtomicF32::new(-100.0),
            output_peak_db: AtomicF32::new(-100.0),
            drum_class: AtomicU8::new(255),
            adaptive_hold_ms: AtomicF32::new(0.0),
            adaptive_release_ms: AtomicF32::new(0.0),
            resonant_freq: AtomicF32::new(0.0),
        }
    }
}

// ── Parameters ───────────────────────────────────────────────────────

#[derive(Params)]
pub struct FtsGateParams {
    #[id = "threshold"]
    pub threshold_db: FloatParam,
    #[id = "hysteresis"]
    pub hysteresis_db: FloatParam,
    #[id = "attack"]
    pub attack_ms: FloatParam,
    #[id = "hold"]
    pub hold_ms: FloatParam,
    #[id = "release"]
    pub release_ms: FloatParam,
    #[id = "range"]
    pub range_db: FloatParam,
    #[id = "lookahead"]
    pub lookahead_ms: FloatParam,
    #[id = "sc_hpf"]
    pub sc_hpf_freq: FloatParam,
    #[id = "sc_lpf"]
    pub sc_lpf_freq: FloatParam,
    #[id = "sc_listen"]
    pub sc_listen: FloatParam,
    #[id = "sc_source"]
    pub sc_source: FloatParam,
    #[id = "drum_target"]
    pub drum_target: FloatParam,
    #[id = "drum_strictness"]
    pub drum_strictness: FloatParam,
    #[id = "adaptive_decay"]
    pub adaptive_decay: FloatParam,
    #[id = "decay_sensitivity"]
    pub decay_sensitivity: FloatParam,
    #[id = "sync_enabled"]
    pub sync_enabled: FloatParam,
    #[id = "sync_max_align"]
    pub sync_max_align_ms: FloatParam,
}

fn bool_param(name: &str) -> FloatParam {
    FloatParam::new(name, 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
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
        }))
}

impl Default for FtsGateParams {
    fn default() -> Self {
        Self {
            threshold_db: FloatParam::new(
                "Threshold",
                -40.0,
                FloatRange::Linear {
                    min: -80.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            hysteresis_db: FloatParam::new(
                "Hysteresis",
                10.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 40.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            attack_ms: FloatParam::new(
                "Attack",
                0.5,
                FloatRange::Skewed {
                    min: 0.01,
                    max: 100.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            hold_ms: FloatParam::new(
                "Hold",
                50.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            release_ms: FloatParam::new(
                "Release",
                100.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            range_db: FloatParam::new(
                "Range",
                -80.0,
                FloatRange::Linear {
                    min: -80.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            lookahead_ms: FloatParam::new(
                "Lookahead",
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 20.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            sc_hpf_freq: FloatParam::new(
                "SC HPF",
                0.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            sc_lpf_freq: FloatParam::new(
                "SC LPF",
                0.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 20000.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            sc_listen: bool_param("SC Listen"),
            sc_source: FloatParam::new("SC Source", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_step_size(1.0)
                .with_value_to_string(Arc::new(|v| {
                    if v > 0.5 {
                        "External".to_string()
                    } else {
                        "Internal".to_string()
                    }
                }))
                .with_string_to_value(Arc::new(|s| match s.trim().to_lowercase().as_str() {
                    "internal" | "int" | "0" => Some(0.0),
                    "external" | "ext" | "1" => Some(1.0),
                    _ => s.parse().ok(),
                })),
            drum_target: FloatParam::new("Target", 0.0, FloatRange::Linear { min: 0.0, max: 5.0 })
                .with_step_size(1.0)
                .with_value_to_string(Arc::new(|v| match v.round() as i32 {
                    1 => "Kick".to_string(),
                    2 => "Snare".to_string(),
                    3 => "Hi-Hat".to_string(),
                    4 => "Tom".to_string(),
                    5 => "Guitar".to_string(),
                    _ => "Off".to_string(),
                }))
                .with_string_to_value(Arc::new(|s| match s.trim().to_lowercase().as_str() {
                    "off" | "0" => Some(0.0),
                    "kick" | "1" => Some(1.0),
                    "snare" | "2" => Some(2.0),
                    "hi-hat" | "hihat" | "hat" | "3" => Some(3.0),
                    "tom" | "4" => Some(4.0),
                    "guitar" | "gtr" | "di" | "5" => Some(5.0),
                    _ => None,
                })),
            drum_strictness: FloatParam::new(
                "Strictness",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0)),
            adaptive_decay: bool_param("Adaptive Decay"),
            decay_sensitivity: FloatParam::new(
                "Decay Sens",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0)),
            sync_enabled: bool_param("Sync"),
            sync_max_align_ms: FloatParam::new(
                "Max Align",
                5.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 10.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
        }
    }
}

// ── Plugin ───────────────────────────────────────────────────────────

struct FtsGate {
    params: Arc<FtsGateParams>,
    ui_state: Arc<GateUiState>,
    editor_state: Arc<DioxusState>,
    chain: GateChain,
    sample_rate: f64,
}

impl Default for FtsGate {
    fn default() -> Self {
        let params = Arc::new(FtsGateParams::default());
        let ui_state = Arc::new(GateUiState::new(params.clone()));
        Self {
            params,
            ui_state,
            editor_state: DioxusState::new(|| (800, 520)),
            chain: GateChain::new(),
            sample_rate: 48000.0,
        }
    }
}

fn param_to_drum_class(v: f32) -> DrumClass {
    match v.round() as i32 {
        1 => DrumClass::Kick,
        2 => DrumClass::Snare,
        3 => DrumClass::HiHat,
        4 => DrumClass::Tom,
        5 => DrumClass::Guitar,
        _ => DrumClass::Unknown,
    }
}

fn drum_class_to_u8(c: DrumClass) -> u8 {
    match c {
        DrumClass::Kick => 0,
        DrumClass::Snare => 1,
        DrumClass::HiHat => 2,
        DrumClass::Tom => 3,
        DrumClass::Guitar => 4,
        DrumClass::Unknown => 255,
    }
}

impl FtsGate {
    fn sync_params(&mut self) {
        let threshold = self.params.threshold_db.value() as f64;
        let hysteresis = self.params.hysteresis_db.value() as f64;
        self.chain.open_threshold_db = threshold;
        self.chain.close_threshold_db = threshold - hysteresis;
        self.chain.attack_ms = self.params.attack_ms.value() as f64;
        self.chain.hold_ms = self.params.hold_ms.value() as f64;
        self.chain.release_ms = self.params.release_ms.value() as f64;
        self.chain.range_db = self.params.range_db.value() as f64;
        self.chain.lookahead_ms = self.params.lookahead_ms.value() as f64;
        self.chain.sc_listen = self.params.sc_listen.value() > 0.5;
        self.chain
            .set_sc_hpf(self.params.sc_hpf_freq.value() as f64);
        self.chain
            .set_sc_lpf(self.params.sc_lpf_freq.value() as f64);

        let target = param_to_drum_class(self.params.drum_target.value());
        self.chain.classifier.enabled = target != DrumClass::Unknown;
        self.chain.classifier.target_drum = target;
        self.chain.classifier.strictness = self.params.drum_strictness.value() as f64;

        self.chain.decay_tracker.enabled = self.params.adaptive_decay.value() > 0.5;
        self.chain.decay_tracker.decay_sensitivity = self.params.decay_sensitivity.value() as f64;

        self.chain.aligner.enabled = self.params.sync_enabled.value() > 0.5;
        self.chain.aligner.max_alignment_ms = self.params.sync_max_align_ms.value() as f64;

        self.chain.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: 512,
        });
    }
}

impl Plugin for FtsGate {
    const NAME: &'static str = "FTS Gate";
    const VENDOR: &'static str = "FastTrackStudio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        // Stereo with sidechain input
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[new_nonzero_u32(2)],
            ..AudioIOLayout::const_default()
        },
        // Stereo without sidechain (fallback)
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
    ];
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
        context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate as f64;
        self.chain.aligner.join_session("fts-gate-default");
        self.chain.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: buffer_config.max_buffer_size as usize,
        });
        context.set_latency_samples(self.chain.latency_samples() as u32);
        true
    }

    fn reset(&mut self) {
        self.chain.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.sync_params();
        context.set_latency_samples(self.chain.latency_samples() as u32);

        let use_external_sc = self.params.sc_source.value() > 0.5
            && !aux.inputs.is_empty()
            && aux.inputs[0].samples() > 0;

        // Pre-read sidechain slices (avoids borrow conflicts in the sample loop)
        let sc_slices: Option<(&[f32], &[f32])> = if use_external_sc {
            let sc = aux.inputs[0].as_slice_immutable();
            let sc_l = sc.get(0).map(|s| &**s).unwrap_or(&[]);
            let sc_r = sc.get(1).map(|s| &**s).unwrap_or(sc_l);
            Some((sc_l, sc_r))
        } else {
            None
        };

        for (sample_idx, mut frame) in buffer.iter_samples().enumerate() {
            let mut channels = frame.iter_mut();
            let left_ref = channels.next().unwrap();
            let right_ref = channels.next().unwrap();

            let input_peak = (*left_ref as f64).abs().max((*right_ref as f64).abs()) as f32;

            let mut l_buf = [*left_ref as f64];
            let mut r_buf = [*right_ref as f64];

            if let Some((sc_l_buf, sc_r_buf)) = sc_slices {
                let sc_l = *sc_l_buf.get(sample_idx).unwrap_or(&0.0) as f64;
                let sc_r = *sc_r_buf.get(sample_idx).unwrap_or(&0.0) as f64;
                self.chain
                    .process_with_sidechain(&mut l_buf, &mut r_buf, &[sc_l], &[sc_r]);
            } else {
                self.chain.process(&mut l_buf, &mut r_buf);
            }

            *left_ref = l_buf[0] as f32;
            *right_ref = r_buf[0] as f32;

            let output_peak = l_buf[0].abs().max(r_buf[0].abs()) as f32;

            self.ui_state
                .gate_gain
                .store(self.chain.last_gain[0] as f32, Ordering::Relaxed);
            self.ui_state.drum_class.store(
                drum_class_to_u8(self.chain.last_drum_class),
                Ordering::Relaxed,
            );
            self.ui_state.adaptive_hold_ms.store(
                self.chain.decay_tracker.computed_hold_ms as f32,
                Ordering::Relaxed,
            );
            self.ui_state.adaptive_release_ms.store(
                self.chain.decay_tracker.computed_release_ms as f32,
                Ordering::Relaxed,
            );
            self.ui_state.resonant_freq.store(
                self.chain.decay_tracker.resonant_freq() as f32,
                Ordering::Relaxed,
            );

            let prev_in = self.ui_state.input_peak_db.load(Ordering::Relaxed);
            let in_db = if input_peak > 0.0 {
                20.0 * input_peak.log10()
            } else {
                -100.0
            };
            self.ui_state.input_peak_db.store(
                if in_db > prev_in {
                    in_db
                } else {
                    prev_in - 0.3
                },
                Ordering::Relaxed,
            );

            let prev_out = self.ui_state.output_peak_db.load(Ordering::Relaxed);
            let out_db = if output_peak > 0.0 {
                20.0 * output_peak.log10()
            } else {
                -100.0
            };
            self.ui_state.output_peak_db.store(
                if out_db > prev_out {
                    out_db
                } else {
                    prev_out - 0.3
                },
                Ordering::Relaxed,
            );
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsGate {
    const CLAP_ID: &'static str = "com.fasttrackstudio.gate";
    const CLAP_DESCRIPTION: Option<&'static str> = Some(
        "Drum gate with timbre classification, adaptive resonant decay, and multi-instance sync",
    );
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Utility,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for FtsGate {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsGatePlugin001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nih_export_clap!(FtsGate);
nih_export_vst3!(FtsGate);
