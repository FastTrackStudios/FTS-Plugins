//! FTS Saturate — nih-plug entry point.
//!
//! Multi-style saturation plugin with category + variant selection.
//! Categories: Tape, Tube, Saturation, Amp, Transformer, FX.

use std::sync::Arc;

use fts_dsp::{AudioConfig, Processor};
use fts_plugin_core::prelude::*;
use saturate_dsp::engine::SaturateEngine;
use saturate_dsp::style::{Category, Style};

// ── Formatters ──────────────────────────────────────────────────────

fn category_formatter() -> Arc<dyn Fn(i32) -> String + Send + Sync> {
    Arc::new(|v| Category::from_index(v as usize).name().to_string())
}

fn variant_formatter() -> Arc<dyn Fn(i32) -> String + Send + Sync> {
    // This shows the variant name based on the index alone.
    // The actual category context comes from sync_params.
    // We show all possible variant names up to the max across categories.
    Arc::new(|v| format!("Variant {v}"))
}

// ── Parameters ──────────────────────────────────────────────────────

#[derive(Params)]
pub struct FtsSaturateParams {
    // ── Style selection ─────────────────────────────────────────
    #[id = "category"]
    pub category: IntParam,
    #[id = "variant"]
    pub variant: IntParam,

    // ── Universal controls ──────────────────────────────────────
    #[id = "drive"]
    pub drive: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "output"]
    pub output: FloatParam,
    #[id = "tone"]
    pub tone: FloatParam,
    #[id = "body"]
    pub body: FloatParam,

    // ── Tape-specific ───────────────────────────────────────────
    #[id = "flutter"]
    pub flutter: FloatParam,
    #[id = "flutter_speed"]
    pub flutter_speed: FloatParam,
    #[id = "bias"]
    pub bias: FloatParam,
}

impl Default for FtsSaturateParams {
    fn default() -> Self {
        Self {
            category: IntParam::new(
                "Category",
                0,
                IntRange::Linear {
                    min: 0,
                    max: (Category::COUNT - 1) as i32,
                },
            )
            .with_value_to_string(category_formatter()),

            variant: IntParam::new(
                "Variant",
                2, // Tape — Warm
                IntRange::Linear { min: 0, max: 4 },
            )
            .with_value_to_string(variant_formatter()),

            drive: FloatParam::new("Drive", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            mix: FloatParam::new("Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            output: FloatParam::new("Output", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(Arc::new(|v| {
                    let gain_db = 20.0 * (v * 2.0_f32).log10();
                    if gain_db <= -60.0 {
                        "-inf dB".to_string()
                    } else {
                        format!("{gain_db:.1} dB")
                    }
                })),

            tone: FloatParam::new("Tone", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(Arc::new(|v| {
                    if (v - 0.5).abs() < 0.01 {
                        "Flat".to_string()
                    } else if v < 0.5 {
                        format!("Dark {:.0}%", (0.5 - v) * 200.0)
                    } else {
                        format!("Bright {:.0}%", (v - 0.5) * 200.0)
                    }
                })),

            body: FloatParam::new("Body", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            flutter: FloatParam::new("Flutter", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            flutter_speed: FloatParam::new(
                "Flut Speed",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            bias: FloatParam::new("Bias", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),
        }
    }
}

// ── Plugin ──────────────────────────────────────────────────────────

struct FtsSaturate {
    params: Arc<FtsSaturateParams>,
    engine: SaturateEngine,
    sample_rate: f64,
}

impl Default for FtsSaturate {
    fn default() -> Self {
        Self {
            params: Arc::new(FtsSaturateParams::default()),
            engine: SaturateEngine::new(),
            sample_rate: 44100.0,
        }
    }
}

impl FtsSaturate {
    fn sync_params(&mut self) {
        let p = &self.params;

        // Style selection
        let cat = Category::from_index(p.category.value() as usize);
        let var = (p.variant.value() as usize).min(cat.variant_count().saturating_sub(1));
        let style = Style::new(cat, var);
        self.engine.set_style(style);

        // Universal params
        let e = &mut self.engine.params;
        e.drive = p.drive.value() as f64;
        e.mix = p.mix.value() as f64;
        e.output = p.output.value() as f64;
        e.tone = p.tone.value() as f64;
        e.body = p.body.value() as f64;

        // Tape-specific
        e.flutter = p.flutter.value() as f64;
        e.flutter_speed = p.flutter_speed.value() as f64;
        e.bias = p.bias.value() as f64;

        self.engine.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: 512,
        });
    }
}

impl Plugin for FtsSaturate {
    const NAME: &'static str = "FTS Saturate";
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

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate as f64;
        self.engine.update(AudioConfig {
            sample_rate: self.sample_rate,
            max_buffer_size: buffer_config.max_buffer_size as usize,
        });
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.sync_params();

        const CHUNK: usize = 128;
        let num_samples = buffer.samples();
        let mut offset = 0;

        while offset < num_samples {
            let end = (offset + CHUNK).min(num_samples);
            let len = end - offset;

            let mut left_f64 = [0.0f64; CHUNK];
            let mut right_f64 = [0.0f64; CHUNK];

            let channel_slices = buffer.as_slice();
            for i in 0..len {
                left_f64[i] = channel_slices[0][offset + i] as f64;
                right_f64[i] = channel_slices[1][offset + i] as f64;
            }

            self.engine
                .process(&mut left_f64[..len], &mut right_f64[..len]);

            let channel_slices = buffer.as_slice();
            for i in 0..len {
                channel_slices[0][offset + i] = left_f64[i] as f32;
                channel_slices[1][offset + i] = right_f64[i] as f32;
            }

            offset = end;
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsSaturate {
    const CLAP_ID: &'static str = "com.fasttrackstudio.saturate";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Multi-style saturation — tape, tube, amp, transformer, and FX");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Distortion,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for FtsSaturate {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsSaturatePl_01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Distortion];
}

nih_export_clap!(FtsSaturate);
nih_export_vst3!(FtsSaturate);
