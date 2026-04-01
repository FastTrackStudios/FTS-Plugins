//! Dataset discovery and loading for GuitarSet and Guitar-TECHS.

use std::path::{Path, PathBuf};

/// Default path for GuitarSet dataset.
pub const GUITARSET_DIR: &str = "/home/cody/Development/mir-datasets/data/guitarset";

/// Default path for Guitar-TECHS dataset.
pub const GUITAR_TECHS_DIR: &str = "/home/cody/Downloads/Guitar-TECHS";

/// A GuitarSet recording with paired audio and annotation files.
#[derive(Debug, Clone)]
pub struct GuitarSetEntry {
    /// Base name (e.g. "00_BN1-129-Eb_comp").
    pub name: String,
    /// Path to JAMS annotation file.
    pub annotation: PathBuf,
    /// Path to hexaphonic debleeded WAV (6 channels).
    pub hex_audio: PathBuf,
    /// Path to mono pickup mix WAV (1 channel).
    pub mono_audio: PathBuf,
}

/// A Guitar-TECHS recording with paired audio and MIDI files.
#[derive(Debug, Clone)]
pub struct GuitarTechsEntry {
    /// Category path (e.g. "P1_singlenotes").
    pub category: String,
    /// Path to direct input WAV.
    pub di_audio: PathBuf,
    /// Path to MIDI annotation.
    pub midi: PathBuf,
}

/// Discover all GuitarSet entries with paired annotation + audio.
pub fn discover_guitarset(base: &Path) -> Vec<GuitarSetEntry> {
    let ann_dir = base.join("annotation");
    let hex_dir = base.join("audio_hex-pickup_debleeded");
    let mono_dir = base.join("audio_mono-pickup_mix");

    if !ann_dir.exists() {
        return Vec::new();
    }

    let mut entries = Vec::new();
    let mut jams_files: Vec<PathBuf> = std::fs::read_dir(&ann_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "jams"))
        .collect();
    jams_files.sort();

    for jams_path in jams_files {
        let stem = jams_path.file_stem().unwrap().to_string_lossy().to_string();
        let hex_path = hex_dir.join(format!("{stem}_hex_cln.wav"));
        let mono_path = mono_dir.join(format!("{stem}_mix.wav"));

        if hex_path.exists() && mono_path.exists() {
            entries.push(GuitarSetEntry {
                name: stem,
                annotation: jams_path,
                hex_audio: hex_path,
                mono_audio: mono_path,
            });
        }
    }

    entries
}

/// Discover all Guitar-TECHS entries with paired DI audio + MIDI.
pub fn discover_guitar_techs(base: &Path) -> Vec<GuitarTechsEntry> {
    if !base.exists() {
        return Vec::new();
    }

    let mut entries = Vec::new();

    // Each P*_ directory has audio/directinput/ and midi/ subdirs.
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(base)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    for dir in dirs {
        let category = dir.file_name().unwrap().to_string_lossy().to_string();
        let di_dir = dir.join("audio/directinput");
        let midi_dir = dir.join("midi");

        if !di_dir.exists() || !midi_dir.exists() {
            continue;
        }

        // Find paired WAV + MIDI files.
        let wav_files: Vec<PathBuf> = std::fs::read_dir(&di_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |e| e == "wav"))
            .collect();

        let midi_files: Vec<PathBuf> = std::fs::read_dir(&midi_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |e| e == "mid" || e == "midi"))
            .collect();

        // For categories with a single WAV + single MIDI, pair them directly.
        if wav_files.len() == 1 && midi_files.len() == 1 {
            entries.push(GuitarTechsEntry {
                category: category.clone(),
                di_audio: wav_files[0].clone(),
                midi: midi_files[0].clone(),
            });
        } else {
            // Multiple files: try to match by name pattern.
            for wav in &wav_files {
                let wav_stem = wav.file_stem().unwrap().to_string_lossy();
                // Try matching: "directinput_X" -> "midi_X"
                let midi_name = wav_stem.replace("directinput_", "midi_");
                for midi in &midi_files {
                    let midi_stem = midi.file_stem().unwrap().to_string_lossy();
                    if midi_stem == midi_name {
                        entries.push(GuitarTechsEntry {
                            category: category.clone(),
                            di_audio: wav.clone(),
                            midi: midi.clone(),
                        });
                    }
                }
            }
        }
    }

    entries
}

/// Read a WAV file and return (samples, sample_rate, num_channels).
///
/// For multi-channel files, returns interleaved samples.
pub fn read_wav(path: &Path) -> Result<(Vec<f64>, u32, u16), String> {
    let reader = hound::WavReader::open(path)
        .map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels;

    let samples: Vec<f64> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f64;
            reader
                .into_samples::<i32>()
                .map(|s| s.unwrap() as f64 / max)
                .collect()
        }
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .map(|s| s.unwrap() as f64)
            .collect(),
    };

    Ok((samples, sample_rate, channels))
}

/// Read a WAV file and return mono samples (downmix if multi-channel).
pub fn read_wav_mono(path: &Path) -> Result<(Vec<f64>, u32), String> {
    let (samples, sr, channels) = read_wav(path)?;

    if channels == 1 {
        return Ok((samples, sr));
    }

    let ch = channels as usize;
    let frames = samples.len() / ch;
    let mut mono = Vec::with_capacity(frames);
    for i in 0..frames {
        let sum: f64 = (0..ch).map(|c| samples[i * ch + c]).sum();
        mono.push(sum / ch as f64);
    }

    Ok((mono, sr))
}

/// Read individual string channels from a hexaphonic WAV file.
///
/// Returns 6 vectors of f64 samples, one per string (low E to high E).
pub fn read_wav_hex(path: &Path) -> Result<([Vec<f64>; 6], u32), String> {
    let (samples, sr, channels) = read_wav(path)?;
    if channels != 6 {
        return Err(format!(
            "Expected 6-channel hexaphonic WAV, got {} channels",
            channels
        ));
    }

    let frames = samples.len() / 6;
    let mut strings: [Vec<f64>; 6] = Default::default();
    for s in &mut strings {
        s.reserve(frames);
    }

    for i in 0..frames {
        for (c, string) in strings.iter_mut().enumerate() {
            string.push(samples[i * 6 + c]);
        }
    }

    Ok((strings, sr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_guitarset() {
        let entries = discover_guitarset(Path::new(GUITARSET_DIR));
        if entries.is_empty() {
            eprintln!("Skipping: GuitarSet not found");
            return;
        }
        println!("Found {} GuitarSet entries", entries.len());
        for e in entries.iter().take(3) {
            println!("  {}", e.name);
        }
    }

    #[test]
    fn test_discover_guitar_techs() {
        let entries = discover_guitar_techs(Path::new(GUITAR_TECHS_DIR));
        if entries.is_empty() {
            eprintln!("Skipping: Guitar-TECHS not found");
            return;
        }
        println!("Found {} Guitar-TECHS entries", entries.len());
        for e in &entries {
            println!("  {} -> {:?}", e.category, e.di_audio.file_name().unwrap());
        }
    }
}
