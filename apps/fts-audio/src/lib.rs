//! fts-audio — Thin RT VST plugin that executes rule tables from shared memory.
//!
//! This plugin runs on REAPER's audio thread and reads flat, fixed-size rule
//! tables uploaded by guest processes (fts-macros, signal, etc.) via shared
//! memory. It applies FX parameter changes, routes MIDI, and triggers samples
//! with zero added latency.
//!
//! The plugin is intentionally minimal: guests handle all configuration and
//! virtual descriptor resolution. This plugin only executes pre-resolved rules.

use fts_audio_proto::*;
use nih_plug::prelude::*;
use std::num::NonZeroU32;
use std::sync::Arc;

// ============================================================================
// Plugin Parameters
// ============================================================================

/// Automatable macro parameters that guests can map to FX parameters.
#[derive(Params)]
struct FtsAudioParams {
    #[id = "macro_0"]
    macro_0: FloatParam,
    #[id = "macro_1"]
    macro_1: FloatParam,
    #[id = "macro_2"]
    macro_2: FloatParam,
    #[id = "macro_3"]
    macro_3: FloatParam,
    #[id = "macro_4"]
    macro_4: FloatParam,
    #[id = "macro_5"]
    macro_5: FloatParam,
    #[id = "macro_6"]
    macro_6: FloatParam,
    #[id = "macro_7"]
    macro_7: FloatParam,
}

impl Default for FtsAudioParams {
    fn default() -> Self {
        let mk = |name: &'static str| {
            FloatParam::new(name, 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" ")
                .with_value_to_string(formatters::v2s_f32_rounded(3))
        };

        Self {
            macro_0: mk("Macro 1"),
            macro_1: mk("Macro 2"),
            macro_2: mk("Macro 3"),
            macro_3: mk("Macro 4"),
            macro_4: mk("Macro 5"),
            macro_5: mk("Macro 6"),
            macro_6: mk("Macro 7"),
            macro_7: mk("Macro 8"),
        }
    }
}

// ============================================================================
// Plugin State
// ============================================================================

/// The fts-audio plugin — a programmable audio-thread executor.
pub struct FtsAudio {
    params: Arc<FtsAudioParams>,

    /// Previous macro values for change detection.
    prev_macro_values: [f32; NUM_MACROS],

    /// Sample rate from the host.
    sample_rate: f32,
    // TODO: SHM rule table pointer (set during initialization)
    // rule_table: Option<&'static RuleTable>,
}

impl Default for FtsAudio {
    fn default() -> Self {
        Self {
            params: Arc::new(FtsAudioParams::default()),
            prev_macro_values: [0.0; NUM_MACROS],
            sample_rate: 44100.0,
        }
    }
}

// ============================================================================
// Plugin Trait Implementation
// ============================================================================

impl Plugin for FtsAudio {
    const NAME: &'static str = "FTS Audio";
    const VENDOR: &'static str = "FastTrackStudio";
    const URL: &'static str = "https://github.com/FastTrackStudios/FastTrackStudio";
    const EMAIL: &'static str = "info@fasttrackstudio.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

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
        self.sample_rate = buffer_config.sample_rate;

        // TODO: Discover and mmap the SHM segment for rule table access.
        // The segment path can be found via FTS_SHM_RULE_TABLE env var
        // or by convention from the daw-bridge bootstrap socket.

        true
    }

    fn reset(&mut self) {
        self.prev_macro_values = [0.0; NUM_MACROS];
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Read current macro values
        let macro_values = [
            self.params.macro_0.value(),
            self.params.macro_1.value(),
            self.params.macro_2.value(),
            self.params.macro_3.value(),
            self.params.macro_4.value(),
            self.params.macro_5.value(),
            self.params.macro_6.value(),
            self.params.macro_7.value(),
        ];

        // Process MIDI input events (for MIDI routing rules)
        while let Some(event) = context.next_event() {
            // TODO: Match MIDI events against MidiRoute rules from the rule table.
            // For now, pass through all MIDI events.
            context.send_event(event);
        }

        // Change detection on macro parameters
        for (idx, &value) in macro_values.iter().enumerate() {
            if value == self.prev_macro_values[idx] {
                continue;
            }
            self.prev_macro_values[idx] = value;

            // TODO: Look up FxMapping rules for this macro index from the rule table.
            // Apply transformed values via TrackFxSetParamNormalized (same-track only).
            //
            // For now, this is a no-op until SHM rule table is connected.
        }

        // Audio passthrough (fts-audio doesn't modify audio, only FX params and MIDI)
        let _ = buffer;

        ProcessStatus::Normal
    }
}

// ============================================================================
// CLAP / VST3 Export
// ============================================================================

impl ClapPlugin for FtsAudio {
    const CLAP_ID: &'static str = "com.fasttrackstudio.fts-audio";
    const CLAP_DESCRIPTION: Option<&'static str> = Some(
        "Programmable RT executor — applies rule tables from shared memory on the audio thread",
    );
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Utility,
        ClapFeature::Custom("macro-control"),
    ];
}

impl Vst3Plugin for FtsAudio {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsAudioPlugin__";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nih_export_clap!(FtsAudio);
nih_export_vst3!(FtsAudio);
