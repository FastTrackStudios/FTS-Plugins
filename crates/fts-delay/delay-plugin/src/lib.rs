//! FTS Delay — nih-plug entry point.

use nih_plug::prelude::*;

struct FtsDelay {
    params: std::sync::Arc<FtsDelayParams>,
    // delay_chain: delay_dsp::DelayChain,
}

#[derive(Params)]
struct FtsDelayParams {
    // TODO: Define nih-plug params that bridge to delay-dsp + delay-profiles
}

impl Default for FtsDelay {
    fn default() -> Self {
        Self {
            params: std::sync::Arc::new(FtsDelayParams {}),
        }
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

    fn params(&self) -> std::sync::Arc<dyn Params> {
        self.params.clone()
    }

    fn process(
        &mut self,
        _buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // TODO: Bridge nih-plug buffer to delay_dsp::DelayChain::process
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
