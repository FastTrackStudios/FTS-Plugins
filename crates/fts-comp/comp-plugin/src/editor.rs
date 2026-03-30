//! Compressor editor — Dioxus GUI root component.
//!
//! Layout: compact header, full-width waveform with embedded transfer curve
//! and flanking meters, primary dynamics knobs, secondary grouped controls.

use std::sync::atomic::Ordering;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::toggle::Toggle;
use audio_gui::prelude::{
    use_init_theme, ControlGroup, Divider, DragProvider, KnobSize, PeakWaveform,
};
use fts_plugin_core::prelude::*;

use crate::{CompUiState, WAVEFORM_LEN};

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

    let gr_color = if gr_db > 6.0 {
        t.signal_warn
    } else if gr_db > 0.1 {
        t.signal_safe
    } else {
        t.text_dim
    };

    rsx! {
        document::Style { {t.base_css()} }

        DragProvider {
            div {
                style: format!(
                    "{} display:flex; flex-direction:column; gap:6px;",
                    t.root_style(),
                ),

                // ── Header ───────────────────────────────────────────
                div {
                    style: format!(
                        "display:flex; justify-content:space-between; align-items:center; \
                         padding:0 2px 5px 2px; border-bottom:1px solid {};",
                        t.border_subtle,
                    ),
                    div {
                        style: "display:flex; align-items:baseline; gap:10px;",
                        span {
                            style: format!(
                                "font-size:14px; font-weight:700; letter-spacing:1px; \
                                 color:{};",
                                t.text_bright,
                            ),
                            "FTS COMPRESSOR"
                        }
                        span {
                            style: format!(
                                "font-family:{}; font-size:12px; font-weight:600; \
                                 font-variant-numeric:tabular-nums; min-width:60px; \
                                 color:{};",
                                t.font_mono,
                                gr_color,
                            ),
                            "GR {gr_text}"
                        }
                    }
                    span {
                        style: format!(
                            "font-size:9px; color:{}; letter-spacing:0.8px;",
                            t.text_dim,
                        ),
                        "FASTTRACKSTUDIO"
                    }
                }

                // ── Visualization ────────────────────────────────────
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

                // ── Primary controls ─────────────────────────────────
                div {
                    style: format!(
                        "display:flex; justify-content:center; gap:{}; \
                         padding:4px 0;",
                        t.spacing_control,
                    ),
                    Knob { param_ptr: params.threshold_db.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.ratio.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.attack_ms.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.release_ms.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.knee_db.as_ptr(), size: KnobSize::Large }
                }

                // ── Secondary controls ──────────────────────────────
                div {
                    style: format!(
                        "{} padding:8px 12px; \
                         display:flex; gap:{}; justify-content:center; align-items:flex-start;",
                        t.style_card(),
                        t.spacing_control,
                    ),

                    ControlGroup {
                        label: "I/O",
                        Knob { param_ptr: params.input_gain_db.as_ptr(), size: KnobSize::Medium }
                        Knob { param_ptr: params.output_gain_db.as_ptr(), size: KnobSize::Medium }
                        Toggle { param_ptr: params.auto_makeup.as_ptr(), label: "Auto" }
                    }

                    Divider {}

                    ControlGroup {
                        label: "Mix",
                        Knob { param_ptr: params.fold.as_ptr(), size: KnobSize::Medium }
                        Knob { param_ptr: params.channel_link.as_ptr(), size: KnobSize::Medium }
                    }

                    Divider {}

                    ControlGroup {
                        label: "Character",
                        Knob { param_ptr: params.feedback.as_ptr(), size: KnobSize::Medium }
                        Knob { param_ptr: params.ceiling.as_ptr(), size: KnobSize::Medium }
                    }

                    Divider {}

                    ControlGroup {
                        label: "Sidechain",
                        Knob { param_ptr: params.sidechain_freq.as_ptr(), size: KnobSize::Medium }
                    }

                    Divider {}

                    ControlGroup {
                        label: "Advanced",
                        Knob { param_ptr: params.inertia.as_ptr(), size: KnobSize::Medium }
                        Knob { param_ptr: params.inertia_decay.as_ptr(), size: KnobSize::Medium }
                    }
                }
            }
        } // DragProvider
    }
}
