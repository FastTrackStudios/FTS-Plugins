//! Mixer strip and mixer section components for the trigger plugin.

use audio_gui::controls::knob::Knob;
use audio_gui::controls::slider::ParamSlider;
use audio_gui::prelude::{theme, KnobSize};
use fts_plugin_core::prelude::*;

/// A single mixer channel strip for one sample slot.
#[component]
pub fn MixerStrip(
    /// Slot index (0-7).
    slot: usize,
    /// Display name of loaded sample.
    name: String,
    /// Per-slot peak level in dB.
    peak_db: f32,
    /// Whether this slot is currently playing.
    playing: bool,
    /// Param pointers for this slot's controls.
    gain_ptr: ParamPtr,
    pan_ptr: ParamPtr,
    pitch_ptr: ParamPtr,
    enabled_ptr: ParamPtr,
    mute_ptr: ParamPtr,
    solo_ptr: ParamPtr,
) -> Element {
    let slot_num = slot + 1;
    let display_name = if name.is_empty() {
        "---".to_string()
    } else if name.len() > 8 {
        format!("{}...", &name[..6])
    } else {
        name.clone()
    };

    let led_color = if playing {
        theme::SIGNAL_SAFE
    } else {
        theme::TOGGLE_OFF
    };

    let peak_text = if peak_db > -100.0 {
        format!("{:.0}", peak_db)
    } else {
        "-inf".to_string()
    };

    rsx! {
        div {
            style: format!(
                "display:flex; flex-direction:column; align-items:center; gap:4px; \
                 padding:6px 4px; background:{SURFACE}; border-radius:4px; \
                 min-width:110px; flex:1;",
                SURFACE = theme::SURFACE,
            ),

            // Slot number + LED
            div {
                style: "display:flex; align-items:center; gap:4px;",
                div {
                    style: format!(
                        "width:8px; height:8px; border-radius:50%; background:{};",
                        led_color,
                    ),
                }
                div {
                    style: format!(
                        "font-size:11px; font-weight:700; color:{};",
                        theme::TEXT,
                    ),
                    "{slot_num}"
                }
            }

            // Sample name
            div {
                style: format!(
                    "font-size:9px; color:{}; text-align:center; \
                     overflow:hidden; text-overflow:ellipsis; white-space:nowrap; \
                     max-width:100px;",
                    if name.is_empty() { theme::TEXT_DIM } else { theme::TEXT },
                ),
                title: "{name}",
                "{display_name}"
            }

            // Gain fader
            div {
                style: "flex:1; display:flex; flex-direction:column; align-items:center; \
                        gap:2px; min-height:80px; width:100%;",
                ParamSlider { param_ptr: gain_ptr, height: 80.0 }
            }

            // Peak readout
            div {
                style: format!(
                    "font-size:9px; font-variant-numeric:tabular-nums; color:{};",
                    if peak_db > -6.0 { theme::SIGNAL_WARN }
                    else if peak_db > -60.0 { theme::TEXT_DIM }
                    else { theme::TOGGLE_OFF }
                ),
                "{peak_text} dB"
            }

            // Pan knob
            Knob { param_ptr: pan_ptr, size: KnobSize::Small }

            // Pitch knob
            Knob { param_ptr: pitch_ptr, size: KnobSize::Small }

            // Mute / Solo row
            div {
                style: "display:flex; gap:4px;",
                ParamSlider { param_ptr: mute_ptr, height: 20.0 }
                ParamSlider { param_ptr: solo_ptr, height: 20.0 }
            }

            // Enable toggle
            ParamSlider { param_ptr: enabled_ptr, height: 18.0 }
        }
    }
}
