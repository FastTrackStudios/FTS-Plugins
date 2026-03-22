//! Rider editor — Dioxus GUI root component.
//!
//! Layout: header with gain readout, waveform visualization,
//! metering strip, and organized knob groups.

use std::sync::atomic::Ordering;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::toggle::Toggle;
use audio_gui::prelude::{
    use_init_theme, ControlGroup, Divider, DragProvider, GrMeter, KnobSize, LevelMeterDb,
    PeakWaveform, SectionLabel,
};
use fts_plugin_core::prelude::*;

use crate::{RiderUiState, WAVEFORM_LEN};

/// Root editor component.
#[component]
pub fn App() -> Element {
    let shared = use_context::<SharedState>();
    let ui = shared.get::<RiderUiState>().expect("RiderUiState missing");
    let params = &ui.params;

    let t = use_init_theme();
    let t = *t.read();

    // Read metering values
    let gain = ui.gain_db.load(Ordering::Relaxed);
    let input_db = ui.input_peak_db.load(Ordering::Relaxed);
    let output_db = ui.output_peak_db.load(Ordering::Relaxed);

    // Build waveform history from ring buffer
    let pos = ui.waveform_pos.load(Ordering::Relaxed) as usize % WAVEFORM_LEN;
    let mut waveform_in = Vec::with_capacity(WAVEFORM_LEN);
    let mut waveform_gr = Vec::with_capacity(WAVEFORM_LEN);
    for i in 0..WAVEFORM_LEN {
        let idx = (pos + i) % WAVEFORM_LEN;
        waveform_in.push(ui.waveform_input[idx].load(Ordering::Relaxed));
        waveform_gr.push(ui.waveform_gain[idx].load(Ordering::Relaxed));
    }

    // Format gain display with color coding
    let gain_text = if gain.abs() < 0.05 {
        "0.0 dB".to_string()
    } else if gain > 0.0 {
        format!("+{gain:.1} dB")
    } else {
        format!("{gain:.1} dB")
    };

    let gain_color = if gain.abs() < 0.05 {
        t.text_dim
    } else if gain > 0.0 {
        t.signal_safe // green = boost
    } else {
        t.signal_warn // amber = cut
    };

    let base_css = t.base_css();
    let root_style = t.root_style();
    let style_card = t.style_card();
    let style_value = t.style_value();
    let spacing_section = t.spacing_section;
    let border = t.border;
    let font_size_title = t.font_size_title;
    let font_size_label = t.font_size_label;
    let text_dim = t.text_dim;
    let spacing_label = t.spacing_label;

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
                        "FTS RIDER"
                    }
                    div {
                        style: format!(
                            "{style_value} color:{gain_color};",
                        ),
                        "Gain: {gain_text}"
                    }
                }
                div {
                    style: format!("font-size:{font_size_label}; color:{text_dim};"),
                    "FastTrackStudio"
                }
            }

            // ── Visualization row ────────────────────────────────
            div {
                style: format!("display:flex; gap:{spacing_section}; min-height:0;"),

                // Waveform display
                div {
                    style: format!(
                        "{style_card} flex:1; display:flex; flex-direction:column; gap:{spacing_label}; min-width:0;",
                    ),
                    SectionLabel { text: "Waveform / Gain Ride" }
                    PeakWaveform {
                        levels: waveform_in,
                        gr_levels: waveform_gr,
                        width: 420.0,
                        height: 140.0,
                    }
                }

                // Meters
                div {
                    style: format!(
                        "{style_card} display:flex; gap:8px; align-items:stretch;",
                    ),
                    LevelMeterDb { level_db: input_db, label: "IN".to_string(), height: 140.0 }
                    GrMeter { gain_reduction_db: -gain, height: 140.0 }
                    LevelMeterDb { level_db: output_db, label: "OUT".to_string(), height: 140.0 }
                }
            }

            // ── Controls ─────────────────────────────────────────
            div {
                style: format!(
                    "{style_card} display:flex; flex-direction:column; gap:{spacing_section}; flex:1; min-height:0;",
                ),

                // Row 1: Primary "Magic Ride" knobs (large)
                div {
                    style: "display:flex; flex-direction:column; gap:8px;",
                    SectionLabel { text: "Ride" }
                    div {
                        style: "display:flex; justify-content:center; gap:32px;",
                        Knob { param_ptr: params.target_db.as_ptr(), size: KnobSize::Large }
                        Knob { param_ptr: params.range_db.as_ptr(), size: KnobSize::Large }
                        Knob { param_ptr: params.speed_ms.as_ptr(), size: KnobSize::Large }
                    }
                }

                // Row 2: Secondary controls
                div {
                    style: "display:flex; gap:20px; justify-content:center;",

                    // Gate group
                    ControlGroup {
                        label: "Gate",
                        Knob { param_ptr: params.gate_db.as_ptr(), size: KnobSize::Medium }
                    }

                    Divider {}

                    // Sidechain group
                    ControlGroup {
                        label: "Sidechain",
                        Knob { param_ptr: params.sc_freq.as_ptr(), size: KnobSize::Medium }
                        Toggle { param_ptr: params.sc_listen.as_ptr(), label: "Listen" }
                    }

                    Divider {}

                    // Output group
                    ControlGroup {
                        label: "Output",
                        Knob { param_ptr: params.output_gain_db.as_ptr(), size: KnobSize::Medium }
                    }

                    Divider {}

                    // Mode group
                    ControlGroup {
                        label: "Detection",
                        Toggle { param_ptr: params.detect_mode.as_ptr(), label: "Mode" }
                    }
                }
            }
        }
        } // DragProvider
    }
}
