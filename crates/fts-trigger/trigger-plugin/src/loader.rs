//! Sample loading utility using Symphonium.
//!
//! Decodes audio files on a background thread and sends loaded samples
//! to the audio thread via a crossbeam channel.

use crossbeam_channel::Sender;
use std::path::Path;
use symphonium::{ResampleQuality, SymphoniumLoader};
use trigger_dsp::sampler::Sample;

/// Message sent from loader thread to audio thread.
pub struct SampleLoadMessage {
    pub slot: usize,
    pub sample: Sample,
    pub name: String,
}

/// Load a sample file, decode it, and convert to the DSP `Sample` format.
///
/// Resamples to `target_sr` automatically via Symphonium.
pub fn load_sample(
    path: &Path,
    target_sr: f64,
) -> Result<(Sample, String), Box<dyn std::error::Error + Send + Sync>> {
    let mut loader = SymphoniumLoader::new();
    let decoded = loader
        .load_f32(
            path.to_str().unwrap_or(""),
            Some(target_sr as u32),
            ResampleQuality::default(),
            None,
        )
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(std::io::Error::new(std::io::ErrorKind::Other, format!("{e}")))
        })?;

    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("sample")
        .to_string();

    let num_channels = decoded.channels() as usize;
    let num_frames = if num_channels > 0 {
        decoded.data[0].len()
    } else {
        return Err("Empty audio file".into());
    };

    // Convert to Vec<[f64; 2]> stereo pairs
    let mut data = Vec::with_capacity(num_frames);
    for frame in 0..num_frames {
        let l = decoded.data[0][frame] as f64;
        let r = if num_channels > 1 {
            decoded.data[1][frame] as f64
        } else {
            l
        };
        data.push([l, r]);
    }

    let sample = Sample::new(data, target_sr);
    Ok((sample, name))
}

/// Spawn a background thread to load a sample and send it to the audio thread.
pub fn load_sample_async(
    path: String,
    slot: usize,
    target_sr: f64,
    tx: Sender<SampleLoadMessage>,
) {
    std::thread::spawn(move || {
        match load_sample(Path::new(&path), target_sr) {
            Ok((sample, name)) => {
                let _ = tx.send(SampleLoadMessage { slot, sample, name });
            }
            Err(e) => {
                eprintln!("Failed to load sample for slot {}: {}", slot, e);
            }
        }
    });
}
