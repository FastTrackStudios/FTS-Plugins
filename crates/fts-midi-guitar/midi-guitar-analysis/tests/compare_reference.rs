//! Head-to-head comparison: reference C++ implementation vs our Rust implementation.
//!
//! Parses the reference detector's output from /tmp/reference_output.txt
//! and evaluates both against the same GuitarSet ground truth.
//!
//! Run the reference first:
//!   cd pdct-reference && ./test_harness file1.wav file2.wav ... > /tmp/reference_output.txt
//!
//! Then: cargo test -p midi-guitar-analysis --test compare_reference -- --nocapture

use midi_guitar_analysis::datasets::{self, GUITARSET_DIR};
use midi_guitar_analysis::eval::{self, DetNote, DetectorConfig, EvalConfig, RefNote};
use midi_guitar_analysis::jams;
use std::collections::HashMap;
use std::path::Path;

/// Parse the reference detector output file.
/// Format:
///   FILE <path>
///   ON <time> <note>
///   OFF <time> <note>
///   END
fn parse_reference_output(path: &str) -> HashMap<String, Vec<DetNote>> {
    let content = std::fs::read_to_string(path).expect("Could not read reference output");
    let mut results: HashMap<String, Vec<DetNote>> = HashMap::new();
    let mut current_file = String::new();
    let mut active: HashMap<u8, f64> = HashMap::new(); // note -> onset time

    for line in content.lines() {
        if let Some(file) = line.strip_prefix("FILE ") {
            current_file = file.to_string();
            active.clear();
        } else if let Some(rest) = line.strip_prefix("ON ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() == 2 {
                let time: f64 = parts[0].parse().unwrap();
                let note: u8 = parts[1].parse().unwrap();
                // If already active, close previous
                if let Some(onset) = active.remove(&note) {
                    results
                        .entry(current_file.clone())
                        .or_default()
                        .push(DetNote {
                            onset,
                            offset: Some(time),
                            midi_note: note,
                            velocity: 0.5,
                        });
                }
                active.insert(note, time);
            }
        } else if let Some(rest) = line.strip_prefix("OFF ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() == 2 {
                let time: f64 = parts[0].parse().unwrap();
                let note: u8 = parts[1].parse().unwrap();
                if let Some(onset) = active.remove(&note) {
                    results
                        .entry(current_file.clone())
                        .or_default()
                        .push(DetNote {
                            onset,
                            offset: Some(time),
                            midi_note: note,
                            velocity: 0.5,
                        });
                }
            }
        } else if line == "END" {
            // Flush remaining active notes
            for (note, onset) in active.drain() {
                results
                    .entry(current_file.clone())
                    .or_default()
                    .push(DetNote {
                        onset,
                        offset: None,
                        midi_note: note,
                        velocity: 0.5,
                    });
            }
        }
    }

    results
}

/// Map from GuitarSet entry name to mono WAV path.
fn entry_wav_path(entry: &datasets::GuitarSetEntry) -> String {
    entry.mono_audio.to_string_lossy().to_string()
}

#[test]
fn compare_reference_vs_rust() {
    let reference_file = "/tmp/reference_output.txt";
    if !Path::new(reference_file).exists() {
        eprintln!("Skipping: /tmp/reference_output.txt not found");
        eprintln!("Run the reference detector first:");
        eprintln!(
            "  cd pdct-reference && ./test_harness file1.wav ... > /tmp/reference_output.txt"
        );
        return;
    }

    let entries = datasets::discover_guitarset(Path::new(GUITARSET_DIR));
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let names = [
        "00_BN1-129-Eb_solo",
        "00_BN2-131-B_solo",
        "00_BN1-129-Eb_comp",
        "00_BN2-131-B_comp",
    ];
    let subset: Vec<_> = entries
        .iter()
        .filter(|e| names.contains(&e.name.as_str()))
        .collect();

    let ref_output = parse_reference_output(reference_file);
    let eval_config = EvalConfig::default();

    // Reference detector config (matching reference exactly)
    let ref_det_config = DetectorConfig::default();

    // Our optimized config
    let opt_det_config = DetectorConfig {
        threshold: 0.0003,
        harmonic_suppression: true,
        peak_picking: true,
        hysteresis_ratio: 0.3,
        ..DetectorConfig::default()
    };

    println!(
        "\n{:<25} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "", "Ref P%", "Ref R%", "Ref F1%", "Rust P%", "Rust R%", "Rust F1%"
    );
    println!("{:-<85}", "");

    let mut ref_total_tp = 0usize;
    let mut ref_total_fp = 0usize;
    let mut ref_total_fn = 0usize;
    let mut rust_total_tp = 0usize;
    let mut rust_total_fp = 0usize;
    let mut rust_total_fn = 0usize;

    for entry in &subset {
        let wav_path = entry_wav_path(entry);
        let ann = jams::parse_jams(&entry.annotation).unwrap();
        let reference_notes: Vec<RefNote> = ann
            .notes
            .iter()
            .map(|n| RefNote {
                onset: n.time,
                offset: n.time + n.duration,
                midi_note: n.midi_note,
            })
            .collect();

        // Evaluate reference C++ detector
        let ref_detected = ref_output.get(&wav_path).cloned().unwrap_or_default();
        let ref_result = eval::evaluate(&entry.name, &reference_notes, &ref_detected, &eval_config);

        // Evaluate our Rust detector (with optimized settings)
        let (samples, sr) = datasets::read_wav_mono(&entry.mono_audio).unwrap();
        let mut rust_config = opt_det_config.clone();
        rust_config.window_size = (0.020 * sr as f64) as usize;
        let rust_detected = eval::run_detector(&samples, sr as f64, &rust_config);
        let rust_result =
            eval::evaluate(&entry.name, &reference_notes, &rust_detected, &eval_config);

        println!(
            "{:<25} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1}",
            entry.name,
            ref_result.precision * 100.0,
            ref_result.recall * 100.0,
            ref_result.f1 * 100.0,
            rust_result.precision * 100.0,
            rust_result.recall * 100.0,
            rust_result.f1 * 100.0,
        );

        ref_total_tp += ref_result.true_positives;
        ref_total_fp += ref_result.false_positives;
        ref_total_fn += ref_result.false_negatives;
        rust_total_tp += rust_result.true_positives;
        rust_total_fp += rust_result.false_positives;
        rust_total_fn += rust_result.false_negatives;
    }

    // Aggregates
    let ref_p = ref_total_tp as f64 / (ref_total_tp + ref_total_fp).max(1) as f64;
    let ref_r = ref_total_tp as f64 / (ref_total_tp + ref_total_fn).max(1) as f64;
    let ref_f1 = if ref_p + ref_r > 0.0 {
        2.0 * ref_p * ref_r / (ref_p + ref_r)
    } else {
        0.0
    };

    let rust_p = rust_total_tp as f64 / (rust_total_tp + rust_total_fp).max(1) as f64;
    let rust_r = rust_total_tp as f64 / (rust_total_tp + rust_total_fn).max(1) as f64;
    let rust_f1 = if rust_p + rust_r > 0.0 {
        2.0 * rust_p * rust_r / (rust_p + rust_r)
    } else {
        0.0
    };

    println!("{:-<85}", "");
    println!(
        "{:<25} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1}",
        "AGGREGATE",
        ref_p * 100.0,
        ref_r * 100.0,
        ref_f1 * 100.0,
        rust_p * 100.0,
        rust_r * 100.0,
        rust_f1 * 100.0,
    );

    println!(
        "\nReference C++:  TP={} FP={} FN={}",
        ref_total_tp, ref_total_fp, ref_total_fn
    );
    println!(
        "Rust optimized: TP={} FP={} FN={}",
        rust_total_tp, rust_total_fp, rust_total_fn
    );

    // Also run Rust in pure reference mode for direct comparison
    println!("\n--- Rust in reference-matching mode (threshold=0.001, no enhancements) ---");
    let mut rust_ref_total_tp = 0usize;
    let mut rust_ref_total_fp = 0usize;
    let mut rust_ref_total_fn = 0usize;

    for entry in &subset {
        let ann = jams::parse_jams(&entry.annotation).unwrap();
        let reference_notes: Vec<RefNote> = ann
            .notes
            .iter()
            .map(|n| RefNote {
                onset: n.time,
                offset: n.time + n.duration,
                midi_note: n.midi_note,
            })
            .collect();

        let (samples, sr) = datasets::read_wav_mono(&entry.mono_audio).unwrap();
        let mut config = ref_det_config.clone();
        config.window_size = (0.020 * sr as f64) as usize;
        let detected = eval::run_detector(&samples, sr as f64, &config);
        let result = eval::evaluate(&entry.name, &reference_notes, &detected, &eval_config);

        println!(
            "  {}: P={:.1}% R={:.1}% F1={:.1}%",
            entry.name,
            result.precision * 100.0,
            result.recall * 100.0,
            result.f1 * 100.0,
        );

        rust_ref_total_tp += result.true_positives;
        rust_ref_total_fp += result.false_positives;
        rust_ref_total_fn += result.false_negatives;
    }

    let rr_p = rust_ref_total_tp as f64 / (rust_ref_total_tp + rust_ref_total_fp).max(1) as f64;
    let rr_r = rust_ref_total_tp as f64 / (rust_ref_total_tp + rust_ref_total_fn).max(1) as f64;
    let rr_f1 = if rr_p + rr_r > 0.0 {
        2.0 * rr_p * rr_r / (rr_p + rr_r)
    } else {
        0.0
    };

    println!(
        "  Aggregate: P={:.1}% R={:.1}% F1={:.1}%",
        rr_p * 100.0,
        rr_r * 100.0,
        rr_f1 * 100.0
    );
    println!(
        "  TP={} FP={} FN={}",
        rust_ref_total_tp, rust_ref_total_fp, rust_ref_total_fn
    );
}
