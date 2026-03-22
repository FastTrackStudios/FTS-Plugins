//! Sample loading bridge between fts-sample and trigger-dsp.
//!
//! Converts `fts_sample::AudioData` into `trigger_dsp::sampler::Sample`
//! and provides the channel message type for async loading.

use crossbeam_channel::Sender;
use fts_sample::AudioData;
use trigger_dsp::sampler::Sample;

/// Message sent from loader thread to audio thread.
pub struct SampleLoadMessage {
    pub slot: usize,
    pub sample: Sample,
    pub name: String,
}

impl SampleLoadMessage {
    fn from_audio(audio: AudioData, slot: usize) -> Self {
        let name = audio.name.clone();
        let sample = Sample::new(audio.data, audio.sample_rate);
        Self { slot, sample, name }
    }
}

/// Spawn a background thread to load a sample and send it to the audio thread.
pub fn load_sample_async(path: String, slot: usize, target_sr: f64, tx: Sender<SampleLoadMessage>) {
    fts_sample::load_audio_async(path, target_sr, tx, slot, SampleLoadMessage::from_audio);
}
