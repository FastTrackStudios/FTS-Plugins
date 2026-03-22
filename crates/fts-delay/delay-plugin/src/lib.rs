//! FTS Delay — nih-plug entry point.
//!
//! qdelay-inspired stereo tape delay with wow/flutter, feedback EQ,
//! saturation, diffusion, and ducking.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use atomic_float::AtomicF32;
use delay_dsp::chain::{DelayChain, StereoMode};
use fts_dsp::note_sync::NoteValue;
use fts_dsp::{AudioConfig, Processor};
use fts_plugin_core::prelude::*;

mod editor;

// ── UI State ────────────────────────────────────────────────────────

pub struct DelayUiState {
    pub params: Arc<FtsDelayParams>,
    pub input_peak_db: AtomicF32,
    pub output_peak_db: AtomicF32,
}

// ── Parameters ──────────────────────────────────────────────────────

fn note_value_formatter() -> Arc<dyn Fn(i32) -> String + Send + Sync> {
    Arc::new(|v| {
        NoteValue::from_index(v as usize)
            .map(|n| n.label().to_string())
            .unwrap_or_else(|| "1/4".to_string())
    })
}

#[derive(Params)]
pub struct FtsDelayParams {
    // Time & Rhythm
    #[id = "time_l"]
    pub time_l: FloatParam,
    #[id = "time_r"]
    pub time_r: FloatParam,
    #[id = "link_lr"]
    pub link_lr: FloatParam,

    // Tempo Sync
    #[id = "sync_enable"]
    pub sync_enable: FloatParam,
    #[id = "note_l"]
    pub note_l: IntParam,
    #[id = "note_r"]
    pub note_r: IntParam,

    // Feedback & Mix
    #[id = "feedback"]
    pub feedback: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,

    // Stereo
    #[id = "stereo_mode"]
    pub stereo_mode: FloatParam,
    #[id = "width"]
    pub width: FloatParam,
    #[id = "pp_feedback"]
    pub pp_feedback: FloatParam,

    // Feedback EQ
    #[id = "hicut"]
    pub hicut: FloatParam,
    #[id = "locut"]
    pub locut: FloatParam,

    // Saturation
    #[id = "drive"]
    pub drive: FloatParam,

    // Tape Modulation
    #[id = "wow_depth"]
    pub wow_depth: FloatParam,
    #[id = "wow_rate"]
    pub wow_rate: FloatParam,
    #[id = "wow_drift"]
    pub wow_drift: FloatParam,
    #[id = "flutter_depth"]
    pub flutter_depth: FloatParam,
    #[id = "flutter_rate"]
    pub flutter_rate: FloatParam,

    // Diffusion
    #[id = "diff_enable"]
    pub diff_enable: FloatParam,
    #[id = "diff_size"]
    pub diff_size: FloatParam,
    #[id = "diff_smear"]
    pub diff_smear: FloatParam,

    // Ducking
    #[id = "duck_enable"]
    pub duck_enable: FloatParam,
    #[id = "duck_amount"]
    pub duck_amount: FloatParam,
    #[id = "duck_threshold"]
    pub duck_threshold: FloatParam,
    #[id = "duck_attack"]
    pub duck_attack: FloatParam,
    #[id = "duck_release"]
    pub duck_release: FloatParam,
}

fn bool_formatter() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| {
        if v > 0.5 {
            "On".to_string()
        } else {
            "Off".to_string()
        }
    })
}

fn bool_parser() -> Arc<dyn Fn(&str) -> Option<f32> + Send + Sync> {
    Arc::new(|s| match s.trim().to_lowercase().as_str() {
        "on" | "1" | "true" => Some(1.0),
        "off" | "0" | "false" => Some(0.0),
        _ => s.parse().ok(),
    })
}

fn freq_formatter() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| {
        if v < 1.0 {
            "Off".to_string()
        } else if v >= 1000.0 {
            format!("{:.1}k", v / 1000.0)
        } else {
            format!("{:.0}", v)
        }
    })
}

impl Default for FtsDelayParams {
    fn default() -> Self {
        // Default note index: Quarter (index 6 in ALL)
        let default_note = NoteValue::Quarter.to_index() as i32;

        Self {
            // Time & Rhythm
            time_l: FloatParam::new(
                "Time L",
                250.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 5000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            time_r: FloatParam::new(
                "Time R",
                250.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 5000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            link_lr: FloatParam::new("Link L/R", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(bool_formatter())
                .with_string_to_value(bool_parser()),

            // Tempo Sync
            sync_enable: FloatParam::new("Sync", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(bool_formatter())
                .with_string_to_value(bool_parser()),

            note_l: IntParam::new(
                "Note L",
                default_note,
                IntRange::Linear {
                    min: 0,
                    max: (NoteValue::COUNT - 1) as i32,
                },
            )
            .with_value_to_string(note_value_formatter()),

            note_r: IntParam::new(
                "Note R",
                default_note,
                IntRange::Linear {
                    min: 0,
                    max: (NoteValue::COUNT - 1) as i32,
                },
            )
            .with_value_to_string(note_value_formatter()),

            // Feedback & Mix
            feedback: FloatParam::new("Feedback", 0.4, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            mix: FloatParam::new("Mix", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            // Stereo
            stereo_mode: FloatParam::new("Mode", 0.0, FloatRange::Linear { min: 0.0, max: 2.0 })
                .with_step_size(1.0)
                .with_value_to_string(Arc::new(|v| match v as i32 {
                    0 => "Stereo".to_string(),
                    1 => "PingPong".to_string(),
                    2 => "Mono".to_string(),
                    _ => "Stereo".to_string(),
                })),

            width: FloatParam::new("Width", 1.0, FloatRange::Linear { min: 0.0, max: 2.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            pp_feedback: FloatParam::new(
                "PP Feedback",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            // Feedback EQ
            hicut: FloatParam::new(
                "Hi-Cut",
                8000.0,
                FloatRange::Skewed {
                    min: 500.0,
                    max: 20000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(freq_formatter()),

            locut: FloatParam::new(
                "Lo-Cut",
                0.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 1000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(freq_formatter()),

            // Saturation
            drive: FloatParam::new("Drive", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            // Tape Modulation
            wow_depth: FloatParam::new("Wow", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            wow_rate: FloatParam::new(
                "Wow Rate",
                0.5,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 5.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            wow_drift: FloatParam::new("Drift", 0.3, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            flutter_depth: FloatParam::new(
                "Flutter",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            flutter_rate: FloatParam::new(
                "Flutter Rate",
                6.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 15.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            // Diffusion
            diff_enable: FloatParam::new(
                "Diffusion",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(bool_formatter())
            .with_string_to_value(bool_parser()),

            diff_size: FloatParam::new("Diff Size", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            diff_smear: FloatParam::new(
                "Diff Smear",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            // Ducking
            duck_enable: FloatParam::new("Ducking", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(bool_formatter())
                .with_string_to_value(bool_parser()),

            duck_amount: FloatParam::new(
                "Duck Amt",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            duck_threshold: FloatParam::new(
                "Duck Thresh",
                0.1,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            duck_attack: FloatParam::new(
                "Duck Atk",
                5.0,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 50.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            duck_release: FloatParam::new(
                "Duck Rel",
                200.0,
                FloatRange::Skewed {
                    min: 10.0,
                    max: 1000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
        }
    }
}

// ── Plugin ──────────────────────────────────────────────────────────

struct FtsDelay {
    params: Arc<FtsDelayParams>,
    ui_state: Arc<DelayUiState>,
    editor_state: Arc<DioxusState>,
    chain: DelayChain,
    sample_rate: f64,
    /// Last known BPM from host transport (fallback 120).
    host_bpm: f64,
}

impl Default for FtsDelay {
    fn default() -> Self {
        let params = Arc::new(FtsDelayParams::default());
        let ui_state = Arc::new(DelayUiState {
            params: params.clone(),
            input_peak_db: AtomicF32::new(-100.0),
            output_peak_db: AtomicF32::new(-100.0),
        });
        Self {
            params,
            ui_state,
            editor_state: DioxusState::new(|| (900, 540)),
            chain: DelayChain::new(),
            sample_rate: 48000.0,
            host_bpm: 120.0,
        }
    }
}

impl FtsDelay {
    fn sync_params(&mut self) {
        let p = &self.params;
        let c = &mut self.chain;

        // Time — either manual ms or tempo-synced
        let sync = p.sync_enable.value() > 0.5;
        let link = p.link_lr.value() > 0.5;

        if sync {
            let note_l =
                NoteValue::from_index(p.note_l.value() as usize).unwrap_or(NoteValue::Quarter);
            c.delay_l.time_ms = note_l.to_ms(self.host_bpm);

            if link {
                c.delay_r.time_ms = c.delay_l.time_ms;
            } else {
                let note_r =
                    NoteValue::from_index(p.note_r.value() as usize).unwrap_or(NoteValue::Quarter);
                c.delay_r.time_ms = note_r.to_ms(self.host_bpm);
            }
        } else {
            let time_l = p.time_l.value() as f64;
            c.delay_l.time_ms = time_l;
            c.delay_r.time_ms = if link {
                time_l
            } else {
                p.time_r.value() as f64
            };
        }

        // Feedback
        let fb = p.feedback.value() as f64;
        c.delay_l.feedback = fb;
        c.delay_r.feedback = fb;

        // Mix & Stereo
        c.mix = p.mix.value() as f64;
        c.stereo_mode = match p.stereo_mode.value() as i32 {
            1 => StereoMode::PingPong,
            2 => StereoMode::Mono,
            _ => StereoMode::Stereo,
        };
        c.width = p.width.value() as f64;
        c.pingpong_feedback = p.pp_feedback.value() as f64;

        // Feedback EQ
        let hicut = p.hicut.value() as f64;
        c.delay_l.hicut_freq = hicut;
        c.delay_r.hicut_freq = hicut;

        let locut = p.locut.value() as f64;
        c.delay_l.locut_freq = locut;
        c.delay_r.locut_freq = locut;

        // Saturation
        let drive = p.drive.value() as f64;
        c.delay_l.drive = drive;
        c.delay_r.drive = drive;

        // Wow
        let wow_depth = p.wow_depth.value() as f64;
        let wow_rate = p.wow_rate.value() as f64;
        let wow_drift = p.wow_drift.value() as f64;
        c.delay_l.wow_depth = wow_depth;
        c.delay_r.wow_depth = wow_depth;
        c.delay_l.wow_rate = wow_rate;
        c.delay_r.wow_rate = wow_rate;
        c.delay_l.wow_drift = wow_drift;
        c.delay_r.wow_drift = wow_drift;

        // Flutter
        let flutter_depth = p.flutter_depth.value() as f64;
        let flutter_rate = p.flutter_rate.value() as f64;
        c.delay_l.flutter_depth = flutter_depth;
        c.delay_r.flutter_depth = flutter_depth;
        c.delay_l.flutter_rate = flutter_rate;
        c.delay_r.flutter_rate = flutter_rate;

        // Diffusion
        c.diffusion_enabled = p.diff_enable.value() > 0.5;
        c.diffusion_size = p.diff_size.value() as f64;
        c.diffusion_smear = p.diff_smear.value() as f64;

        // Ducking
        c.ducking_enabled = p.duck_enable.value() > 0.5;
        c.ducker.amount = p.duck_amount.value() as f64;
        c.ducker.threshold = p.duck_threshold.value() as f64;
        c.ducker.attack_ms = p.duck_attack.value() as f64;
        c.ducker.release_ms = p.duck_release.value() as f64;

        // Update DSP coefficients
        c.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: 512,
        });
    }
}

impl Plugin for FtsDelay {
    const NAME: &'static str = "FTS Delay";
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
        // Read host transport BPM
        if let Some(tempo) = context.transport().tempo {
            if tempo > 0.0 {
                self.host_bpm = tempo;
            }
        }

        self.sync_params();

        // Process in chunks, converting f32 <-> f64
        const CHUNK: usize = 128;
        let num_samples = buffer.samples();
        let mut offset = 0;
        let mut input_peak: f32 = 0.0;
        let mut output_peak: f32 = 0.0;

        while offset < num_samples {
            let end = (offset + CHUNK).min(num_samples);
            let len = end - offset;

            let mut left_f64 = [0.0f64; CHUNK];
            let mut right_f64 = [0.0f64; CHUNK];

            // f32 -> f64 + measure input
            let channel_slices = buffer.as_slice();
            for i in 0..len {
                let l = channel_slices[0][offset + i];
                let r = channel_slices[1][offset + i];
                input_peak = input_peak.max(l.abs()).max(r.abs());
                left_f64[i] = l as f64;
                right_f64[i] = r as f64;
            }

            // Process
            self.chain
                .process(&mut left_f64[..len], &mut right_f64[..len]);

            // f64 -> f32 + measure output
            let channel_slices = buffer.as_slice();
            for i in 0..len {
                let l = left_f64[i] as f32;
                let r = right_f64[i] as f32;
                output_peak = output_peak.max(l.abs()).max(r.abs());
                channel_slices[0][offset + i] = l;
                channel_slices[1][offset + i] = r;
            }

            offset = end;
        }

        // Update metering with decay
        let in_db = if input_peak > 0.0 {
            20.0 * input_peak.log10()
        } else {
            -100.0
        };
        let out_db = if output_peak > 0.0 {
            20.0 * output_peak.log10()
        } else {
            -100.0
        };

        let prev_in = self.ui_state.input_peak_db.load(Ordering::Relaxed);
        self.ui_state.input_peak_db.store(
            if in_db > prev_in {
                in_db
            } else {
                prev_in - 0.3
            },
            Ordering::Relaxed,
        );

        let prev_out = self.ui_state.output_peak_db.load(Ordering::Relaxed);
        self.ui_state.output_peak_db.store(
            if out_db > prev_out {
                out_db
            } else {
                prev_out - 0.3
            },
            Ordering::Relaxed,
        );

        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsDelay {
    const CLAP_ID: &'static str = "com.fasttrackstudio.delay";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Delay with hardware profiles");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Delay,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for FtsDelay {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsDelayPlugin01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Delay];
}

nih_export_clap!(FtsDelay);
nih_export_vst3!(FtsDelay);
