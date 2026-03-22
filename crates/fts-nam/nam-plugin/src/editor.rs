//! NAM editor — Dioxus GUI root component.
//!
//! Layout: header with metering, model/IR load buttons, knob controls, gate section.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use audio_gui::controls::knob::Knob;
use audio_gui::prelude::{
    use_init_theme, use_theme, ControlGroup, DragProvider, KnobSize, LevelMeterDb, SectionLabel,
};
use fts_plugin_core::prelude::*;

use crate::{NamLoadMessage, NamUiState};

/// Root editor component.
#[component]
pub fn App() -> Element {
    let t = use_init_theme();
    let t = *t.read();

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
    let slot_a_name = ui.slot_a_name.lock().map(|n| n.clone()).unwrap_or_default();
    let slot_b_name = ui.slot_b_name.lock().map(|n| n.clone()).unwrap_or_default();
    let ir_a_name = ui.ir_a_name.lock().map(|n| n.clone()).unwrap_or_default();
    let ir_b_name = ui.ir_b_name.lock().map(|n| n.clone()).unwrap_or_default();

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

    // Bind theme fields for use in format! strings
    let spacing_root = t.spacing_root;
    let bg = t.bg;
    let text = t.text;
    let font_family = t.font_family;
    let spacing_section = t.spacing_section;
    let border = t.border;
    let font_size_title = t.font_size_title;
    let font_size_tiny = t.font_size_tiny;
    let text_dim = t.text_dim;
    let accent = t.accent;
    let spacing_card = t.spacing_card;
    let spacing_control = t.spacing_control;
    let style_card = t.style_card();
    let style_label = t.style_label();
    let _style_value = t.style_value();

    rsx! {
        document::Style { {t.base_css()} }

        DragProvider {
        div {
            style: format!(
                "width:100vw; height:100vh; padding:{spacing_root}; \
                 background:{bg}; color:{text}; \
                 font-family:{font_family}; font-size:13px; user-select:none; \
                 display:flex; flex-direction:column; gap:{spacing_section}; overflow:hidden;",
            ),

            // ── Header ───────────────────────────────────────────
            div {
                style: format!(
                    "display:flex; justify-content:space-between; align-items:center; \
                     padding-bottom:6px; border-bottom:1px solid {border};",
                ),
                div {
                    style: "display:flex; align-items:baseline; gap:12px;",
                    div {
                        style: format!("font-size:{font_size_title}; font-weight:700; letter-spacing:0.5px;"),
                        "FTS NAM"
                    }
                    div {
                        style: format!("{style_label} font-size:{font_size_tiny};"),
                        "Latency: {latency_text}"
                    }
                    div {
                        style: format!(
                            "font-size:{font_size_tiny}; color:{};",
                            if gate_text == "OPEN" { accent } else { text_dim }
                        ),
                        "Gate: {gate_text}"
                    }
                }
                div {
                    style: format!("font-size:{font_size_tiny}; color:{text_dim};"),
                    "FastTrackStudio"
                }
            }

            // ── Main content ─────────────────────────────────────
            div {
                style: format!("display:flex; gap:{spacing_section}; flex:1; min-height:0;"),

                // Left panel: Model & IR loading + controls
                div {
                    style: format!(
                        "flex:1; {style_card} padding:{spacing_card}; \
                         display:flex; flex-direction:column; gap:{spacing_control}; overflow-y:auto;",
                    ),

                    // Model slots
                    SectionLabel { text: "Models" }
                    div {
                        style: format!("display:flex; gap:{spacing_section};"),
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
                        style: format!("display:flex; gap:{spacing_section};"),
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
                        style: format!("display:flex; gap:20px; justify-content:center; margin-top:{spacing_section};"),

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
                        "{style_card} padding:{spacing_section}; \
                         display:flex; gap:{spacing_section}; align-items:stretch;",
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
    let t = use_theme();
    let t = *t.read();

    let display = if name.is_empty() {
        "(empty)".to_string()
    } else {
        name
    };

    // Bind theme fields for use in format! strings
    let surface_raised = t.surface_raised;
    let border = t.border;
    let radius_button = t.radius_button;
    let spacing_section = t.spacing_section;
    let spacing_label = t.spacing_label;
    let transition_fast = t.transition_fast;
    let shadow_subtle = t.shadow_subtle;
    let style_label = t.style_label();
    let style_value = t.style_value();
    let text_dim = t.text_dim;
    let text = t.text;

    rsx! {
        div {
            style: format!(
                "flex:1; background:{surface_raised}; border:1px solid {border}; \
                 border-radius:{radius_button}; padding:{spacing_section}; \
                 cursor:pointer; display:flex; flex-direction:column; gap:{spacing_label}; \
                 transition:{transition_fast}; box-shadow:{shadow_subtle};",
            ),
            onclick: move |_| on_click.call(()),

            div {
                style: format!("{style_label}"),
                "{label}"
            }
            div {
                style: format!(
                    "{style_value} color:{}; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;",
                    if display == "(empty)" { text_dim } else { text }
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
