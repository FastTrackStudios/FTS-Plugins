//! Guitar-TECHS evaluation binary.
//!
//! Runs the MIDI guitar detector against Guitar-TECHS DI recordings
//! and reports precision/recall/F1 for note detection.
//!
//! Usage: cargo run -p midi-guitar-analysis --bin guitar-techs-eval

use midi_guitar_analysis::datasets::{self, GUITAR_TECHS_DIR};
use midi_guitar_analysis::eval::{self, DetectorConfig, EvalConfig, RefNote};
use midi_guitar_analysis::midi_file;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let limit: usize = args
        .iter()
        .position(|a| a == "--limit")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);

    let base = Path::new(GUITAR_TECHS_DIR);
    let entries = datasets::discover_guitar_techs(base);

    if entries.is_empty() {
        eprintln!("Guitar-TECHS not found at {}", GUITAR_TECHS_DIR);
        std::process::exit(1);
    }

    println!("Guitar-TECHS Evaluation");
    println!("=======================");
    println!(
        "Found {} recordings, evaluating {}",
        entries.len(),
        limit.min(entries.len())
    );
    println!();

    let det_config = DetectorConfig {
        threshold: 0.001,
        sensitivity: 0.5,
        window_size: 960, // ~20ms at 48kHz
        low_note: 36,
        high_note: 90,
        harmonic_suppression: true,
        ..DetectorConfig::default()
    };

    let eval_config = EvalConfig::default();

    let mut results = Vec::new();

    for entry in entries.iter().take(limit) {
        eprint!("  Processing {} ...", entry.category);

        // Load audio.
        let (samples, sample_rate) = datasets::read_wav_mono(&entry.di_audio).unwrap();

        // Load MIDI annotations.
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
            eprintln!(" (no reference notes, skipping)");
            continue;
        }

        // Run detector.
        let mut config = det_config.clone();
        config.window_size = (0.020 * sample_rate as f64) as usize;
        let detected = eval::run_detector(&samples, sample_rate as f64, &config);

        // Evaluate.
        let result = eval::evaluate(&entry.category, &reference, &detected, &eval_config);
        eprintln!(
            " P={:.1}% R={:.1}% F1={:.1}% (ref={} det={})",
            result.precision * 100.0,
            result.recall * 100.0,
            result.f1 * 100.0,
            reference.len(),
            detected.len(),
        );
        results.push(result);
    }

    println!();
    eval::print_summary(&results);
}
