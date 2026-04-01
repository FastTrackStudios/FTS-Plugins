//! GuitarSet evaluation binary.
//!
//! Runs the MIDI guitar detector against all GuitarSet recordings and
//! reports precision/recall/F1 for note detection.
//!
//! Usage: cargo run -p midi-guitar-analysis --bin guitarset-eval [-- --mono] [-- --limit N]

use midi_guitar_analysis::datasets::{self, GUITARSET_DIR};
use midi_guitar_analysis::eval::{self, DetectorConfig, EvalConfig, RefNote};
use midi_guitar_analysis::jams;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let use_mono = args.iter().any(|a| a == "--mono");
    let limit: usize = args
        .iter()
        .position(|a| a == "--limit")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);

    let base = Path::new(GUITARSET_DIR);
    let entries = datasets::discover_guitarset(base);

    if entries.is_empty() {
        eprintln!("GuitarSet not found at {}", GUITARSET_DIR);
        eprintln!("Download from: https://zenodo.org/record/3371780");
        std::process::exit(1);
    }

    println!("GuitarSet Evaluation");
    println!("====================");
    println!(
        "Found {} recordings, evaluating {}",
        entries.len(),
        limit.min(entries.len())
    );
    println!(
        "Audio source: {}",
        if use_mono {
            "mono pickup mix"
        } else {
            "hexaphonic (summed)"
        }
    );
    println!();

    let det_config = DetectorConfig {
        threshold: 0.001,
        sensitivity: 0.5,
        window_size: 882, // ~20ms at 44.1kHz
        low_note: 36,     // C2 — slightly below standard guitar range
        high_note: 90,    // F#6 — above highest guitar note
        harmonic_suppression: true,
        ..DetectorConfig::default()
    };

    let eval_config = EvalConfig::default();

    let mut results = Vec::new();

    for entry in entries.iter().take(limit) {
        eprint!("  Processing {}...", entry.name);

        // Load audio.
        let (samples, sample_rate) = if use_mono {
            datasets::read_wav_mono(&entry.mono_audio).unwrap()
        } else {
            // Sum hexaphonic to mono for evaluation.
            datasets::read_wav_mono(&entry.hex_audio).unwrap()
        };

        // Load annotations.
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

        // Run detector.
        let mut config = det_config.clone();
        config.window_size = (0.020 * sample_rate as f64) as usize; // 20ms at actual SR
        let detected = eval::run_detector(&samples, sample_rate as f64, &config);

        // Evaluate.
        let result = eval::evaluate(&entry.name, &reference, &detected, &eval_config);
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
