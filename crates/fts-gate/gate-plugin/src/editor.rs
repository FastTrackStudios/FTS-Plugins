//! Gate editor — Dioxus GUI root component.
//!
//! Layout: header with gate state + drum class, meters (IN/GR/OUT),
//! core gate knobs, sidechain group, drum classification, adaptive decay
//! readouts, and multi-instance sync controls.

use std::sync::atomic::Ordering;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::toggle::Toggle;
use audio_gui::prelude::{
    theme, ControlGroup, DragProvider, GrMeter, KnobSize, LevelMeterDb, SectionLabel,
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

    rsx! {
        document::Style { {theme::BASE_CSS} }

        DragProvider {
        div {
            style: format!(
                "{ROOT} display:flex; flex-direction:column; gap:{SECTION}; overflow:hidden;",
                ROOT = theme::ROOT_STYLE,
                SECTION = theme::SPACING_SECTION,
            ),

            // ── Header ───────────────────────────────────────────
            div {
                style: format!(
                    "display:flex; justify-content:space-between; align-items:center; \
                     padding-bottom:{LABEL}; border-bottom:1px solid {BORDER};",
                    LABEL = theme::SPACING_LABEL,
                    BORDER = theme::BORDER,
                ),
                div {
                    style: "display:flex; align-items:baseline; gap:12px;",
                    div {
                        style: format!(
                            "font-size:{TITLE}; font-weight:700; letter-spacing:0.5px; color:{BRIGHT};",
                            TITLE = theme::FONT_SIZE_TITLE,
                            BRIGHT = theme::TEXT_BRIGHT,
                        ),
                        "FTS GATE"
                    }
                    // Gate state
                    div {
                        style: format!(
                            "{STYLE} color:{COLOR};",
                            STYLE = theme::STYLE_VALUE,
                            COLOR = if gr_db > 6.0 { theme::SIGNAL_WARN }
                                    else if gr_db > 0.1 { theme::SIGNAL_SAFE }
                                    else { theme::TEXT_DIM },
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
                        "font-size:{TINY}; color:{DIM};",
                        TINY = theme::FONT_SIZE_TINY,
                        DIM = theme::TEXT_DIM,
                    ),
                    "FastTrackStudio"
                }
            }

            // ── Top row: Meters + Adaptive readouts ──────────────
            div {
                style: format!(
                    "display:flex; gap:{SECTION}; min-height:0;",
                    SECTION = theme::SPACING_SECTION,
                ),

                // Meters
                div {
                    style: format!(
                        "{CARD} padding:{PAD}; display:flex; gap:8px; align-items:stretch;",
                        CARD = theme::STYLE_CARD,
                        PAD = theme::SPACING_CARD,
                    ),
                    LevelMeterDb { level_db: input_db, label: "IN".to_string(), height: 140.0 }
                    GrMeter { gain_reduction_db: gr_db, height: 140.0 }
                    LevelMeterDb { level_db: output_db, label: "OUT".to_string(), height: 140.0 }
                }

                // Adaptive Decay readouts
                div {
                    style: format!(
                        "{CARD} padding:{PAD}; flex:1; display:flex; flex-direction:column; \
                         gap:{LABEL}; min-width:0;",
                        CARD = theme::STYLE_CARD,
                        PAD = theme::SPACING_CARD,
                        LABEL = theme::SPACING_LABEL,
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
                                style: format!("{};", theme::STYLE_LABEL),
                                "Resonance"
                            }
                            div {
                                style: format!(
                                    "{STYLE} font-size:16px; color:{COLOR};",
                                    STYLE = theme::STYLE_VALUE,
                                    COLOR = if adaptive_active && resonant_freq > 0.0 {
                                        theme::ACCENT
                                    } else {
                                        theme::TEXT_DIM
                                    },
                                ),
                                "{freq_text}"
                            }
                        }

                        // Computed hold
                        div {
                            style: "display:flex; flex-direction:column; align-items:center; gap:4px;",
                            div {
                                style: format!("{};", theme::STYLE_LABEL),
                                "Hold"
                            }
                            div {
                                style: format!(
                                    "{STYLE} font-size:16px; color:{COLOR};",
                                    STYLE = theme::STYLE_VALUE,
                                    COLOR = if adaptive_active { theme::TEXT } else { theme::TEXT_DIM },
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
                                style: format!("{};", theme::STYLE_LABEL),
                                "Release"
                            }
                            div {
                                style: format!(
                                    "{STYLE} font-size:16px; color:{COLOR};",
                                    STYLE = theme::STYLE_VALUE,
                                    COLOR = if adaptive_active { theme::TEXT } else { theme::TEXT_DIM },
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
                    "{CARD} padding:12px 16px; \
                     display:flex; flex-direction:column; gap:{SECTION};",
                    CARD = theme::STYLE_CARD,
                    SECTION = theme::SPACING_SECTION,
                ),
                SectionLabel { text: "Gate" }
                div {
                    style: format!(
                        "display:flex; justify-content:center; gap:{CTL};",
                        CTL = theme::SPACING_CONTROL,
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
                    "{CARD} padding:12px 16px; \
                     display:flex; gap:{CTL}; justify-content:center;",
                    CARD = theme::STYLE_CARD,
                    CTL = theme::SPACING_CONTROL,
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
                        "width:1px; background:{}; align-self:stretch;",
                        theme::BORDER_SUBTLE,
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
                        "width:1px; background:{}; align-self:stretch;",
                        theme::BORDER_SUBTLE,
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
                        "width:1px; background:{}; align-self:stretch;",
                        theme::BORDER_SUBTLE,
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
                        "width:1px; background:{}; align-self:stretch;",
                        theme::BORDER_SUBTLE,
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
