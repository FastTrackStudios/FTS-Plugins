//! Gate editor — Dioxus GUI root component.
//!
//! Layout: header with gate state + drum class, meters (IN/GR/OUT),
//! core gate knobs, sidechain group, drum classification, adaptive decay
//! readouts, and multi-instance sync controls.

use std::sync::atomic::Ordering;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::toggle::Toggle;
use audio_gui::prelude::{
    use_init_theme, ControlGroup, DragProvider, GrMeter, KnobSize, LevelMeterDb, SectionLabel,
};
use fts_plugin_core::prelude::*;

use crate::GateUiState;

/// Map drum_class u8 to display string.
fn drum_class_label(raw: u8) -> &'static str {
    match raw {
        0 => "KICK",
        1 => "SNARE",
        2 => "HI-HAT",
        3 => "TOM",
        _ => "—",
    }
}

/// Map drum_class u8 to color.
fn drum_class_color(raw: u8) -> &'static str {
    match raw {
        0 => "#ef5350", // red — kick
        1 => "#f0c040", // amber — snare
        2 => "#4ade80", // green — hi-hat
        3 => "#8b5cf6", // purple — tom
        _ => "#737380",
    }
}

/// Root editor component.
#[component]
pub fn App() -> Element {
    let shared = use_context::<SharedState>();
    let ui = shared.get::<GateUiState>().expect("GateUiState missing");
    let params = &ui.params;
    let t = use_init_theme();
    let t = *t.read();

    // Read metering values
    let gate_gain = ui.gate_gain.load(Ordering::Relaxed);
    let input_db = ui.input_peak_db.load(Ordering::Relaxed);
    let output_db = ui.output_peak_db.load(Ordering::Relaxed);
    let drum_class_raw = ui.drum_class.load(Ordering::Relaxed);
    let adaptive_hold = ui.adaptive_hold_ms.load(Ordering::Relaxed);
    let adaptive_release = ui.adaptive_release_ms.load(Ordering::Relaxed);
    let resonant_freq = ui.resonant_freq.load(Ordering::Relaxed);

    // Gate reduction in dB
    let gr_db = if gate_gain > 0.0001 {
        -20.0 * gate_gain.log10()
    } else {
        60.0
    };

    // Gate state text
    let gate_text = if gate_gain > 0.99 {
        "OPEN".to_string()
    } else if gate_gain < 0.01 {
        "CLOSED".to_string()
    } else {
        format!("-{:.1} dB", gr_db)
    };

    // Drum classification display
    let drum_label = drum_class_label(drum_class_raw);
    let drum_color = drum_class_color(drum_class_raw);

    // Adaptive decay active?
    let adaptive_active = params.adaptive_decay.value() > 0.5;

    // Resonant frequency display
    let freq_text = if resonant_freq > 0.0 {
        if resonant_freq >= 1000.0 {
            format!("{:.1} kHz", resonant_freq / 1000.0)
        } else {
            format!("{:.0} Hz", resonant_freq)
        }
    } else {
        "—".to_string()
    };

    let base_css = t.base_css();
    let root_style = t.root_style();
    let spacing_section = t.spacing_section;
    let spacing_label = t.spacing_label;
    let spacing_card = t.spacing_card;
    let spacing_control = t.spacing_control;
    let border = t.border;
    let border_subtle = t.border_subtle;
    let font_size_title = t.font_size_title;
    let font_size_tiny = t.font_size_tiny;
    let text_bright = t.text_bright;
    let text_dim = t.text_dim;
    let text = t.text;
    let accent = t.accent;
    let style_value = t.style_value();
    let style_card = t.style_card();
    let style_label = t.style_label();
    let signal_warn = t.signal_warn;
    let signal_safe = t.signal_safe;

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
                     padding-bottom:{spacing_label}; border-bottom:1px solid {border};",
                ),
                div {
                    style: "display:flex; align-items:baseline; gap:12px;",
                    div {
                        style: format!(
                            "font-size:{font_size_title}; font-weight:700; letter-spacing:0.5px; color:{text_bright};",
                        ),
                        "FTS GATE"
                    }
                    // Gate state
                    div {
                        style: format!(
                            "{style_value} color:{COLOR};",
                            COLOR = if gr_db > 6.0 { signal_warn }
                                    else if gr_db > 0.1 { signal_safe }
                                    else { text_dim },
                        ),
                        "{gate_text}"
                    }
                    // Drum class badge
                    if drum_class_raw < 255 {
                        div {
                            style: format!(
                                "font-size:9px; font-weight:700; letter-spacing:0.8px; \
                                 color:{color}; background:rgba(255,255,255,0.06); \
                                 padding:2px 6px; border-radius:3px; border:1px solid {color}30;",
                                color = drum_color,
                            ),
                            "{drum_label}"
                        }
                    }
                }
                div {
                    style: format!(
                        "font-size:{font_size_tiny}; color:{text_dim};",
                    ),
                    "FastTrackStudio"
                }
            }

            // ── Top row: Meters + Adaptive readouts ──────────────
            div {
                style: format!(
                    "display:flex; gap:{spacing_section}; min-height:0;",
                ),

                // Meters
                div {
                    style: format!(
                        "{style_card} padding:{spacing_card}; display:flex; gap:8px; align-items:stretch;",
                    ),
                    LevelMeterDb { level_db: input_db, label: "IN".to_string(), height: 140.0 }
                    GrMeter { gain_reduction_db: gr_db, height: 140.0 }
                    LevelMeterDb { level_db: output_db, label: "OUT".to_string(), height: 140.0 }
                }

                // Adaptive Decay readouts
                div {
                    style: format!(
                        "{style_card} padding:{spacing_card}; flex:1; display:flex; flex-direction:column; \
                         gap:{spacing_label}; min-width:0;",
                    ),
                    SectionLabel { text: "Adaptive Decay" }

                    // Readout grid
                    div {
                        style: "display:grid; grid-template-columns: 1fr 1fr 1fr; gap:12px; \
                                padding:8px 0;",

                        // Resonant frequency
                        div {
                            style: "display:flex; flex-direction:column; align-items:center; gap:4px;",
                            div {
                                style: format!("{style_label};"),
                                "Resonance"
                            }
                            div {
                                style: format!(
                                    "{style_value} font-size:16px; color:{COLOR};",
                                    COLOR = if adaptive_active && resonant_freq > 0.0 {
                                        accent
                                    } else {
                                        text_dim
                                    },
                                ),
                                "{freq_text}"
                            }
                        }

                        // Computed hold
                        div {
                            style: "display:flex; flex-direction:column; align-items:center; gap:4px;",
                            div {
                                style: format!("{style_label};"),
                                "Hold"
                            }
                            div {
                                style: format!(
                                    "{style_value} font-size:16px; color:{COLOR};",
                                    COLOR = if adaptive_active { text } else { text_dim },
                                ),
                                if adaptive_active {
                                    "{adaptive_hold:.0} ms"
                                } else {
                                    "—"
                                }
                            }
                        }

                        // Computed release
                        div {
                            style: "display:flex; flex-direction:column; align-items:center; gap:4px;",
                            div {
                                style: format!("{style_label};"),
                                "Release"
                            }
                            div {
                                style: format!(
                                    "{style_value} font-size:16px; color:{COLOR};",
                                    COLOR = if adaptive_active { text } else { text_dim },
                                ),
                                if adaptive_active {
                                    "{adaptive_release:.0} ms"
                                } else {
                                    "—"
                                }
                            }
                        }
                    }
                }
            }

            // ── Core Gate Controls (large knobs) ─────────────────
            div {
                style: format!(
                    "{style_card} padding:12px 16px; \
                     display:flex; flex-direction:column; gap:{spacing_section};",
                ),
                SectionLabel { text: "Gate" }
                div {
                    style: format!(
                        "display:flex; justify-content:center; gap:{spacing_control};",
                    ),
                    Knob { param_ptr: params.threshold_db.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.hysteresis_db.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.attack_ms.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.hold_ms.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.release_ms.as_ptr(), size: KnobSize::Large }
                }
            }

            // ── Secondary Controls Row ───────────────────────────
            div {
                style: format!(
                    "{style_card} padding:12px 16px; \
                     display:flex; gap:{spacing_control}; justify-content:center;",
                ),

                // Depth group
                ControlGroup {
                    label: "Depth",
                    Knob { param_ptr: params.range_db.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.lookahead_ms.as_ptr(), size: KnobSize::Medium }
                }

                // Divider
                div {
                    style: format!(
                        "width:1px; background:{border_subtle}; align-self:stretch;",
                    ),
                }

                // Sidechain group
                ControlGroup {
                    label: "Sidechain",
                    Knob { param_ptr: params.sc_hpf_freq.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.sc_lpf_freq.as_ptr(), size: KnobSize::Medium }
                    Toggle { param_ptr: params.sc_listen.as_ptr(), label: "Listen" }
                }

                // Divider
                div {
                    style: format!(
                        "width:1px; background:{border_subtle}; align-self:stretch;",
                    ),
                }

                // Drum Classification group
                ControlGroup {
                    label: "Drum Target",
                    Knob { param_ptr: params.drum_target.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.drum_strictness.as_ptr(), size: KnobSize::Medium }
                }

                // Divider
                div {
                    style: format!(
                        "width:1px; background:{border_subtle}; align-self:stretch;",
                    ),
                }

                // Adaptive Decay group
                ControlGroup {
                    label: "Adaptive",
                    Toggle { param_ptr: params.adaptive_decay.as_ptr(), label: "Decay" }
                    Knob { param_ptr: params.decay_sensitivity.as_ptr(), size: KnobSize::Medium }
                }

                // Divider
                div {
                    style: format!(
                        "width:1px; background:{border_subtle}; align-self:stretch;",
                    ),
                }

                // Sync group
                ControlGroup {
                    label: "Multi-Inst Sync",
                    Toggle { param_ptr: params.sync_enabled.as_ptr(), label: "Sync" }
                    Knob { param_ptr: params.sync_max_align_ms.as_ptr(), size: KnobSize::Medium }
                }
            }
        }
        } // DragProvider
    }
}
