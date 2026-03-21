//! Compressor editor — Dioxus GUI root component.
//!
//! Layout: header, visualization row (transfer curve + waveform + meters),
//! and organized knob groups for all parameters.

use std::sync::atomic::Ordering;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::toggle::Toggle;
use audio_gui::prelude::{
    theme, ControlGroup, DragProvider, GrMeter, KnobSize, LevelMeterDb, PeakWaveform,
    SectionLabel, TransferCurve,
};
use fts_plugin_core::prelude::*;

use crate::{CompUiState, WAVEFORM_LEN};

/// Root editor component.
#[component]
pub fn App() -> Element {
    let shared = use_context::<SharedState>();
    let ui = shared.get::<CompUiState>().expect("CompUiState missing");
    let params = &ui.params;

    // Read metering values
    let gr_db = ui.gain_reduction_db.load(Ordering::Relaxed);
    let input_db = ui.input_peak_db.load(Ordering::Relaxed);
    let output_db = ui.output_peak_db.load(Ordering::Relaxed);

    // Read current param values for transfer curve
    let threshold = params.threshold_db.value();
    let ratio = params.ratio.value();
    let knee = params.knee_db.value();

    // Build waveform history from ring buffer
    let pos = ui.waveform_pos.load(Ordering::Relaxed) as usize % WAVEFORM_LEN;
    let mut waveform_in = Vec::with_capacity(WAVEFORM_LEN);
    let mut waveform_gr = Vec::with_capacity(WAVEFORM_LEN);
    for i in 0..WAVEFORM_LEN {
        let idx = (pos + i) % WAVEFORM_LEN;
        waveform_in.push(ui.waveform_input[idx].load(Ordering::Relaxed));
        waveform_gr.push(ui.waveform_gr[idx].load(Ordering::Relaxed));
    }

    // Input level as cursor on transfer curve
    let input_level = if input_db > -60.0 {
        Some(input_db)
    } else {
        None
    };

    // Format GR display
    let gr_text = if gr_db > 0.01 {
        format!("-{:.1} dB", gr_db)
    } else {
        "0.0 dB".to_string()
    };

    rsx! {
        document::Style { {theme::BASE_CSS} }

        DragProvider {
        div {
            style: format!(
                "width:100vw; height:100vh; padding:10px 14px; \
                 background:{BG}; color:{TEXT}; \
                 font-family:system-ui,sans-serif; font-size:13px; user-select:none; \
                 display:flex; flex-direction:column; gap:8px; overflow:hidden;",
                BG = theme::BG, TEXT = theme::TEXT,
            ),

            // ── Header ───────────────────────────────────────────
            div {
                style: format!(
                    "display:flex; justify-content:space-between; align-items:center; \
                     padding-bottom:6px; border-bottom:1px solid {BORDER};",
                    BORDER = theme::BORDER,
                ),
                div {
                    style: "display:flex; align-items:baseline; gap:12px;",
                    div {
                        style: "font-size:16px; font-weight:700; letter-spacing:0.5px;",
                        "FTS COMPRESSOR"
                    }
                    // GR readout in header
                    div {
                        style: format!(
                            "font-size:12px; color:{}; font-variant-numeric:tabular-nums;",
                            if gr_db > 6.0 { theme::SIGNAL_WARN }
                            else if gr_db > 0.1 { theme::SIGNAL_SAFE }
                            else { theme::TEXT_DIM }
                        ),
                        "GR: {gr_text}"
                    }
                }
                div {
                    style: format!("font-size:11px; color:{};", theme::TEXT_DIM),
                    "FastTrackStudio"
                }
            }

            // ── Visualization row ────────────────────────────────
            div {
                style: "display:flex; gap:10px; min-height:0;",

                // Transfer curve
                div {
                    style: format!(
                        "background:{CARD_BG}; border-radius:6px; padding:8px; \
                         display:flex; flex-direction:column; gap:4px;",
                        CARD_BG = theme::CARD_BG,
                    ),
                    SectionLabel { text: "Transfer Curve" }
                    TransferCurve {
                        threshold_db: threshold,
                        ratio: ratio,
                        knee_db: knee,
                        input_level_db: input_level,
                        width: 160.0,
                        height: 160.0,
                    }
                }

                // Waveform display
                div {
                    style: format!(
                        "flex:1; background:{CARD_BG}; border-radius:6px; padding:8px; \
                         display:flex; flex-direction:column; gap:4px; min-width:0;",
                        CARD_BG = theme::CARD_BG,
                    ),
                    SectionLabel { text: "Waveform / Gain Reduction" }
                    PeakWaveform {
                        levels: waveform_in,
                        gr_levels: waveform_gr,
                        width: 340.0,
                        height: 160.0,
                    }
                }

                // Meters
                div {
                    style: format!(
                        "background:{CARD_BG}; border-radius:6px; padding:8px; \
                         display:flex; gap:8px; align-items:stretch;",
                        CARD_BG = theme::CARD_BG,
                    ),
                    LevelMeterDb { level_db: input_db, label: "IN".to_string(), height: 160.0 }
                    GrMeter { gain_reduction_db: gr_db, height: 160.0 }
                    LevelMeterDb { level_db: output_db, label: "OUT".to_string(), height: 160.0 }
                }
            }

            // ── Controls ─────────────────────────────────────────
            div {
                style: format!(
                    "background:{CARD_BG}; border-radius:6px; padding:12px 16px; \
                     display:flex; flex-direction:column; gap:10px; flex:1; min-height:0;",
                    CARD_BG = theme::CARD_BG,
                ),

                // Row 1: Core dynamics (large knobs)
                div {
                    style: "display:flex; flex-direction:column; gap:8px;",
                    SectionLabel { text: "Dynamics" }
                    div {
                        style: "display:flex; justify-content:center; gap:24px;",
                        Knob { param_ptr: params.threshold_db.as_ptr(), size: KnobSize::Large }
                        Knob { param_ptr: params.ratio.as_ptr(), size: KnobSize::Large }
                        Knob { param_ptr: params.attack_ms.as_ptr(), size: KnobSize::Large }
                        Knob { param_ptr: params.release_ms.as_ptr(), size: KnobSize::Large }
                        Knob { param_ptr: params.knee_db.as_ptr(), size: KnobSize::Large }
                    }
                }

                // Row 2: Grouped secondary controls
                div {
                    style: "display:flex; gap:20px; justify-content:center;",

                    // I/O group
                    ControlGroup {
                        label: "I/O",
                        Knob { param_ptr: params.input_gain_db.as_ptr(), size: KnobSize::Medium }
                        Knob { param_ptr: params.output_gain_db.as_ptr(), size: KnobSize::Medium }
                        Toggle { param_ptr: params.auto_makeup.as_ptr(), label: "Auto" }
                    }

                    // Divider
                    div {
                        style: format!(
                            "width:1px; background:{}; align-self:stretch;",
                            theme::BORDER,
                        ),
                    }

                    // Mix group
                    ControlGroup {
                        label: "Mix",
                        Knob { param_ptr: params.fold.as_ptr(), size: KnobSize::Medium }
                        Knob { param_ptr: params.channel_link.as_ptr(), size: KnobSize::Medium }
                    }

                    // Divider
                    div {
                        style: format!(
                            "width:1px; background:{}; align-self:stretch;",
                            theme::BORDER,
                        ),
                    }

                    // Character group
                    ControlGroup {
                        label: "Character",
                        Knob { param_ptr: params.feedback.as_ptr(), size: KnobSize::Medium }
                        Knob { param_ptr: params.ceiling.as_ptr(), size: KnobSize::Medium }
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
                        Knob { param_ptr: params.sidechain_freq.as_ptr(), size: KnobSize::Medium }
                    }

                    // Divider
                    div {
                        style: format!(
                            "width:1px; background:{}; align-self:stretch;",
                            theme::BORDER,
                        ),
                    }

                    // Advanced group
                    ControlGroup {
                        label: "Advanced",
                        Knob { param_ptr: params.inertia.as_ptr(), size: KnobSize::Medium }
                        Knob { param_ptr: params.inertia_decay.as_ptr(), size: KnobSize::Medium }
                    }
                }
            }
        }
        } // DragProvider
    }
}

