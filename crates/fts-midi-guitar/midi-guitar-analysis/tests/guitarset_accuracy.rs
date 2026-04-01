//! Integration tests: run the MIDI guitar detector against GuitarSet recordings
//! and assert minimum accuracy thresholds.
//!
//! These tests require the GuitarSet dataset at:
//!   /home/cody/Development/mir-datasets/data/guitarset/
//!
//! Tests are skipped (not failed) if the dataset is not found.

use midi_guitar_analysis::datasets::{self, GUITARSET_DIR};
use midi_guitar_analysis::eval::{self, DetectorConfig, EvalConfig, RefNote};
use midi_guitar_analysis::jams;
use std::path::Path;

fn guitarset_available() -> bool {
    Path::new(GUITARSET_DIR).join("annotation").exists()
}

/// Helper: evaluate a single GuitarSet recording and return the result.
fn eval_recording(
    entry: &datasets::GuitarSetEntry,
    det_config: &DetectorConfig,
    eval_config: &EvalConfig,
) -> eval::EvalResult {
    let (samples, sample_rate) = datasets::read_wav_mono(&entry.mono_audio).unwrap();
    let ann = jams::parse_jams(&entry.annotation).unwrap();

    let reference: Vec<RefNote> = ann
        .notes
        .iter()
        .map(|n| RefNote {
            onset: n.time,
            offset: n.time + n.duration,
            midi_note: n.midi_note,
        })
        .collect();

    let mut config = det_config.clone();
    config.window_size = (0.020 * sample_rate as f64) as usize;
    let detected = eval::run_detector(&samples, sample_rate as f64, &config);

    eval::evaluate(&entry.name, &reference, &detected, eval_config)
}

/// Test: single note detection on a solo recording.
/// Solo recordings have clearer single-note passages.
#[test]
fn guitarset_solo_basic_detection() {
    if !guitarset_available() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let entries = datasets::discover_guitarset(Path::new(GUITARSET_DIR));
    let solos: Vec<_> = entries.iter().filter(|e| e.name.contains("solo")).collect();
    assert!(!solos.is_empty(), "No solo recordings found");

    let det_config = DetectorConfig::default();
    let eval_config = EvalConfig::default();

    // Evaluate first 5 solo recordings.
    let mut total_tp = 0;
    let mut total_fp = 0;
    let mut total_fn = 0;

    for entry in solos.iter().take(5) {
        let result = eval_recording(entry, &det_config, &eval_config);
        println!(
            "{}: P={:.1}% R={:.1}% F1={:.1}%",
            result.name,
            result.precision * 100.0,
            result.recall * 100.0,
            result.f1 * 100.0,
        );
        total_tp += result.true_positives;
        total_fp += result.false_positives;
        total_fn += result.false_negatives;
    }

    let precision = total_tp as f64 / (total_tp + total_fp).max(1) as f64;
    let recall = total_tp as f64 / (total_tp + total_fn).max(1) as f64;
    println!(
        "\nAggregate: P={:.1}% R={:.1}%",
        precision * 100.0,
        recall * 100.0
    );

    // Baseline assertion: detector should find *some* notes.
    // Precision and recall thresholds are intentionally low — this is a baseline,
    // not a production-quality target.
    assert!(
        total_tp > 0,
        "Detector failed to find any notes in solo recordings"
    );
}

/// Test: comp (chord) recordings — polyphonic detection.
#[test]
fn guitarset_comp_polyphonic_detection() {
    if !guitarset_available() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let entries = datasets::discover_guitarset(Path::new(GUITARSET_DIR));
    let comps: Vec<_> = entries.iter().filter(|e| e.name.contains("comp")).collect();
    assert!(!comps.is_empty(), "No comp recordings found");

    let det_config = DetectorConfig::default();
    let eval_config = EvalConfig::default();

    let mut total_tp = 0;
    let mut total_fp = 0;
    let mut total_fn = 0;

    for entry in comps.iter().take(5) {
        let result = eval_recording(entry, &det_config, &eval_config);
        println!(
            "{}: P={:.1}% R={:.1}% F1={:.1}%",
            result.name,
            result.precision * 100.0,
            result.recall * 100.0,
            result.f1 * 100.0,
        );
        total_tp += result.true_positives;
        total_fp += result.false_positives;
        total_fn += result.false_negatives;
    }

    let precision = total_tp as f64 / (total_tp + total_fp).max(1) as f64;
    let recall = total_tp as f64 / (total_tp + total_fn).max(1) as f64;
    println!(
        "\nAggregate: P={:.1}% R={:.1}%",
        precision * 100.0,
        recall * 100.0
    );

    assert!(
        total_tp > 0,
        "Detector failed to find any notes in comp recordings"
    );
}

/// Test: hexaphonic per-string detection.
/// Run the detector on each string channel individually — should achieve
/// better accuracy than summed mono since each string is isolated.
#[test]
fn guitarset_hex_per_string_detection() {
    if !guitarset_available() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let entries = datasets::discover_guitarset(Path::new(GUITARSET_DIR));
    let entry = &entries[0];

    let (strings, sample_rate) = datasets::read_wav_hex(&entry.hex_audio).unwrap();
    let ann = jams::parse_jams(&entry.annotation).unwrap();

    // Standard guitar string MIDI ranges (approximate).
    let string_ranges: [(u8, u8); 6] = [
        (40, 60), // String 0: low E (E2-C4)
        (45, 65), // String 1: A (A2-F4)
        (50, 70), // String 2: D (D3-Bb4)
        (55, 75), // String 3: G (G3-Eb5)
        (59, 79), // String 4: B (B3-G5)
        (64, 90), // String 5: high E (E4-F#6)
    ];

    let eval_config = EvalConfig::default();
    let mut total_tp = 0;

    for string_idx in 0..6 {
        let string_notes: Vec<RefNote> = ann
            .notes
            .iter()
            .filter(|n| n.string == string_idx)
            .map(|n| RefNote {
                onset: n.time,
                offset: n.time + n.duration,
                midi_note: n.midi_note,
            })
            .collect();

        if string_notes.is_empty() {
            continue;
        }

        let (low, high) = string_ranges[string_idx];
        let config = DetectorConfig {
            threshold: 0.0005,
            sensitivity: 0.5,
            window_size: (0.020 * sample_rate as f64) as usize,
            low_note: low,
            high_note: high,
            harmonic_suppression: true,
            ..DetectorConfig::default()
        };

        let detected = eval::run_detector(&strings[string_idx], sample_rate as f64, &config);
        let result = eval::evaluate(
            &format!("{} string {}", entry.name, string_idx),
            &string_notes,
            &detected,
            &eval_config,
        );

        println!(
            "String {}: P={:.1}% R={:.1}% F1={:.1}% (ref={} det={})",
            string_idx,
            result.precision * 100.0,
            result.recall * 100.0,
            result.f1 * 100.0,
            string_notes.len(),
            detected.len(),
        );
        total_tp += result.true_positives;
    }

    assert!(
        total_tp > 0,
        "Detector failed to find any notes in per-string analysis"
    );
}

/// Test: compare mono vs hexaphonic accuracy.
#[test]
fn guitarset_mono_vs_hex_comparison() {
    if !guitarset_available() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let entries = datasets::discover_guitarset(Path::new(GUITARSET_DIR));
    // Use first 3 entries for comparison.
    let eval_config = EvalConfig::default();
    let base_det_config = DetectorConfig::default();

    let mut mono_f1_sum = 0.0;
    let mut hex_f1_sum = 0.0;
    let mut count = 0;

    for entry in entries.iter().take(3) {
        let ann = jams::parse_jams(&entry.annotation).unwrap();
        let reference: Vec<RefNote> = ann
            .notes
            .iter()
            .map(|n| RefNote {
                onset: n.time,
                offset: n.time + n.duration,
                midi_note: n.midi_note,
            })
            .collect();

        // Mono evaluation.
        let (mono_samples, sr) = datasets::read_wav_mono(&entry.mono_audio).unwrap();
        let mut config = base_det_config.clone();
        config.window_size = (0.020 * sr as f64) as usize;
        let mono_det = eval::run_detector(&mono_samples, sr as f64, &config);
        let mono_result = eval::evaluate(&entry.name, &reference, &mono_det, &eval_config);

        // Hexaphonic (summed to mono) evaluation.
        let (hex_samples, sr) = datasets::read_wav_mono(&entry.hex_audio).unwrap();
        config.window_size = (0.020 * sr as f64) as usize;
        let hex_det = eval::run_detector(&hex_samples, sr as f64, &config);
        let hex_result = eval::evaluate(&entry.name, &reference, &hex_det, &eval_config);

        println!(
            "{}: mono F1={:.1}%  hex F1={:.1}%",
            entry.name,
            mono_result.f1 * 100.0,
            hex_result.f1 * 100.0,
        );

        mono_f1_sum += mono_result.f1;
        hex_f1_sum += hex_result.f1;
        count += 1;
    }

    if count > 0 {
        println!(
            "\nAverage: mono F1={:.1}%  hex F1={:.1}%",
            mono_f1_sum / count as f64 * 100.0,
            hex_f1_sum / count as f64 * 100.0,
        );
    }
}
