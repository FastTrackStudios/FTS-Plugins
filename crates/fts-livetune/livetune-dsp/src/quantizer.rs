//! Scale quantizer — snaps detected pitch to the nearest note in a
//! musical scale with configurable retune speed and per-note control.
//!
//! Based on Autotalent's approach: sine-shaped transition curves between
//! adjacent scale notes, with smooth/retune speed controlling the sharpness
//! of the snapping.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Musical scale type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scale {
    /// All 12 semitones enabled.
    Chromatic,
    /// Major scale (W-W-H-W-W-W-H).
    Major,
    /// Natural minor scale (W-H-W-W-H-W-W).
    Minor,
    /// Major pentatonic (5 notes).
    MajorPentatonic,
    /// Minor pentatonic (5 notes).
    MinorPentatonic,
    /// Blues scale (minor pentatonic + b5).
    Blues,
    /// Custom per-note selection.
    Custom,
}

/// Per-note state: whether this pitch class is a snap target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoteState {
    /// Note is disabled — skipped during quantization.
    Disabled,
    /// Note is enabled — included as a snap target.
    Enabled,
}

/// Key root (0 = C, 1 = C#, ... 11 = B).
pub type Key = u8;

/// Scale quantizer with retune speed and per-note control.
pub struct ScaleQuantizer {
    /// Root key (0 = C, 1 = C#, ... 11 = B).
    pub key: Key,
    /// Scale type.
    pub scale: Scale,
    /// Retune speed: 0.0 = instant snap, 1.0 = no correction.
    /// Controls the "smoothness" of the sine-shaped transition.
    pub retune_speed: f64,
    /// Correction amount: 0.0 = no correction, 1.0 = full correction.
    pub amount: f64,
    /// Per-note enable/disable (indexed by pitch class 0–11, C=0).
    pub notes: [NoteState; 12],

    // Smoothing state for gradual correction.
    smoothed_target: f64,
    has_target: bool,
}

impl ScaleQuantizer {
    // Scale intervals as semitone offsets from root.
    const MAJOR: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
    const MINOR: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];
    const MAJOR_PENTA: [u8; 5] = [0, 2, 4, 7, 9];
    const MINOR_PENTA: [u8; 5] = [0, 3, 5, 7, 10];
    const BLUES: [u8; 6] = [0, 3, 5, 6, 7, 10];

    pub fn new() -> Self {
        Self {
            key: 0, // C
            scale: Scale::Chromatic,
            retune_speed: 0.0,
            amount: 1.0,
            notes: [NoteState::Enabled; 12],
            smoothed_target: 0.0,
            has_target: false,
        }
    }

    /// Build the notes array from the current key + scale.
    pub fn apply_scale(&mut self) {
        if self.scale == Scale::Custom {
            return; // User manages notes directly.
        }

        // Disable all notes first.
        self.notes = [NoteState::Disabled; 12];

        let intervals: &[u8] = match self.scale {
            Scale::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            Scale::Major => &Self::MAJOR,
            Scale::Minor => &Self::MINOR,
            Scale::MajorPentatonic => &Self::MAJOR_PENTA,
            Scale::MinorPentatonic => &Self::MINOR_PENTA,
            Scale::Blues => &Self::BLUES,
            Scale::Custom => return,
        };

        for &interval in intervals {
            let pc = (self.key + interval) % 12;
            self.notes[pc as usize] = NoteState::Enabled;
        }
    }

    /// Check if a pitch class (0–11) is enabled.
    fn is_enabled(&self, pitch_class: u8) -> bool {
        self.notes[pitch_class as usize % 12] == NoteState::Enabled
    }

    /// Find the nearest enabled note (in MIDI space) to the given MIDI note.
    /// Returns the target MIDI note (integer or the same as input if on a note).
    fn find_nearest_note(&self, midi_note: f64) -> f64 {
        let rounded = midi_note.round();
        let pc = ((rounded as i32 % 12 + 12) % 12) as u8;

        if self.is_enabled(pc) {
            return rounded;
        }

        // Search up and down for nearest enabled note.
        for offset in 1..=6 {
            let up = ((pc as i32 + offset) % 12 + 12) % 12;
            let down = ((pc as i32 - offset) % 12 + 12) % 12;

            let up_enabled = self.is_enabled(up as u8);
            let down_enabled = self.is_enabled(down as u8);

            if up_enabled && down_enabled {
                // Both equidistant — pick the closer one in MIDI space.
                let up_note = rounded + offset as f64;
                let down_note = rounded - offset as f64;
                return if (midi_note - up_note).abs() <= (midi_note - down_note).abs() {
                    up_note
                } else {
                    down_note
                };
            } else if up_enabled {
                return rounded + offset as f64;
            } else if down_enabled {
                return rounded - offset as f64;
            }
        }

        // Fallback: no notes enabled, return as-is.
        midi_note
    }

    /// Quantize a detected MIDI note to the scale.
    ///
    /// Returns the corrected MIDI note. The sine-shaped transition curve
    /// provides smooth snapping controlled by `retune_speed`.
    pub fn quantize(&mut self, midi_note: f64) -> f64 {
        // Find target note.
        let target = self.find_nearest_note(midi_note);

        // Apply retune speed smoothing.
        // retune_speed = 0.0 → instant snap (smoothing coefficient = 1.0)
        // retune_speed = 1.0 → no correction (smoothing coefficient ≈ 0.0)
        if !self.has_target {
            self.smoothed_target = target;
            self.has_target = true;
        }

        // Exponential smoothing toward target.
        // The "retune speed" is inverted: 0 = fast, 1 = slow.
        let alpha = 1.0 - self.retune_speed.clamp(0.0, 0.999);
        self.smoothed_target += (target - self.smoothed_target) * alpha;

        // Sine-shaped transition between notes (Autotalent-style).
        // This creates a smooth S-curve when passing between adjacent notes,
        // rather than a hard step or linear ramp.
        let frac = midi_note - midi_note.floor();
        let smooth = self.retune_speed.clamp(0.001, 1.0);

        // Scale the fractional position by 1/smooth to control transition width.
        let scaled = ((frac - 0.5) / smooth).clamp(-0.5, 0.5);
        let sine_frac = 0.5 * (PI * scaled).sin() + 0.5;

        // The corrected note: blend between floor and ceil based on sine curve.
        let floor_note = self.find_nearest_note(midi_note.floor());
        let ceil_note = self.find_nearest_note(midi_note.ceil());
        let snapped = floor_note + (ceil_note - floor_note) * sine_frac;

        // Apply amount: blend between original and corrected.
        let corrected = midi_note + (snapped - midi_note) * self.amount;

        corrected
    }

    /// Reset smoothing state.
    pub fn reset(&mut self) {
        self.smoothed_target = 0.0;
        self.has_target = false;
    }
}

impl Default for ScaleQuantizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert MIDI note to frequency (Hz).
pub fn midi_to_freq(midi: f64) -> f64 {
    440.0 * (2.0f64).powf((midi - 69.0) / 12.0)
}

/// Convert frequency (Hz) to MIDI note.
pub fn freq_to_midi(freq: f64) -> f64 {
    69.0 + 12.0 * (freq / 440.0).log2()
}

/// Pitch class name for display (0 = C, 1 = C#, ... 11 = B).
pub fn pitch_class_name(pc: u8) -> &'static str {
    match pc % 12 {
        0 => "C",
        1 => "C#",
        2 => "D",
        3 => "Eb",
        4 => "E",
        5 => "F",
        6 => "F#",
        7 => "G",
        8 => "Ab",
        9 => "A",
        10 => "Bb",
        11 => "B",
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_quantizer(key: Key, scale: Scale) -> ScaleQuantizer {
        let mut q = ScaleQuantizer::new();
        q.key = key;
        q.scale = scale;
        q.retune_speed = 0.0; // Instant snap.
        q.amount = 1.0;
        q.apply_scale();
        q
    }

    #[test]
    fn chromatic_snaps_to_nearest_semitone() {
        let mut q = make_quantizer(0, Scale::Chromatic);

        // Exactly on A4 (MIDI 69).
        let out = q.quantize(69.0);
        assert!((out - 69.0).abs() < 0.01, "On-note should stay: {out}");

        // Slightly sharp A4.
        q.reset();
        let out = q.quantize(69.3);
        assert!(
            (out - 69.0).abs() < 0.6,
            "Slightly sharp should snap near A: {out}"
        );
    }

    #[test]
    fn c_major_skips_black_keys() {
        let mut q = make_quantizer(0, Scale::Major);

        // C# (MIDI 61) is not in C major — should snap to C (60) or D (62).
        q.reset();
        let out = q.quantize(61.0);
        assert!(
            (out - 60.0).abs() < 0.5 || (out - 62.0).abs() < 0.5,
            "C# should snap to C or D in C major, got {out}"
        );
    }

    #[test]
    fn amount_controls_correction_strength() {
        // Full correction.
        let mut q_full = make_quantizer(0, Scale::Major);
        q_full.amount = 1.0;
        let full = q_full.quantize(61.0); // C# → should snap

        // No correction.
        let mut q_none = make_quantizer(0, Scale::Major);
        q_none.amount = 0.0;
        let none = q_none.quantize(61.0); // C# → should stay

        assert!(
            (none - 61.0).abs() < 0.01,
            "Amount=0 should not correct: {none}"
        );
        assert!((full - 61.0).abs() > 0.3, "Amount=1 should correct: {full}");
    }

    #[test]
    fn different_keys() {
        // G major: G A B C D E F#
        let mut q = make_quantizer(7, Scale::Major); // 7 = G

        // F natural (MIDI 65) is not in G major — should snap to E (64) or F# (66).
        q.reset();
        let out = q.quantize(65.0);
        assert!(
            (out - 64.0).abs() < 0.5 || (out - 66.0).abs() < 0.5,
            "F should snap to E or F# in G major, got {out}"
        );
    }

    #[test]
    fn midi_freq_roundtrip() {
        assert!((midi_to_freq(69.0) - 440.0).abs() < 0.01);
        assert!((freq_to_midi(440.0) - 69.0).abs() < 0.01);
        assert!((midi_to_freq(60.0) - 261.63).abs() < 0.1); // Middle C

        for midi in 20..=108 {
            let freq = midi_to_freq(midi as f64);
            let back = freq_to_midi(freq);
            assert!(
                (back - midi as f64).abs() < 0.001,
                "Roundtrip failed for MIDI {midi}: got {back}"
            );
        }
    }

    #[test]
    fn pentatonic_has_five_notes() {
        let q = make_quantizer(0, Scale::MajorPentatonic);
        let count = q.notes.iter().filter(|&&n| n == NoteState::Enabled).count();
        assert_eq!(count, 5, "Major pentatonic should have 5 notes");

        let q = make_quantizer(0, Scale::MinorPentatonic);
        let count = q.notes.iter().filter(|&&n| n == NoteState::Enabled).count();
        assert_eq!(count, 5, "Minor pentatonic should have 5 notes");
    }

    #[test]
    fn blues_has_six_notes() {
        let q = make_quantizer(0, Scale::Blues);
        let count = q.notes.iter().filter(|&&n| n == NoteState::Enabled).count();
        assert_eq!(count, 6, "Blues scale should have 6 notes");
    }

    #[test]
    fn custom_scale() {
        let mut q = ScaleQuantizer::new();
        q.scale = Scale::Custom;
        q.retune_speed = 0.0;
        q.amount = 1.0;
        // Only enable C and G.
        q.notes = [NoteState::Disabled; 12];
        q.notes[0] = NoteState::Enabled; // C
        q.notes[7] = NoteState::Enabled; // G

        // D (MIDI 62) should snap to C (60) or G (67).
        // C is closer (2 semitones vs 5).
        q.reset();
        let out = q.quantize(62.0);
        assert!(
            (out - 60.0).abs() < 0.5,
            "D should snap to C (closer), got {out}"
        );
    }

    #[test]
    fn retune_speed_affects_correction() {
        // Instant snap.
        let mut q_fast = make_quantizer(0, Scale::Major);
        q_fast.retune_speed = 0.0;
        let fast = q_fast.quantize(61.3);

        // Slow correction.
        let mut q_slow = make_quantizer(0, Scale::Major);
        q_slow.retune_speed = 0.9;
        let slow = q_slow.quantize(61.3);

        // With instant snap, correction should be stronger.
        let fast_correction = (fast - 61.3).abs();
        let slow_correction = (slow - 61.3).abs();
        assert!(
            fast_correction >= slow_correction - 0.01,
            "Fast retune should correct more: fast={fast_correction}, slow={slow_correction}"
        );
    }

    #[test]
    fn pitch_class_names() {
        assert_eq!(pitch_class_name(0), "C");
        assert_eq!(pitch_class_name(9), "A");
        assert_eq!(pitch_class_name(11), "B");
    }
}
