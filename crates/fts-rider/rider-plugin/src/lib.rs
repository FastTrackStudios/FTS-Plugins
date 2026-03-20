//! FTS Rider — nih-plug entry point.

use nih_plug::prelude::*;

struct FtsRider {
    params: std::sync::Arc<FtsRiderParams>,
    // rider_chain: rider_dsp::RiderChain,
}

#[derive(Params)]
struct FtsRiderParams {
    // TODO: Define nih-plug params that bridge to rider-dsp + rider-profiles
}

impl Default for FtsRider {
    fn default() -> Self {
        Self {
            params: std::sync::Arc::new(FtsRiderParams {}),
        }
    }
}

impl Plugin for FtsRider {
    const NAME: &'static str = "FTS Rider";
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
        // TODO: Bridge nih-plug buffer to rider_dsp::RiderChain::process
        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsRider {
    const CLAP_ID: &'static str = "com.fasttrackstudio.rider";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Vocal rider with automatic level control");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Utility,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for FtsRider {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsRiderPlug0001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nih_export_clap!(FtsRider);
nih_export_vst3!(FtsRider);
