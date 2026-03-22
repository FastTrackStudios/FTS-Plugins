//! FTS Trigger — Dioxus GUI editor.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::slider::ParamSlider;
use audio_gui::prelude::{
    theme, ControlGroup, Divider, DragProvider, KnobSize, LevelMeterDb, PeakWaveform,
};
use fts_plugin_core::prelude::*;

use crate::engine::NUM_SLOTS;
use crate::loader;
use crate::{TriggerUiState, WAVEFORM_LEN};
use trigger_ui::control_view::MixerStrip;

/// Root editor component.
#[component]
pub fn App() -> Element {
    let shared = use_context::<SharedState>();
    let ui = shared
        .get::<TriggerUiState>()
        .expect("TriggerUiState missing");
    let ui_for_load = ui.clone();
    let params = &ui.params;

    // Read metering
    let velocity = ui.last_velocity.load(Ordering::Relaxed);
    let triggered = ui.triggered.load(Ordering::Relaxed);
    let input_db = ui.input_peak_db.load(Ordering::Relaxed);
    let output_db = ui.output_peak_db.load(Ordering::Relaxed);

    // Build waveform from ring buffer
    let pos = ui.waveform_pos.load(Ordering::Relaxed) as usize % WAVEFORM_LEN;
    let mut waveform_in = Vec::with_capacity(WAVEFORM_LEN);
    let mut waveform_trig = Vec::with_capacity(WAVEFORM_LEN);
    for i in 0..WAVEFORM_LEN {
        let idx = (pos + i) % WAVEFORM_LEN;
        waveform_in.push(ui.waveform_input[idx].load(Ordering::Relaxed));
        waveform_trig.push(ui.waveform_triggers[idx].load(Ordering::Relaxed));
    }

    let vel_text = format!("{:.2}", velocity);
    let trig_color = if triggered > 0.5 {
        theme::SIGNAL_DANGER
    } else if triggered > 0.01 {
        theme::SIGNAL_WARN
    } else {
        theme::TOGGLE_OFF
    };

    // Slot data
    let slot_names: Vec<String> = (0..NUM_SLOTS)
        .map(|s| {
            ui.slot_names[s]
                .lock()
                .map(|n| n.clone())
                .unwrap_or_default()
        })
        .collect();
    let slot_peaks: Vec<f32> = (0..NUM_SLOTS)
        .map(|s| ui.slot_peak_db[s].load(Ordering::Relaxed))
        .collect();
    let slot_playing: Vec<bool> = (0..NUM_SLOTS)
        .map(|s| ui.slot_playing[s].load(Ordering::Relaxed) > 0.5)
        .collect();

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
                        "FTS TRIGGER"
                    }
                    div {
                        style: format!(
                            "font-size:12px; color:{}; font-variant-numeric:tabular-nums;",
                            if velocity > 0.8 { theme::SIGNAL_WARN }
                            else if velocity > 0.01 { theme::SIGNAL_SAFE }
                            else { theme::TEXT_DIM }
                        ),
                        "Vel: {vel_text}"
                    }
                    div {
                        style: format!(
                            "width:12px; height:12px; border-radius:50%; background:{};",
                            trig_color,
                        ),
                    }
                }
                div {
                    style: format!("font-size:11px; color:{};", theme::TEXT_DIM),
                    "FastTrackStudio"
                }
            }

            // ── Waveform ─────────────────────────────────────────
            div {
                style: format!(
                    "background:{SURFACE}; border-radius:4px; padding:4px; min-height:60px;",
                    SURFACE = theme::SURFACE,
                ),
                PeakWaveform {
                    levels: waveform_in,
                    gr_levels: waveform_trig,
                    width: 1020.0,
                    height: 56.0,
                }
            }

            // ── Mixer Section (8 strips) ─────────────────────────
            div {
                style: format!(
                    "background:{CARD_BG}; border-radius:6px; padding:8px; \
                     display:flex; gap:4px; flex:1; min-height:0;",
                    CARD_BG = theme::CARD_BG,
                ),
                for slot in 0..NUM_SLOTS {
                    MixerStrip {
                        key: "{slot}",
                        slot: slot,
                        name: slot_names[slot].clone(),
                        peak_db: slot_peaks[slot],
                        playing: slot_playing[slot],
                        gain_ptr: params.slots[slot].gain.as_ptr(),
                        pan_ptr: params.slots[slot].pan.as_ptr(),
                        pitch_ptr: params.slots[slot].pitch.as_ptr(),
                        enabled_ptr: params.slots[slot].enabled.as_ptr(),
                        mute_ptr: params.slots[slot].mute.as_ptr(),
                        solo_ptr: params.slots[slot].solo.as_ptr(),
                        on_load: {
                            let ui = ui_for_load.clone();
                            move |slot: usize| {
                                open_file_dialog(slot, ui.clone());
                            }
                        },
                    }
                }
            }

            // ── Bottom Controls ──────────────────────────────────
            div {
                style: format!(
                    "background:{CARD_BG}; border-radius:6px; padding:10px 16px; \
                     display:flex; gap:20px; align-items:flex-start;",
                    CARD_BG = theme::CARD_BG,
                ),

                ControlGroup {
                    label: "Detection",
                    Knob { param_ptr: params.threshold.as_ptr(), size: KnobSize::Medium }
                    Knob { param_ptr: params.sensitivity.as_ptr(), size: KnobSize::Small }
                    Knob { param_ptr: params.retrigger.as_ptr(), size: KnobSize::Small }
                    Knob { param_ptr: params.reactivity.as_ptr(), size: KnobSize::Small }
                    Knob { param_ptr: params.release_time.as_ptr(), size: KnobSize::Small }
                    Knob { param_ptr: params.release_ratio.as_ptr(), size: KnobSize::Small }
                    ParamSlider { param_ptr: params.detect_mode.as_ptr() }
                }

                Divider {}

                ControlGroup {
                    label: "Algorithm",
                    ParamSlider { param_ptr: params.detect_algorithm.as_ptr() }
                }

                Divider {}

                ControlGroup {
                    label: "Sidechain",
                    Knob { param_ptr: params.sc_hpf.as_ptr(), size: KnobSize::Small }
                    Knob { param_ptr: params.sc_lpf.as_ptr(), size: KnobSize::Small }
                    ParamSlider { param_ptr: params.sc_listen.as_ptr() }
                }

                Divider {}

                ControlGroup {
                    label: "Velocity",
                    Knob { param_ptr: params.dynamics.as_ptr(), size: KnobSize::Medium }
                    ParamSlider { param_ptr: params.vel_curve.as_ptr() }
                }

                Divider {}

                ControlGroup {
                    label: "Output",
                    ParamSlider { param_ptr: params.mix_mode.as_ptr() }
                    Knob { param_ptr: params.mix_amount.as_ptr(), size: KnobSize::Small }
                    Knob { param_ptr: params.output_gain.as_ptr(), size: KnobSize::Medium }
                }

                Divider {}

                div {
                    style: "display:flex; gap:8px; align-items:flex-end;",
                    LevelMeterDb { level_db: input_db, label: "IN".to_string() }
                    LevelMeterDb { level_db: output_db, label: "OUT".to_string() }
                }
            }
        }
        } // DragProvider
    }
}

/// Open a native file dialog on a background thread, then send the
/// selected file to the audio thread for loading.
fn open_file_dialog(slot: usize, ui: Arc<TriggerUiState>) {
    std::thread::spawn(move || {
        let title = format!("Load Sample — Slot {}", slot + 1);
        let file = fts_sample::dialog::pick_audio_file(&title);

        if let Some(path) = file {
            let sr = ui.sample_rate.load(Ordering::Relaxed) as f64;
            let tx = ui.sample_tx.clone();
            let path_str = path.to_string_lossy().to_string();

            // Persist the path for DAW recall
            if let Ok(mut paths) = ui.params.slot_paths.lock() {
                paths.paths[slot] = Some(path_str.clone());
            }

            loader::load_sample_async(path_str, slot, sr, tx);
        }
    });
}
