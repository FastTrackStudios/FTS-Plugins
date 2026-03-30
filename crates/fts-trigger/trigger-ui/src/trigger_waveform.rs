//! Large waveform display with draggable threshold line and trigger markers.
//!
//! Modeled after Slate Digital Trigger 2's analysis window.

use audio_gui::drag::DragState;
use audio_gui::prelude::use_theme;
use fts_plugin_core::prelude::*;

/// Large trigger analysis waveform with threshold overlay.
///
/// Shows scrolling input peaks as vertical bars, trigger event markers,
/// and a horizontal threshold line that can be dragged to adjust detection.
#[component]
pub fn TriggerWaveform(
    levels: Vec<f32>,
    #[props(default = Vec::new())] triggers: Vec<f32>,
    threshold_db: f32,
    threshold_ptr: ParamPtr,
    #[props(default = 600.0)] width: f32,
    #[props(default = 200.0)] height: f32,
) -> Element {
    let t = use_theme();
    let t = *t.read();
    let mut drag = use_context::<Signal<DragState>>();
    let ctx = use_param_context();

    let num_bars = levels.len().max(1);
    let bar_width = width / num_bars as f32;

    // Convert threshold from dB to linear (0-1) for positioning.
    // threshold_db ranges from -60 to 0, map to 0-1 linear amplitude.
    let threshold_linear = 10.0_f32.powf(threshold_db / 20.0).clamp(0.0, 1.0);
    let threshold_y = height - (threshold_linear * height);

    // dB scale markers
    let db_markers = [
        (0.0_f32, "0"),
        (-6.0, "-6"),
        (-12.0, "-12"),
        (-24.0, "-24"),
        (-48.0, "-48"),
    ];

    rsx! {
        div {
            style: format!(
                "position:relative; width:{width}px; height:{height}px; \
                 background:#080810; border-radius:4px; overflow:hidden; \
                 border:1px solid {}; cursor:ns-resize;",
                t.border,
            ),

            onmousedown: {
                let ctx = ctx.clone();
                move |evt: MouseEvent| {
                    let click_y = evt.element_coordinates().y as f32;
                    // Convert click position to normalized param value.
                    // y=0 is top (max amplitude = 0dB), y=height is bottom (-60dB).
                    // Linear amplitude at click: (height - click_y) / height
                    // But threshold param is normalized linearly over -60..0 dB range,
                    // so we need to convert from linear amplitude to the param's
                    // normalized range. Set the threshold directly to click position.
                    let linear_amp = ((height - click_y) / height).clamp(0.0, 1.0);
                    // Convert linear amplitude to dB, then to normalized param (0 = -60dB, 1 = 0dB)
                    let db = if linear_amp > 0.001 {
                        20.0 * linear_amp.log10()
                    } else {
                        -60.0
                    };
                    let normalized = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
                    ctx.begin_set_raw(threshold_ptr);
                    ctx.set_normalized_raw(threshold_ptr, normalized);
                    // Start drag from this new position
                    drag.set(DragState {
                        active: true,
                        param_ptr: Some(threshold_ptr),
                        start_value: normalized as f64,
                        start_y: evt.client_coordinates().y,
                        sensitivity: height as f64 * 0.8,
                        last_shift: false,
                        move_count: 0,
                    });
                }
            },

            // dB grid lines
            for (db, label) in db_markers.iter().copied() {
                {
                    let lin = 10.0_f32.powf(db / 20.0).clamp(0.0, 1.0);
                    let y = height - (lin * height);
                    let label_y = y - 10.0;
                    rsx! {
                        div {
                            style: format!(
                                "position:absolute; left:0; top:{y}px; width:100%; height:1px; \
                                 background:rgba(255,255,255,0.06); pointer-events:none;"
                            ),
                        }
                        div {
                            style: format!(
                                "position:absolute; right:4px; top:{label_y}px; \
                                 font-size:8px; color:rgba(255,255,255,0.2); \
                                 pointer-events:none;"
                            ),
                            "{label}"
                        }
                    }
                }
            }

            // Waveform bars
            for (i, &level) in levels.iter().enumerate() {
                {
                    let bar_h = (level.clamp(0.0, 1.0) * height).max(0.0);
                    let x = i as f32 * bar_width;
                    let is_trigger = triggers.get(i).copied().unwrap_or(0.0) > 0.5;
                    let color = if is_trigger {
                        "rgba(100,200,255,0.9)"
                    } else {
                        "rgba(60,140,200,0.4)"
                    };
                    rsx! {
                        div {
                            style: format!(
                                "position:absolute; left:{x}px; bottom:0; \
                                 width:{bar_width}px; height:{bar_h}px; \
                                 background:{color}; pointer-events:none;"
                            ),
                        }
                    }
                }
            }

            // Trigger flash overlays (vertical highlight columns)
            for (i, &trig) in triggers.iter().enumerate() {
                if trig > 0.5 {
                    {
                        let x = i as f32 * bar_width;
                        let bw = bar_width.max(2.0);
                        rsx! {
                            div {
                                style: format!(
                                    "position:absolute; left:{x}px; top:0; \
                                     width:{bw}px; height:100%; \
                                     background:rgba(100,200,255,0.08); pointer-events:none;"
                                ),
                            }
                        }
                    }
                }
            }

            // Threshold line
            div {
                style: format!(
                    "position:absolute; left:0; top:{threshold_y}px; width:100%; height:2px; \
                     background:rgba(248,113,113,0.8); pointer-events:none; \
                     box-shadow:0 0 6px rgba(248,113,113,0.4);"
                ),
            }

            // Threshold label
            div {
                style: format!(
                    "position:absolute; left:6px; top:{label_y}px; \
                     font-size:9px; font-weight:600; color:rgba(248,113,113,0.9); \
                     pointer-events:none;",
                    label_y = threshold_y - 14.0,
                ),
                "{threshold_db:.1} dB"
            }
        }
    }
}
