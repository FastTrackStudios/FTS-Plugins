//! Iteration test: focused subset for tuning the detection algorithm.
//! Run with: cargo test -p midi-guitar-analysis --test iterate -- --nocapture

use midi_guitar_analysis::datasets::{self, GUITARSET_DIR};
use midi_guitar_analysis::eval::{self, DetectorConfig, EvalConfig, RefNote};
use midi_guitar_analysis::jams;
use std::path::Path;

/// Small fixed subset: 2 solo + 2 comp recordings for fast iteration.
fn test_subset() -> Vec<datasets::GuitarSetEntry> {
    let entries = datasets::discover_guitarset(Path::new(GUITARSET_DIR));
    if entries.is_empty() {
        return Vec::new();
    }
    // Pick specific entries with variety.
    let names = [
        "00_BN1-129-Eb_solo",
        "00_BN2-131-B_solo",
        "00_BN1-129-Eb_comp",
        "00_BN2-131-B_comp",
    ];
    entries
        .into_iter()
        .filter(|e| names.contains(&e.name.as_str()))
        .collect()
}

fn eval_subset(
    entries: &[datasets::GuitarSetEntry],
    det_config: &DetectorConfig,
    eval_config: &EvalConfig,
) -> (f64, f64, f64) {
    let mut total_tp = 0usize;
    let mut total_fp = 0usize;
    let mut total_fn = 0usize;

    for entry in entries {
        let (samples, sr) = datasets::read_wav_mono(&entry.mono_audio).unwrap();
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

        let config = det_config.clone();
        let detected = eval::run_detector(&samples, sr as f64, &config);
        let result = eval::evaluate(&entry.name, &reference, &detected, eval_config);

        total_tp += result.true_positives;
        total_fp += result.false_positives;
        total_fn += result.false_negatives;
    }

    let p = total_tp as f64 / (total_tp + total_fp).max(1) as f64;
    let r = total_tp as f64 / (total_tp + total_fn).max(1) as f64;
    let f1 = if p + r > 0.0 {
        2.0 * p * r / (p + r)
    } else {
        0.0
    };
    (p, r, f1)
}

/// Diagnostic: what do false positives look like?
#[test]
fn diagnose_false_positives() {
    let entries = test_subset();
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let entry = &entries[0]; // solo recording
    let (samples, sr) = datasets::read_wav_mono(&entry.mono_audio).unwrap();
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

    let det_config = DetectorConfig::default();
    let _eval_config = EvalConfig::default();

    let mut config = det_config.clone();
    config.window_size = (0.020 * sr as f64) as usize;
    let detected = eval::run_detector(&samples, sr as f64, &config);

    // Match and find unmatched detections.
    let mut ref_matched = vec![false; reference.len()];

    for det in &detected {
        let mut matched = false;
        for (ri, reff) in reference.iter().enumerate() {
            if ref_matched[ri] {
                continue;
            }
            if (det.onset - reff.onset).abs() <= 0.05
                && (det.midi_note as f64 - reff.midi_note).abs() <= 0.5
            {
                ref_matched[ri] = true;
                matched = true;
                break;
            }
        }
        if !matched {
            // Find nearest reference note for context.
            let nearest = reference
                .iter()
                .min_by(|a, b| {
                    (a.onset - det.onset)
                        .abs()
                        .partial_cmp(&(b.onset - det.onset).abs())
                        .unwrap()
                })
                .unwrap();
            let time_diff = det.onset - nearest.onset;
            let pitch_diff = det.midi_note as f64 - nearest.midi_note;
            println!(
                "FP: t={:.3}s note={} vel={:.2} | nearest_ref: t={:.3}s note={:.0} (dt={:.3}s dp={:.1}st)",
                det.onset, det.midi_note, det.velocity,
                nearest.onset, nearest.midi_note,
                time_diff, pitch_diff,
            );
        }
    }

    // Also show missed notes.
    let missed: Vec<_> = reference
        .iter()
        .zip(ref_matched.iter())
        .filter(|(_, m)| !**m)
        .map(|(r, _)| r)
        .collect();
    println!("\nMissed notes ({}):", missed.len());
    for r in missed.iter().take(20) {
        println!(
            "  t={:.3}s note={:.0} dur={:.3}s",
            r.onset,
            r.midi_note,
            r.offset - r.onset
        );
    }
}

/// Parameter sweep: find best threshold.
#[test]
fn sweep_threshold() {
    let entries = test_subset();
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let eval_config = EvalConfig::default();
    println!("Threshold sweep:");
    println!("{:<12} {:>6} {:>6} {:>6}", "threshold", "P%", "R%", "F1%");

    for &threshold in &[
        0.0001, 0.0003, 0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05,
    ] {
        let det_config = DetectorConfig {
            threshold,
            ..DetectorConfig::default()
        };
        let (p, r, f1) = eval_subset(&entries, &det_config, &eval_config);
        println!(
            "{:<12} {:>5.1} {:>5.1} {:>5.1}",
            threshold,
            p * 100.0,
            r * 100.0,
            f1 * 100.0,
        );
    }
}

/// Parameter sweep: find best window size.
#[test]
fn sweep_window_size() {
    let entries = test_subset();
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let eval_config = EvalConfig::default();
    println!("Window size sweep (at 44.1kHz):");
    println!("{:<12} {:>6} {:>6} {:>6}", "window_ms", "P%", "R%", "F1%");

    for &window_ms in &[5.0, 10.0, 15.0, 20.0, 30.0, 40.0, 50.0, 75.0, 100.0] {
        let det_config = DetectorConfig {
            window_size: (window_ms * 44.1) as usize,
            ..DetectorConfig::default()
        };
        let (p, r, f1) = eval_subset(&entries, &det_config, &eval_config);
        println!(
            "{:<12} {:>5.1} {:>5.1} {:>5.1}",
            window_ms,
            p * 100.0,
            r * 100.0,
            f1 * 100.0,
        );
    }
}

/// Parameter sweep: harmonic suppression on/off.
#[test]
fn sweep_harmonic_suppression() {
    let entries = test_subset();
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let eval_config = EvalConfig::default();
    println!("Harmonic suppression comparison:");

    for suppress in [false, true] {
        let det_config = DetectorConfig {
            harmonic_suppression: suppress,
            ..DetectorConfig::default()
        };
        let (p, r, f1) = eval_subset(&entries, &det_config, &eval_config);
        println!(
            "  suppress={}: P={:.1}% R={:.1}% F1={:.1}%",
            suppress,
            p * 100.0,
            r * 100.0,
            f1 * 100.0,
        );
    }
}

/// Comprehensive feature comparison: test all combinations of enhancements.
#[test]
fn sweep_feature_combinations() {
    let entries = test_subset();
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let eval_config = EvalConfig::default();
    println!("Feature combination sweep (threshold=0.001):");
    println!(
        "{:<6} {:<6} {:<6} {:>6} {:>6} {:>6}",
        "harm", "peak", "hyst", "P%", "R%", "F1%"
    );

    for &harmonic in &[false, true] {
        for &peak in &[false, true] {
            for &hyst in &[1.0, 0.5, 0.3] {
                let det_config = DetectorConfig {
                    harmonic_suppression: harmonic,
                    peak_picking: peak,
                    hysteresis_ratio: hyst,
                    ..DetectorConfig::default()
                };
                let (p, r, f1) = eval_subset(&entries, &det_config, &eval_config);
                println!(
                    "{:<6} {:<6} {:<6.1} {:>5.1} {:>5.1} {:>5.1}",
                    harmonic,
                    peak,
                    hyst,
                    p * 100.0,
                    r * 100.0,
                    f1 * 100.0,
                );
            }
        }
    }
}

/// Best threshold sweep with all enhancements enabled.
#[test]
fn sweep_threshold_enhanced() {
    let entries = test_subset();
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let eval_config = EvalConfig::default();
    println!("Threshold sweep (harm=true, peak=true, hyst=0.3):");
    println!("{:<12} {:>6} {:>6} {:>6}", "threshold", "P%", "R%", "F1%");

    for &threshold in &[
        0.00001, 0.00005, 0.0001, 0.0003, 0.0005, 0.001, 0.002, 0.005, 0.01,
    ] {
        let det_config = DetectorConfig {
            threshold,
            harmonic_suppression: true,
            peak_picking: true,
            hysteresis_ratio: 0.3,
            ..DetectorConfig::default()
        };
        let (p, r, f1) = eval_subset(&entries, &det_config, &eval_config);
        println!(
            "{:<12} {:>5.1} {:>5.1} {:>5.1}",
            threshold,
            p * 100.0,
            r * 100.0,
            f1 * 100.0,
        );
    }
}

/// Threshold sweep with generous tolerance to find recall ceiling.
#[test]
fn sweep_threshold_generous() {
    let entries = test_subset();
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let eval_config = EvalConfig {
        onset_tolerance: 0.3,
        pitch_tolerance: 0.5,
    };
    println!("Threshold sweep (generous 300ms tolerance, harm=true, peak=true):");
    println!("{:<12} {:>6} {:>6} {:>6}", "threshold", "P%", "R%", "F1%");

    for &threshold in &[
        0.000001, 0.000005, 0.00001, 0.00005, 0.0001, 0.0003, 0.001, 0.01,
    ] {
        let det_config = DetectorConfig {
            threshold,
            harmonic_suppression: true,
            peak_picking: true,
            hysteresis_ratio: 0.3,
            ..DetectorConfig::default()
        };
        let (p, r, f1) = eval_subset(&entries, &det_config, &eval_config);
        println!(
            "{:<12.6} {:>5.1} {:>5.1} {:>5.1}",
            threshold,
            p * 100.0,
            r * 100.0,
            f1 * 100.0,
        );
    }
}

/// Compare reference vs optimized config across solo recordings.
#[test]
fn compare_reference_vs_optimized() {
    let entries = test_subset();
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let eval_config = EvalConfig::default();

    let configs: Vec<(&str, DetectorConfig)> = vec![
        ("reference", DetectorConfig::default()),
        (
            "optimized",
            DetectorConfig {
                threshold: 0.0003,
                harmonic_suppression: true,
                peak_picking: true,
                hysteresis_ratio: 0.3,
                ..DetectorConfig::default()
            },
        ),
    ];

    for (name, det_config) in &configs {
        let (p, r, f1) = eval_subset(&entries, det_config, &eval_config);
        println!(
            "{:>12}: P={:.1}% R={:.1}% F1={:.1}%",
            name,
            p * 100.0,
            r * 100.0,
            f1 * 100.0,
        );
    }
}

/// Compare all feature combinations including new DI enhancements.
#[test]
fn compare_all_configs() {
    let entries = test_subset();
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let eval_config = EvalConfig::default();

    let configs: Vec<(&str, DetectorConfig)> = vec![
        ("reference", DetectorConfig::default()),
        (
            "old_optimized",
            DetectorConfig {
                threshold: 0.0003,
                harmonic_suppression: true,
                peak_picking: true,
                hysteresis_ratio: 0.3,
                ..DetectorConfig::default()
            },
        ),
        (
            "preprocess_only",
            DetectorConfig {
                threshold: 0.0003,
                harmonic_suppression: true,
                peak_picking: true,
                hysteresis_ratio: 0.3,
                preprocessing: true,
                ..DetectorConfig::default()
            },
        ),
        (
            "whitening_only",
            DetectorConfig {
                threshold: 0.0003,
                harmonic_suppression: true,
                peak_picking: true,
                hysteresis_ratio: 0.3,
                whitening: true,
                ..DetectorConfig::default()
            },
        ),
        (
            "klapuri_only",
            DetectorConfig {
                threshold: 0.0003,
                peak_picking: true,
                hysteresis_ratio: 0.3,
                klapuri: true,
                ..DetectorConfig::default()
            },
        ),
        (
            "adaptive_only",
            DetectorConfig {
                threshold: 0.0003,
                harmonic_suppression: true,
                peak_picking: true,
                hysteresis_ratio: 0.3,
                adaptive_threshold: true,
                ..DetectorConfig::default()
            },
        ),
        (
            "best_no_klapuri",
            DetectorConfig {
                threshold: 0.0003,
                harmonic_suppression: true,
                peak_picking: true,
                hysteresis_ratio: 0.3,
                whitening: true,
                ..DetectorConfig::default()
            },
        ),
        (
            "best_w_preproc",
            DetectorConfig {
                threshold: 0.0003,
                harmonic_suppression: true,
                peak_picking: true,
                hysteresis_ratio: 0.3,
                preprocessing: true,
                whitening: true,
                ..DetectorConfig::default()
            },
        ),
        (
            "best_w_adaptive",
            DetectorConfig {
                threshold: 0.0003,
                harmonic_suppression: true,
                peak_picking: true,
                hysteresis_ratio: 0.3,
                whitening: true,
                adaptive_threshold: true,
                ..DetectorConfig::default()
            },
        ),
        (
            "best_all_no_klap",
            DetectorConfig {
                threshold: 0.0003,
                harmonic_suppression: true,
                peak_picking: true,
                hysteresis_ratio: 0.3,
                preprocessing: true,
                whitening: true,
                adaptive_threshold: true,
                ..DetectorConfig::default()
            },
        ),
    ];

    println!("\n{:<20} {:>8} {:>8} {:>8}", "Config", "P%", "R%", "F1%");
    println!("{:-<48}", "");

    for (name, det_config) in &configs {
        let (p, r, f1) = eval_subset(&entries, det_config, &eval_config);
        println!(
            "{:<20} {:>7.1} {:>7.1} {:>7.1}",
            name,
            p * 100.0,
            r * 100.0,
            f1 * 100.0,
        );
    }
}

/// Exhaustive grid search for best parameters.
#[test]
fn grid_search_best_config() {
    let entries = test_subset();
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let eval_config = EvalConfig::default();

    println!(
        "\n{:<10} {:<6} {:<5} {:<5} {:<5} {:<5} {:>7} {:>7} {:>7}",
        "thresh", "hyst", "harm", "peak", "whit", "prep", "P%", "R%", "F1%"
    );
    println!("{:-<75}", "");

    let mut best_f1 = 0.0;
    let mut best_label = String::new();

    for &threshold in &[0.00005, 0.0001, 0.0003, 0.0005, 0.001, 0.003] {
        for &hysteresis in &[0.2, 0.3, 0.5, 1.0] {
            for &harmonic in &[false, true] {
                for &peak in &[false, true] {
                    for &whiten in &[false, true] {
                        for &preproc in &[false, true] {
                            let det_config = DetectorConfig {
                                threshold,
                                harmonic_suppression: harmonic,
                                peak_picking: peak,
                                hysteresis_ratio: hysteresis,
                                whitening: whiten,
                                preprocessing: preproc,
                                ..DetectorConfig::default()
                            };
                            let (p, r, f1) = eval_subset(&entries, &det_config, &eval_config);

                            if f1 > best_f1 {
                                best_f1 = f1;
                                best_label = format!(
                                    "t={} h={} harm={} peak={} whit={} prep={}",
                                    threshold, hysteresis, harmonic, peak, whiten, preproc
                                );
                                println!(
                                    "{:<10.5} {:<6.1} {:<5} {:<5} {:<5} {:<5} {:>6.1} {:>6.1} {:>6.1}  *** BEST",
                                    threshold, hysteresis, harmonic, peak, whiten, preproc,
                                    p * 100.0, r * 100.0, f1 * 100.0,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    println!("\nBest F1={:.1}%: {}", best_f1 * 100.0, best_label);
}

/// Sweep onset tolerance to see how much timing error costs us.
#[test]
fn sweep_onset_tolerance() {
    let entries = test_subset();
    if entries.is_empty() {
        eprintln!("Skipping: GuitarSet not found");
        return;
    }

    let det_config = DetectorConfig {
        threshold: 0.0003,
        harmonic_suppression: true,
        peak_picking: true,
        hysteresis_ratio: 0.3,
        ..DetectorConfig::default()
    };

    println!("Onset tolerance sweep (threshold=0.0003, enhanced):");
    println!("{:<12} {:>6} {:>6} {:>6}", "tol_ms", "P%", "R%", "F1%");

    for &tol_ms in &[25.0, 50.0, 75.0, 100.0, 150.0, 200.0, 300.0, 500.0] {
        let eval_config = EvalConfig {
            onset_tolerance: tol_ms / 1000.0,
            pitch_tolerance: 0.5,
        };
        let (p, r, f1) = eval_subset(&entries, &det_config, &eval_config);
        println!(
            "{:<12} {:>5.1} {:>5.1} {:>5.1}",
            tol_ms,
            p * 100.0,
            r * 100.0,
            f1 * 100.0,
        );
    }
}
