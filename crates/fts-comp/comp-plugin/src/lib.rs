//! FTS Compressor — nih-plug entry point with full DSP bridge and Dioxus GUI.

use atomic_float::AtomicF32;
use fts_plugin_core::prelude::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use comp_dsp_v2::chain::CompChain;
use fts_dsp::{AudioConfig, Processor};

pub mod editor;

// ── Shared UI State ──────────────────────────────────────────────────

/// Audio-thread → UI metering data.
pub struct CompUiState {
    pub params: Arc<FtsCompParams>,
    /// Current gain reduction in dB (positive = reducing).
    pub gain_reduction_db: AtomicF32,
    /// Peak input level in dB.
    pub input_peak_db: AtomicF32,
    /// Peak output level in dB.
    pub output_peak_db: AtomicF32,
    /// Waveform history: input peaks (0.0–1.0 normalized), ring buffer.
    pub waveform_input: Box<[AtomicF32]>,
    /// Waveform history: GR (0.0–1.0 normalized), ring buffer.
    pub waveform_gr: Box<[AtomicF32]>,
    /// Integer write position into waveform ring buffers.
    pub waveform_pos: AtomicF32,
    /// Fractional scroll phase: counter / interval (0.0–1.0).
    /// The renderer uses this to smoothly interpolate x-positions between
    /// data updates, giving sub-pixel accurate scrolling at any refresh rate.
    pub waveform_phase: AtomicF32,
}

/// Number of waveform history entries.
/// At 240 Hz updates: 960 entries ≈ 4 seconds of history.
pub const WAVEFORM_LEN: usize = 960;

impl CompUiState {
    pub fn new(params: Arc<FtsCompParams>) -> Self {
        let waveform_input: Box<[AtomicF32]> = (0..WAVEFORM_LEN)
            .map(|_| AtomicF32::new(0.0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let waveform_gr: Box<[AtomicF32]> = (0..WAVEFORM_LEN)
            .map(|_| AtomicF32::new(0.0))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            params,
            gain_reduction_db: AtomicF32::new(0.0),
            input_peak_db: AtomicF32::new(-100.0),
            output_peak_db: AtomicF32::new(-100.0),
            waveform_input,
            waveform_gr,
            waveform_pos: AtomicF32::new(0.0),
            waveform_phase: AtomicF32::new(0.0),
        }
    }
}

// ── Parameters ───────────────────────────────────────────────────────

#[derive(Params)]
pub struct FtsCompParams {
    #[id = "threshold"]
    pub threshold_db: FloatParam,

    #[id = "ratio"]
    pub ratio: FloatParam,

    #[id = "attack"]
    pub attack_ms: FloatParam,

    #[id = "release"]
    pub release_ms: FloatParam,

    #[id = "knee"]
    pub knee_db: FloatParam,

    #[id = "auto_makeup"]
    pub auto_makeup: FloatParam,

    #[id = "feedback"]
    pub feedback: FloatParam,

    #[id = "link"]
    pub channel_link: FloatParam,

    #[id = "inertia"]
    pub inertia: FloatParam,

    #[id = "inertia_decay"]
    pub inertia_decay: FloatParam,

    #[id = "ceiling"]
    pub ceiling: FloatParam,

    #[id = "mix"]
    pub fold: FloatParam,

    #[id = "input_gain"]
    pub input_gain_db: FloatParam,

    #[id = "output_gain"]
    pub output_gain_db: FloatParam,

    #[id = "sc_freq"]
    pub sidechain_freq: FloatParam,

    #[id = "range"]
    pub range_db: FloatParam,

    #[id = "hold"]
    pub hold_ms: FloatParam,

    #[id = "lookahead"]
    pub lookahead_ms: FloatParam,

    /// Read-only gain reduction output for host metering.
    #[id = "gr_out"]
    pub gr_output_db: FloatParam,
}

impl Default for FtsCompParams {
    fn default() -> Self {
        Self {
            threshold_db: FloatParam::new(
                "Threshold",
                -10.0,
                FloatRange::Linear {
                    min: -60.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            ratio: FloatParam::new(
                "Ratio",
                4.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 20.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(":1")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            attack_ms: FloatParam::new(
                "Attack",
                3.0,
                FloatRange::Skewed {
                    min: 0.005,
                    max: 300.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            release_ms: FloatParam::new(
                "Release",
                100.0,
                FloatRange::Skewed {
                    min: 10.0,
                    max: 3000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            knee_db: FloatParam::new(
                "Knee",
                6.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 72.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            auto_makeup: FloatParam::new(
                "Auto Gain",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(Arc::new(|v| {
                if v > 0.5 {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }))
            .with_string_to_value(Arc::new(|s| {
                match s.trim().to_lowercase().as_str() {
                    "on" | "1" | "true" => Some(1.0),
                    "off" | "0" | "false" => Some(0.0),
                    _ => s.parse().ok(),
                }
            })),

            feedback: FloatParam::new("Feedback", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            channel_link: FloatParam::new(
                "Stereo Link",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            inertia: FloatParam::new(
                "Inertia",
                0.0,
                FloatRange::Linear {
                    min: -1.0,
                    max: 0.3,
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            inertia_decay: FloatParam::new(
                "Inertia Decay",
                0.94,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            ceiling: FloatParam::new(
                "Ceiling",
                1.0,
                FloatRange::Linear {
                    min: 0.01,
                    max: 4.0,
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            fold: FloatParam::new("Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

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

            sidechain_freq: FloatParam::new(
                "SC HPF",
                85.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 300.0,
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            range_db: FloatParam::new(
                "Range",
                60.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 60.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            hold_ms: FloatParam::new(
                "Hold",
                0.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 500.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

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

            gr_output_db: FloatParam::new(
                "GR",
                0.0,
                FloatRange::Linear {
                    min: -60.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1))
            .non_automatable()
            .hide(),
        }
    }
}

// ── Plugin ───────────────────────────────────────────────────────────

struct FtsComp {
    params: Arc<FtsCompParams>,
    ui_state: Arc<CompUiState>,
    editor_state: Arc<DioxusState>,
    chain: CompChain,
    sample_rate: f64,
    /// Counter for waveform decimation.
    waveform_counter: usize,
    /// Samples between waveform writes (~50 updates/sec at 48kHz).
    waveform_interval: usize,
    /// Accumulated peak for current waveform interval.
    waveform_peak: f32,
    waveform_gr_peak: f32,
}

impl Default for FtsComp {
    fn default() -> Self {
        let params = Arc::new(FtsCompParams::default());
        let ui_state = Arc::new(CompUiState::new(params.clone()));
        Self {
            params,
            ui_state,
            editor_state: DioxusState::new(|| (900, 620)),
            chain: CompChain::new(),
            sample_rate: 48000.0,
            waveform_counter: 0,
            waveform_interval: 200, // ~240 Hz at 48kHz
            waveform_peak: 0.0,
            waveform_gr_peak: 0.0,
        }
    }
}

impl FtsComp {
    /// Sync nih-plug params → comp-dsp parameters.
    fn sync_params(&mut self) {
        static mut SYNC_COUNT: u64 = 0;
        unsafe {
            SYNC_COUNT += 1;
            if SYNC_COUNT == 1 {
                eprintln!(
                    "[PLUGIN] sync_params: threshold={}, attack={} ms, release={} ms",
                    self.params.threshold_db.value(),
                    self.params.attack_ms.value(),
                    self.params.release_ms.value()
                );
            }
        }

        let c = &mut self.chain.comp;
        c.set_threshold(self.params.threshold_db.value() as f64);
        c.set_ratio(self.params.ratio.value() as f64);
        c.set_attack_ms(self.params.attack_ms.value() as f64);
        c.set_release_ms(self.params.release_ms.value() as f64);
        c.set_knee(self.params.knee_db.value() as f64);
        c.auto_makeup = self.params.auto_makeup.value() > 0.5;
        c.feedback = self.params.feedback.value() as f64;
        c.channel_link = self.params.channel_link.value() as f64;
        c.inertia = self.params.inertia.value() as f64;
        c.inertia_decay = self.params.inertia_decay.value() as f64;
        c.ceiling = self.params.ceiling.value() as f64;
        c.set_fold(self.params.fold.value() as f64);
        c.input_gain_db = self.params.input_gain_db.value() as f64;
        c.output_gain_db = self.params.output_gain_db.value() as f64;

        c.range_db = self.params.range_db.value() as f64;
        c.hold_ms = self.params.hold_ms.value() as f64;

        let sc_freq = self.params.sidechain_freq.value() as f64;
        self.chain.set_sidechain_freq(sc_freq);

        let la_ms = self.params.lookahead_ms.value() as f64;
        self.chain.set_lookahead(la_ms);

        self.chain.comp.update(self.sample_rate);
    }
}

impl Plugin for FtsComp {
    const NAME: &'static str = "FTS Compressor";
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
        eprintln!(
            "[PLUGIN] initialize() called! sample_rate={}",
            buffer_config.sample_rate
        );
        self.sample_rate = buffer_config.sample_rate as f64;
        self.waveform_interval = (buffer_config.sample_rate as usize / 240).max(1);
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
        self.waveform_gr_peak = 0.0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        static mut PROCESS_COUNT: u64 = 0;
        unsafe {
            PROCESS_COUNT += 1;
            if PROCESS_COUNT == 1 {
                eprintln!("[PLUGIN] process() called!");
            }
        }

        self.sync_params();

        // Report lookahead latency so the DAW can compensate
        context.set_latency_samples(self.chain.lookahead_samples as u32);

        // Process in blocks — convert f32 ↔ f64 for comp-dsp
        for mut frame in buffer.iter_samples() {
            let mut channels = frame.iter_mut();
            let left_ref = channels.next().unwrap();
            let right_ref = channels.next().unwrap();

            let mut left = *left_ref as f64;
            let mut right = *right_ref as f64;

            // Track input peak
            let input_peak = left.abs().max(right.abs()) as f32;

            // Process through compressor chain (includes sidechain HPF if active)
            self.chain.process_sample(&mut left, &mut right);

            *left_ref = left as f32;
            *right_ref = right as f32;

            // Track output peak
            let output_peak = (left.abs().max(right.abs())) as f32;

            // Update metering atomics
            let gr = self.chain.comp.gain_reduction_db() as f32;
            self.ui_state.gain_reduction_db.store(gr, Ordering::Relaxed);

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
                prev_in - 0.3 // ~15 dB/s decay at 48kHz
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
            self.waveform_gr_peak = self.waveform_gr_peak.max(gr / 30.0); // normalize to 0-1
            self.waveform_counter += 1;

            // Phase: fractional progress within the current interval (0..1).
            // Updated every sample so the renderer can interpolate sub-pixel scroll.
            let phase = self.waveform_counter as f32 / self.waveform_interval as f32;
            self.ui_state.waveform_phase.store(phase, Ordering::Relaxed);

            if self.waveform_counter >= self.waveform_interval {
                let pos =
                    self.ui_state.waveform_pos.load(Ordering::Relaxed) as usize % WAVEFORM_LEN;
                self.ui_state.waveform_input[pos]
                    .store(self.waveform_peak.min(1.0), Ordering::Relaxed);
                self.ui_state.waveform_gr[pos]
                    .store(self.waveform_gr_peak.min(1.0), Ordering::Relaxed);
                self.ui_state
                    .waveform_pos
                    .store((pos + 1) as f32, Ordering::Relaxed);
                self.ui_state.waveform_phase.store(0.0, Ordering::Relaxed);

                self.waveform_counter = 0;
                self.waveform_peak = 0.0;
                self.waveform_gr_peak = 0.0;
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsComp {
    const CLAP_ID: &'static str = "com.fasttrackstudio.comp";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Compressor with hardware profiles");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Compressor,
        ClapFeature::Stereo,
    ];

    fn gain_adjustment_db(&self) -> f64 {
        // Return negative dB for gain reduction (compressor convention)
        -(self.ui_state.gain_reduction_db.load(Ordering::Relaxed) as f64)
    }
}

impl Vst3Plugin for FtsComp {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsCompPlugin001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nih_export_clap!(FtsComp);
nih_export_vst3!(FtsComp);
