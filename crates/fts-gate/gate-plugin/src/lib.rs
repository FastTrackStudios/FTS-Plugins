//! FTS Gate — nih-plug entry point.

use nih_plug::prelude::*;

struct FtsGate {
    params: std::sync::Arc<FtsGateParams>,
    // gate_chain: gate_dsp::GateChain,
}

#[derive(Params)]
struct FtsGateParams {
    // TODO: Define nih-plug params that bridge to gate-dsp + gate-profiles
}

impl Default for FtsGate {
    fn default() -> Self {
        Self {
            params: std::sync::Arc::new(FtsGateParams {}),
        }
    }
}

impl Plugin for FtsGate {
    const NAME: &'static str = "FTS Gate";
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
        // TODO: Bridge nih-plug buffer to gate_dsp::GateChain::process
        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsGate {
    const CLAP_ID: &'static str = "com.fasttrackstudio.gate";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Noise gate with zero-crossing awareness and sidechain filtering");
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
