//! FTS LiveTune — nih-plug entry point with auto-tune pitch correction and Dioxus GUI.

use atomic_float::AtomicF32;
use fts_plugin_core::prelude::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use fts_dsp::{AudioConfig, Processor};
use livetune_dsp::chain::{DetectorMode, LiveTuneChain, ShifterMode};
use livetune_dsp::quantizer::{Key, NoteState, Scale};

mod editor;

// ── Shared UI State ──────────────────────────────────────────────────

/// Audio-thread -> UI metering data.
pub struct LiveTuneUiState {
    pub params: Arc<FtsLiveTuneParams>,
    /// Peak input level in dB.
    pub input_peak_db: AtomicF32,
    /// Peak output level in dB.
    pub output_peak_db: AtomicF32,
    /// Current detected pitch in Hz (0 = unvoiced).
    pub detected_freq_hz: AtomicF32,
    /// Current detected MIDI note.
    pub detected_midi: AtomicF32,
    /// Detection confidence (0.0-1.0).
    pub confidence: AtomicF32,
    /// Current correction in semitones.
    pub correction_st: AtomicF32,
}

impl LiveTuneUiState {
    fn new(params: Arc<FtsLiveTuneParams>) -> Self {
        Self {
            params,
            input_peak_db: AtomicF32::new(-100.0),
            output_peak_db: AtomicF32::new(-100.0),
            detected_freq_hz: AtomicF32::new(0.0),
            detected_midi: AtomicF32::new(0.0),
            confidence: AtomicF32::new(0.0),
            correction_st: AtomicF32::new(0.0),
        }
    }
}

// ── Parameters ───────────────────────────────────────────────────────

#[derive(Params)]
pub struct FtsLiveTuneParams {
    /// Root key: 0=C, 1=C#, ... 11=B.
    #[id = "key"]
    pub key: IntParam,

    /// Scale type: 0=Chromatic, 1=Major, 2=Minor, 3=MajorPenta,
    /// 4=MinorPenta, 5=Blues, 6=Custom.
    #[id = "scale"]
    pub scale: IntParam,

    /// Retune speed: 0.0 = instant snap, 1.0 = no correction.
    #[id = "retune_speed"]
    pub retune_speed: FloatParam,

    /// Correction amount: 0.0 = bypass, 1.0 = full correction.
    #[id = "amount"]
    pub amount: FloatParam,

    /// Dry/wet mix.
    #[id = "mix"]
    pub mix: FloatParam,

    /// Detector mode: 0=YIN, 1=YAAPT.
    #[id = "detector_mode"]
    pub detector_mode: IntParam,

    /// Shifter mode: 0=Auto, 1=PSOLA, 2=Vocoder.
    #[id = "shifter_mode"]
    pub shifter_mode: IntParam,

    /// Confidence threshold.
    #[id = "confidence"]
    pub confidence_threshold: FloatParam,

    /// Formant preservation toggle (0.0 = off, 1.0 = on).
    #[id = "formants"]
    pub preserve_formants: FloatParam,

    /// Per-note enable (12 params, one per pitch class).
    #[id = "note_c"]
    pub note_c: FloatParam,
    #[id = "note_cs"]
    pub note_cs: FloatParam,
    #[id = "note_d"]
    pub note_d: FloatParam,
    #[id = "note_eb"]
    pub note_eb: FloatParam,
    #[id = "note_e"]
    pub note_e: FloatParam,
    #[id = "note_f"]
    pub note_f: FloatParam,
    #[id = "note_fs"]
    pub note_fs: FloatParam,
    #[id = "note_g"]
    pub note_g: FloatParam,
    #[id = "note_ab"]
    pub note_ab: FloatParam,
    #[id = "note_a"]
    pub note_a: FloatParam,
    #[id = "note_bb"]
    pub note_bb: FloatParam,
    #[id = "note_b"]
    pub note_b: FloatParam,

    /// Output gain in dB.
    #[id = "output_gain"]
    pub output_gain_db: FloatParam,
}

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

fn make_note_param(name: &str) -> FloatParam {
    FloatParam::new(name, 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
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

impl Default for FtsLiveTuneParams {
    fn default() -> Self {
        Self {
            key: IntParam::new("Key", 0, IntRange::Linear { min: 0, max: 11 })
                .with_value_to_string(Arc::new(|v| {
                    NOTE_NAMES.get(v as usize).unwrap_or(&"?").to_string()
                }))
                .with_string_to_value(Arc::new(|s| {
                    NOTE_NAMES
                        .iter()
                        .position(|n| n.eq_ignore_ascii_case(s.trim()))
                        .map(|i| i as i32)
                        .or_else(|| s.parse().ok())
                })),

            scale: IntParam::new("Scale", 0, IntRange::Linear { min: 0, max: 6 })
                .with_value_to_string(Arc::new(|v| {
                    match v {
                        0 => "Chromatic".to_string(),
                        1 => "Major".to_string(),
                        2 => "Minor".to_string(),
                        3 => "Maj Penta".to_string(),
                        4 => "Min Penta".to_string(),
                        5 => "Blues".to_string(),
                        6 => "Custom".to_string(),
                        _ => format!("{v}"),
                    }
                }))
                .with_string_to_value(Arc::new(|s| {
                    match s.trim().to_lowercase().as_str() {
                        "chromatic" | "0" => Some(0),
                        "major" | "1" => Some(1),
                        "minor" | "2" => Some(2),
                        "maj penta" | "major pentatonic" | "3" => Some(3),
                        "min penta" | "minor pentatonic" | "4" => Some(4),
                        "blues" | "5" => Some(5),
                        "custom" | "6" => Some(6),
                        _ => s.parse().ok(),
                    }
                })),

            retune_speed: FloatParam::new(
                "Retune Speed",
                0.1,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            amount: FloatParam::new(
                "Amount",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            mix: FloatParam::new("Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            detector_mode: IntParam::new("Detector", 0, IntRange::Linear { min: 0, max: 4 })
                .with_value_to_string(Arc::new(|v| {
                    match v {
                        0 => "YIN".to_string(),
                        1 => "YAAPT".to_string(),
                        2 => "pYIN".to_string(),
                        3 => "MPM".to_string(),
                        4 => "Bitstream".to_string(),
                        _ => format!("{v}"),
                    }
                }))
                .with_string_to_value(Arc::new(|s| {
                    match s.trim().to_lowercase().as_str() {
                        "yin" | "0" => Some(0),
                        "yaapt" | "1" => Some(1),
                        "pyin" | "2" => Some(2),
                        "mpm" | "3" => Some(3),
                        "bitstream" | "4" => Some(4),
                        _ => s.parse().ok(),
                    }
                })),

            shifter_mode: IntParam::new("Shifter", 0, IntRange::Linear { min: 0, max: 3 })
                .with_value_to_string(Arc::new(|v| {
                    match v {
                        0 => "Auto".to_string(),
                        1 => "PSOLA".to_string(),
                        2 => "Vocoder".to_string(),
                        3 => "PVSOLA".to_string(),
                        _ => format!("{v}"),
                    }
                }))
                .with_string_to_value(Arc::new(|s| {
                    match s.trim().to_lowercase().as_str() {
                        "auto" | "0" => Some(0),
                        "psola" | "1" => Some(1),
                        "vocoder" | "2" => Some(2),
                        "pvsola" | "hybrid" | "3" => Some(3),
                        _ => s.parse().ok(),
                    }
                })),

            confidence_threshold: FloatParam::new(
                "Confidence",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            preserve_formants: FloatParam::new(
                "Formants",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
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

            note_c: make_note_param("C"),
            note_cs: make_note_param("C#"),
            note_d: make_note_param("D"),
            note_eb: make_note_param("Eb"),
            note_e: make_note_param("E"),
            note_f: make_note_param("F"),
            note_fs: make_note_param("F#"),
            note_g: make_note_param("G"),
            note_ab: make_note_param("Ab"),
            note_a: make_note_param("A"),
            note_bb: make_note_param("Bb"),
            note_b: make_note_param("B"),

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
        }
    }
}

// ── Plugin ───────────────────────────────────────────────────────────

struct FtsLiveTune {
    params: Arc<FtsLiveTuneParams>,
    ui_state: Arc<LiveTuneUiState>,
    editor_state: Arc<DioxusState>,
    chain: LiveTuneChain,
    sample_rate: f64,
}

impl Default for FtsLiveTune {
    fn default() -> Self {
        let params = Arc::new(FtsLiveTuneParams::default());
        let ui_state = Arc::new(LiveTuneUiState::new(params.clone()));
        Self {
            params,
            ui_state,
            editor_state: DioxusState::new(|| (720, 480)),
            chain: LiveTuneChain::new(),
            sample_rate: 48000.0,
        }
    }
}

impl FtsLiveTune {
    /// Read the per-note enable params into a NoteState array.
    fn read_note_states(&self) -> [NoteState; 12] {
        let p = &self.params;
        let note_params = [
            &p.note_c,
            &p.note_cs,
            &p.note_d,
            &p.note_eb,
            &p.note_e,
            &p.note_f,
            &p.note_fs,
            &p.note_g,
            &p.note_ab,
            &p.note_a,
            &p.note_bb,
            &p.note_b,
        ];
        let mut states = [NoteState::Enabled; 12];
        for (i, param) in note_params.iter().enumerate() {
            states[i] = if param.value() > 0.5 {
                NoteState::Enabled
            } else {
                NoteState::Disabled
            };
        }
        states
    }

    /// Sync nih-plug params -> livetune-dsp parameters.
    fn sync_params(&mut self) {
        self.chain.key = self.params.key.value() as Key;
        self.chain.scale = match self.params.scale.value() {
            0 => Scale::Chromatic,
            1 => Scale::Major,
            2 => Scale::Minor,
            3 => Scale::MajorPentatonic,
            4 => Scale::MinorPentatonic,
            5 => Scale::Blues,
            6 => Scale::Custom,
            _ => Scale::Chromatic,
        };
        self.chain.retune_speed = self.params.retune_speed.value() as f64;
        self.chain.amount = self.params.amount.value() as f64;
        self.chain.mix = self.params.mix.value() as f64;
        self.chain.detector_mode = match self.params.detector_mode.value() {
            0 => DetectorMode::Yin,
            1 => DetectorMode::Yaapt,
            2 => DetectorMode::Pyin,
            3 => DetectorMode::Mpm,
            4 => DetectorMode::Bitstream,
            _ => DetectorMode::Yin,
        };
        self.chain.shifter_mode = match self.params.shifter_mode.value() {
            0 => ShifterMode::Auto,
            1 => ShifterMode::Psola,
            2 => ShifterMode::Vocoder,
            3 => ShifterMode::Pvsola,
            _ => ShifterMode::Auto,
        };
        self.chain.confidence_threshold = self.params.confidence_threshold.value() as f64;
        self.chain.preserve_formants = self.params.preserve_formants.value() > 0.5;
        self.chain.notes = self.read_note_states();
    }
}

impl Plugin for FtsLiveTune {
    const NAME: &'static str = "FTS LiveTune";
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
        self.chain.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: buffer_config.max_buffer_size as usize,
        });
        true
    }

    fn reset(&mut self) {
        self.chain.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.sync_params();

        // Report latency to host.
        let latency = self.chain.latency() as u32;
        context.set_latency_samples(latency);

        let output_gain = 10.0f64.powf(self.params.output_gain_db.value() as f64 / 20.0);

        for mut frame in buffer.iter_samples() {
            let mut channels = frame.iter_mut();
            let left_ref = channels.next().unwrap();
            let right_ref = channels.next().unwrap();

            let input_peak = (*left_ref).abs().max((*right_ref).abs());

            let mut mono = [*left_ref as f64];
            let mut mono_r = [*right_ref as f64];
            self.chain.process(&mut mono, &mut mono_r);
            let left = mono[0] * output_gain;
            let right = mono_r[0] * output_gain;

            *left_ref = left as f32;
            *right_ref = right as f32;

            let output_peak = (left.abs().max(right.abs())) as f32;

            // Update metering.
            let prev_in = self.ui_state.input_peak_db.load(Ordering::Relaxed);
            let in_db = if input_peak > 0.0 {
                20.0 * input_peak.log10()
            } else {
                -100.0
            };
            let new_in = if in_db > prev_in { in_db } else { prev_in - 0.3 };
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

            // Update pitch detection metering.
            let pitch = self.chain.detected_pitch();
            self.ui_state
                .detected_freq_hz
                .store(pitch.freq_hz as f32, Ordering::Relaxed);
            self.ui_state
                .detected_midi
                .store(pitch.midi_note as f32, Ordering::Relaxed);
            self.ui_state
                .confidence
                .store(pitch.confidence as f32, Ordering::Relaxed);
            self.ui_state
                .correction_st
                .store(self.chain.current_correction() as f32, Ordering::Relaxed);
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsLiveTune {
    const CLAP_ID: &'static str = "com.fasttrackstudio.livetune";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Real-time pitch correction with scale quantization and formant preservation");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::PitchShifter,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for FtsLiveTune {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsLiveTunePl001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::PitchShift];
}

nih_export_clap!(FtsLiveTune);
nih_export_vst3!(FtsLiveTune);
