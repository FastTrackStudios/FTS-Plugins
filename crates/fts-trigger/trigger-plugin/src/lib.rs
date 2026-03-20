//! FTS Trigger — nih-plug entry point.

use nih_plug::prelude::*;

struct FtsTrigger {
    params: std::sync::Arc<FtsTriggerParams>,
    // trigger_chain: trigger_dsp::TriggerChain,
}

#[derive(Params)]
struct FtsTriggerParams {
    // TODO: Define nih-plug params that bridge to trigger-dsp + trigger-profiles
}

impl Default for FtsTrigger {
    fn default() -> Self {
        Self {
            params: std::sync::Arc::new(FtsTriggerParams {}),
        }
    }
}

impl Plugin for FtsTrigger {
    const NAME: &'static str = "FTS Trigger";
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

    fn params(&self) -> std::sync::Arc<dyn Params> {
        self.params.clone()
    }

    fn process(
        &mut self,
        _buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // TODO: Bridge nih-plug buffer to trigger_dsp::TriggerChain::process
        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsTrigger {
    const CLAP_ID: &'static str = "com.fasttrackstudio.trigger";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Drum trigger with transient detection and sample playback");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Utility,
        ClapFeature::Drum,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for FtsTrigger {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsTrigPlugin001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Dynamics,
        Vst3SubCategory::Instrument,
    ];
}

nih_export_clap!(FtsTrigger);
nih_export_vst3!(FtsTrigger);
