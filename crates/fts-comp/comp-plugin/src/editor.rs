//! Compressor editor — Dioxus GUI root component.
//!
//! Layout: header, visualization row (transfer curve + waveform + meters),
//! and organized knob groups for all parameters.

use std::sync::atomic::Ordering;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::toggle::Toggle;
use audio_gui::prelude::{
    use_init_theme, ControlGroup, DragProvider, KnobSize, LevelMeterDb, PeakWaveform, SectionLabel,
};
use fts_plugin_core::prelude::*;

use crate::{CompUiState, WAVEFORM_LEN};

/// Set to `true` to replace all visualizers/meters with plain colored boxes.
/// This isolates whether the viz components are the FPS bottleneck.
const DUMMY_VIZ: bool = false;

/// Root editor component.
#[component]
pub fn App() -> Element {
    let t = use_init_theme();
    let t = *t.read();

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
        document::Style { {t.base_css()} }

        DragProvider {
            div {
                style: format!(
                    "{} display:flex; flex-direction:column; gap:{};",
                    t.root_style(),
                    t.spacing_section,
                ),

                // ── Header ───────────────────────────────────────────
                div {
                    style: format!(
                        "display:flex; justify-content:space-between; align-items:center; \
                         padding-bottom:{}; border-bottom:1px solid {};",
                        t.spacing_label,
                        t.border,
                    ),
                    div {
                        style: "display:flex; align-items:baseline; gap:12px;",
                        div {
                            style: format!(
                                "font-size:{}; font-weight:700; letter-spacing:0.5px; \
                                 color:{};",
                                t.font_size_title,
                                t.text_bright,
                            ),
                            "FTS COMPRESSOR"
                        }
                        // GR readout in header
                        div {
                            style: format!(
                                "{} color:{};",
                                t.style_value(),
                                if gr_db > 6.0 { t.signal_warn }
                                else if gr_db > 0.1 { t.signal_safe }
                                else { t.text_dim },
                            ),
                            "GR: {gr_text}"
                        }
                    }
                    div {
                        style: format!(
                            "font-size:{}; color:{};",
                            t.font_size_tiny,
                            t.text_dim,
                        ),
                        "FastTrackStudio"
                    }
                }

                // ── Visualization row ────────────────────────────────
                if DUMMY_VIZ {
                    // Dummy placeholders — same sizes, no complex DOM
                    div {
                        style: format!(
                            "display:flex; gap:{}; min-height:0;",
                            t.spacing_section,
                        ),
                        // Transfer curve placeholder
                        div {
                            style: format!(
                                "{} padding:{}; display:flex; flex-direction:column; gap:{};",
                                t.style_card(), t.spacing_card, t.spacing_label,
                            ),
                            SectionLabel { text: "Transfer Curve (DUMMY)" }
                            div {
                                style: "width:160px; height:160px; background:#1a3a2a; border:1px dashed #4a4a4a; display:flex; align-items:center; justify-content:center; color:#666; font-size:11px;",
                                "160x160"
                            }
                        }
                        // Waveform placeholder
                        div {
                            style: format!(
                                "{} flex:1; padding:{}; display:flex; flex-direction:column; gap:{}; min-width:0;",
                                t.style_card(), t.spacing_card, t.spacing_label,
                            ),
                            SectionLabel { text: "Waveform (DUMMY)" }
                            div {
                                style: "width:340px; height:160px; background:#1a2a3a; border:1px dashed #4a4a4a; display:flex; align-items:center; justify-content:center; color:#666; font-size:11px;",
                                "340x160"
                            }
                        }
                        // Meters placeholder
                        div {
                            style: format!(
                                "{} padding:{}; display:flex; gap:{}; align-items:stretch;",
                                t.style_card(), t.spacing_card, t.spacing_section,
                            ),
                            div {
                                style: "width:24px; height:160px; background:#2a1a1a; border:1px dashed #4a4a4a; display:flex; align-items:center; justify-content:center; color:#666; font-size:9px; writing-mode:vertical-rl;",
                                "IN"
                            }
                            div {
                                style: "width:24px; height:160px; background:#2a2a1a; border:1px dashed #4a4a4a; display:flex; align-items:center; justify-content:center; color:#666; font-size:9px; writing-mode:vertical-rl;",
                                "GR"
                            }
                            div {
                                style: "width:24px; height:160px; background:#2a1a1a; border:1px dashed #4a4a4a; display:flex; align-items:center; justify-content:center; color:#666; font-size:9px; writing-mode:vertical-rl;",
                                "OUT"
                            }
                        }
                    }
                } else {
                    div {
                        style: format!(
                            "display:flex; gap:{}; flex:1; min-height:0; align-items:stretch;",
                            t.spacing_section,
                        ),

                        // Input meter (left edge)
                        LevelMeterDb { level_db: input_db, label: "IN".to_string(), width: 18.0, fill: true }

                        // Waveform + GR + transfer curve overlay — fills all remaining space
                        PeakWaveform {
                            levels: waveform_in,
                            gr_levels: waveform_gr,
                            threshold_db: Some(threshold),
                            ratio: Some(ratio),
                            knee_db: knee,
                            input_level_db: input_level,
                            fill: true,
                            style: format!(
                                "{} flex:1; min-width:0; min-height:0; overflow:hidden;",
                                t.style_card(),
                            ),
                        }

                        // Output meter (right edge)
                        LevelMeterDb { level_db: output_db, label: "OUT".to_string(), width: 18.0, fill: true }
                    }
                }

                // ── Controls ─────────────────────────────────────────
                div {
                    style: format!(
                        "{} padding:12px 16px; \
                         display:flex; flex-direction:column; gap:{};",
                        t.style_card(),
                        t.spacing_section,
                    ),

                    // Row 1: Core dynamics (large knobs)
                    div {
                        style: format!(
                            "display:flex; flex-direction:column; gap:{};",
                            t.spacing_section,
                        ),
                        SectionLabel { text: "Dynamics" }
                        div {
                            style: format!(
                                "display:flex; justify-content:center; gap:{};",
                                t.spacing_control,
                            ),
                            Knob { param_ptr: params.threshold_db.as_ptr(), size: KnobSize::Large }
                            Knob { param_ptr: params.ratio.as_ptr(), size: KnobSize::Large }
                            Knob { param_ptr: params.attack_ms.as_ptr(), size: KnobSize::Large }
                            Knob { param_ptr: params.release_ms.as_ptr(), size: KnobSize::Large }
                            Knob { param_ptr: params.knee_db.as_ptr(), size: KnobSize::Large }
                        }
                    }

                    // Row 2: Grouped secondary controls
                    div {
                        style: format!(
                            "display:flex; gap:{}; justify-content:center;",
                            t.spacing_control,
                        ),

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
                                t.border_subtle,
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
                                t.border_subtle,
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
                                t.border_subtle,
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
                                t.border_subtle,
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
