//! Integration tests using real vocal recordings.
//!
//! These tests require vocal MP3 files in `test-data/vocals/`.
//! Run `test-data/vocals/download.sh` first to fetch them.
//!
//! Attribution (CC-BY):
//! - "Persephone" by snowflake
//! - "Ophelia's Song" by musetta
//! - "Harmony" by snowflake

use fts_dsp::{AudioConfig, Processor};
use rider_dsp::detector::{DetectMode, LevelDetector};
use rider_dsp::rider::GainRider;
use rider_dsp::RiderChain;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use std::path::PathBuf;

// ── Audio loading ───────────────────────────────────────────────────────

/// Decoded audio: mono f64 samples + sample rate.
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

        // Convert to mono f64
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

/// Convenience: use both L and R as the same mono signal.
fn as_stereo(audio: &Audio) -> (Vec<f64>, Vec<f64>) {
    (audio.samples.clone(), audio.samples.clone())
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn rms(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    (data.iter().map(|x| x * x).sum::<f64>() / data.len() as f64).sqrt()
}

fn max_abs(data: &[f64]) -> f64 {
    data.iter().map(|x| x.abs()).fold(0.0, f64::max)
}

// ── Test files (skip if not downloaded) ─────────────────────────────────

const VOCAL_FILES: &[&str] = &[
    "snowflake_-_Persephone.mp3",
    "musetta_-_Ophelias_Song_Vocals.mp3",
    "snowflake_-_Harmony.mp3",
];

// ── Tests ───────────────────────────────────────────────────────────────

#[test]
fn real_vocal_no_nan() {
    for filename in VOCAL_FILES {
        let Some(audio) = load_mp3(filename) else {
            continue;
        };

        let mut chain = RiderChain::new();
        chain.set_target_db(-18.0);
        chain.set_range_db(12.0);
        chain.rider.detector.mode = DetectMode::KWeighted;
        chain.rider.attack_ms = 15.0;
        chain.rider.release_ms = 60.0;
        chain.update(AudioConfig {
            sample_rate: audio.sample_rate,
            max_buffer_size: 512,
        });

        let (mut left, mut right) = as_stereo(&audio);

        // Process in blocks
        let block = 512;
        for start in (0..left.len()).step_by(block) {
            let end = (start + block).min(left.len());
            chain.process(&mut left[start..end], &mut right[start..end]);
        }

        // Check for NaN/Inf
        for (i, &s) in left.iter().enumerate() {
            assert!(s.is_finite(), "{filename}: NaN/Inf at sample {i}: {s}");
        }
    }
}

#[test]
fn real_vocal_gain_within_range() {
    for filename in VOCAL_FILES {
        let Some(audio) = load_mp3(filename) else {
            continue;
        };

        let range_db = 12.0;

        let mut rider = GainRider::new();
        rider.target_db = -18.0;
        rider.max_boost_db = range_db;
        rider.max_cut_db = range_db;
        rider.detector.mode = DetectMode::KWeighted;
        rider.attack_ms = 15.0;
        rider.release_ms = 60.0;
        rider.update(audio.sample_rate);

        let mut max_gain = f64::NEG_INFINITY;
        let mut min_gain = f64::INFINITY;

        for &s in &audio.samples {
            let g = rider.tick(s, s);
            max_gain = max_gain.max(g);
            min_gain = min_gain.min(g);
        }

        assert!(
            max_gain <= range_db + 0.5,
            "{filename}: gain exceeded max: {max_gain:.2} dB"
        );
        assert!(
            min_gain >= -range_db - 0.5,
            "{filename}: gain exceeded min: {min_gain:.2} dB"
        );
    }
}

#[test]
fn real_vocal_smooth_gain_curve() {
    for filename in VOCAL_FILES {
        let Some(audio) = load_mp3(filename) else {
            continue;
        };

        let mut rider = GainRider::new();
        rider.target_db = -18.0;
        rider.max_boost_db = 12.0;
        rider.max_cut_db = 12.0;
        rider.detector.mode = DetectMode::KWeighted;
        rider.attack_ms = 15.0;
        rider.release_ms = 60.0;
        rider.update(audio.sample_rate);

        let mut prev_gain = 0.0_f64;
        let mut max_jump = 0.0_f64;
        let mut max_jump_at = 0;

        for (i, &s) in audio.samples.iter().enumerate() {
            let g = rider.tick(s, s);
            let jump = (g - prev_gain).abs();
            if jump > max_jump {
                max_jump = jump;
                max_jump_at = i;
            }
            prev_gain = g;
        }

        // Maximum per-sample gain jump should be small for smooth riding.
        // With 15ms attack at 44.1/48kHz, max jump ~0.02 dB/sample is typical.
        assert!(
            max_jump < 0.1,
            "{filename}: gain curve not smooth enough — max jump {max_jump:.4} dB/sample at sample {max_jump_at}"
        );
    }
}

#[test]
fn real_vocal_output_closer_to_target() {
    for filename in VOCAL_FILES {
        let Some(audio) = load_mp3(filename) else {
            continue;
        };

        let target_db = -18.0;
        let sr = audio.sample_rate;

        let mut chain = RiderChain::new();
        chain.set_target_db(target_db);
        chain.set_range_db(12.0);
        chain.rider.detector.mode = DetectMode::KWeighted;
        chain.rider.attack_ms = 15.0;
        chain.rider.release_ms = 60.0;
        chain.update(AudioConfig {
            sample_rate: sr,
            max_buffer_size: 512,
        });

        let (mut left, mut right) = as_stereo(&audio);

        // Measure input level (RMS of non-silent portions)
        let input_rms = rms(&audio.samples);

        // Process
        let block = 512;
        for start in (0..left.len()).step_by(block) {
            let end = (start + block).min(left.len());
            chain.process(&mut left[start..end], &mut right[start..end]);
        }

        let output_rms = rms(&left);

        // Convert to dB
        let input_db = if input_rms > 0.0 {
            20.0 * input_rms.log10()
        } else {
            -200.0
        };
        let output_db = if output_rms > 0.0 {
            20.0 * output_rms.log10()
        } else {
            -200.0
        };

        // Output should be closer to target than input
        let input_dist = (input_db - target_db).abs();
        let output_dist = (output_db - target_db).abs();

        eprintln!(
            "{filename}: input={input_db:.1}dB, output={output_db:.1}dB, target={target_db}dB \
             (input_dist={input_dist:.1}, output_dist={output_dist:.1})"
        );

        // The rider should bring the output closer to target (or at least not
        // make it dramatically worse). We allow some slack because the rider
        // has attack/release smoothing and range limits.
        assert!(
            output_dist < input_dist + 3.0,
            "{filename}: rider made level worse — input_dist={input_dist:.1}, output_dist={output_dist:.1}"
        );
    }
}

#[test]
fn real_vocal_no_clipping() {
    for filename in VOCAL_FILES {
        let Some(audio) = load_mp3(filename) else {
            continue;
        };

        let mut chain = RiderChain::new();
        chain.set_target_db(-18.0);
        chain.set_range_db(12.0);
        chain.rider.detector.mode = DetectMode::KWeighted;
        chain.rider.attack_ms = 15.0;
        chain.rider.release_ms = 60.0;
        chain.update(AudioConfig {
            sample_rate: audio.sample_rate,
            max_buffer_size: 512,
        });

        let (mut left, mut right) = as_stereo(&audio);

        let block = 512;
        for start in (0..left.len()).step_by(block) {
            let end = (start + block).min(left.len());
            chain.process(&mut left[start..end], &mut right[start..end]);
        }

        let peak = max_abs(&left);
        // With 12dB range, peaks can go up to ~4x (12dB gain on near-unity input).
        // We're checking for runaway gain, not clipping — the limiter handles that.
        assert!(
            peak < 4.0,
            "{filename}: output peak too high (possible runaway): {peak:.4}"
        );
    }
}

#[test]
fn real_vocal_voice_activity_detection() {
    // Test that the rider correctly freezes gain during silent/quiet passages.
    for filename in VOCAL_FILES {
        let Some(audio) = load_mp3(filename) else {
            continue;
        };

        let mut rider = GainRider::new();
        rider.target_db = -18.0;
        rider.max_boost_db = 12.0;
        rider.max_cut_db = 12.0;
        rider.activity_threshold_db = -50.0;
        rider.detector.mode = DetectMode::KWeighted;
        rider.attack_ms = 15.0;
        rider.release_ms = 60.0;
        rider.update(audio.sample_rate);

        // Find a quiet region (if any) by scanning blocks
        let block = 1024;
        let mut quiet_blocks = 0;
        let mut loud_blocks = 0;
        let mut gain_changes_in_quiet = 0;

        for chunk in audio.samples.chunks(block) {
            let chunk_rms = rms(chunk);
            let chunk_db = if chunk_rms > 0.0 {
                20.0 * chunk_rms.log10()
            } else {
                -200.0
            };

            let gain_before = rider.gain_db();
            for &s in chunk {
                rider.tick(s, s);
            }
            let gain_after = rider.gain_db();

            if chunk_db < -50.0 {
                quiet_blocks += 1;
                // During quiet blocks, gain should change very little
                if (gain_after - gain_before).abs() > 0.5 {
                    gain_changes_in_quiet += 1;
                }
            } else {
                loud_blocks += 1;
            }
        }

        eprintln!(
            "{filename}: {loud_blocks} loud blocks, {quiet_blocks} quiet blocks, \
             {gain_changes_in_quiet} gain changes during quiet"
        );

        // If there are quiet blocks, gain should rarely change during them
        if quiet_blocks > 5 {
            let ratio = gain_changes_in_quiet as f64 / quiet_blocks as f64;
            assert!(
                ratio < 0.2,
                "{filename}: too many gain changes during quiet: {gain_changes_in_quiet}/{quiet_blocks} = {ratio:.2}"
            );
        }
    }
}

#[test]
fn real_vocal_offline_vs_realtime() {
    // Offline bidirectional analysis should produce a smoother gain curve
    // than real-time forward-only processing.
    use rider_dsp::detector::DetectMode;

    for filename in VOCAL_FILES {
        let Some(audio) = load_mp3(filename) else {
            continue;
        };

        let sr = audio.sample_rate;

        // ── Real-time gain curve ────────────────────────────────────────
        let mut rider = GainRider::new();
        rider.target_db = -18.0;
        rider.max_boost_db = 12.0;
        rider.max_cut_db = 12.0;
        rider.detector.mode = DetectMode::KWeighted;
        rider.detector.window_ms = 50.0;
        rider.attack_ms = 15.0;
        rider.release_ms = 60.0;
        rider.update(sr);

        let rt_gains: Vec<f64> = audio.samples.iter().map(|&s| rider.tick(s, s)).collect();

        // ── Offline gain curve ──────────────────────────────────────────
        // Use the offline analyzer from rider-analysis
        // (We can't depend on rider-analysis from rider-dsp tests, so
        //  replicate the bidirectional logic here for comparison.)
        let mut detector = LevelDetector::new();
        detector.mode = DetectMode::KWeighted;
        detector.window_ms = 50.0;
        detector.update(sr);

        let levels: Vec<f64> = audio.samples.iter().map(|&s| detector.tick(s, s)).collect();

        let raw: Vec<f64> = levels
            .iter()
            .map(|&l| {
                if l < -50.0 {
                    0.0
                } else {
                    (-18.0 - l).clamp(-12.0, 12.0)
                }
            })
            .collect();

        let n = raw.len();
        let smooth_coeff = (-1.0 / (0.1 * sr)).exp(); // 100ms

        let mut fwd = vec![0.0; n];
        fwd[0] = raw[0];
        for i in 1..n {
            fwd[i] = smooth_coeff * fwd[i - 1] + (1.0 - smooth_coeff) * raw[i];
        }
        let mut bwd = vec![0.0; n];
        bwd[n - 1] = raw[n - 1];
        for i in (0..n - 1).rev() {
            bwd[i] = smooth_coeff * bwd[i + 1] + (1.0 - smooth_coeff) * raw[i];
        }
        let offline_gains: Vec<f64> = (0..n).map(|i| (fwd[i] + bwd[i]) * 0.5).collect();

        // ── Compare smoothness ──────────────────────────────────────────
        let smoothness = |gains: &[f64]| -> f64 {
            if gains.len() < 2 {
                return 0.0;
            }
            let mut sum = 0.0;
            for i in 1..gains.len() {
                let d = gains[i] - gains[i - 1];
                sum += d * d;
            }
            (sum / (gains.len() - 1) as f64).sqrt()
        };

        let rt_smooth = smoothness(&rt_gains);
        let off_smooth = smoothness(&offline_gains);

        eprintln!(
            "{filename}: realtime smoothness={rt_smooth:.6}, offline smoothness={off_smooth:.6}"
        );

        // Offline should be at least as smooth (lower = smoother)
        assert!(
            off_smooth <= rt_smooth * 1.1,
            "{filename}: offline should be smoother — rt={rt_smooth:.6}, offline={off_smooth:.6}"
        );
    }
}
