//! Compressor editor — Dioxus GUI root component.
//!
//! Layout: header, visualization row (transfer curve + waveform + meters),
//! and two rows of rotary knobs for all parameters.

use std::sync::atomic::Ordering;

use audio_gui::controls::knob::Knob;
use audio_gui::prelude::{
    theme, DragProvider, GrMeter, KnobSize, LevelMeterDb, PeakWaveform, TransferCurve,
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

    // Debug: log metering values periodically
    {
        use std::sync::atomic::AtomicU64;
        static RENDER_COUNT: AtomicU64 = AtomicU64::new(0);
        let n = RENDER_COUNT.fetch_add(1, Ordering::Relaxed);
        if n % 300 == 0 {
            nih_plug::nih_log!(
                "[Editor] render={} in={:.1} out={:.1} gr={:.1} pos={}",
                n,
                input_db,
                output_db,
                gr_db,
                ui.waveform_pos.load(Ordering::Relaxed)
            );
        }
    }

    // Read current param values for transfer curve
    let threshold = params.threshold_db.value();
    let ratio = params.ratio.value();
    let convexity = params.convexity.value();

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
                    style: "font-size:16px; font-weight:700; letter-spacing:0.5px;",
                    "FTS COMPRESSOR"
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
                        convexity: convexity,
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

            // ── Knob rows ────────────────────────────────────────
            div {
                style: format!(
                    "background:{CARD_BG}; border-radius:6px; padding:12px 16px; \
                     display:flex; flex-direction:column; gap:12px; flex:1; min-height:0;",
                    CARD_BG = theme::CARD_BG,
                ),

                // Row 1: Core dynamics (large knobs)
                div {
                    style: "display:flex; gap:4px;",
                    SectionLabel { text: "Dynamics" }
                }
                div {
                    style: "display:flex; justify-content:center; gap:24px;",
                    Knob { param_ptr: params.threshold_db.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.ratio.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.attack_ms.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.release_ms.as_ptr(), size: KnobSize::Large }
                    Knob { param_ptr: params.convexity.as_ptr(), size: KnobSize::Large, label: "Knee".to_string() }
                }

                // Row 2: I/O + advanced (medium knobs)
                div {
                    style: "display:flex; gap:4px;",
                    SectionLabel { text: "I/O & Character" }
                }
                div {
                    style: "display:flex; justify-content:center; gap:16px;",
                    Knob { param_ptr: params.input_gain_db.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.output_gain_db.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.fold.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.feedback.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.channel_link.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.sidechain_freq.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.inertia.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.inertia_decay.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.ceiling.as_ptr(), size: KnobSize::Medium }
                }
            }
        }
        } // DragProvider
    }
}

/// Tiny section label used above visualization and knob rows.
#[component]
fn SectionLabel(text: &'static str) -> Element {
    rsx! {
        div {
            style: format!(
                "font-size:10px; color:{TEXT_DIM}; text-transform:uppercase; \
                 letter-spacing:0.4px;",
                TEXT_DIM = theme::TEXT_DIM,
            ),
            "{text}"
        }
    }
}
