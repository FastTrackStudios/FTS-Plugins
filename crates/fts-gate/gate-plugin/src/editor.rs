//! Gate editor — Dioxus GUI root component.
//!
//! Layout: header, meters (input + gate gain + output), and knob groups.

use std::sync::atomic::Ordering;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::toggle::Toggle;
use audio_gui::prelude::{
    theme, ControlGroup, DragProvider, GrMeter, KnobSize, LevelMeterDb, SectionLabel,
};
use fts_plugin_core::prelude::*;

use crate::GateUiState;

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

    // Gate reduction in dB (for GR meter display)
    let gr_db = if gate_gain > 0.0001 {
        -20.0 * gate_gain.log10()
    } else {
        60.0
    };

    // Format gate state display
    let gate_text = if gate_gain > 0.99 {
        "OPEN".to_string()
    } else if gate_gain < 0.01 {
        "CLOSED".to_string()
    } else {
        format!("-{:.1} dB", gr_db)
    };

    rsx! {
        document::Style { {theme::BASE_CSS} }

        DragProvider {
        div {
            style: format!(
                "{} display:flex; flex-direction:column; gap:{}; overflow:hidden;",
                theme::ROOT_STYLE, theme::SPACING_SECTION,
            ),

            // ── Header ───────────────────────────────────────────
            div {
                style: format!(
                    "display:flex; justify-content:space-between; align-items:center; \
                     padding-bottom:6px; border-bottom:1px solid {};",
                    theme::BORDER,
                ),
                div {
                    style: "display:flex; align-items:baseline; gap:12px;",
                    div {
                        style: format!("font-size:{}; font-weight:700; letter-spacing:0.5px;", theme::FONT_SIZE_TITLE),
                        "FTS GATE"
                    }
                    div {
                        style: format!(
                            "{} color:{};",
                            theme::STYLE_VALUE,
                            if gr_db > 6.0 { theme::SIGNAL_WARN }
                            else if gr_db > 0.1 { theme::SIGNAL_SAFE }
                            else { theme::TEXT_DIM }
                        ),
                        "{gate_text}"
                    }
                }
                div {
                    style: format!("font-size:{}; color:{};", theme::FONT_SIZE_LABEL, theme::TEXT_DIM),
                    "FastTrackStudio"
                }
            }

            // ── Meters ───────────────────────────────────────────
            div {
                style: format!(
                    "{} display:flex; justify-content:center; gap:8px;",
                    theme::STYLE_CARD,
                ),
                LevelMeterDb { level_db: input_db, label: "IN".to_string(), height: 140.0 }
                GrMeter { gain_reduction_db: gr_db, height: 140.0 }
                LevelMeterDb { level_db: output_db, label: "OUT".to_string(), height: 140.0 }
            }

            // ── Controls ─────────────────────────────────────────
            div {
                style: format!(
                    "{} display:flex; flex-direction:column; gap:{}; flex:1; min-height:0;",
                    theme::STYLE_CARD, theme::SPACING_SECTION,
                ),

                // Row 1: Core gate parameters (large knobs)
                div {
                    style: "display:flex; flex-direction:column; gap:8px;",
                    SectionLabel { text: "Gate" }
                    div {
                        style: "display:flex; justify-content:center; gap:24px;",
                        Knob { param_ptr: params.threshold_db.as_ptr(), size: KnobSize::Large }
                        Knob { param_ptr: params.hysteresis_db.as_ptr(), size: KnobSize::Large }
                        Knob { param_ptr: params.attack_ms.as_ptr(), size: KnobSize::Large }
                        Knob { param_ptr: params.hold_ms.as_ptr(), size: KnobSize::Large }
                        Knob { param_ptr: params.release_ms.as_ptr(), size: KnobSize::Large }
                    }
                }

                // Row 2: Grouped secondary controls
                div {
                    style: "display:flex; gap:20px; justify-content:center;",

                    // Range / Lookahead
                    ControlGroup {
                        label: "Depth",
                        Knob { param_ptr: params.range_db.as_ptr(), size: KnobSize::Medium }
                        Knob { param_ptr: params.lookahead_ms.as_ptr(), size: KnobSize::Medium }
                    }

                    // Divider
                    div {
                        style: format!(
                            "width:1px; background:{}; align-self:stretch;",
                            theme::BORDER,
                        ),
                    }

                    // Sidechain group
                    ControlGroup {
                        label: "Sidechain",
                        Knob { param_ptr: params.sc_hpf_freq.as_ptr(), size: KnobSize::Medium }
                        Knob { param_ptr: params.sc_lpf_freq.as_ptr(), size: KnobSize::Medium }
                        Toggle { param_ptr: params.sc_listen.as_ptr(), label: "Listen" }
                    }
                }
            }
        }
        } // DragProvider
    }
}
