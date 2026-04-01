//! JAMS (JSON Annotated Music Specification) parser for GuitarSet annotations.
//!
//! Extracts `note_midi` annotations: per-string note events with time, duration,
//! and fractional MIDI note number.

use serde::Deserialize;
use std::path::Path;

/// A single note event from a JAMS annotation.
#[derive(Debug, Clone)]
pub struct JamsNote {
    /// Onset time in seconds.
    pub time: f64,
    /// Duration in seconds.
    pub duration: f64,
    /// MIDI note number (fractional — e.g. 44.02 for slightly sharp).
    pub midi_note: f64,
    /// Guitar string index (0 = low E, 5 = high E).
    pub string: usize,
}

/// All note annotations from a JAMS file.
#[derive(Debug)]
pub struct JamsAnnotation {
    pub notes: Vec<JamsNote>,
    pub tempo: Option<f64>,
    pub key: Option<String>,
}

// ── Serde types for JAMS JSON ────────────────────────────────────────

#[derive(Deserialize)]
struct JamsFile {
    annotations: Vec<Annotation>,
}

#[derive(Deserialize)]
struct Annotation {
    namespace: Option<String>,
    data: AnnotationData,
    #[serde(default)]
    annotation_metadata: Option<AnnotationMetadata>,
}

/// JAMS annotation data can be either a list of observations or
/// a dict with parallel arrays (used for pitch_contour).
#[derive(Deserialize)]
#[serde(untagged)]
enum AnnotationData {
    List(Vec<Observation>),
    Parallel(ParallelData),
}

#[derive(Deserialize)]
struct Observation {
    time: f64,
    duration: f64,
    value: serde_json::Value,
}

#[derive(Deserialize)]
struct ParallelData {
    time: Vec<f64>,
    duration: Vec<f64>,
    value: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct AnnotationMetadata {
    data_source: Option<serde_json::Value>,
}

/// Parse a JAMS file and extract all `note_midi` annotations.
pub fn parse_jams(path: &Path) -> Result<JamsAnnotation, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let jams: JamsFile =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse JAMS: {e}"))?;

    let mut notes = Vec::new();
    let mut tempo = None;
    let mut key = None;

    for ann in &jams.annotations {
        let ns = ann.namespace.as_deref().unwrap_or("");

        match ns {
            "note_midi" => {
                // Determine string index from annotation_metadata.data_source.
                let string_idx = ann
                    .annotation_metadata
                    .as_ref()
                    .and_then(|m| m.data_source.as_ref())
                    .and_then(|v| match v {
                        serde_json::Value::Number(n) => n.as_u64().map(|n| n as usize),
                        serde_json::Value::String(s) => s.parse().ok(),
                        _ => None,
                    })
                    .unwrap_or(0);

                match &ann.data {
                    AnnotationData::List(obs) => {
                        for o in obs {
                            if let Some(midi) = o.value.as_f64() {
                                notes.push(JamsNote {
                                    time: o.time,
                                    duration: o.duration,
                                    midi_note: midi,
                                    string: string_idx,
                                });
                            }
                        }
                    }
                    AnnotationData::Parallel(p) => {
                        for i in 0..p.time.len() {
                            if let Some(midi) = p.value.get(i).and_then(|v| v.as_f64()) {
                                notes.push(JamsNote {
                                    time: p.time[i],
                                    duration: p.duration[i],
                                    midi_note: midi,
                                    string: string_idx,
                                });
                            }
                        }
                    }
                }
            }
            "tempo" => {
                if let AnnotationData::List(obs) = &ann.data {
                    if let Some(o) = obs.first() {
                        tempo = o.value.as_f64();
                    }
                }
            }
            "key_mode" => {
                if let AnnotationData::List(obs) = &ann.data {
                    if let Some(o) = obs.first() {
                        key = o.value.as_str().map(String::from);
                    }
                }
            }
            _ => {}
        }
    }

    // Sort by onset time.
    notes.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());

    Ok(JamsAnnotation { notes, tempo, key })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_guitarset_jams() {
        let path = Path::new(
            "/home/cody/Development/mir-datasets/data/guitarset/annotation/00_BN1-129-Eb_comp.jams",
        );
        if !path.exists() {
            eprintln!("Skipping: GuitarSet not found at {}", path.display());
            return;
        }

        let ann = parse_jams(path).unwrap();
        assert!(!ann.notes.is_empty(), "Should have parsed notes");
        assert!(ann.tempo.is_some(), "Should have tempo");

        // Check we got notes from multiple strings.
        let strings_used: std::collections::HashSet<_> =
            ann.notes.iter().map(|n| n.string).collect();
        assert!(
            strings_used.len() > 1,
            "Should have notes from multiple strings, got: {:?}",
            strings_used
        );

        println!(
            "Parsed {} notes across {} strings, tempo={:?}, key={:?}",
            ann.notes.len(),
            strings_used.len(),
            ann.tempo,
            ann.key
        );
        for n in ann.notes.iter().take(5) {
            println!(
                "  t={:.3}s dur={:.3}s midi={:.1} string={}",
                n.time, n.duration, n.midi_note, n.string
            );
        }
    }
}
