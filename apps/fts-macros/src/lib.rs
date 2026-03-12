//! FTS Macros - Parameter surface plugin for macro control
//!
//! This plugin exposes 8 macro parameters that can be automated via REAPER's
//! automation system. The mapping system enables each macro to control any FX
//! parameter on any track with sample-accurate real-time processing.
//!
//! Architecture:
//! - **Source:** Macro parameters (0-7), each 0.0–1.0
//! - **Mapping:** Virtual→actual track/FX resolution at runtime
//! - **Mode:** Value transformation (passthrough, scale, relative, toggle)
//! - **Target:** Any FX parameter on any track
//! - **Persistence:** Mappings stored in plugin state (JSONL format)
//! - **Sample-accuracy:** Mappings applied within audio processing loop
//!
//! Design:
//! - No audio I/O (utility plugin)
//! - 8 fixed macro slots (matches macromod MAX_KNOBS)
//! - Each slot is a FloatParam with range 0.0–1.0
//! - CLAP export for REAPER automation compatibility
//! - Self-contained (works without fts-control extension)

pub mod mapping;
pub mod resolver;

use fts_plugin_core::prelude::*;
use std::sync::Arc;
use daw_control_sync::DawSync;
use std::sync::Mutex;

const CLAP_ID: &str = "com.fasttrackstudio.fts-macros";
const PLUGIN_NAME: &str = "FTS Macros";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Macro parameters
#[derive(Params)]
struct MacroParams {
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
        }
    }
}

/// Main FTS Macros plugin
pub struct FtsMacros {
    params: Arc<MacroParams>,
    /// Macro-to-FX parameter mappings
    mapping_bank: Arc<mapping::MacroMappingBank>,
    /// Per-buffer resolution cache to minimize API calls
    resolution_cache: resolver::ResolutionCache,
    /// Synchronous DAW control for setting target FX parameters
    /// Wrapped in Mutex since it's initialized after plugin creation
    daw_sync: Arc<Mutex<Option<DawSync>>>,
}

impl Default for FtsMacros {
    fn default() -> Self {
        Self {
            params: Arc::new(MacroParams::default()),
            mapping_bank: Arc::new(mapping::MacroMappingBank::new()),
            resolution_cache: resolver::ResolutionCache::new(),
            daw_sync: Arc::new(Mutex::new(None)),
        }
    }
}

impl FtsMacros {
    /// Initialize DAW sync with a connection handle
    /// This should be called from the plugin's init hook when the DAW connection is available
    pub fn init_daw_sync(&self, handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
        let daw_sync = DawSync::new(handle)?;
        let mut sync = self.daw_sync.lock().map_err(|_| eyre::eyre!("Failed to acquire lock"))?;
        *sync = Some(daw_sync);
        tracing::info!("FTS Macros: DAW sync initialized");
        Ok(())
    }
}

impl Plugin for FtsMacros {
    const NAME: &'static str = PLUGIN_NAME;
    const VENDOR: &'static str = "FastTrackStudio";
    const URL: &'static str = "https://fasttrackstudio.com";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = PLUGIN_VERSION;

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn process(
        &mut self,
        _buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Clear per-buffer resolution cache to prepare for fresh lookups
        self.resolution_cache.clear();

        // Read current macro parameter values and apply active mappings
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

        // Apply each mapping for each macro parameter
        for (macro_idx, value) in macro_values.iter().enumerate() {
            let mappings = self.mapping_bank.get_mappings_for_param(macro_idx as u8);

            for mapping in mappings {
                // Resolve target track and FX
                match (
                    self.resolution_cache
                        .resolve_track_cached(&mapping.target_track),
                    self.resolution_cache
                        .resolve_fx_cached(0, &mapping.target_fx),
                    resolver::FxParameterResolver::validate_param_index(
                        0,
                        0,
                        mapping.target_param_index,
                    ),
                ) {
                    (Ok(track_idx), Ok(fx_idx), Ok(())) => {
                        // Phase 3: Set the target FX parameter via DawSync (non-blocking)
                        let transformed_value = mapping.mode.apply(*value);

                        // Queue the parameter change via DawSync
                        // This is non-blocking and real-time safe - the actual parameter
                        // change happens asynchronously on the background tokio runtime
                        if let Ok(mut daw_opt) = self.daw_sync.lock() {
                            if let Some(daw) = daw_opt.as_ref() {
                                // Fire and forget - queue the parameter change
                                let _ = daw.queue_set_param(
                                    track_idx,
                                    fx_idx,
                                    mapping.target_param_index,
                                    transformed_value,
                                );
                            }
                        }
                    }
                    (Err(_e), _, _) => {
                        // Track resolution failed - mapping will be skipped
                    }
                    (_, Err(_e), _) => {
                        // FX resolution failed - mapping will be skipped
                    }
                    (_, _, Err(_e)) => {
                        // Parameter index validation failed - mapping will be skipped
                    }
                }
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
