//! MIDI Guitar editor — Dioxus GUI root component.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::toggle::Toggle;
use audio_gui::prelude::{
    use_init_theme, use_theme, DragProvider, KnobSize, LevelMeterDb, SectionLabel,
};
use fts_plugin_core::prelude::*;

use crate::{FtsMidiGuitarParams, MidiGuitarUiState};

/// Root editor component.
#[component]
pub fn App() -> Element {
    let shared = use_context::<SharedState>();
    let ui: Arc<MidiGuitarUiState> = shared
        .get::<MidiGuitarUiState>()
        .expect("MidiGuitarUiState missing");
    let params: Arc<FtsMidiGuitarParams> = ui.params.clone();

    let t = use_init_theme();
    let t = *t.read();

    // Read metering values.
    let input_db = ui.input_peak_db.load(Ordering::Relaxed);
    let note_count = ui.active_note_count.load(Ordering::Relaxed);

    // Build active notes display.
    let lo = ui.active_notes_lo.load(Ordering::Relaxed);
    let hi = ui.active_notes_hi.load(Ordering::Relaxed);
    let active_notes_text = format_active_notes(lo, hi);

    // Extract ParamPtrs for Knob components.
    let threshold_ptr = params.threshold.as_ptr();
    let sensitivity_ptr = params.sensitivity.as_ptr();
    let window_ptr = params.window_ms.as_ptr();
    let channel_ptr = params.channel.as_ptr();
    let low_note_ptr = params.lowest_note.as_ptr();
    let high_note_ptr = params.highest_note.as_ptr();
    let harmonic_ptr = params.harmonic_suppression.as_ptr();

    let base_css = t.base_css();
    let root_style = t.root_style();
    let spacing_section = t.spacing_section;
    let border = t.border;
    let font_size_title = t.font_size_title;
    let style_value = t.style_value();
    let text_dim = t.text_dim;
    let font_size_label = t.font_size_label;
    let style_card = t.style_card();

    rsx! {
        document::Style { {base_css} }

        DragProvider {
        div {
            style: format!(
                "{root_style} display:flex; flex-direction:column; gap:{spacing_section}; overflow:hidden;",
            ),

            // ── Header ───────────────────────────────────────────
            div {
                style: format!(
                    "display:flex; justify-content:space-between; align-items:center; \
                     padding-bottom:6px; border-bottom:1px solid {border};",
                ),
                div {
                    style: "display:flex; align-items:baseline; gap:12px;",
                    div {
                        style: format!("font-size:{font_size_title}; font-weight:700; letter-spacing:0.5px;"),
                        "FTS MIDI GUITAR"
                    }
                    div {
                        style: format!(
                            "{style_value} color:{text_dim};",
                        ),
                        "{note_count} notes"
                    }
                }
                div {
                    style: format!("font-size:{font_size_label}; color:{text_dim};"),
                    "FastTrackStudio"
                }
            }

            // ── Main controls + Meters ───────────────────────────
            div {
                style: format!("display:flex; gap:{spacing_section}; flex:1; min-height:0;"),

                // Controls card
                div {
                    style: format!(
                        "{style_card} flex:1; display:flex; flex-direction:column; gap:12px;",
                    ),

                    // Detection controls
                    div {
                        style: "display:flex; flex-direction:column; gap:8px;",
                        SectionLabel { text: "Detection" }
                        div {
                            style: "display:flex; justify-content:center; gap:24px;",
                            Knob { param_ptr: threshold_ptr, size: KnobSize::Large }
                            Knob { param_ptr: sensitivity_ptr, size: KnobSize::Large }
                            Knob { param_ptr: window_ptr, size: KnobSize::Large }
                        }
                    }

                    // MIDI settings
                    div {
                        style: "display:flex; flex-direction:column; gap:8px;",
                        SectionLabel { text: "MIDI" }
                        div {
                            style: "display:flex; justify-content:center; gap:24px;",
                            Knob { param_ptr: channel_ptr, size: KnobSize::Medium }
                            Knob { param_ptr: low_note_ptr, size: KnobSize::Medium }
                            Knob { param_ptr: high_note_ptr, size: KnobSize::Medium }
                        }
                    }

                    // Options
                    div {
                        style: "display:flex; align-items:center; gap:16px;",
                        Toggle { param_ptr: harmonic_ptr, label: "Harmonic Suppression" }
                    }
                }

                // Right column: meter + note display
                div {
                    style: "display:flex; gap:8px; align-items:stretch;",

                    // Note keyboard display
                    NoteDisplay { lo: lo, hi: hi, active_text: active_notes_text }

                    // Meter
                    div {
                        style: format!(
                            "{style_card} display:flex; gap:8px; align-items:stretch;",
                        ),
                        LevelMeterDb { level_db: input_db, label: "IN".to_string(), height: 240.0 }
                    }
                }
            }
        }
        } // DragProvider
    }
}

/// Vertical mini-keyboard showing active notes.
#[component]
fn NoteDisplay(lo: u64, hi: u64, active_text: String) -> Element {
    let t = use_theme();
    let t = *t.read();

    // Show notes from E2 (40) to E6 (88) — guitar range.
    let low = 39u8;
    let high = 89u8;

    let style_card = t.style_card();
    let spacing_label = t.spacing_label;
    let style_label = t.style_label();
    let radius_small = t.radius_small;
    let signal_safe = t.signal_safe;
    let surface = t.surface;
    let surface_raised = t.surface_raised;
    let bg = t.bg;
    let font_size_label = t.font_size_label;
    let font_mono = t.font_mono;
    let text = t.text;

    rsx! {
        div {
            style: format!(
                "{style_card} display:flex; flex-direction:column; gap:{spacing_label}; width:90px;",
            ),

            div {
                style: format!(
                    "{style_label} text-align:center; margin-bottom:2px;",
                ),
                "Notes"
            }

            // Mini keyboard: vertical strip of note cells
            div {
                style: format!(
                    "flex:1; display:flex; flex-direction:column-reverse; gap:0px; \
                     overflow:hidden; border-radius:{radius_small};",
                ),

                for midi_note in low..=high {
                    {
                        let is_active = if midi_note < 64 {
                            lo & (1u64 << midi_note) != 0
                        } else {
                            hi & (1u64 << (midi_note - 64)) != 0
                        };
                        let is_black = matches!(midi_note % 12, 1 | 3 | 6 | 8 | 10);

                        let note_bg = if is_active {
                            signal_safe.to_string()
                        } else if is_black {
                            surface.to_string()
                        } else {
                            surface_raised.to_string()
                        };

                        rsx! {
                            div {
                                key: "{midi_note}",
                                style: format!(
                                    "height:4px; background:{note_bg}; \
                                     border-bottom:1px solid {bg};",
                                ),
                            }
                        }
                    }
                }
            }

            // Active notes text
            div {
                style: format!(
                    "font-size:{font_size_label}; font-family:{font_mono}; text-align:center; \
                     color:{text}; margin-top:{spacing_label}; min-height:28px; line-height:14px; \
                     word-break:break-all;",
                ),
                "{active_text}"
            }
        }
    }
}

/// Format active MIDI notes as a string like "E2 A2 D3".
fn format_active_notes(lo: u64, hi: u64) -> String {
    const NAMES: &[&str] = &[
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];

    let mut notes = Vec::new();
    for bit in 0..64 {
        if lo & (1 << bit) != 0 {
            let octave = (bit as i32 / 12) - 1;
            notes.push(format!("{}{}", NAMES[(bit % 12) as usize], octave));
        }
    }
    for bit in 0..64 {
        if hi & (1 << bit) != 0 {
            let note = bit + 64;
            let octave = (note as i32 / 12) - 1;
            notes.push(format!("{}{}", NAMES[(note % 12) as usize], octave));
        }
    }

    if notes.is_empty() {
        "\u{2014}".to_string()
    } else {
        notes.join(" ")
    }
}
