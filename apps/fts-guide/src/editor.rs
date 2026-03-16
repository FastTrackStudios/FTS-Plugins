//! Dioxus-based editor for FTS Guide plugin
//!
//! Uses inline styles throughout because the Blitz renderer in nih_plug_dioxus
//! doesn't have full Tailwind CSS coverage for external component libraries.
//! All interactive controls use basic div onclick events which Blitz handles reliably.

use atomic_float::AtomicF32;
use fts_plugin_core::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::GuideParams;

/// Shared UI state for the plugin editor
#[derive(Clone)]
pub struct GuideUiState {
    pub params: Arc<GuideParams>,
    pub transport_tempo: Arc<AtomicF32>,
    pub transport_time_sig_numerator: Arc<AtomicI32>,
    pub transport_time_sig_denominator: Arc<AtomicI32>,
    pub transport_bar_number: Arc<AtomicI32>,
    pub transport_beat_position: Arc<AtomicF32>,
    pub transport_playing: Arc<AtomicBool>,
    pub request_generate_midi: Arc<AtomicBool>,
}

/// Create the plugin editor
pub fn create(
    params: Arc<GuideParams>,
    transport_tempo: Arc<AtomicF32>,
    transport_time_sig_numerator: Arc<AtomicI32>,
    transport_time_sig_denominator: Arc<AtomicI32>,
    transport_bar_number: Arc<AtomicI32>,
    transport_beat_position: Arc<AtomicF32>,
    transport_playing: Arc<AtomicBool>,
    request_generate_midi: Arc<AtomicBool>,
) -> Option<Box<dyn Editor>> {
    let ui_state = Arc::new(GuideUiState {
        params,
        transport_tempo,
        transport_time_sig_numerator,
        transport_time_sig_denominator,
        transport_bar_number,
        transport_beat_position,
        transport_playing,
        request_generate_midi,
    });

    create_dioxus_editor_with_state(ui_state.params.editor_state.clone(), ui_state, App)
}

// ── Color constants ──────────────────────────────────────────────────
const BG: &str = "#1a1a2e";
const CARD_BG: &str = "#222240";
const TEXT: &str = "#e0e0e0";
const TEXT_DIM: &str = "#888";
const ACCENT: &str = "#6c63ff";
const ACCENT_HOVER: &str = "#7c74ff";
const GREEN: &str = "#4ade80";
const TOGGLE_OFF: &str = "#444";
const BORDER: &str = "#333";

/// Main app component
fn App() -> Element {
    let shared = use_context::<SharedState>();
    let ui_state = shared
        .get::<GuideUiState>()
        .expect("GuideUiState not in context");
    let ctx = use_param_context();

    // Read transport
    let tempo = ui_state.transport_tempo.load(Ordering::Relaxed);
    let time_sig_num = ui_state.transport_time_sig_numerator.load(Ordering::Relaxed);
    let time_sig_den = ui_state.transport_time_sig_denominator.load(Ordering::Relaxed);
    let beat_position = ui_state.transport_beat_position.load(Ordering::Relaxed);
    let is_playing = ui_state.transport_playing.load(Ordering::Relaxed);

    let params = &ui_state.params;

    // Read click sound (normalized 0..1 → index 0..7)
    let click_idx = (use_param_normalized(&params.click_sound) * 7.0).round() as usize;
    let click_sounds = ["Blip", "Classic", "Cowbell", "Digital", "Gentle", "Perc", "Saw", "Wood"];

    let generate_flag = ui_state.request_generate_midi.clone();

    // Revision signal — bumped on any param/size change to force App re-render
    let mut app_rev = use_signal(|| 0u32);
    let _ = *app_rev.read();

    // Track window size changes so the status bar updates
    let mut last_size = use_signal(|| (0u32, 0u32));
    if let Some(state) = try_use_context::<Arc<DioxusState>>() {
        let current = state.size();
        if *last_size.read() != current {
            last_size.set(current);
            app_rev += 1;
        }
    }

    rsx! {
        document::Style { {TAILWIND_CSS} }
        document::Style { "*, *::before, *::after {{ box-sizing: border-box; margin: 0; padding: 0; }} html, body {{ background: {BG}; width: 100%; height: 100%; overflow: hidden; }}" }

        div {
            style: "width:100vw; height:100vh; padding:12px 16px; \
                    background:{BG}; color:{TEXT}; \
                    font-family:system-ui,sans-serif; font-size:13px; user-select:none; \
                    overflow-y:auto; position:relative;",

            // ── Header ───────────────────────────────────────────
            div {
                style: "display:flex; justify-content:space-between; align-items:center; \
                        margin-bottom:10px; padding-bottom:8px; border-bottom:1px solid {BORDER};",
                div { style: "font-size:18px; font-weight:700;", "FTS Guide" }
                div {
                    style: "display:flex; gap:12px; align-items:center; font-size:12px; color:{TEXT_DIM};",
                    span { "{tempo:.0} BPM" }
                    span { "{time_sig_num}/{time_sig_den}" }
                    span {
                        style: format!("padding:2px 8px; border-radius:4px; font-size:11px; \
                                       background:{}; color:#fff;",
                                       if is_playing { GREEN } else { "#555" }),
                        if is_playing { "PLAY" } else { "STOP" }
                    }
                }
            }

            // ── Sync to Transport toggle ─────────────────────────
            div {
                style: "display:flex; align-items:center; gap:8px; margin-bottom:10px;",
                Toggle { param_ptr: params.sync_to_transport.as_ptr() }
                span { style: "font-size:12px; font-weight:600;", "Sync to Transport" }
                span { style: "font-size:11px; color:{TEXT_DIM};", "(auto-click on playback)" }
            }

            // ── Click Section ────────────────────────────────────
            Section { title: "Click",
                // Sound selector
                div {
                    style: "display:flex; gap:2px; margin-bottom:8px;",
                    for (i, name) in click_sounds.iter().enumerate() {
                        SegmentButton {
                            label: name,
                            selected: i == click_idx,
                            on_click: {
                                let ptr = params.click_sound.as_ptr();
                                let ctx = ctx.clone();
                                move |_| {
                                    let norm = i as f32 / 7.0;
                                    ctx.begin_set_raw(ptr);
                                    ctx.set_normalized_raw(ptr, norm);
                                    ctx.end_set_raw(ptr);
                                    app_rev += 1;
                                }
                            },
                        }
                    }
                }

                // Subdivision toggles
                div {
                    style: "display:flex; gap:12px; margin-bottom:8px;",
                    Toggle { param_ptr: params.enable_beat.as_ptr(), label: "Beat" }
                    Toggle { param_ptr: params.enable_eighth.as_ptr(), label: "8th" }
                    Toggle { param_ptr: params.enable_sixteenth.as_ptr(), label: "16th" }
                    Toggle { param_ptr: params.enable_triplet.as_ptr(), label: "Trip" }
                    Toggle { param_ptr: params.enable_measure_accent.as_ptr(), label: "Accent" }
                }

                ParamSlider { param_ptr: params.click_volume.as_ptr() }
            }

            // ── Count-In Section ─────────────────────────────────
            Section { title: "Count-In",
                div {
                    style: "display:flex; gap:12px; margin-bottom:8px;",
                    Toggle { param_ptr: params.enable_count.as_ptr(), label: "Enable" }
                    Toggle { param_ptr: params.offset_count_by_one.as_ptr(), label: "Offset +1" }
                    Toggle { param_ptr: params.extend_songend_count.as_ptr(), label: "Extend End" }
                    Toggle { param_ptr: params.full_count_odd_time.as_ptr(), label: "Full Odd" }
                }
                ParamSlider { param_ptr: params.count_volume.as_ptr() }
            }

            // ── Guide Section ────────────────────────────────────
            Section { title: "Guide",
                div {
                    style: "display:flex; gap:12px; margin-bottom:8px;",
                    Toggle { param_ptr: params.enable_guide.as_ptr(), label: "Enable" }
                    Toggle { param_ptr: params.guide_replace_beat1.as_ptr(), label: "Replace Beat 1" }
                }
                ParamSlider { param_ptr: params.guide_volume.as_ptr() }
            }

            // ── Master ───────────────────────────────────────────
            Section { title: "Master",
                ParamSlider { param_ptr: params.gain.as_ptr() }
            }

            // ── Generate MIDI button ─────────────────────────────
            div {
                style: format!(
                    "padding:8px 0; cursor:pointer; text-align:center; \
                     background:{ACCENT}; color:#fff; border-radius:6px; \
                     font-size:13px; font-weight:600; margin-top:4px;"
                ),
                onclick: {
                    let flag = generate_flag.clone();
                    move |_| { flag.store(true, Ordering::Relaxed); }
                },
                "Generate Guide MIDI"
            }

            // ── Status bar ───────────────────────────────────────
            div {
                style: "padding-top:6px; margin-top:8px; border-top:1px solid {BORDER}; \
                        font-size:11px; color:{TEXT_DIM};",
                {
                    // Read DioxusState from context to show current window size
                    let size_info = if let Some(state) = try_use_context::<Arc<DioxusState>>() {
                        let (w, h) = state.size();
                        format!("Beat: {beat_position:.2} | {w}x{h}")
                    } else {
                        format!("Beat: {beat_position:.2}")
                    };
                    rsx! { "{size_info}" }
                }
            }

        }
    }
}

// ── Section wrapper ──────────────────────────────────────────────────

#[component]
fn Section(title: &'static str, children: Element) -> Element {
    rsx! {
        div {
            style: "background:{CARD_BG}; border-radius:6px; padding:10px 12px; margin-bottom:8px;",
            div {
                style: "font-size:11px; font-weight:600; text-transform:uppercase; \
                        letter-spacing:0.5px; color:{TEXT_DIM}; margin-bottom:6px;",
                "{title}"
            }
            {children}
        }
    }
}

// ── Toggle (inline-styled, works in Blitz) ───────────────────────────

#[component]
fn Toggle(param_ptr: ParamPtr, label: Option<&'static str>) -> Element {
    let ctx = use_param_context();
    // Local signal drives re-renders — toggled on each click
    let mut revision = use_signal(|| 0u32);
    let _ = *revision.read(); // subscribe to it

    let normalized = unsafe { param_ptr.modulated_normalized_value() };
    let on = normalized > 0.5;

    let track_bg = if on { ACCENT } else { TOGGLE_OFF };
    let thumb_x = if on { "18px" } else { "2px" };

    rsx! {
        div {
            style: "display:flex; align-items:center; gap:6px; cursor:pointer;",
            onclick: {
                let ctx = ctx.clone();
                move |_| {
                    ctx.begin_set_raw(param_ptr);
                    ctx.set_normalized_raw(param_ptr, if on { 0.0 } else { 1.0 });
                    ctx.end_set_raw(param_ptr);
                    // Bump the signal to force Dioxus to re-render this component
                    revision += 1;
                }
            },
            // Track
            div {
                style: format!(
                    "width:36px; height:20px; border-radius:10px; position:relative; \
                     background:{track_bg}; transition:background 0.15s;"
                ),
                // Thumb
                div {
                    style: format!(
                        "width:16px; height:16px; border-radius:8px; background:#fff; \
                         position:absolute; top:2px; left:{thumb_x}; \
                         transition:left 0.15s;"
                    ),
                }
            }
            if let Some(lbl) = label {
                span {
                    style: format!("font-size:12px; color:{};", if on { TEXT } else { TEXT_DIM }),
                    "{lbl}"
                }
            }
        }
    }
}

// ── Segment button (for click sound picker) ──────────────────────────

#[component]
fn SegmentButton(label: &'static str, selected: bool, on_click: EventHandler<()>) -> Element {
    let bg = if selected { ACCENT } else { "transparent" };
    let border = if selected { ACCENT } else { BORDER };
    let color = if selected { "#fff" } else { TEXT_DIM };

    rsx! {
        div {
            style: format!(
                "padding:4px 8px; border-radius:4px; font-size:11px; font-weight:500; \
                 cursor:pointer; border:1px solid {border}; background:{bg}; color:{color};"
            ),
            onclick: move |_| on_click.call(()),
            "{label}"
        }
    }
}
