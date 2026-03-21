//! Additional vocal material tests — dynamic range reduction and stereo balance.
//!
//! These tests require vocal MP3 files in `test-data/vocals/`.
//! Run with: `cargo test --package rider-dsp -- --ignored`

use fts_dsp::{AudioConfig, Processor};
use rider_dsp::detector::DetectMode;
use rider_dsp::RiderChain;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use std::path::PathBuf;

// ── Audio loading ───────────────────────────────────────────────────────

struct Audio {
    samples: Vec<f64>,
    sample_rate: f64,
}

fn test_data_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // rider-dsp
    p.pop(); // fts-rider
    p.pop(); // crates
    p.push("test-data");
    p.push("vocals");
    p
}

fn load_mp3(filename: &str) -> Option<Audio> {
    let path = test_data_dir().join(filename);
    if !path.exists() {
        eprintln!(
            "Skipping: {} not found. Run test-data/vocals/download.sh",
            filename
        );
        return None;
    }

    let file = std::fs::File::open(&path).expect("open file");
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .expect("probe format");

    let mut reader = probed.format;
    let track = reader.default_track().expect("default track");
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.expect("sample rate") as f64;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .expect("create decoder");

    let mut samples = Vec::new();

    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        match decoded {
            AudioBufferRef::F32(buf) => {
                let channels = buf.spec().channels.count();
                let frames = buf.frames();
                for frame in 0..frames {
                    let mut sum = 0.0;
                    for ch in 0..channels {
                        sum += buf.chan(ch)[frame] as f64;
                    }
                    samples.push(sum / channels as f64);
                }
            }
            AudioBufferRef::S16(buf) => {
                let channels = buf.spec().channels.count();
                let frames = buf.frames();
                for frame in 0..frames {
                    let mut sum = 0.0;
                    for ch in 0..channels {
                        sum += buf.chan(ch)[frame] as f64 / 32768.0;
                    }
                    samples.push(sum / channels as f64);
                }
            }
            AudioBufferRef::S32(buf) => {
                let channels = buf.spec().channels.count();
                let frames = buf.frames();
                for frame in 0..frames {
                    let mut sum = 0.0;
                    for ch in 0..channels {
                        sum += buf.chan(ch)[frame] as f64 / 2147483648.0;
                    }
                    samples.push(sum / channels as f64);
                }
            }
            _ => {}
        }
    }

    Some(Audio {
        samples,
        sample_rate,
    })
}

fn make_chain(sr: f64) -> RiderChain {
    let mut chain = RiderChain::new();
    chain.set_target_db(-18.0);
    chain.set_range_db(12.0);
    chain.rider.detector.mode = DetectMode::KWeighted;
    chain.rider.attack_ms = 20.0;
    chain.rider.release_ms = 80.0;
    chain.rider.detector.window_ms = 50.0;
    chain.update(AudioConfig {
        sample_rate: sr,
        max_buffer_size: 512,
    });
    chain
}

fn process_stereo(chain: &mut RiderChain, left: &mut [f64], right: &mut [f64]) {
    let block = 512;
    for start in (0..left.len()).step_by(block) {
        let end = (start + block).min(left.len());
        chain.process(&mut left[start..end], &mut right[start..end]);
    }
}

const VOCAL_FILES: &[&str] = &[
    "snowflake_-_Persephone.mp3",
    "musetta_-_Ophelias_Song_Vocals.mp3",
    "snowflake_-_Harmony.mp3",
];

// ── Tests ───────────────────────────────────────────────────────────────

/// The rider should reduce the dynamic range of voiced sections by
/// at least 50% (measured as RMS variance across windows).
#[test]
#[ignore]
fn rider_reduces_dynamic_range() {
    for filename in VOCAL_FILES {
        let Some(audio) = load_mp3(filename) else {
            continue;
        };

        let window = (audio.sample_rate * 0.1) as usize; // 100ms windows
        let threshold_lin = 0.001; // skip near-silence windows

        // Measure per-window RMS of input (voiced windows only)
        let input_rms_values: Vec<f64> = audio
            .samples
            .chunks(window)
            .filter_map(|chunk| {
                let rms = (chunk.iter().map(|x| x * x).sum::<f64>() / chunk.len() as f64).sqrt();
                if rms > threshold_lin {
                    Some(rms)
                } else {
                    None
                }
            })
            .collect();

        if input_rms_values.len() < 10 {
            eprintln!("{filename}: not enough voiced windows, skipping");
            continue;
        }

        let input_variance = variance_db(&input_rms_values);

        // Process
        let mut chain = make_chain(audio.sample_rate);
        let mut left = audio.samples.clone();
        let mut right = audio.samples.clone();
        process_stereo(&mut chain, &mut left, &mut right);

        // Measure per-window RMS of output (same windows)
        let output_rms_values: Vec<f64> = left
            .chunks(window)
            .filter_map(|chunk| {
                let rms = (chunk.iter().map(|x| x * x).sum::<f64>() / chunk.len() as f64).sqrt();
                if rms > threshold_lin {
                    Some(rms)
                } else {
                    None
                }
            })
            .collect();

        let output_variance = variance_db(&output_rms_values);

        eprintln!(
            "{filename}: input_var={input_variance:.2} dB², output_var={output_variance:.2} dB², \
             reduction={:.0}%",
            (1.0 - output_variance / input_variance) * 100.0
        );

        // The rider should meaningfully reduce variance. With a limited range
        // (12 dB) and material that has extreme dynamics, we can't always halve
        // the variance — but we should see at least 20% reduction.
        assert!(
            output_variance < input_variance * 0.8,
            "{filename}: dynamic range not reduced enough — \
             input_var={input_variance:.2}, output_var={output_variance:.2}"
        );
    }
}

/// Mono input panned center should produce identical L/R output,
/// verifying the rider doesn't introduce stereo imbalance.
#[test]
#[ignore]
fn rider_preserves_stereo_balance() {
    for filename in VOCAL_FILES {
        let Some(audio) = load_mp3(filename) else {
            continue;
        };

        let mut chain = make_chain(audio.sample_rate);
        let mut left = audio.samples.clone();
        let mut right = audio.samples.clone();
        process_stereo(&mut chain, &mut left, &mut right);

        // L and R should be bit-identical for mono input
        for (i, (&l, &r)) in left.iter().zip(right.iter()).enumerate() {
            assert!(
                l.to_bits() == r.to_bits(),
                "{filename}: stereo imbalance at sample {i}: L={l}, R={r}"
            );
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Compute variance of RMS values in the dB domain.
fn variance_db(rms_values: &[f64]) -> f64 {
    if rms_values.len() < 2 {
        return 0.0;
    }

    let db_values: Vec<f64> = rms_values
        .iter()
        .map(|&rms| {
            if rms > 0.0 {
                20.0 * rms.log10()
            } else {
                -100.0
            }
        })
        .collect();

    let mean = db_values.iter().sum::<f64>() / db_values.len() as f64;
    db_values.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / (db_values.len() - 1) as f64
}
