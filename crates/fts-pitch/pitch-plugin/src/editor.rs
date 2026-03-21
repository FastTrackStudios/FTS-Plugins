//! Pitch shifter editor — Dioxus GUI root component.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::segment::SegmentButton;
use audio_gui::prelude::{theme, DragProvider, KnobSize, LevelMeterDb};
use fts_plugin_core::prelude::*;

use crate::{FtsPitchParams, PitchUiState};

/// Root editor component.
#[component]
pub fn App() -> Element {
    let shared = use_context::<SharedState>();
    let ui: Arc<PitchUiState> =
        shared.get::<PitchUiState>().expect("PitchUiState missing");
    let params: Arc<FtsPitchParams> = ui.params.clone();
    let ctx = use_param_context();

    // Read metering values.
    let input_db = ui.input_peak_db.load(Ordering::Relaxed);
    let output_db = ui.output_peak_db.load(Ordering::Relaxed);
    let latency = ui.latency_samples.load(Ordering::Relaxed) as usize;

    let algo = params.algorithm.value();

    let latency_text = if latency == 0 {
        "0 smp".to_string()
    } else {
        format!("{latency} smp")
    };

    let st = params.semitones.value();
    let st_text = if st > 0.0 {
        format!("+{st:.1} st")
    } else {
        format!("{st:.1} st")
    };

    // Extract ParamPtrs (Copy) for Knob components.
    let semitones_ptr = params.semitones.as_ptr();
    let mix_ptr = params.mix.as_ptr();
    let output_gain_ptr = params.output_gain_db.as_ptr();
    let grain_size_ptr = params.grain_size.as_ptr();
    let wf = params.pll_waveform.value();

    // Build algorithm setter closures (need owned Arcs).
    let algo_setter = |value: i32| {
        let ctx = ctx.clone();
        let p = params.clone();
        move |_: ()| {
            ctx.begin_set(&p.algorithm);
            ctx.set(&p.algorithm, value);
            ctx.end_set(&p.algorithm);
        }
    };

    let wf_setter = |value: i32| {
        let ctx = ctx.clone();
        let p = params.clone();
        move |_: ()| {
            ctx.begin_set(&p.pll_waveform);
            ctx.set(&p.pll_waveform, value);
            ctx.end_set(&p.pll_waveform);
        }
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
                        "FTS PITCH"
                    }
                    div {
                        style: format!(
                            "font-size:12px; color:{}; font-variant-numeric:tabular-nums;",
                            theme::TEXT_DIM,
                        ),
                        "{st_text}"
                    }
                    div {
                        style: format!("font-size:11px; color:{};", theme::TEXT_DIM),
                        "Latency: {latency_text}"
                    }
                }
                div {
                    style: format!("font-size:11px; color:{};", theme::TEXT_DIM),
                    "FastTrackStudio"
                }
            }

            // ── Algorithm selector ───────────────────────────────
            div {
                style: format!(
                    "display:flex; gap:4px; background:{CARD_BG}; border-radius:6px; padding:8px;",
                    CARD_BG = theme::CARD_BG,
                ),
                SectionLabel { text: "Algorithm" }
                div {
                    style: "display:flex; gap:4px; margin-left:12px;",
                    SegmentButton { label: "Divider", selected: algo == 0, on_click: algo_setter(0) }
                    SegmentButton { label: "PLL", selected: algo == 1, on_click: algo_setter(1) }
                    SegmentButton { label: "Granular", selected: algo == 2, on_click: algo_setter(2) }
                    SegmentButton { label: "PSOLA", selected: algo == 3, on_click: algo_setter(3) }
                }
            }

            // ── Main controls + Meters ───────────────────────────
            div {
                style: "display:flex; gap:10px; flex:1; min-height:0;",

                // Controls card
                div {
                    style: format!(
                        "flex:1; background:{CARD_BG}; border-radius:6px; padding:12px 16px; \
                         display:flex; flex-direction:column; gap:12px;",
                        CARD_BG = theme::CARD_BG,
                    ),

                    // Core controls
                    div {
                        style: "display:flex; flex-direction:column; gap:8px;",
                        SectionLabel { text: "Pitch" }
                        div {
                            style: "display:flex; justify-content:center; gap:24px;",
                            Knob { param_ptr: semitones_ptr, size: KnobSize::Large }
                            Knob { param_ptr: mix_ptr, size: KnobSize::Large }
                            Knob { param_ptr: output_gain_ptr, size: KnobSize::Large }
                        }
                    }

                    // PLL waveform selector
                    if algo == 1 {
                        div {
                            style: "display:flex; flex-direction:column; gap:8px;",
                            SectionLabel { text: "PLL Waveform" }
                            div {
                                style: "display:flex; gap:4px; justify-content:center;",
                                SegmentButton { label: "Square", selected: wf == 0, on_click: wf_setter(0) }
                                SegmentButton { label: "Saw", selected: wf == 1, on_click: wf_setter(1) }
                                SegmentButton { label: "Triangle", selected: wf == 2, on_click: wf_setter(2) }
                            }
                        }
                    }

                    // Granular grain size
                    if algo == 2 {
                        div {
                            style: "display:flex; flex-direction:column; gap:8px;",
                            SectionLabel { text: "Granular" }
                            div {
                                style: "display:flex; justify-content:center;",
                                Knob { param_ptr: grain_size_ptr, size: KnobSize::Medium }
                            }
                        }
                    }
                }

                // Meters
                div {
                    style: format!(
                        "background:{CARD_BG}; border-radius:6px; padding:8px; \
                         display:flex; gap:8px; align-items:stretch;",
                        CARD_BG = theme::CARD_BG,
                    ),
                    LevelMeterDb { level_db: input_db, label: "IN".to_string(), height: 240.0 }
                    LevelMeterDb { level_db: output_db, label: "OUT".to_string(), height: 240.0 }
                }
            }
        }
        } // DragProvider
    }
}

/// Tiny section label.
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
