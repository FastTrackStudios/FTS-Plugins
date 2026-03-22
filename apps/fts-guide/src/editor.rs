//! Dioxus-based editor for FTS Guide plugin.

use atomic_float::AtomicF32;
use fts_plugin_core::prelude::*;
use fts_plugin_core::ui::prelude::{
    use_init_theme, ActionButton, Header, ParamSlider, Section, SegmentButton, StatusBar, Toggle,
};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;

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

/// Main app component
fn App() -> Element {
    let t = use_init_theme();
    let t = *t.read();
    let shared = use_context::<SharedState>();
    let ui_state = shared
        .get::<GuideUiState>()
        .expect("GuideUiState not in context");
    let ctx = use_param_context();

    // Read transport
    let tempo = ui_state.transport_tempo.load(Ordering::Relaxed);
    let time_sig_num = ui_state
        .transport_time_sig_numerator
        .load(Ordering::Relaxed);
    let time_sig_den = ui_state
        .transport_time_sig_denominator
        .load(Ordering::Relaxed);
    let beat_position = ui_state.transport_beat_position.load(Ordering::Relaxed);
    let is_playing = ui_state.transport_playing.load(Ordering::Relaxed);

    let params = &ui_state.params;

    // Click sound enum → index
    let click_idx = (use_param_normalized(&params.click_sound) * 7.0).round() as usize;
    let click_sounds = [
        "Blip", "Classic", "Cowbell", "Digital", "Gentle", "Perc", "Saw", "Wood",
    ];

    let generate_flag = ui_state.request_generate_midi.clone();

    // Revision signal — bumped on param/size change to force re-render
    let mut app_rev = use_signal(|| 0u32);
    let _ = *app_rev.read();

    // Track window size changes
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
        document::Style { {t.base_css()} }

        div {
            style: t.root_style(),

            Header {
                title: "FTS Guide",
                tempo: tempo,
                time_sig_num: time_sig_num,
                time_sig_den: time_sig_den,
                is_playing: is_playing,
            }

            // Sync to Transport
            div {
                style: "display:flex; align-items:center; gap:8px; margin-bottom:10px;",
                Toggle { param_ptr: params.sync_to_transport.as_ptr() }
                span { style: "font-size:12px; font-weight:600;", "Sync to Transport" }
                span { style: format!("font-size:11px; color:{};", t.text_dim), "(auto-click on playback)" }
            }

            // Click
            Section { title: "Click",
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

            // Count-In
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

            // Guide
            Section { title: "Guide",
                div {
                    style: "display:flex; gap:12px; margin-bottom:8px;",
                    Toggle { param_ptr: params.enable_guide.as_ptr(), label: "Enable" }
                    Toggle { param_ptr: params.guide_replace_beat1.as_ptr(), label: "Replace Beat 1" }
                }
                ParamSlider { param_ptr: params.guide_volume.as_ptr() }
            }

            // Master
            Section { title: "Master",
                ParamSlider { param_ptr: params.gain.as_ptr() }
            }

            // Actions
            ActionButton {
                label: "Generate Guide MIDI",
                on_click: {
                    let flag = generate_flag.clone();
                    move |_| { flag.store(true, Ordering::Relaxed); }
                },
            }

            StatusBar { beat_position: beat_position }
        }
    }
}
