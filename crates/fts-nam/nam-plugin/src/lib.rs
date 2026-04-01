//! FTS NAM — nih-plug entry point with dual-slot NAM processing, noise gate,
//! and Dioxus GUI.

use atomic_float::AtomicF32;
use fts_plugin_core::prelude::*;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use fts_dsp::{AudioConfig, Processor};
use gate_dsp::GateChain;
use nam_dsp::chain::NamChain;

mod editor;

// ── Model Loading ────────────────────────────────────────────────────

/// Message sent from the UI/loader thread to the audio thread.
pub struct NamLoadMessage {
    /// 0 = slot A, 1 = slot B, 2 = IR A, 3 = IR B.
    pub slot: usize,
    /// Absolute path to the file.
    pub path: String,
    /// Display name (filename without directory).
    pub name: String,
}

// ── Shared UI State ──────────────────────────────────────────────────

/// Audio-thread → UI metering data, plus channels for model loading.
pub struct NamUiState {
    pub params: Arc<FtsNamParams>,
    /// Peak input level in dB.
    pub input_peak_db: AtomicF32,
    /// Peak output level in dB.
    pub output_peak_db: AtomicF32,
    /// Current latency in samples.
    pub latency_samples: AtomicF32,
    /// Current gate gain (0.0 = closed, 1.0 = open).
    pub gate_gain: AtomicF32,
    /// Sender for model/IR load requests (UI → audio thread).
    pub load_tx: crossbeam_channel::Sender<NamLoadMessage>,
    /// Display names for loaded models.
    pub slot_a_name: Mutex<String>,
    pub slot_b_name: Mutex<String>,
    pub ir_a_name: Mutex<String>,
    pub ir_b_name: Mutex<String>,
}

impl NamUiState {
    fn new(params: Arc<FtsNamParams>, tx: crossbeam_channel::Sender<NamLoadMessage>) -> Self {
        Self {
            params,
            input_peak_db: AtomicF32::new(-100.0),
            output_peak_db: AtomicF32::new(-100.0),
            latency_samples: AtomicF32::new(0.0),
            gate_gain: AtomicF32::new(0.0),
            load_tx: tx,
            slot_a_name: Mutex::new(String::new()),
            slot_b_name: Mutex::new(String::new()),
            ir_a_name: Mutex::new(String::new()),
            ir_b_name: Mutex::new(String::new()),
        }
    }
}

// ── Persisted Model Paths ────────────────────────────────────────────

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct NamModelPaths {
    pub model_a: Option<String>,
    pub model_b: Option<String>,
    pub ir_a: Option<String>,
    pub ir_b: Option<String>,
}

// ── Parameters ───────────────────────────────────────────────────────

#[derive(Params)]
pub struct FtsNamParams {
    // ── NAM ──
    #[id = "blend"]
    pub blend: FloatParam,

    #[id = "ir_mix"]
    pub ir_mix: FloatParam,

    #[id = "output_gain"]
    pub output_gain_db: FloatParam,

    #[id = "input_gain"]
    pub input_gain_db: FloatParam,

    #[id = "delta_delay"]
    pub delta_delay_samples: FloatParam,

    // ── Gate ──
    #[id = "gate_threshold"]
    pub gate_threshold_db: FloatParam,

    #[id = "gate_hysteresis"]
    pub gate_hysteresis_db: FloatParam,

    #[id = "gate_attack"]
    pub gate_attack_ms: FloatParam,

    #[id = "gate_hold"]
    pub gate_hold_ms: FloatParam,

    #[id = "gate_release"]
    pub gate_release_ms: FloatParam,

    #[id = "gate_range"]
    pub gate_range_db: FloatParam,

    #[id = "gate_sc_hpf"]
    pub gate_sc_hpf_freq: FloatParam,

    #[id = "gate_sc_lpf"]
    pub gate_sc_lpf_freq: FloatParam,

    /// 0.0 = gate off, 1.0 = gate on.
    #[id = "gate_enabled"]
    pub gate_enabled: FloatParam,

    /// 0.0 = internal (main input), 1.0 = external (sidechain input).
    #[id = "gate_sc_source"]
    pub gate_sc_source: FloatParam,

    /// Persisted file paths for DAW session recall.
    #[persist = "model_paths"]
    pub model_paths: Arc<Mutex<NamModelPaths>>,
}

impl Default for FtsNamParams {
    fn default() -> Self {
        Self {
            blend: FloatParam::new("Blend", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            ir_mix: FloatParam::new("IR Mix", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            output_gain_db: FloatParam::new(
                "Output",
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            input_gain_db: FloatParam::new(
                "Input",
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            delta_delay_samples: FloatParam::new(
                "Delta Delay",
                0.0,
                FloatRange::Linear {
                    min: -512.0,
                    max: 512.0,
                },
            )
            .with_unit(" smp")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            // ── Gate defaults ──
            gate_threshold_db: FloatParam::new(
                "Gate Thresh",
                -40.0,
                FloatRange::Linear {
                    min: -80.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            gate_hysteresis_db: FloatParam::new(
                "Gate Hyst",
                10.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 40.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            gate_attack_ms: FloatParam::new(
                "Gate Attack",
                0.5,
                FloatRange::Skewed {
                    min: 0.01,
                    max: 100.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            gate_hold_ms: FloatParam::new(
                "Gate Hold",
                50.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            gate_release_ms: FloatParam::new(
                "Gate Release",
                100.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            gate_range_db: FloatParam::new(
                "Gate Range",
                -80.0,
                FloatRange::Linear {
                    min: -80.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            gate_sc_hpf_freq: FloatParam::new(
                "Gate SC HPF",
                0.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            gate_sc_lpf_freq: FloatParam::new(
                "Gate SC LPF",
                0.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 20000.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            gate_enabled: FloatParam::new("Gate", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
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

            gate_sc_source: FloatParam::new(
                "Gate SC Src",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(Arc::new(|v| {
                if v > 0.5 {
                    "Ext".to_string()
                } else {
                    "Int".to_string()
                }
            }))
            .with_string_to_value(Arc::new(|s| {
                match s.trim().to_lowercase().as_str() {
                    "ext" | "external" | "1" => Some(1.0),
                    "int" | "internal" | "0" => Some(0.0),
                    _ => s.parse().ok(),
                }
            })),

            model_paths: Arc::new(Mutex::new(NamModelPaths::default())),
        }
    }
}

// ── Plugin ───────────────────────────────────────────────────────────

struct FtsNam {
    params: Arc<FtsNamParams>,
    ui_state: Arc<NamUiState>,
    editor_state: Arc<DioxusState>,
    chain: NamChain,
    gate: GateChain,
    sample_rate: f64,
    load_rx: crossbeam_channel::Receiver<NamLoadMessage>,
    /// Pre-allocated scratch buffers to avoid allocating on the audio thread.
    scratch_left: Vec<f64>,
    scratch_right: Vec<f64>,
    /// Scratch buffers for external sidechain gate detection.
    scratch_sc_left: Vec<f64>,
    scratch_sc_right: Vec<f64>,
}

impl Default for FtsNam {
    fn default() -> Self {
        let params = Arc::new(FtsNamParams::default());
        let (tx, rx) = crossbeam_channel::bounded(16);
        let ui_state = Arc::new(NamUiState::new(params.clone(), tx));
        Self {
            params,
            ui_state,
            editor_state: DioxusState::new(|| (700, 520)),
            chain: NamChain::new(),
            gate: GateChain::new(),
            sample_rate: 48000.0,
            load_rx: rx,
            scratch_left: Vec::new(),
            scratch_right: Vec::new(),
            scratch_sc_left: Vec::new(),
            scratch_sc_right: Vec::new(),
        }
    }
}

impl FtsNam {
    /// Sync nih-plug params → nam-dsp parameters.
    fn sync_params(&mut self) {
        // NAM chain
        self.chain.blend = self.params.blend.value() as f64;
        self.chain.ir_mix = self.params.ir_mix.value() as f64;
        self.chain.output_gain = 10.0_f64.powf(self.params.output_gain_db.value() as f64 / 20.0);
        self.chain.delta_delay_samples = self.params.delta_delay_samples.value() as f64;

        let input_gain = 10.0_f64.powf(self.params.input_gain_db.value() as f64 / 20.0);
        self.chain.slot_a.input_gain = input_gain;
        self.chain.slot_b.input_gain = input_gain;

        // Gate
        let threshold = self.params.gate_threshold_db.value() as f64;
        let hysteresis = self.params.gate_hysteresis_db.value() as f64;
        self.gate.open_threshold_db = threshold;
        self.gate.close_threshold_db = threshold - hysteresis;
        self.gate.attack_ms = self.params.gate_attack_ms.value() as f64;
        self.gate.hold_ms = self.params.gate_hold_ms.value() as f64;
        self.gate.release_ms = self.params.gate_release_ms.value() as f64;
        self.gate.range_db = self.params.gate_range_db.value() as f64;
        self.gate.lookahead_ms = 0.0; // no lookahead — keep zero latency for the gate
        self.gate.sc_listen = false;

        self.gate
            .set_sc_hpf(self.params.gate_sc_hpf_freq.value() as f64);
        self.gate
            .set_sc_lpf(self.params.gate_sc_lpf_freq.value() as f64);

        self.gate.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: 512,
        });
    }

    /// Poll for model/IR load messages from the UI thread.
    fn poll_loaded_models(&mut self) {
        while let Ok(msg) = self.load_rx.try_recv() {
            match msg.slot {
                0 => {
                    // Model A
                    if let Err(e) = self.chain.slot_a.load(&msg.path) {
                        nih_log!("NAM: Failed to load model A: {e}");
                    } else {
                        self.chain
                            .slot_a
                            .update(self.sample_rate, self.chain.latency().max(8192));
                        if let Ok(mut name) = self.ui_state.slot_a_name.lock() {
                            *name = msg.name;
                        }
                    }
                }
                1 => {
                    // Model B
                    if let Err(e) = self.chain.slot_b.load(&msg.path) {
                        nih_log!("NAM: Failed to load model B: {e}");
                    } else {
                        self.chain
                            .slot_b
                            .update(self.sample_rate, self.chain.latency().max(8192));
                        if let Ok(mut name) = self.ui_state.slot_b_name.lock() {
                            *name = msg.name;
                        }
                    }
                }
                2 => {
                    // IR A
                    let ir = load_ir_file(&msg.path, self.sample_rate);
                    if let Some(samples) = ir {
                        self.chain.ir_a.load_ir(&samples, self.sample_rate);
                        if let Ok(mut name) = self.ui_state.ir_a_name.lock() {
                            *name = msg.name;
                        }
                    } else {
                        nih_log!("NAM: Failed to load IR A: {}", msg.path);
                    }
                }
                3 => {
                    // IR B
                    let ir = load_ir_file(&msg.path, self.sample_rate);
                    if let Some(samples) = ir {
                        self.chain.ir_b.load_ir(&samples, self.sample_rate);
                        if let Ok(mut name) = self.ui_state.ir_b_name.lock() {
                            *name = msg.name;
                        }
                    } else {
                        nih_log!("NAM: Failed to load IR B: {}", msg.path);
                    }
                }
                _ => {}
            }
        }
    }

    /// Send a load message for a given slot/path.
    fn request_load(tx: &crossbeam_channel::Sender<NamLoadMessage>, slot: usize, path: String) {
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let _ = tx.try_send(NamLoadMessage { slot, path, name });
    }
}

/// Load an IR file as mono f64 samples using fts-sample's Symphonium loader.
fn load_ir_file(path: &str, sample_rate: f64) -> Option<Vec<f64>> {
    let audio = fts_sample::load_audio(std::path::Path::new(path), sample_rate).ok()?;
    // Take left channel only (IRs are mono)
    let samples: Vec<f64> = audio.data.iter().map(|frame| frame[0]).collect();
    if samples.is_empty() {
        None
    } else {
        Some(samples)
    }
}

impl Plugin for FtsNam {
    const NAME: &'static str = "FTS NAM";
    const VENDOR: &'static str = "FastTrackStudio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        // Layout with sidechain input
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[new_nonzero_u32(2)],
            ..AudioIOLayout::const_default()
        },
        // Fallback layout without sidechain
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
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate as f64;
        let max_buf = buffer_config.max_buffer_size as usize;
        self.chain.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: max_buf,
        });
        self.gate.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: max_buf,
        });

        // Pre-allocate scratch buffers
        self.scratch_left.resize(max_buf, 0.0);
        self.scratch_right.resize(max_buf, 0.0);
        self.scratch_sc_left.resize(max_buf, 0.0);
        self.scratch_sc_right.resize(max_buf, 0.0);

        // Reload models/IRs from persisted paths
        if let Ok(paths) = self.params.model_paths.lock() {
            let tx = &self.ui_state.load_tx;
            if let Some(ref p) = paths.model_a {
                Self::request_load(tx, 0, p.clone());
            }
            if let Some(ref p) = paths.model_b {
                Self::request_load(tx, 1, p.clone());
            }
            if let Some(ref p) = paths.ir_a {
                Self::request_load(tx, 2, p.clone());
            }
            if let Some(ref p) = paths.ir_b {
                Self::request_load(tx, 3, p.clone());
            }
        }

        true
    }

    fn reset(&mut self) {
        self.chain.reset();
        self.gate.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Poll for model/IR loads
        self.poll_loaded_models();

        self.sync_params();

        // Report latency to host for PDC (plugin delay compensation)
        let latency = self.chain.latency() as u32;
        _context.set_latency_samples(latency);

        let num_samples = buffer.samples();
        if num_samples == 0 {
            return ProcessStatus::Normal;
        }

        // Ensure scratch buffers are large enough (no-op after initialize)
        if self.scratch_left.len() < num_samples {
            self.scratch_left.resize(num_samples, 0.0);
            self.scratch_right.resize(num_samples, 0.0);
            self.scratch_sc_left.resize(num_samples, 0.0);
            self.scratch_sc_right.resize(num_samples, 0.0);
        }

        let gate_enabled = self.params.gate_enabled.value() > 0.5;
        let sc_external = self.params.gate_sc_source.value() > 0.5;
        let has_sidechain = !aux.inputs.is_empty() && aux.inputs[0].samples() > 0;

        let left_f64 = &mut self.scratch_left[..num_samples];
        let right_f64 = &mut self.scratch_right[..num_samples];

        // Read input and track peak
        let mut input_peak: f32 = 0.0;
        for (i, mut frame) in buffer.iter_samples().enumerate() {
            let l = *frame.get_mut(0).unwrap();
            let r = *frame.get_mut(1).unwrap();
            left_f64[i] = l as f64;
            right_f64[i] = r as f64;
            input_peak = input_peak.max(l.abs().max(r.abs()));
        }

        // ── Gate ────────────────────────────────────────────────
        if gate_enabled {
            if sc_external && has_sidechain {
                // External sidechain: detect on SC signal, apply gain to main.
                // Copy sidechain into scratch SC buffers.
                let sc_left = &mut self.scratch_sc_left[..num_samples];
                let sc_right = &mut self.scratch_sc_right[..num_samples];
                for (i, mut frame) in aux.inputs[0].iter_samples().enumerate() {
                    sc_left[i] = *frame.get_mut(0).unwrap() as f64;
                    sc_right[i] = if frame.len() > 1 {
                        *frame.get_mut(1).unwrap() as f64
                    } else {
                        sc_left[i]
                    };
                }

                // Process sidechain through gate to derive gain envelope
                self.gate.process(sc_left, sc_right);

                // Apply the gate's gain to the main signal
                let gain = self.gate.last_gain;
                for i in 0..num_samples {
                    left_f64[i] *= gain[0];
                    right_f64[i] *= gain[1];
                }
            } else {
                // Internal sidechain: gate detects and applies on main signal
                self.gate.process(left_f64, right_f64);
            }
        }

        // ── NAM chain ──────────────────────────────────────────
        self.chain.process(left_f64, right_f64);

        // Write back and track output peak
        let mut output_peak: f32 = 0.0;
        for (i, mut frame) in buffer.iter_samples().enumerate() {
            let l = left_f64[i] as f32;
            let r = right_f64[i] as f32;
            *frame.get_mut(0).unwrap() = l;
            *frame.get_mut(1).unwrap() = r;
            output_peak = output_peak.max(l.abs().max(r.abs()));
        }

        // Update metering with exponential peak decay
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

        // Update gate gain for UI
        self.ui_state
            .gate_gain
            .store(self.gate.last_gain[0] as f32, Ordering::Relaxed);

        // Update UI latency display
        self.ui_state
            .latency_samples
            .store(latency as f32, Ordering::Relaxed);

        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsNam {
    const CLAP_ID: &'static str = "com.fasttrackstudio.nam";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Neural Amp Modeler with dual slots, IR convolution, and noise gate");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Custom("amp-simulator"),
    ];
}

impl Vst3Plugin for FtsNam {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsNamPlugin0001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx];
}

nih_export_clap!(FtsNam);
nih_export_vst3!(FtsNam);
