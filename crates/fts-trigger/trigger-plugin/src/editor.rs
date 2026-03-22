//! FTS Trigger — Dioxus GUI editor (Trigger 2-inspired layout).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use audio_gui::controls::knob::Knob;
use audio_gui::controls::slider::ParamSlider;
use audio_gui::prelude::{theme, Divider, DragProvider, KnobSize, LevelMeterDb};
use fts_plugin_core::prelude::*;

use crate::engine::NUM_SLOTS;
use crate::loader;
use crate::{TriggerUiState, WAVEFORM_LEN};
use trigger_ui::control_view::MixerStrip;
use trigger_ui::trigger_waveform::TriggerWaveform;

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
                "width:100vw; height:100vh; padding:{SPACING}; \
                 background:{BG}; color:{TEXT}; \
                 font-family:system-ui,sans-serif; font-size:13px; user-select:none; \
                 display:flex; flex-direction:column; gap:{GAP}; overflow:hidden;",
                SPACING = theme::SPACING_ROOT, BG = theme::BG, TEXT = theme::TEXT,
                GAP = theme::SPACING_SECTION,
            ),

            // ══════════════════════════════════════════════════════════
            // TOP ROW: Controls | Waveform | Controls | Output
            // ══════════════════════════════════════════════════════════
            div {
                style: format!(
                    "{CARD} display:flex; gap:8px; align-items:stretch; \
                     padding:{PADDING};",
                    CARD = theme::STYLE_CARD, PADDING = theme::SPACING_CARD,
                ),

                // ── Left Panel: Title + Detection + Sidechain ──────
                div {
                    style: "display:flex; flex-direction:column; gap:8px; \
                            min-width:140px; justify-content:space-between;",

                    // Title + trigger indicator
                    div {
                        style: format!(
                            "display:flex; flex-direction:column; gap:{LABEL_GAP};",
                            LABEL_GAP = theme::SPACING_LABEL,
                        ),
                        div {
                            style: format!(
                                "font-size:{SIZE}; font-weight:700; \
                                 letter-spacing:0.5px; color:{BRIGHT};",
                                SIZE = theme::FONT_SIZE_TITLE, BRIGHT = theme::TEXT_BRIGHT,
                            ),
                            "FTS TRIGGER"
                        }
                        div {
                            style: "display:flex; align-items:center; gap:6px;",
                            div {
                                style: format!(
                                    "width:10px; height:10px; border-radius:{ROUND}; background:{};",
                                    trig_color,
                                    ROUND = theme::RADIUS_ROUND,
                                ),
                            }
                            div {
                                style: format!(
                                    "{VALUE} font-size:11px; color:{};",
                                    if velocity > 0.8 { theme::SIGNAL_WARN }
                                    else if velocity > 0.01 { theme::SIGNAL_SAFE }
                                    else { theme::TEXT_DIM },
                                    VALUE = theme::STYLE_VALUE,
                                ),
                                "Vel: {vel_text}"
                            }
                        }
                    }

                    // Detection controls
                    div {
                        style: format!(
                            "display:flex; flex-direction:column; align-items:center; \
                             gap:{LABEL_GAP};",
                            LABEL_GAP = theme::SPACING_LABEL,
                        ),
                        div {
                            style: format!(
                                "{LABEL}",
                                LABEL = theme::STYLE_LABEL,
                            ),
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
                             gap:{LABEL_GAP};",
                            LABEL_GAP = theme::SPACING_LABEL,
                        ),
                        div {
                            style: format!(
                                "{LABEL}",
                                LABEL = theme::STYLE_LABEL,
                            ),
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
                        "flex:1; display:flex; flex-direction:column; gap:{LABEL_GAP}; min-width:0;",
                        LABEL_GAP = theme::SPACING_LABEL,
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
                            "display:flex; gap:{GAP}; align-items:center; justify-content:center;",
                            GAP = theme::SPACING_CONTROL,
                        ),

                        // Algorithm
                        div {
                            style: "display:flex; gap:6px; align-items:center;",
                            div {
                                style: format!(
                                    "{LABEL}",
                                    LABEL = theme::STYLE_LABEL,
                                ),
                                "ALGORITHM"
                            }
                            ParamSlider { param_ptr: params.detect_algorithm.as_ptr() }
                        }

                        Divider {}

                        // MIDI output controls
                        div {
                            style: "display:flex; gap:6px; align-items:center;",
                            div {
                                style: format!(
                                    "{LABEL}",
                                    LABEL = theme::STYLE_LABEL,
                                ),
                                "MIDI"
                            }
                            ParamSlider { param_ptr: params.midi_enabled.as_ptr() }
                            Knob { param_ptr: params.midi_channel.as_ptr(), size: KnobSize::Small }
                            Knob { param_ptr: params.midi_note_length.as_ptr(), size: KnobSize::Small }
                        }
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
                             gap:{LABEL_GAP};",
                            LABEL_GAP = theme::SPACING_LABEL,
                        ),
                        div {
                            style: format!(
                                "{LABEL}",
                                LABEL = theme::STYLE_LABEL,
                            ),
                            "VELOCITY"
                        }
                        Knob { param_ptr: params.dynamics.as_ptr(), size: KnobSize::Medium }
                        ParamSlider { param_ptr: params.vel_curve.as_ptr() }
                    }

                    // Output
                    div {
                        style: format!(
                            "display:flex; flex-direction:column; align-items:center; \
                             gap:{LABEL_GAP};",
                            LABEL_GAP = theme::SPACING_LABEL,
                        ),
                        div {
                            style: format!(
                                "{LABEL}",
                                LABEL = theme::STYLE_LABEL,
                            ),
                            "OUTPUT"
                        }
                        Knob { param_ptr: params.output_gain.as_ptr(), size: KnobSize::Medium }
                        div {
                            style: format!(
                                "display:flex; gap:{LABEL_GAP};",
                                LABEL_GAP = theme::SPACING_LABEL,
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
                    "{CARD} display:flex; gap:3px; flex:1; min-height:0; \
                     padding:6px;",
                    CARD = theme::STYLE_CARD,
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
                        midi_note_ptr: params.slots[slot].midi_note.as_ptr(),
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
