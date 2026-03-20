//! FTS Reverb — nih-plug entry point.

use nih_plug::prelude::*;

struct FtsReverb {
    params: std::sync::Arc<FtsReverbParams>,
    // reverb_chain: reverb_dsp::ReverbChain,
}

#[derive(Params)]
struct FtsReverbParams {
    // TODO: Define nih-plug params that bridge to reverb-dsp + reverb-profiles
}

impl Default for FtsReverb {
    fn default() -> Self {
        Self {
            params: std::sync::Arc::new(FtsReverbParams {}),
        }
    }
}

impl Plugin for FtsReverb {
    const NAME: &'static str = "FTS Reverb";
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
        // TODO: Bridge nih-plug buffer to reverb_dsp::ReverbChain::process
        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsReverb {
    const CLAP_ID: &'static str = "com.fasttrackstudio.reverb";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Reverb with hardware profiles");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Reverb,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for FtsReverb {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsReverbPlug001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Reverb];
}

nih_export_clap!(FtsReverb);
nih_export_vst3!(FtsReverb);
