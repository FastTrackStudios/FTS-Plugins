//! MIDI file parser for Guitar-TECHS annotations.
//!
//! Extracts note-on/note-off events with timing from standard MIDI files.

use std::path::Path;

/// A note event extracted from a MIDI file.
#[derive(Debug, Clone)]
pub struct MidiFileNote {
    /// Onset time in seconds.
    pub time: f64,
    /// Duration in seconds.
    pub duration: f64,
    /// MIDI note number (integer).
    pub note: u8,
    /// Velocity (0..127 mapped to 0.0..1.0).
    pub velocity: f32,
    /// MIDI channel.
    pub channel: u8,
}

/// Parse a MIDI file and extract all note events with absolute timing in seconds.
pub fn parse_midi_file(path: &Path) -> Result<Vec<MidiFileNote>, String> {
    let data =
        std::fs::read(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let smf = midly::Smf::parse(&data).map_err(|e| format!("Failed to parse MIDI: {e}"))?;

    let ticks_per_beat = match smf.header.timing {
        midly::Timing::Metrical(tpb) => tpb.as_int() as f64,
        midly::Timing::Timecode(fps, sub) => {
            // For timecode, approximate ticks per beat assuming 120 BPM.
            let ticks_per_sec = fps.as_f32() as f64 * sub as f64;
            ticks_per_sec * 0.5 // 120 BPM = 0.5 sec per beat
        }
    };

    let mut notes = Vec::new();

    for track in &smf.tracks {
        let mut tempo_changes: Vec<(u64, f64)> = Vec::new();

        // First pass: collect tempo changes.
        let mut tick = 0u64;
        for event in track {
            tick += event.delta.as_int() as u64;
            if let midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(t)) = event.kind {
                tempo_changes.push((tick, t.as_int() as f64));
            }
        }

        // Helper: convert tick to seconds using tempo map.
        let tick_to_seconds = |target_tick: u64| -> f64 {
            let mut current_tick: u64 = 0;
            let mut current_tempo = 500_000.0;
            let mut time_s = 0.0;

            for &(change_tick, new_tempo) in &tempo_changes {
                if change_tick >= target_tick {
                    break;
                }
                let delta_ticks = change_tick.saturating_sub(current_tick);
                time_s += delta_ticks as f64 * current_tempo / (ticks_per_beat * 1_000_000.0);
                current_tick = change_tick;
                current_tempo = new_tempo;
            }

            let remaining = target_tick.saturating_sub(current_tick);
            time_s += remaining as f64 * current_tempo / (ticks_per_beat * 1_000_000.0);
            time_s
        };

        // Second pass: extract note events.
        // Track active notes for duration calculation.
        // Key: (channel, note), Value: (onset_tick, velocity)
        let mut active: std::collections::HashMap<(u8, u8), (u64, f32)> =
            std::collections::HashMap::new();

        let mut abs_tick: u64 = 0;
        for event in track {
            abs_tick += event.delta.as_int() as u64;

            match event.kind {
                midly::TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    match message {
                        midly::MidiMessage::NoteOn { key, vel } => {
                            let note = key.as_int();
                            let velocity = vel.as_int();
                            if velocity == 0 {
                                // Note-on with velocity 0 = note-off.
                                if let Some((onset_tick, vel)) = active.remove(&(ch, note)) {
                                    let onset = tick_to_seconds(onset_tick);
                                    let offset = tick_to_seconds(abs_tick);
                                    notes.push(MidiFileNote {
                                        time: onset,
                                        duration: offset - onset,
                                        note,
                                        velocity: vel,
                                        channel: ch,
                                    });
                                }
                            } else {
                                active.insert((ch, note), (abs_tick, velocity as f32 / 127.0));
                            }
                        }
                        midly::MidiMessage::NoteOff { key, .. } => {
                            let note = key.as_int();
                            if let Some((onset_tick, vel)) = active.remove(&(ch, note)) {
                                let onset = tick_to_seconds(onset_tick);
                                let offset = tick_to_seconds(abs_tick);
                                notes.push(MidiFileNote {
                                    time: onset,
                                    duration: offset - onset,
                                    note,
                                    velocity: vel,
                                    channel: ch,
                                });
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    notes.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_guitar_techs_midi() {
        let path = Path::new(
            "/home/cody/Downloads/Guitar-TECHS/P1_singlenotes/midi/midi_allsinglenotes.mid",
        );
        if !path.exists() {
            eprintln!("Skipping: Guitar-TECHS not found at {}", path.display());
            return;
        }

        let notes = parse_midi_file(path).unwrap();
        assert!(!notes.is_empty(), "Should have parsed notes");

        println!("Parsed {} notes from Guitar-TECHS MIDI", notes.len());
        for n in notes.iter().take(10) {
            println!(
                "  t={:.3}s dur={:.3}s note={} vel={:.2} ch={}",
                n.time, n.duration, n.note, n.velocity, n.channel
            );
        }
    }
}
