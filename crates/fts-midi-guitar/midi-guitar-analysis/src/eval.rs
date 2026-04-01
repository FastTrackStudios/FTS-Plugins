//! Evaluation metrics for note detection accuracy.
//!
//! Computes precision, recall, and F1 using mir_eval-style note matching:
//! - Onset tolerance (default ±50ms)
//! - Pitch tolerance (default ±0.5 semitones)
//! - Optional offset matching

use midi_guitar_dsp::MidiGuitarDetector;

/// A reference (ground truth) note with timing.
#[derive(Debug, Clone)]
pub struct RefNote {
    /// Onset time in seconds.
    pub onset: f64,
    /// Offset time in seconds.
    pub offset: f64,
    /// MIDI note number (can be fractional for microtuning).
    pub midi_note: f64,
}

/// A detected note with timing.
#[derive(Debug, Clone)]
pub struct DetNote {
    /// Onset time in seconds.
    pub onset: f64,
    /// Offset time in seconds (if known).
    pub offset: Option<f64>,
    /// MIDI note number (integer).
    pub midi_note: u8,
    /// Velocity.
    pub velocity: f32,
}

/// Evaluation results for a single recording.
#[derive(Debug, Clone)]
pub struct EvalResult {
    pub name: String,
    /// Number of correctly detected notes (true positives).
    pub true_positives: usize,
    /// Number of spurious detections (false positives).
    pub false_positives: usize,
    /// Number of missed notes (false negatives).
    pub false_negatives: usize,
    /// Precision = TP / (TP + FP).
    pub precision: f64,
    /// Recall = TP / (TP + FN).
    pub recall: f64,
    /// F1 = 2 * P * R / (P + R).
    pub f1: f64,
}

/// Configuration for note matching.
#[derive(Debug, Clone)]
pub struct EvalConfig {
    /// Onset tolerance in seconds (default 0.05 = 50ms).
    pub onset_tolerance: f64,
    /// Pitch tolerance in semitones (default 0.5).
    pub pitch_tolerance: f64,
}

impl Default for EvalConfig {
    fn default() -> Self {
        Self {
            // 100ms tolerance accounts for analysis window latency (~20ms)
            // plus resonator group delay, matching mir_eval's default.
            onset_tolerance: 0.1,
            pitch_tolerance: 0.5,
        }
    }
}

/// Match detected notes against reference notes and compute metrics.
pub fn evaluate(
    name: &str,
    reference: &[RefNote],
    detected: &[DetNote],
    config: &EvalConfig,
) -> EvalResult {
    let mut ref_matched = vec![false; reference.len()];
    let mut det_matched = vec![false; detected.len()];

    // Greedy matching: for each detected note, find the closest unmatched reference.
    for (di, det) in detected.iter().enumerate() {
        let mut best_idx = None;
        let mut best_dist = f64::MAX;

        for (ri, reff) in reference.iter().enumerate() {
            if ref_matched[ri] {
                continue;
            }

            let onset_diff = (det.onset - reff.onset).abs();
            let pitch_diff = (det.midi_note as f64 - reff.midi_note).abs();

            if onset_diff <= config.onset_tolerance && pitch_diff <= config.pitch_tolerance {
                let dist = onset_diff + pitch_diff * 0.01; // weight onset more
                if dist < best_dist {
                    best_dist = dist;
                    best_idx = Some(ri);
                }
            }
        }

        if let Some(ri) = best_idx {
            ref_matched[ri] = true;
            det_matched[di] = true;
        }
    }

    let true_positives = det_matched.iter().filter(|&&m| m).count();
    let false_positives = det_matched.iter().filter(|&&m| !m).count();
    let false_negatives = ref_matched.iter().filter(|&&m| !m).count();

    let precision = if true_positives + false_positives > 0 {
        true_positives as f64 / (true_positives + false_positives) as f64
    } else {
        0.0
    };
    let recall = if true_positives + false_negatives > 0 {
        true_positives as f64 / (true_positives + false_negatives) as f64
    } else {
        0.0
    };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };

    EvalResult {
        name: name.to_string(),
        true_positives,
        false_positives,
        false_negatives,
        precision,
        recall,
        f1,
    }
}

/// Run the detector on audio samples and return detected notes with timing.
pub fn run_detector(samples: &[f64], sample_rate: f64, config: &DetectorConfig) -> Vec<DetNote> {
    let mut detector = MidiGuitarDetector::new();
    detector.set_sample_rate(sample_rate);
    detector.set_threshold(config.threshold);
    detector.set_sensitivity(config.sensitivity);
    detector.set_window_size(config.window_size);
    detector.set_note_range(config.low_note, config.high_note);
    detector.set_harmonic_suppression(config.harmonic_suppression);
    detector.set_peak_picking(config.peak_picking);
    detector.set_hysteresis_ratio(config.hysteresis_ratio);
    detector.set_preprocessing(config.preprocessing);
    detector.set_whitening(config.whitening);
    detector.set_adaptive_threshold(config.adaptive_threshold);
    detector.set_klapuri(config.klapuri);

    let mut detected = Vec::new();
    // Track active notes for offset timing.
    let mut active: std::collections::HashMap<u8, (f64, f32)> = std::collections::HashMap::new();

    // The detector fires at the END of each analysis window.
    // Compensate onset by subtracting the window duration so the reported
    // onset aligns with when the note actually started.
    let window_duration = config.window_size as f64 / sample_rate;

    for (i, &sample) in samples.iter().enumerate() {
        if let Some(events) = detector.process_sample(sample) {
            let time = (i as f64 / sample_rate - window_duration).max(0.0);
            for event in events {
                if event.is_on {
                    // If this note was already active, emit the previous one.
                    if let Some((onset, vel)) = active.remove(&event.note) {
                        detected.push(DetNote {
                            onset,
                            offset: Some(time),
                            midi_note: event.note,
                            velocity: vel,
                        });
                    }
                    active.insert(event.note, (time, event.velocity));
                } else {
                    if let Some((onset, vel)) = active.remove(&event.note) {
                        detected.push(DetNote {
                            onset,
                            offset: Some(time),
                            midi_note: event.note,
                            velocity: vel,
                        });
                    }
                }
            }
        }
    }

    // Flush remaining active notes.
    let end_time = samples.len() as f64 / sample_rate;
    for (note, (onset, vel)) in active {
        detected.push(DetNote {
            onset,
            offset: Some(end_time),
            midi_note: note,
            velocity: vel,
        });
    }

    detected.sort_by(|a, b| a.onset.partial_cmp(&b.onset).unwrap());
    detected
}

/// Detector configuration for evaluation runs.
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    pub threshold: f64,
    pub sensitivity: f64,
    pub window_size: usize,
    pub low_note: u8,
    pub high_note: u8,
    pub harmonic_suppression: bool,
    pub peak_picking: bool,
    pub hysteresis_ratio: f64,
    pub preprocessing: bool,
    pub whitening: bool,
    pub adaptive_threshold: bool,
    pub klapuri: bool,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        // Match reference implementation defaults exactly.
        Self {
            threshold: 0.001,
            sensitivity: 0.5,
            window_size: 960, // ~20ms at 48kHz
            low_note: 39,     // Reference: FIRST_NOTE_NUMBER = 39
            high_note: 89,    // Reference: LAST_NOTE_NUMBER = 89
            harmonic_suppression: false,
            peak_picking: false,
            hysteresis_ratio: 1.0,
            preprocessing: false,
            whitening: false,
            adaptive_threshold: false,
            klapuri: false,
        }
    }
}

/// Print a summary table of evaluation results.
pub fn print_summary(results: &[EvalResult]) {
    println!(
        "┌{:─<40}┬{:─<8}┬{:─<8}┬{:─<8}┬{:─<6}┬{:─<6}┬{:─<6}┐",
        "", "", "", "", "", "", ""
    );
    println!(
        "│{:<40}│{:>8}│{:>8}│{:>8}│{:>6}│{:>6}│{:>6}│",
        " Recording", " TP", " FP", " FN", " P", " R", " F1"
    );
    println!(
        "├{:─<40}┼{:─<8}┼{:─<8}┼{:─<8}┼{:─<6}┼{:─<6}┼{:─<6}┤",
        "", "", "", "", "", "", ""
    );

    for r in results {
        let name = if r.name.len() > 38 {
            &r.name[..38]
        } else {
            &r.name
        };
        println!(
            "│ {:<39}│{:>7} │{:>7} │{:>7} │{:>5.1}%│{:>5.1}%│{:>5.1}%│",
            name,
            r.true_positives,
            r.false_positives,
            r.false_negatives,
            r.precision * 100.0,
            r.recall * 100.0,
            r.f1 * 100.0,
        );
    }

    println!(
        "└{:─<40}┴{:─<8}┴{:─<8}┴{:─<8}┴{:─<6}┴{:─<6}┴{:─<6}┘",
        "", "", "", "", "", "", ""
    );

    // Aggregated metrics.
    if results.len() > 1 {
        let total_tp: usize = results.iter().map(|r| r.true_positives).sum();
        let total_fp: usize = results.iter().map(|r| r.false_positives).sum();
        let total_fn: usize = results.iter().map(|r| r.false_negatives).sum();
        let avg_p = if total_tp + total_fp > 0 {
            total_tp as f64 / (total_tp + total_fp) as f64
        } else {
            0.0
        };
        let avg_r = if total_tp + total_fn > 0 {
            total_tp as f64 / (total_tp + total_fn) as f64
        } else {
            0.0
        };
        let avg_f1 = if avg_p + avg_r > 0.0 {
            2.0 * avg_p * avg_r / (avg_p + avg_r)
        } else {
            0.0
        };
        println!(
            "\nAggregate: TP={} FP={} FN={} | P={:.1}% R={:.1}% F1={:.1}%",
            total_tp,
            total_fp,
            total_fn,
            avg_p * 100.0,
            avg_r * 100.0,
            avg_f1 * 100.0,
        );
    }
}
