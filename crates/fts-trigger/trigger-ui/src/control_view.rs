//! Mixer strip and mixer section components for the trigger plugin.

use audio_gui::controls::knob::Knob;
use audio_gui::controls::slider::ParamSlider;
use audio_gui::prelude::{use_theme, KnobSize};
use fts_plugin_core::prelude::*;

/// A single mixer channel strip for one sample slot.
#[component]
pub fn MixerStrip(
    slot: usize,
    name: String,
    peak_db: f32,
    playing: bool,
    gain_ptr: ParamPtr,
    pan_ptr: ParamPtr,
    pitch_ptr: ParamPtr,
    enabled_ptr: ParamPtr,
    mute_ptr: ParamPtr,
    solo_ptr: ParamPtr,
    // TODO: midi_note_ptr: ParamPtr, (param not yet added)
    on_load: EventHandler<usize>,
) -> Element {
    let t = use_theme();
    let t = *t.read();
    let slot_num = slot + 1;
    let display_name = if name.is_empty() {
        "---".to_string()
    } else if name.len() > 8 {
        format!("{}...", &name[..6])
    } else {
        name.clone()
    };

    let led_color = if playing { t.signal_safe } else { t.toggle_off };

    let peak_text = if peak_db > -100.0 {
        format!("{:.0}", peak_db)
    } else {
        "-inf".to_string()
    };

    let style_inset = t.style_inset();
    let style_value = t.style_value();

    rsx! {
        div {
            style: format!(
                "{style_inset} display:flex; flex-direction:column; align-items:center; gap:3px; \
                 padding:5px 3px; min-width:110px; flex:1;",
            ),

            // Slot number + LED
            div {
                style: format!(
                    "display:flex; align-items:center; gap:{};",
                    t.spacing_label,
                ),
                div {
                    style: format!(
                        "width:8px; height:8px; border-radius:{}; background:{};",
                        t.radius_round, led_color,
                    ),
                }
                div {
                    style: format!(
                        "font-size:11px; font-weight:700; color:{};",
                        t.text_bright,
                    ),
                    "{slot_num}"
                }
            }

            // Sample name
            div {
                style: format!(
                    "font-size:{}; color:{}; text-align:center; \
                     overflow:hidden; text-overflow:ellipsis; white-space:nowrap; \
                     max-width:100px;",
                    t.font_size_tiny,
                    if name.is_empty() { t.text_dim } else { t.text },
                ),
                title: "{name}",
                "{display_name}"
            }

            // Load button
            div {
                style: format!(
                    "padding:2px 8px; border-radius:{}; cursor:pointer; \
                     font-size:{}; font-weight:600; text-transform:uppercase; \
                     letter-spacing:0.3px; \
                     background:{}; color:{}; \
                     border:1px solid {};",
                    t.radius_small, t.font_size_tiny,
                    t.surface, t.text_dim,
                    t.border,
                ),
                onclick: move |_| on_load.call(slot),
                "Load"
            }

            // Gain fader
            div {
                style: format!(
                    "flex:1; display:flex; flex-direction:column; align-items:center; \
                     gap:{}; min-height:60px; width:100%;",
                    t.spacing_tight,
                ),
                ParamSlider { param_ptr: gain_ptr, height: 60.0 }
            }

            // Peak readout
            div {
                style: format!(
                    "{style_value} font-size:{}; color:{};",
                    t.font_size_tiny,
                    if peak_db > -6.0 { t.signal_warn }
                    else if peak_db > -60.0 { t.text_dim }
                    else { t.toggle_off },
                ),
                "{peak_text} dB"
            }

            // Pan + Pitch row
            div {
                style: format!(
                    "display:flex; gap:{};",
                    t.spacing_label,
                ),
                Knob { param_ptr: pan_ptr, size: KnobSize::Small }
                Knob { param_ptr: pitch_ptr, size: KnobSize::Small }
            }

            // TODO: MIDI note knob (param not yet added)
            // Knob { param_ptr: midi_note_ptr, size: KnobSize::Small }

            // Mute / Solo row
            div {
                style: format!(
                    "display:flex; gap:{};",
                    t.spacing_label,
                ),
                ParamSlider { param_ptr: mute_ptr, height: 20.0 }
                ParamSlider { param_ptr: solo_ptr, height: 20.0 }
            }

            // Enable toggle
            ParamSlider { param_ptr: enabled_ptr, height: 18.0 }
        }
    }
}
