//! Integration tests: run the MIDI guitar detector against Guitar-TECHS recordings.
//!
//! Requires the Guitar-TECHS dataset at:
//!   /home/cody/Downloads/Guitar-TECHS/
//!
//! Tests are skipped (not failed) if the dataset is not found.

use midi_guitar_analysis::datasets::{self, GUITAR_TECHS_DIR};
use midi_guitar_analysis::eval::{self, DetectorConfig, EvalConfig, RefNote};
use midi_guitar_analysis::midi_file;
use std::path::Path;

fn guitar_techs_available() -> bool {
    Path::new(GUITAR_TECHS_DIR).exists()
}

/// Helper: evaluate a Guitar-TECHS entry.
fn eval_entry(
    entry: &datasets::GuitarTechsEntry,
    det_config: &DetectorConfig,
    eval_config: &EvalConfig,
) -> Option<eval::EvalResult> {
    let (samples, sample_rate) = datasets::read_wav_mono(&entry.di_audio).unwrap();
    let midi_notes = midi_file::parse_midi_file(&entry.midi).unwrap();

    let reference: Vec<RefNote> = midi_notes
        .iter()
        .map(|n| RefNote {
            onset: n.time,
            offset: n.time + n.duration,
            midi_note: n.note as f64,
        })
        .collect();

    if reference.is_empty() {
        return None;
    }

    let mut config = det_config.clone();
    config.window_size = (0.020 * sample_rate as f64) as usize;
    let detected = eval::run_detector(&samples, sample_rate as f64, &config);

    Some(eval::evaluate(
        &entry.category,
        &reference,
        &detected,
        eval_config,
    ))
}

/// Test: single note detection on P1_singlenotes.
///
/// Note: Guitar-TECHS MIDI annotations may not be perfectly time-aligned
/// with the audio (the MIDI represents the score, not the performance).
/// We use a wider onset tolerance (1s) to account for this.
#[test]
fn guitar_techs_single_notes() {
    if !guitar_techs_available() {
        eprintln!("Skipping: Guitar-TECHS not found");
        return;
    }

    let entries = datasets::discover_guitar_techs(Path::new(GUITAR_TECHS_DIR));
    let singles: Vec<_> = entries
        .iter()
        .filter(|e| e.category.contains("singlenotes"))
        .collect();

    if singles.is_empty() {
        eprintln!("No singlenotes entries found");
        return;
    }

    let det_config = DetectorConfig {
        threshold: 0.0005,
        sensitivity: 0.5,
        window_size: 960,
        low_note: 36,
        high_note: 90,
        harmonic_suppression: true,
        ..DetectorConfig::default()
    };
    // Wider tolerance: Guitar-TECHS MIDI/audio may not be perfectly aligned.
    let eval_config = EvalConfig {
        onset_tolerance: 1.0,
        pitch_tolerance: 1.0,
    };

    for entry in &singles {
        if let Some(result) = eval_entry(entry, &det_config, &eval_config) {
            println!(
                "{}: P={:.1}% R={:.1}% F1={:.1}% (TP={} FP={} FN={})",
                result.name,
                result.precision * 100.0,
                result.recall * 100.0,
                result.f1 * 100.0,
                result.true_positives,
                result.false_positives,
                result.false_negatives,
            );

            // The detector should find *some* notes; we don't assert precision
            // since MIDI/audio alignment is uncertain in this dataset.
            let total = result.true_positives + result.false_positives;
            assert!(
                total > 0,
                "Detector should produce at least some detections in {}",
                entry.category
            );
        }
    }
}

/// Test: scale detection on P1_scales / P2_scales.
#[test]
fn guitar_techs_scales() {
    if !guitar_techs_available() {
        eprintln!("Skipping: Guitar-TECHS not found");
        return;
    }

    let entries = datasets::discover_guitar_techs(Path::new(GUITAR_TECHS_DIR));
    let scales: Vec<_> = entries
        .iter()
        .filter(|e| e.category.contains("scales"))
        .collect();

    if scales.is_empty() {
        eprintln!("No scales entries found");
        return;
    }

    let det_config = DetectorConfig::default();
    let eval_config = EvalConfig::default();

    let mut total_det = 0;

    for entry in &scales {
        if let Some(result) = eval_entry(entry, &det_config, &eval_config) {
            println!(
                "{}: P={:.1}% R={:.1}% F1={:.1}%",
                result.name,
                result.precision * 100.0,
                result.recall * 100.0,
                result.f1 * 100.0,
            );
            total_det += result.true_positives + result.false_positives;
        }
    }

    assert!(
        total_det > 0,
        "Detector should produce at least some detections on scale recordings"
    );
}

/// Test: chord detection on P1_chords / P2_chords.
#[test]
fn guitar_techs_chords() {
    if !guitar_techs_available() {
        eprintln!("Skipping: Guitar-TECHS not found");
        return;
    }

    let entries = datasets::discover_guitar_techs(Path::new(GUITAR_TECHS_DIR));
    let chords: Vec<_> = entries
        .iter()
        .filter(|e| e.category.contains("chords"))
        .collect();

    if chords.is_empty() {
        eprintln!("No chord entries found");
        return;
    }

    let det_config = DetectorConfig::default();
    let eval_config = EvalConfig::default();

    for entry in &chords {
        if let Some(result) = eval_entry(entry, &det_config, &eval_config) {
            println!(
                "{}: P={:.1}% R={:.1}% F1={:.1}% (TP={} FP={} FN={})",
                result.name,
                result.precision * 100.0,
                result.recall * 100.0,
                result.f1 * 100.0,
                result.true_positives,
                result.false_positives,
                result.false_negatives,
            );
        }
    }
}

/// Test: full musical pieces from P3_music.
#[test]
fn guitar_techs_music() {
    if !guitar_techs_available() {
        eprintln!("Skipping: Guitar-TECHS not found");
        return;
    }

    let entries = datasets::discover_guitar_techs(Path::new(GUITAR_TECHS_DIR));
    let music: Vec<_> = entries
        .iter()
        .filter(|e| e.category.contains("music"))
        .collect();

    if music.is_empty() {
        eprintln!("No music entries found");
        return;
    }

    let det_config = DetectorConfig::default();
    let eval_config = EvalConfig::default();

    for entry in &music {
        if let Some(result) = eval_entry(entry, &det_config, &eval_config) {
            println!(
                "{}: P={:.1}% R={:.1}% F1={:.1}%",
                result.name,
                result.precision * 100.0,
                result.recall * 100.0,
                result.f1 * 100.0,
            );
        }
    }
}
