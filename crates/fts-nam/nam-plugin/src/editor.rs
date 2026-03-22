//! NAM editor — Dioxus GUI root component.
//!
//! Layout: header with metering, model/IR load buttons, knob controls, gate section.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use audio_gui::controls::knob::Knob;
use audio_gui::prelude::{
    theme, ControlGroup, DragProvider, KnobSize, LevelMeterDb, SectionLabel,
};
use fts_plugin_core::prelude::*;

use crate::{NamLoadMessage, NamUiState};

/// Root editor component.
#[component]
pub fn App() -> Element {
    let shared = use_context::<SharedState>();
    let ui = shared.get::<NamUiState>().expect("NamUiState missing");
    let ui_for_load = ui.clone();
    let params = &ui.params;

    // Read metering values
    let input_db = ui.input_peak_db.load(Ordering::Relaxed);
    let output_db = ui.output_peak_db.load(Ordering::Relaxed);
    let latency = ui.latency_samples.load(Ordering::Relaxed);
    let gate_gain = ui.gate_gain.load(Ordering::Relaxed);

    // Read slot names
    let slot_a_name = ui
        .slot_a_name
        .lock()
        .map(|n| n.clone())
        .unwrap_or_default();
    let slot_b_name = ui
        .slot_b_name
        .lock()
        .map(|n| n.clone())
        .unwrap_or_default();
    let ir_a_name = ui
        .ir_a_name
        .lock()
        .map(|n| n.clone())
        .unwrap_or_default();
    let ir_b_name = ui
        .ir_b_name
        .lock()
        .map(|n| n.clone())
        .unwrap_or_default();

    // Format latency display
    let latency_text = if latency > 0.0 {
        format!("{:.0} smp", latency)
    } else {
        "0".to_string()
    };

    // Gate indicator
    let gate_text = if params.gate_enabled.value() > 0.5 {
        if gate_gain > 0.95 {
            "OPEN"
        } else if gate_gain < 0.05 {
            "CLOSED"
        } else {
            "..."
        }
    } else {
        "OFF"
    };

    rsx! {
        document::Style { {theme::BASE_CSS} }

        DragProvider {
        div {
            style: format!(
                "width:100vw; height:100vh; padding:{SPACING_ROOT}; \
                 background:{BG}; color:{TEXT}; \
                 font-family:{FONT}; font-size:13px; user-select:none; \
                 display:flex; flex-direction:column; gap:{SPACING_SECTION}; overflow:hidden;",
                SPACING_ROOT = theme::SPACING_ROOT,
                BG = theme::BG, TEXT = theme::TEXT,
                FONT = theme::FONT_FAMILY,
                SPACING_SECTION = theme::SPACING_SECTION,
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
                        style: format!("font-size:{}; font-weight:700; letter-spacing:0.5px;", theme::FONT_SIZE_TITLE),
                        "FTS NAM"
                    }
                    div {
                        style: format!("{} font-size:{};", theme::STYLE_LABEL, theme::FONT_SIZE_TINY),
                        "Latency: {latency_text}"
                    }
                    div {
                        style: format!(
                            "font-size:{}; color:{};",
                            theme::FONT_SIZE_TINY,
                            if gate_text == "OPEN" { theme::ACCENT } else { theme::TEXT_DIM }
                        ),
                        "Gate: {gate_text}"
                    }
                }
                div {
                    style: format!("font-size:{}; color:{};", theme::FONT_SIZE_TINY, theme::TEXT_DIM),
                    "FastTrackStudio"
                }
            }

            // ── Main content ─────────────────────────────────────
            div {
                style: format!("display:flex; gap:{}; flex:1; min-height:0;", theme::SPACING_SECTION),

                // Left panel: Model & IR loading + controls
                div {
                    style: format!(
                        "flex:1; {STYLE_CARD} padding:{SPACING_CARD}; \
                         display:flex; flex-direction:column; gap:{SPACING_CONTROL}; overflow-y:auto;",
                        STYLE_CARD = theme::STYLE_CARD,
                        SPACING_CARD = theme::SPACING_CARD,
                        SPACING_CONTROL = theme::SPACING_CONTROL,
                    ),

                    // Model slots
                    SectionLabel { text: "Models" }
                    div {
                        style: format!("display:flex; gap:{};", theme::SPACING_SECTION),
                        LoadSlot {
                            label: "Model A",
                            name: slot_a_name,
                            on_click: {
                                let ui = ui_for_load.clone();
                                move |_| open_nam_dialog(0, ui.clone())
                            },
                        }
                        LoadSlot {
                            label: "Model B",
                            name: slot_b_name,
                            on_click: {
                                let ui = ui_for_load.clone();
                                move |_| open_nam_dialog(1, ui.clone())
                            },
                        }
                    }

                    // IR slots
                    SectionLabel { text: "Cabinet IRs" }
                    div {
                        style: format!("display:flex; gap:{};", theme::SPACING_SECTION),
                        LoadSlot {
                            label: "IR A",
                            name: ir_a_name,
                            on_click: {
                                let ui = ui_for_load.clone();
                                move |_| open_ir_dialog(2, ui.clone())
                            },
                        }
                        LoadSlot {
                            label: "IR B",
                            name: ir_b_name,
                            on_click: {
                                let ui = ui_for_load.clone();
                                move |_| open_ir_dialog(3, ui.clone())
                            },
                        }
                    }

                    // NAM knob controls
                    div {
                        style: format!("display:flex; gap:20px; justify-content:center; margin-top:{};", theme::SPACING_SECTION),

                        ControlGroup {
                            label: "Models",
                            Knob { param_ptr: params.blend.as_ptr(), size: KnobSize::Large }
                            Knob { param_ptr: params.delta_delay_samples.as_ptr(), size: KnobSize::Medium }
                        }

                        ControlGroup {
                            label: "Cabinet",
                            Knob { param_ptr: params.ir_mix.as_ptr(), size: KnobSize::Large }
                        }

                        ControlGroup {
                            label: "I/O",
                            Knob { param_ptr: params.input_gain_db.as_ptr(), size: KnobSize::Medium }
                            Knob { param_ptr: params.output_gain_db.as_ptr(), size: KnobSize::Medium }
                        }
                    }

                    // ── Gate section ─────────────────────────────
                    SectionLabel { text: "Noise Gate" }
                    div {
                        style: "display:flex; gap:16px; justify-content:center; flex-wrap:wrap;",

                        ControlGroup {
                            label: "Gate",
                            Knob { param_ptr: params.gate_enabled.as_ptr(), size: KnobSize::Small }
                            Knob { param_ptr: params.gate_threshold_db.as_ptr(), size: KnobSize::Large }
                            Knob { param_ptr: params.gate_hysteresis_db.as_ptr(), size: KnobSize::Medium }
                        }

                        ControlGroup {
                            label: "Envelope",
                            Knob { param_ptr: params.gate_attack_ms.as_ptr(), size: KnobSize::Medium }
                            Knob { param_ptr: params.gate_hold_ms.as_ptr(), size: KnobSize::Medium }
                            Knob { param_ptr: params.gate_release_ms.as_ptr(), size: KnobSize::Medium }
                        }

                        ControlGroup {
                            label: "Range / SC",
                            Knob { param_ptr: params.gate_range_db.as_ptr(), size: KnobSize::Medium }
                            Knob { param_ptr: params.gate_sc_hpf_freq.as_ptr(), size: KnobSize::Small }
                            Knob { param_ptr: params.gate_sc_lpf_freq.as_ptr(), size: KnobSize::Small }
                            Knob { param_ptr: params.gate_sc_source.as_ptr(), size: KnobSize::Small }
                        }
                    }
                }

                // Meters
                div {
                    style: format!(
                        "{STYLE_CARD} padding:{SPACING_SECTION}; \
                         display:flex; gap:{SPACING_SECTION}; align-items:stretch;",
                        STYLE_CARD = theme::STYLE_CARD,
                        SPACING_SECTION = theme::SPACING_SECTION,
                    ),
                    LevelMeterDb { level_db: input_db, label: "IN".to_string(), height: 380.0 }
                    LevelMeterDb { level_db: output_db, label: "OUT".to_string(), height: 380.0 }
                }
            }
        }
        } // DragProvider
    }
}

// ── Load Slot Component ──────────────────────────────────────────────

#[component]
fn LoadSlot(label: String, name: String, on_click: EventHandler<()>) -> Element {
    let display = if name.is_empty() {
        "(empty)".to_string()
    } else {
        name
    };

    rsx! {
        div {
            style: format!(
                "flex:1; background:{SURFACE_RAISED}; border:1px solid {BORDER}; \
                 border-radius:{RADIUS_BUTTON}; padding:{SPACING_SECTION}; \
                 cursor:pointer; display:flex; flex-direction:column; gap:{SPACING_LABEL}; \
                 transition:{TRANSITION}; box-shadow:{SHADOW};",
                SURFACE_RAISED = theme::SURFACE_RAISED,
                BORDER = theme::BORDER,
                RADIUS_BUTTON = theme::RADIUS_BUTTON,
                SPACING_SECTION = theme::SPACING_SECTION,
                SPACING_LABEL = theme::SPACING_LABEL,
                TRANSITION = theme::TRANSITION_FAST,
                SHADOW = theme::SHADOW_SUBTLE,
            ),
            onclick: move |_| on_click.call(()),

            div {
                style: format!("{}", theme::STYLE_LABEL),
                "{label}"
            }
            div {
                style: format!(
                    "{STYLE_VALUE} color:{}; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;",
                    if display == "(empty)" { theme::TEXT_DIM } else { theme::TEXT },
                    STYLE_VALUE = theme::STYLE_VALUE,
                ),
                "{display}"
            }
        }
    }
}

// ── File Dialogs ─────────────────────────────────────────────────────

/// Open a native file dialog for `.nam` model files on a background thread.
fn open_nam_dialog(slot: usize, ui: Arc<NamUiState>) {
    std::thread::spawn(move || {
        let label = if slot == 0 { "A" } else { "B" };
        let title = format!("Load NAM Model — Slot {label}");
        let file = fts_sample::dialog::pick_file(&title, &["nam"]);

        if let Some(path) = file {
            let path_str = path.to_string_lossy().to_string();
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            // Persist for DAW recall
            if let Ok(mut paths) = ui.params.model_paths.lock() {
                if slot == 0 {
                    paths.model_a = Some(path_str.clone());
                } else {
                    paths.model_b = Some(path_str.clone());
                }
            }

            let _ = ui.load_tx.try_send(NamLoadMessage {
                slot,
                path: path_str,
                name,
            });
        }
    });
}

/// Open a native file dialog for IR (WAV) files on a background thread.
fn open_ir_dialog(slot: usize, ui: Arc<NamUiState>) {
    std::thread::spawn(move || {
        let label = if slot == 2 { "A" } else { "B" };
        let title = format!("Load Cabinet IR — Slot {label}");
        let file = fts_sample::dialog::pick_file(&title, &["wav", "flac", "aiff", "aif"]);

        if let Some(path) = file {
            let path_str = path.to_string_lossy().to_string();
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            // Persist for DAW recall
            if let Ok(mut paths) = ui.params.model_paths.lock() {
                if slot == 2 {
                    paths.ir_a = Some(path_str.clone());
                } else {
                    paths.ir_b = Some(path_str.clone());
                }
            }

            let _ = ui.load_tx.try_send(NamLoadMessage {
                slot,
                path: path_str,
                name,
            });
        }
    });
}
