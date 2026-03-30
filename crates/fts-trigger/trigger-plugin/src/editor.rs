//! FTS Trigger — Dioxus GUI editor (Trigger 2-inspired layout).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::slider::ParamSlider;
use audio_gui::prelude::{use_init_theme, Divider, DragProvider, KnobSize, LevelMeterDb};
use fts_plugin_core::prelude::*;

use crate::engine::NUM_SLOTS;
use crate::loader;
use crate::{TriggerUiState, WAVEFORM_LEN};
use trigger_ui::control_view::MixerStrip;
use trigger_ui::trigger_waveform::TriggerWaveform;

/// Root editor component.
#[component]
pub fn App() -> Element {
    let t = use_init_theme();
    let t = *t.read();

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
    let threshold_db = params.threshold.value();

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
    let signal_danger = t.signal_danger;
    let signal_warn = t.signal_warn;
    let signal_safe = t.signal_safe;
    let toggle_off = t.toggle_off;
    let text_dim = t.text_dim;
    let trig_color = if triggered > 0.5 {
        signal_danger
    } else if triggered > 0.01 {
        signal_warn
    } else {
        toggle_off
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

    let base_css = t.base_css();
    let spacing_root = t.spacing_root;
    let bg = t.bg;
    let text = t.text;
    let spacing_section = t.spacing_section;
    let spacing_card = t.spacing_card;
    let spacing_label = t.spacing_label;
    let spacing_control = t.spacing_control;
    let font_size_title = t.font_size_title;
    let text_bright = t.text_bright;
    let radius_round = t.radius_round;
    let style_card = t.style_card();
    let style_value = t.style_value();
    let style_label = t.style_label();

    rsx! {
        document::Style { {base_css} }

        DragProvider {
        div {
            style: format!(
                "width:100vw; height:100vh; padding:{spacing_root}; \
                 background:{bg}; color:{text}; \
                 font-family:system-ui,sans-serif; font-size:13px; user-select:none; \
                 display:flex; flex-direction:column; gap:{spacing_section}; overflow:hidden;",
            ),

            // ══════════════════════════════════════════════════════════
            // TOP ROW: Controls | Waveform | Controls | Output
            // ══════════════════════════════════════════════════════════
            div {
                style: format!(
                    "{style_card} display:flex; gap:8px; align-items:stretch; \
                     padding:{spacing_card};",
                ),

                // ── Left Panel: Title + Detection + Sidechain ──────
                div {
                    style: "display:flex; flex-direction:column; gap:8px; \
                            min-width:140px; justify-content:space-between;",

                    // Title + trigger indicator
                    div {
                        style: format!(
                            "display:flex; flex-direction:column; gap:{spacing_label};",
                        ),
                        div {
                            style: format!(
                                "font-size:{font_size_title}; font-weight:700; \
                                 letter-spacing:0.5px; color:{text_bright};",
                            ),
                            "FTS TRIGGER"
                        }
                        div {
                            style: "display:flex; align-items:center; gap:6px;",
                            div {
                                style: format!(
                                    "width:10px; height:10px; border-radius:{radius_round}; background:{trig_color};",
                                ),
                            }
                            div {
                                style: format!(
                                    "{style_value} font-size:11px; color:{vel_color};",
                                    vel_color = if velocity > 0.8 { signal_warn }
                                    else if velocity > 0.01 { signal_safe }
                                    else { text_dim },
                                ),
                                "Vel: {vel_text}"
                            }
                        }
                    }

                    // Detection controls
                    div {
                        style: format!(
                            "display:flex; flex-direction:column; align-items:center; \
                             gap:{spacing_label};",
                        ),
                        div {
                            style: format!("{style_label}"),
                            "DETECTION"
                        }
                        div {
                            style: "display:flex; gap:6px; flex-wrap:wrap; justify-content:center;",
                            Knob { param_ptr: params.sensitivity.as_ptr(), size: KnobSize::Small }
                            Knob { param_ptr: params.reactivity.as_ptr(), size: KnobSize::Small }
                        }
                        ParamSlider { param_ptr: params.detect_mode.as_ptr() }
                    }

                    // Sidechain
                    div {
                        style: format!(
                            "display:flex; flex-direction:column; align-items:center; \
                             gap:{spacing_label};",
                        ),
                        div {
                            style: format!("{style_label}"),
                            "SIDECHAIN"
                        }
                        div {
                            style: "display:flex; gap:6px;",
                            Knob { param_ptr: params.sc_hpf.as_ptr(), size: KnobSize::Small }
                            Knob { param_ptr: params.sc_lpf.as_ptr(), size: KnobSize::Small }
                        }
                        ParamSlider { param_ptr: params.sc_listen.as_ptr() }
                    }
                }

                Divider {}

                // ── Center: Large Waveform ─────────────────────────
                div {
                    style: format!(
                        "flex:1; display:flex; flex-direction:column; gap:{spacing_label}; min-width:0;",
                    ),
                    TriggerWaveform {
                        levels: waveform_in,
                        triggers: waveform_trig,
                        threshold_db: threshold_db,
                        threshold_ptr: params.threshold.as_ptr(),
                        width: 700.0,
                        height: 210.0,
                    }
                    // Controls below waveform
                    div {
                        style: format!(
                            "display:flex; gap:{spacing_control}; align-items:center; justify-content:center;",
                        ),

                        // Algorithm
                        div {
                            style: "display:flex; gap:6px; align-items:center;",
                            div {
                                style: format!("{style_label}"),
                                "ALGORITHM"
                            }
                            ParamSlider { param_ptr: params.detect_algorithm.as_ptr() }
                        }

                        Divider {}

                        // TODO: MIDI output controls (params not yet added)
                        // div {
                        //     style: "display:flex; gap:6px; align-items:center;",
                        //     div { style: format!("{style_label}"), "MIDI" }
                        //     ParamSlider { param_ptr: params.midi_enabled.as_ptr() }
                        //     Knob { param_ptr: params.midi_channel.as_ptr(), size: KnobSize::Small }
                        //     Knob { param_ptr: params.midi_note_length.as_ptr(), size: KnobSize::Small }
                        // }
                    }
                }

                Divider {}

                // ── Right Panel: Sensitivity/Retrigger + Output ────
                div {
                    style: "display:flex; flex-direction:column; gap:8px; \
                            min-width:140px; justify-content:space-between;",

                    // Sensitivity / Retrigger / Detail
                    div {
                        style: "display:flex; flex-direction:column; align-items:center; gap:6px;",
                        Knob { param_ptr: params.retrigger.as_ptr(), size: KnobSize::Medium }
                        div {
                            style: "display:flex; gap:6px;",
                            Knob { param_ptr: params.release_time.as_ptr(), size: KnobSize::Small }
                            Knob { param_ptr: params.release_ratio.as_ptr(), size: KnobSize::Small }
                        }
                    }

                    // Velocity
                    div {
                        style: format!(
                            "display:flex; flex-direction:column; align-items:center; \
                             gap:{spacing_label};",
                        ),
                        div {
                            style: format!("{style_label}"),
                            "VELOCITY"
                        }
                        Knob { param_ptr: params.dynamics.as_ptr(), size: KnobSize::Medium }
                        ParamSlider { param_ptr: params.vel_curve.as_ptr() }
                    }

                    // Output
                    div {
                        style: format!(
                            "display:flex; flex-direction:column; align-items:center; \
                             gap:{spacing_label};",
                        ),
                        div {
                            style: format!("{style_label}"),
                            "OUTPUT"
                        }
                        Knob { param_ptr: params.output_gain.as_ptr(), size: KnobSize::Medium }
                        div {
                            style: format!(
                                "display:flex; gap:{spacing_label};",
                            ),
                            ParamSlider { param_ptr: params.mix_mode.as_ptr() }
                            Knob { param_ptr: params.mix_amount.as_ptr(), size: KnobSize::Small }
                        }
                    }
                }

                Divider {}

                // ── Far Right: Level Meters ────────────────────────
                div {
                    style: "display:flex; gap:6px; align-items:stretch;",
                    LevelMeterDb { level_db: input_db, label: "IN".to_string() }
                    LevelMeterDb { level_db: output_db, label: "OUT".to_string() }
                }
            }

            // ══════════════════════════════════════════════════════════
            // BOTTOM: Mixer Section (8 strips)
            // ══════════════════════════════════════════════════════════
            div {
                style: format!(
                    "{style_card} display:flex; gap:3px; flex:1; min-height:0; \
                     padding:6px;",
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
                        // TODO: midi_note_ptr (param not yet added)
                        on_load: {
                            let ui = ui_for_load.clone();
                            move |slot: usize| {
                                open_file_dialog(slot, ui.clone());
                            }
                        },
                    }
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
