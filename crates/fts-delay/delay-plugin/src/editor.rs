//! Delay editor — Dioxus GUI root component.
//!
//! Layout: header, main controls (time, feedback, stereo, EQ, tape, diffusion, ducking), meters.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::segment::SegmentButton;
use audio_gui::controls::toggle::Toggle;
use audio_gui::prelude::{theme, ControlGroup, DragProvider, KnobSize, LevelMeterDb, SectionLabel};
use fts_dsp::note_sync::NoteValue;
use fts_plugin_core::prelude::*;

use crate::{DelayUiState, FtsDelayParams};

/// Root editor component.
#[component]
pub fn App() -> Element {
    let shared = use_context::<SharedState>();
    let ui: Arc<DelayUiState> = shared.get::<DelayUiState>().expect("DelayUiState missing");
    let params: Arc<FtsDelayParams> = ui.params.clone();
    let ctx = use_param_context();

    // Read metering values
    let input_db = ui.input_peak_db.load(Ordering::Relaxed);
    let output_db = ui.output_peak_db.load(Ordering::Relaxed);

    // State
    let mode = params.stereo_mode.value() as i32;
    let link = params.link_lr.value() > 0.5;
    let sync = params.sync_enable.value() > 0.5;
    let diff_on = params.diff_enable.value() > 0.5;
    let duck_on = params.duck_enable.value() > 0.5;

    // Stereo mode setter
    let mode_setter = |value: f32| {
        let ctx = ctx.clone();
        let p = params.clone();
        move |_: ()| {
            ctx.begin_set_raw(p.stereo_mode.as_ptr());
            ctx.set_normalized_raw(p.stereo_mode.as_ptr(), value / 2.0);
            ctx.end_set_raw(p.stereo_mode.as_ptr());
        }
    };

    // Note value setter for L/R
    let note_setter_l = |idx: i32| {
        let ctx = ctx.clone();
        let p = params.clone();
        move |_: ()| {
            ctx.begin_set(&p.note_l);
            ctx.set(&p.note_l, idx);
            ctx.end_set(&p.note_l);
        }
    };
    let note_setter_r = |idx: i32| {
        let ctx = ctx.clone();
        let p = params.clone();
        move |_: ()| {
            ctx.begin_set(&p.note_r);
            ctx.set(&p.note_r, idx);
            ctx.end_set(&p.note_r);
        }
    };

    let note_l_idx = params.note_l.value();
    let note_r_idx = params.note_r.value();

    // Common note values for the compact selector (8 most useful)
    let common_notes: &[(i32, &str)] = &[
        (NoteValue::Half.to_index() as i32, "1/2"),
        (NoteValue::DottedQuarter.to_index() as i32, "1/4."),
        (NoteValue::Quarter.to_index() as i32, "1/4"),
        (NoteValue::DottedEighth.to_index() as i32, "1/8."),
        (NoteValue::TripletQuarter.to_index() as i32, "1/4T"),
        (NoteValue::Eighth.to_index() as i32, "1/8"),
        (NoteValue::TripletEighth.to_index() as i32, "1/8T"),
        (NoteValue::Sixteenth.to_index() as i32, "1/16"),
    ];

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
                        style: format!(
                            "font-size:{}; font-weight:700; letter-spacing:{}; color:{};",
                            theme::FONT_SIZE_TITLE, theme::LETTER_SPACING_LABEL, theme::TEXT_BRIGHT,
                        ),
                        "FTS DELAY"
                    }
                }
                div {
                    style: format!("font-size:{}; color:{};", theme::FONT_SIZE_LABEL, theme::TEXT_DIM),
                    "FastTrackStudio"
                }
            }

            // ── Main content: controls + meters ──────────────────
            div {
                style: format!("display:flex; gap:{}; flex:1; min-height:0;", theme::SPACING_SECTION),

                // Controls area
                div {
                    style: format!(
                        "flex:1; {} padding:{}; \
                         display:flex; flex-direction:column; gap:{}; overflow-y:auto;",
                        theme::STYLE_CARD, theme::SPACING_CARD, theme::SPACING_SECTION,
                    ),

                    // ── Row 1: Time & Feedback & Mix ─────────────────
                    div {
                        style: "display:flex; gap:20px; justify-content:center;",

                        // Time section
                        div {
                            style: format!(
                                "display:flex; flex-direction:column; gap:{}; align-items:center;",
                                theme::SPACING_SECTION,
                            ),
                            div {
                                style: format!(
                                    "display:flex; gap:{}; align-items:center;",
                                    theme::SPACING_SECTION,
                                ),
                                SectionLabel { text: "Time" }
                                Toggle { param_ptr: params.sync_enable.as_ptr(), label: "Sync" }
                                Toggle { param_ptr: params.link_lr.as_ptr(), label: "Link" }
                            }

                            if sync {
                                // Note value selectors
                                div {
                                    style: format!(
                                        "display:flex; flex-direction:column; gap:{}; align-items:center;",
                                        theme::SPACING_LABEL,
                                    ),
                                    // L note selector
                                    div {
                                        style: format!(
                                            "display:flex; gap:{}; flex-wrap:wrap; justify-content:center;",
                                            theme::SPACING_TIGHT,
                                        ),
                                        for &(idx, label) in common_notes.iter() {
                                            SegmentButton {
                                                label: label,
                                                selected: note_l_idx == idx,
                                                on_click: note_setter_l(idx),
                                            }
                                        }
                                    }
                                    // R note selector (only when unlinked)
                                    if !link {
                                        div {
                                            style: format!(
                                                "display:flex; gap:{}; flex-wrap:wrap; justify-content:center; \
                                                 padding-top:{}; border-top:1px solid {};",
                                                theme::SPACING_TIGHT, theme::SPACING_LABEL, theme::BORDER,
                                            ),
                                            span {
                                                style: format!(
                                                    "font-size:{}; color:{}; margin-right:4px; align-self:center;",
                                                    theme::FONT_SIZE_LABEL, theme::TEXT_DIM,
                                                ),
                                                "R"
                                            }
                                            for &(idx, label) in common_notes.iter() {
                                                SegmentButton {
                                                    label: label,
                                                    selected: note_r_idx == idx,
                                                    on_click: note_setter_r(idx),
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                // Manual ms knobs
                                div {
                                    style: format!(
                                        "display:flex; gap:{}; justify-content:center;",
                                        theme::SPACING_CONTROL,
                                    ),
                                    Knob { param_ptr: params.time_l.as_ptr(), size: KnobSize::Large }
                                    if !link {
                                        Knob { param_ptr: params.time_r.as_ptr(), size: KnobSize::Large }
                                    }
                                }
                            }
                        }

                        // Divider
                        div {
                            style: format!(
                                "width:1px; background:{}; align-self:stretch;",
                                theme::BORDER,
                            ),
                        }

                        ControlGroup {
                            label: "Feedback & Mix",
                            Knob { param_ptr: params.feedback.as_ptr(), size: KnobSize::Large }
                            Knob { param_ptr: params.mix.as_ptr(), size: KnobSize::Large }
                        }
                    }

                    // ── Row 2: Stereo mode + EQ + Drive ──────────────
                    div {
                        style: "display:flex; gap:20px; justify-content:center; align-items:flex-start;",

                        // Stereo
                        div {
                            style: format!(
                                "display:flex; flex-direction:column; gap:{}; align-items:center;",
                                theme::SPACING_SECTION,
                            ),
                            SectionLabel { text: "Stereo" }
                            div {
                                style: format!("display:flex; gap:{};", theme::SPACING_LABEL),
                                SegmentButton { label: "Stereo", selected: mode == 0, on_click: mode_setter(0.0) }
                                SegmentButton { label: "PingPong", selected: mode == 1, on_click: mode_setter(1.0) }
                                SegmentButton { label: "Mono", selected: mode == 2, on_click: mode_setter(2.0) }
                            }
                            div {
                                style: format!("display:flex; gap:{};", theme::SPACING_CONTROL),
                                Knob { param_ptr: params.width.as_ptr(), size: KnobSize::Small }
                                if mode == 1 {
                                    Knob { param_ptr: params.pp_feedback.as_ptr(), size: KnobSize::Small }
                                }
                            }
                        }

                        div {
                            style: format!(
                                "width:1px; background:{}; align-self:stretch;",
                                theme::BORDER,
                            ),
                        }

                        // Feedback EQ
                        ControlGroup {
                            label: "Feedback EQ",
                            Knob { param_ptr: params.hicut.as_ptr(), size: KnobSize::Medium }
                            Knob { param_ptr: params.locut.as_ptr(), size: KnobSize::Medium }
                        }

                        div {
                            style: format!(
                                "width:1px; background:{}; align-self:stretch;",
                                theme::BORDER,
                            ),
                        }

                        // Drive
                        ControlGroup {
                            label: "Saturation",
                            Knob { param_ptr: params.drive.as_ptr(), size: KnobSize::Medium }
                        }
                    }

                    // ── Row 3: Tape Modulation ───────────────────────
                    div {
                        style: "display:flex; gap:20px; justify-content:center;",

                        ControlGroup {
                            label: "Wow",
                            Knob { param_ptr: params.wow_depth.as_ptr(), size: KnobSize::Medium }
                            Knob { param_ptr: params.wow_rate.as_ptr(), size: KnobSize::Small }
                            Knob { param_ptr: params.wow_drift.as_ptr(), size: KnobSize::Small }
                        }

                        div {
                            style: format!(
                                "width:1px; background:{}; align-self:stretch;",
                                theme::BORDER,
                            ),
                        }

                        ControlGroup {
                            label: "Flutter",
                            Knob { param_ptr: params.flutter_depth.as_ptr(), size: KnobSize::Medium }
                            Knob { param_ptr: params.flutter_rate.as_ptr(), size: KnobSize::Small }
                        }
                    }

                    // ── Row 4: Diffusion + Ducking ───────────────────
                    div {
                        style: "display:flex; gap:20px; justify-content:center;",

                        // Diffusion
                        div {
                            style: format!(
                                "display:flex; flex-direction:column; gap:{}; align-items:center;",
                                theme::SPACING_SECTION,
                            ),
                            div {
                                style: format!(
                                    "display:flex; gap:{}; align-items:center;",
                                    theme::SPACING_SECTION,
                                ),
                                SectionLabel { text: "Diffusion" }
                                Toggle { param_ptr: params.diff_enable.as_ptr(), label: "" }
                            }
                            if diff_on {
                                div {
                                    style: format!("display:flex; gap:{};", theme::SPACING_CONTROL),
                                    Knob { param_ptr: params.diff_size.as_ptr(), size: KnobSize::Small }
                                    Knob { param_ptr: params.diff_smear.as_ptr(), size: KnobSize::Small }
                                }
                            }
                        }

                        div {
                            style: format!(
                                "width:1px; background:{}; align-self:stretch;",
                                theme::BORDER,
                            ),
                        }

                        // Ducking
                        div {
                            style: format!(
                                "display:flex; flex-direction:column; gap:{}; align-items:center;",
                                theme::SPACING_SECTION,
                            ),
                            div {
                                style: format!(
                                    "display:flex; gap:{}; align-items:center;",
                                    theme::SPACING_SECTION,
                                ),
                                SectionLabel { text: "Ducking" }
                                Toggle { param_ptr: params.duck_enable.as_ptr(), label: "" }
                            }
                            if duck_on {
                                div {
                                    style: format!("display:flex; gap:{};", theme::SPACING_CONTROL),
                                    Knob { param_ptr: params.duck_amount.as_ptr(), size: KnobSize::Small }
                                    Knob { param_ptr: params.duck_threshold.as_ptr(), size: KnobSize::Small }
                                    Knob { param_ptr: params.duck_attack.as_ptr(), size: KnobSize::Small }
                                    Knob { param_ptr: params.duck_release.as_ptr(), size: KnobSize::Small }
                                }
                            }
                        }
                    }
                }

                // Meters
                div {
                    style: format!(
                        "{} padding:8px; display:flex; gap:{}; align-items:stretch;",
                        theme::STYLE_CARD, theme::SPACING_SECTION,
                    ),
                    LevelMeterDb { level_db: input_db, label: "IN".to_string() }
                    LevelMeterDb { level_db: output_db, label: "OUT".to_string() }
                }
            }
        }
        } // DragProvider
    }
}
