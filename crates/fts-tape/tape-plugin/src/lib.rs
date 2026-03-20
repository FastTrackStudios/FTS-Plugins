//! FTS Tape Machine — nih-plug entry point.

use nih_plug::prelude::*;

struct FtsTape {
    params: std::sync::Arc<FtsTapeParams>,
    // tape_chain: tape_dsp::TapeChain,
}

#[derive(Params)]
struct FtsTapeParams {
    // TODO: Define nih-plug params that bridge to tape-dsp + tape-profiles
}

impl Default for FtsTape {
    fn default() -> Self {
        Self {
            params: std::sync::Arc::new(FtsTapeParams {}),
        }
    }
}

impl Plugin for FtsTape {
    const NAME: &'static str = "FTS Tape";
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
        // TODO: Bridge nih-plug buffer to tape_dsp::TapeChain::process
        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsTape {
    const CLAP_ID: &'static str = "com.fasttrackstudio.tape";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Tape machine with hardware profiles");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Distortion,
        ClapFeature::Analyzer,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for FtsTape {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsTapePlugin001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx];
}

nih_export_clap!(FtsTape);
nih_export_vst3!(FtsTape);
