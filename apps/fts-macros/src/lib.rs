//! FTS Macros - Parameter surface plugin for macro control
//!
//! This plugin exposes 8 macro parameters that can be automated via REAPER's
//! automation system. The mapping system enables each macro to control any FX
//! parameter on any track.
//!
//! Architecture:
//! - **Source:** Macro parameters (0-7), each 0.0–1.0
//! - **Mapping:** Virtual→actual track/FX resolution at runtime
//! - **Mode:** Value transformation (passthrough, scale, relative, toggle)
//! - **Target:** Any FX parameter on any track
//! - **Persistence:** Mappings stored in plugin state (JSONL format)
//! - **Timer-driven:** Macro values polled at ~30Hz via REAPER timer callback
//!
//! Design:
//! - Stereo passthrough (no audio modification)
//! - 8 fixed macro slots (matches macromod MAX_KNOBS)
//! - Each slot is a FloatParam with range 0.0–1.0
//! - CLAP export for REAPER automation compatibility
//! - Self-contained (works without fts-control extension)

pub mod mapping;
pub mod reaper_bootstrap;
pub mod resolver;
mod routed_handler;

use fts_plugin_core::prelude::*;
use std::num::NonZeroU32;
use std::sync::Arc;
use daw_control_sync::DawSync;
use std::sync::Mutex;

const CLAP_ID: &str = "com.fasttrackstudio.fts-macros";
const PLUGIN_NAME: &str = "FTS Macros";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Macro parameters
#[derive(Params)]
pub struct MacroParams {
    #[id = "macro_0"]
    pub macro_0: FloatParam,
    #[id = "macro_1"]
    pub macro_1: FloatParam,
    #[id = "macro_2"]
    pub macro_2: FloatParam,
    #[id = "macro_3"]
    pub macro_3: FloatParam,
    #[id = "macro_4"]
    pub macro_4: FloatParam,
    #[id = "macro_5"]
    pub macro_5: FloatParam,
    #[id = "macro_6"]
    pub macro_6: FloatParam,
    #[id = "macro_7"]
    pub macro_7: FloatParam,

    /// Macro-to-FX parameter mappings, persisted in the plugin's CLAP state chunk.
    /// Updated at runtime via ExtState IPC; survives project save/load.
    #[persist = "mappings"]
    pub mapping_bank: Arc<Mutex<mapping::MacroMappingBank>>,
}

impl Default for MacroParams {
    fn default() -> Self {
        let mk = |name: &'static str| {
            FloatParam::new(name, 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
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
            mapping_bank: Arc::new(Mutex::new(mapping::MacroMappingBank::new())),
        }
    }
}

/// Number of macro parameters.
pub const NUM_MACROS: usize = 8;

/// Main FTS Macros plugin
pub struct FtsMacros {
    params: Arc<MacroParams>,
    /// Synchronous DAW control for setting target FX parameters
    /// Wrapped in Mutex since it's initialized after plugin creation
    daw_sync: Arc<Mutex<Option<DawSync>>>,
    /// Previous macro values for change detection in process().
    prev_values: [f32; NUM_MACROS],
}

impl Default for FtsMacros {
    fn default() -> Self {
        Self {
            params: Arc::new(MacroParams::default()),
            daw_sync: Arc::new(Mutex::new(None)),
            prev_values: [f32::NAN; NUM_MACROS],
        }
    }
}

impl FtsMacros {
    /// Pick up DawSync from the REAPER bootstrap static.
    ///
    /// Called during plugin initialization. If the extension eagerly loaded us
    /// and called `ReaperPluginEntry`, the bootstrap static will have a DawSync.
    fn pick_up_daw_sync(&self) {
        if let Some(daw) = reaper_bootstrap::daw_sync() {
            let mut sync = self.daw_sync.lock().expect("lock poisoned");
            *sync = Some(daw.clone());
            tracing::info!("FTS Macros: DawSync acquired from REAPER bootstrap");
        } else {
            tracing::info!("FTS Macros: No REAPER bootstrap — running without DAW control");
        }
    }
}

impl Plugin for FtsMacros {
    const NAME: &'static str = PLUGIN_NAME;
    const VENDOR: &'static str = "FastTrackStudio";
    const URL: &'static str = "https://fasttrackstudio.com";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = PLUGIN_VERSION;

    // Stereo passthrough — FTS Macros doesn't modify audio, but REAPER
    // requires at least one audio port for the plugin to appear in the chain.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    // MIDI input is required for REAPER to call process() on this plugin.
    // Without it, REAPER skips process() for utility plugins without audio routing.
    // This enables audio-rate parameter polling in process().
    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        _buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        // Pick up DawSync from the REAPER bootstrap (if available).
        self.pick_up_daw_sync();

        // Log current mapping state (may have been restored from plugin chunk).
        if let Ok(bank) = self.params.mapping_bank.lock() {
            tracing::info!(
                "FTS Macros: initialize() with {} persisted mappings",
                bank.mappings.len()
            );
        }

        // Register the shared mapping bank with the timer-based macro poller.
        // The timer callback (running at ~30Hz on the main thread) reads param
        // values and applies mappings via REAPER API — this works even when
        // REAPER doesn't call process() (e.g., no audio routing).
        // The same Arc<Mutex<>> is shared so ExtState IPC updates are visible
        // to both the timer and process().
        reaper_bootstrap::register_macro_state(
            self.params.clone(),
            self.params.mapping_bank.clone(),
        );

        true
    }

    fn process(
        &mut self,
        _buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // try_lock() to avoid blocking the audio thread — if the main thread
        // is updating mappings via ExtState IPC, we skip this buffer (rare).
        let Ok(bank) = self.params.mapping_bank.try_lock() else {
            return ProcessStatus::Normal;
        };

        // Read current macro values from nih_plug's atomic params.
        // These are updated by the CLAP host when the user moves faders.
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

        // Change detection — only queue when a value actually changes.
        for (macro_idx, &value) in macro_values.iter().enumerate() {
            let prev = self.prev_values[macro_idx];
            // NaN != NaN, so first buffer always fires
            if prev == value {
                continue;
            }
            self.prev_values[macro_idx] = value;

            let mappings = bank.get_mappings_for_param(macro_idx as u8);

            for mapping in mappings {
                let track_idx = match &mapping.target_track {
                    mapping::TrackDescriptor::ByIndex(idx) => *idx,
                    _ => continue,
                };
                let fx_idx = match &mapping.target_fx {
                    mapping::FxDescriptor::ByIndex(idx) => *idx,
                    _ => continue,
                };

                let transformed = mapping.mode.apply(value);

                // Queue for the main-thread timer to apply via REAPER API.
                reaper_bootstrap::queue_param_change(
                    track_idx,
                    fx_idx,
                    mapping.target_param_index,
                    transformed as f64,
                );
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsMacros {
    const CLAP_ID: &'static str = CLAP_ID;
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Macro parameter controller for FTS signal system");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Utility];
}

nih_export_clap!(FtsMacros);
