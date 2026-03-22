//! LiveTune editor — Dioxus GUI root component.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::segment::SegmentButton;
use audio_gui::controls::toggle::Toggle;
use audio_gui::prelude::{use_init_theme, DragProvider, KnobSize, LevelMeterDb, SectionLabel};
use fts_plugin_core::prelude::*;

use crate::{FtsLiveTuneParams, LiveTuneUiState};

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

/// Root editor component.
#[component]
pub fn App() -> Element {
    let t = use_init_theme();
    let t = *t.read();

    let shared = use_context::<SharedState>();
    let ui: Arc<LiveTuneUiState> = shared
        .get::<LiveTuneUiState>()
        .expect("LiveTuneUiState missing");
    let params: Arc<FtsLiveTuneParams> = ui.params.clone();
    let ctx = use_param_context();

    // Read metering values.
    let input_db = ui.input_peak_db.load(Ordering::Relaxed);
    let output_db = ui.output_peak_db.load(Ordering::Relaxed);
    let detected_hz = ui.detected_freq_hz.load(Ordering::Relaxed);
    let detected_midi = ui.detected_midi.load(Ordering::Relaxed);
    let confidence = ui.confidence.load(Ordering::Relaxed);
    let correction_st = ui.correction_st.load(Ordering::Relaxed);

    // Pitch display.
    let pitch_text = if confidence > 0.3 && detected_hz > 0.0 {
        let note_idx = ((detected_midi.round() as i32 % 12) + 12) % 12;
        let octave = (detected_midi.round() as i32 / 12) - 1;
        let note_name = NOTE_NAMES[note_idx as usize];
        format!("{note_name}{octave} ({detected_hz:.1} Hz)")
    } else {
        "---".to_string()
    };

    let correction_text = if correction_st.abs() > 0.01 {
        if correction_st > 0.0 {
            format!("+{correction_st:.1} st")
        } else {
            format!("{correction_st:.1} st")
        }
    } else {
        "0.0 st".to_string()
    };

    let confidence_color = if confidence > 0.7 {
        t.signal_safe
    } else if confidence > 0.3 {
        t.signal_warn
    } else {
        t.text_dim
    };

    let current_key = params.key.value();
    let current_scale = params.scale.value();
    let current_detector = params.detector_mode.value();
    let current_shifter = params.shifter_mode.value();

    // Extract ParamPtrs (Copy) for Knob/Toggle components.
    let retune_speed_ptr = params.retune_speed.as_ptr();
    let amount_ptr = params.amount.as_ptr();
    let mix_ptr = params.mix.as_ptr();
    let output_gain_ptr = params.output_gain_db.as_ptr();
    let confidence_ptr = params.confidence_threshold.as_ptr();
    let formants_ptr = params.preserve_formants.as_ptr();
    let note_ptrs: [ParamPtr; 12] = [
        params.note_c.as_ptr(),
        params.note_cs.as_ptr(),
        params.note_d.as_ptr(),
        params.note_eb.as_ptr(),
        params.note_e.as_ptr(),
        params.note_f.as_ptr(),
        params.note_fs.as_ptr(),
        params.note_g.as_ptr(),
        params.note_ab.as_ptr(),
        params.note_a.as_ptr(),
        params.note_bb.as_ptr(),
        params.note_b.as_ptr(),
    ];

    // Closure builders for int param setters.
    let key_setter = |value: i32| {
        let ctx = ctx.clone();
        let p = params.clone();
        move |_: ()| {
            ctx.begin_set(&p.key);
            ctx.set(&p.key, value);
            ctx.end_set(&p.key);
        }
    };
    let scale_setter = |value: i32| {
        let ctx = ctx.clone();
        let p = params.clone();
        move |_: ()| {
            ctx.begin_set(&p.scale);
            ctx.set(&p.scale, value);
            ctx.end_set(&p.scale);
        }
    };
    let detector_setter = |value: i32| {
        let ctx = ctx.clone();
        let p = params.clone();
        move |_: ()| {
            ctx.begin_set(&p.detector_mode);
            ctx.set(&p.detector_mode, value);
            ctx.end_set(&p.detector_mode);
        }
    };
    let shifter_setter = |value: i32| {
        let ctx = ctx.clone();
        let p = params.clone();
        move |_: ()| {
            ctx.begin_set(&p.shifter_mode);
            ctx.set(&p.shifter_mode, value);
            ctx.end_set(&p.shifter_mode);
        }
    };

    // Pre-compute composite styles that return String.
    let base_css = t.base_css();
    let root_style = t.root_style();
    let style_card = t.style_card();
    let style_value = t.style_value();

    rsx! {
        document::Style { {base_css} }

        DragProvider {
        div {
            style: format!(
                "{root_style} display:flex; flex-direction:column; gap:{spacing}; overflow:hidden;",
                spacing = t.spacing_section,
            ),

            // ── Header ───────────────────────────────────────────
            div {
                style: format!(
                    "display:flex; justify-content:space-between; align-items:center; \
                     padding-bottom:6px; border-bottom:1px solid {border};",
                    border = t.border,
                ),
                div {
                    style: "display:flex; align-items:baseline; gap:12px;",
                    div {
                        style: format!(
                            "font-size:{font_title}; font-weight:700; letter-spacing:0.5px;",
                            font_title = t.font_size_title,
                        ),
                        "FTS LIVETUNE"
                    }
                    div {
                        style: format!(
                            "font-size:14px; color:{confidence_color}; \
                             font-family:{font_mono}; font-variant-numeric:tabular-nums; font-weight:600;",
                            font_mono = t.font_mono,
                        ),
                        "{pitch_text}"
                    }
                    div {
                        style: format!(
                            "{style_value} color:{color};",
                            color = if correction_st.abs() > 0.5 { t.signal_warn }
                            else { t.text_dim },
                        ),
                        "Correction: {correction_text}"
                    }
                }
                div {
                    style: format!(
                        "font-size:{font_label}; color:{text_dim};",
                        font_label = t.font_size_label,
                        text_dim = t.text_dim,
                    ),
                    "FastTrackStudio"
                }
            }

            // ── Key & Scale row ──────────────────────────────────
            div {
                style: format!(
                    "display:flex; gap:{spacing};",
                    spacing = t.spacing_section,
                ),

                // Key selector
                div {
                    style: format!(
                        "{style_card} display:flex; flex-direction:column; gap:6px;",
                    ),
                    SectionLabel { text: "Key" }
                    div {
                        style: "display:flex; gap:3px; flex-wrap:wrap; max-width:260px;",
                        for i in 0..12i32 {
                            SegmentButton {
                                label: NOTE_NAMES[i as usize],
                                selected: current_key == i,
                                on_click: key_setter(i),
                            }
                        }
                    }
                }

                // Scale selector
                div {
                    style: format!(
                        "{style_card} display:flex; flex-direction:column; gap:6px;",
                    ),
                    SectionLabel { text: "Scale" }
                    div {
                        style: "display:flex; gap:3px; flex-wrap:wrap;",
                        SegmentButton { label: "Chromatic", selected: current_scale == 0, on_click: scale_setter(0) }
                        SegmentButton { label: "Major", selected: current_scale == 1, on_click: scale_setter(1) }
                        SegmentButton { label: "Minor", selected: current_scale == 2, on_click: scale_setter(2) }
                        SegmentButton { label: "Maj Penta", selected: current_scale == 3, on_click: scale_setter(3) }
                        SegmentButton { label: "Min Penta", selected: current_scale == 4, on_click: scale_setter(4) }
                        SegmentButton { label: "Blues", selected: current_scale == 5, on_click: scale_setter(5) }
                        SegmentButton { label: "Custom", selected: current_scale == 6, on_click: scale_setter(6) }
                    }
                }
            }

            // ── Main controls + Meters ───────────────────────────
            div {
                style: format!(
                    "display:flex; gap:{spacing}; flex:1; min-height:0;",
                    spacing = t.spacing_section,
                ),

                // Main controls card
                div {
                    style: format!(
                        "{style_card} flex:1; display:flex; flex-direction:column; gap:12px;",
                    ),

                    // Tuning controls
                    div {
                        style: "display:flex; flex-direction:column; gap:8px;",
                        SectionLabel { text: "Tuning" }
                        div {
                            style: "display:flex; justify-content:center; gap:24px;",
                            Knob { param_ptr: retune_speed_ptr, size: KnobSize::Large }
                            Knob { param_ptr: amount_ptr, size: KnobSize::Large }
                            Knob { param_ptr: mix_ptr, size: KnobSize::Large }
                            Knob { param_ptr: output_gain_ptr, size: KnobSize::Large }
                        }
                    }

                    // Detector + Shifter + Advanced
                    div {
                        style: "display:flex; gap:16px; justify-content:center;",

                        div {
                            style: "display:flex; flex-direction:column; gap:6px; align-items:center;",
                            SectionLabel { text: "Detector" }
                            div {
                                style: "display:flex; gap:4px; flex-wrap:wrap;",
                                SegmentButton { label: "YIN", selected: current_detector == 0, on_click: detector_setter(0) }
                                SegmentButton { label: "YAAPT", selected: current_detector == 1, on_click: detector_setter(1) }
                                SegmentButton { label: "pYIN", selected: current_detector == 2, on_click: detector_setter(2) }
                                SegmentButton { label: "MPM", selected: current_detector == 3, on_click: detector_setter(3) }
                                SegmentButton { label: "Bitstream", selected: current_detector == 4, on_click: detector_setter(4) }
                            }
                        }

                        div {
                            style: format!(
                                "width:1px; background:{border}; align-self:stretch;",
                                border = t.border,
                            ),
                        }

                        div {
                            style: "display:flex; flex-direction:column; gap:6px; align-items:center;",
                            SectionLabel { text: "Shifter" }
                            div {
                                style: "display:flex; gap:4px;",
                                SegmentButton { label: "Auto", selected: current_shifter == 0, on_click: shifter_setter(0) }
                                SegmentButton { label: "PSOLA", selected: current_shifter == 1, on_click: shifter_setter(1) }
                                SegmentButton { label: "Vocoder", selected: current_shifter == 2, on_click: shifter_setter(2) }
                                SegmentButton { label: "PVSOLA", selected: current_shifter == 3, on_click: shifter_setter(3) }
                            }
                        }

                        div {
                            style: format!(
                                "width:1px; background:{border}; align-self:stretch;",
                                border = t.border,
                            ),
                        }

                        div {
                            style: "display:flex; flex-direction:column; gap:6px; align-items:center;",
                            SectionLabel { text: "Advanced" }
                            div {
                                style: format!(
                                    "display:flex; gap:{spacing}; align-items:flex-end;",
                                    spacing = t.spacing_control,
                                ),
                                Knob { param_ptr: confidence_ptr, size: KnobSize::Medium }
                                Toggle { param_ptr: formants_ptr, label: "Formants" }
                            }
                        }
                    }

                    // Note enable grid (Custom scale mode only)
                    if current_scale == 6 {
                        div {
                            style: "display:flex; flex-direction:column; gap:6px;",
                            SectionLabel { text: "Note Enable" }
                            div {
                                style: "display:flex; gap:6px; justify-content:center; flex-wrap:wrap;",
                                for i in 0..12usize {
                                    Toggle { param_ptr: note_ptrs[i], label: NOTE_NAMES[i] }
                                }
                            }
                        }
                    }
                }

                // Meters
                div {
                    style: format!(
                        "{style_card} display:flex; gap:8px; align-items:stretch;",
                    ),
                    LevelMeterDb { level_db: input_db, label: "IN".to_string(), height: 280.0 }
                    LevelMeterDb { level_db: output_db, label: "OUT".to_string(), height: 280.0 }
                }
            }
        }
        } // DragProvider
    }
}
