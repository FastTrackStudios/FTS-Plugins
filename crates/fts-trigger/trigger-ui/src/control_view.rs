//! Mixer strip and mixer section components for the trigger plugin.

use audio_gui::controls::knob::Knob;
use audio_gui::controls::slider::ParamSlider;
use audio_gui::prelude::{theme, KnobSize};
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
    midi_note_ptr: ParamPtr,
    on_load: EventHandler<usize>,
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
                "{INSET} display:flex; flex-direction:column; align-items:center; gap:3px; \
                 padding:5px 3px; min-width:110px; flex:1;",
                INSET = theme::STYLE_INSET,
            ),

            // Slot number + LED
            div {
                style: format!(
                    "display:flex; align-items:center; gap:{LABEL_GAP};",
                    LABEL_GAP = theme::SPACING_LABEL,
                ),
                div {
                    style: format!(
                        "width:8px; height:8px; border-radius:{ROUND}; background:{};",
                        led_color,
                        ROUND = theme::RADIUS_ROUND,
                    ),
                }
                div {
                    style: format!(
                        "font-size:11px; font-weight:700; color:{};",
                        theme::TEXT_BRIGHT,
                    ),
                    "{slot_num}"
                }
            }

            // Sample name
            div {
                style: format!(
                    "font-size:{TINY}; color:{}; text-align:center; \
                     overflow:hidden; text-overflow:ellipsis; white-space:nowrap; \
                     max-width:100px;",
                    if name.is_empty() { theme::TEXT_DIM } else { theme::TEXT },
                    TINY = theme::FONT_SIZE_TINY,
                ),
                title: "{name}",
                "{display_name}"
            }

            // Load button
            div {
                style: format!(
                    "padding:2px 8px; border-radius:{RADIUS}; cursor:pointer; \
                     font-size:{TINY}; font-weight:600; text-transform:uppercase; \
                     letter-spacing:0.3px; \
                     background:{SURFACE}; color:{DIM}; \
                     border:1px solid {BORDER};",
                    RADIUS = theme::RADIUS_SMALL, TINY = theme::FONT_SIZE_TINY,
                    SURFACE = theme::SURFACE, DIM = theme::TEXT_DIM,
                    BORDER = theme::BORDER,
                ),
                onclick: move |_| on_load.call(slot),
                "Load"
            }

            // Gain fader
            div {
                style: format!(
                    "flex:1; display:flex; flex-direction:column; align-items:center; \
                     gap:{TIGHT}; min-height:60px; width:100%;",
                    TIGHT = theme::SPACING_TIGHT,
                ),
                ParamSlider { param_ptr: gain_ptr, height: 60.0 }
            }

            // Peak readout
            div {
                style: format!(
                    "{VALUE} font-size:{TINY}; color:{};",
                    if peak_db > -6.0 { theme::SIGNAL_WARN }
                    else if peak_db > -60.0 { theme::TEXT_DIM }
                    else { theme::TOGGLE_OFF },
                    VALUE = theme::STYLE_VALUE, TINY = theme::FONT_SIZE_TINY,
                ),
                "{peak_text} dB"
            }

            // Pan + Pitch row
            div {
                style: format!(
                    "display:flex; gap:{LABEL_GAP};",
                    LABEL_GAP = theme::SPACING_LABEL,
                ),
                Knob { param_ptr: pan_ptr, size: KnobSize::Small }
                Knob { param_ptr: pitch_ptr, size: KnobSize::Small }
            }

            // MIDI note knob
            Knob { param_ptr: midi_note_ptr, size: KnobSize::Small }

            // Mute / Solo row
            div {
                style: format!(
                    "display:flex; gap:{LABEL_GAP};",
                    LABEL_GAP = theme::SPACING_LABEL,
                ),
                ParamSlider { param_ptr: mute_ptr, height: 20.0 }
                ParamSlider { param_ptr: solo_ptr, height: 20.0 }
            }

            // Enable toggle
            ParamSlider { param_ptr: enabled_ptr, height: 18.0 }
        }
    }
}
