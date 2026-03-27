//! Headless screenshot tool for GUI testing.
//!
//! Renders a Dioxus component offscreen via wgpu and saves as PNG.
//! This lets you visually verify rendering without opening a window.
//!
//! Run with:
//!   cargo run -p gui-test --bin gui-test-screenshot [output.png]

use nih_plug_dioxus::dioxus_native::prelude::*;
use nih_plug_dioxus::TAILWIND_CSS;
use std::path::PathBuf;

fn main() {
    let output_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("screenshot.png"));

    let width = 800;
    let height = 600;

    eprintln!("Rendering {}x{} screenshot...", width, height);

    let pixels = nih_plug_dioxus::render_screenshot(App, width, height, None);

    // Save as PNG
    let file = std::fs::File::create(&output_path).expect("Failed to create output file");
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("Failed to write PNG header");
    writer.write_image_data(&pixels).expect("Failed to write PNG data");

    eprintln!("Saved to {}", output_path.display());
}

/// Test component — exercises various layout and styling patterns.
#[component]
fn App() -> Element {
    rsx! {
        document::Style { {TAILWIND_CSS} }
        div {
            style: "width: 100%; height: 100%; background: #1a1a2e; padding: 16px; font-family: sans-serif; color: white; box-sizing: border-box;",

            // Title bar
            div {
                style: "font-size: 20px; font-weight: bold; color: #e94560; margin-bottom: 16px;",
                "FTS GUI Component Gallery"
            }

            // Row of colored boxes (tests flexbox + border-radius + hsl colors)
            div {
                style: "display: flex; gap: 8px; margin-bottom: 16px;",
                for i in 0..8 {
                    {
                        let hue = i * 45;
                        rsx! {
                            div {
                                style: "width: 48px; height: 48px; border-radius: 8px; background: hsl({hue}, 70%, 50%); display: flex; align-items: center; justify-content: center; font-size: 10px; color: white;",
                                "{hue}"
                            }
                        }
                    }
                }
            }

            // Layout test: two columns
            div {
                style: "display: flex; gap: 16px; margin-bottom: 16px;",

                // Left column — simulated knob area
                div {
                    style: "flex: 1; background: #16213e; border-radius: 8px; padding: 12px;",

                    div {
                        style: "font-size: 12px; color: #888; margin-bottom: 8px; text-transform: uppercase; letter-spacing: 1px;",
                        "Controls"
                    }

                    // Simulated knobs (circles with labels)
                    div {
                        style: "display: flex; gap: 16px; flex-wrap: wrap;",
                        for label in ["Gain", "Freq", "Q", "Mix"] {
                            div {
                                style: "display: flex; flex-direction: column; align-items: center; gap: 4px;",
                                div {
                                    style: "width: 48px; height: 48px; border-radius: 50%; background: #0f3460; border: 2px solid #e94560; display: flex; align-items: center; justify-content: center; font-size: 11px;",
                                    "0.5"
                                }
                                div {
                                    style: "font-size: 10px; color: #888;",
                                    "{label}"
                                }
                            }
                        }
                    }
                }

                // Right column — simulated dropdown + text
                div {
                    style: "flex: 1; background: #16213e; border-radius: 8px; padding: 12px;",

                    div {
                        style: "font-size: 12px; color: #888; margin-bottom: 8px; text-transform: uppercase; letter-spacing: 1px;",
                        "Settings"
                    }

                    // Simulated dropdown
                    div {
                        style: "background: #0f3460; border: 1px solid #333; border-radius: 4px; padding: 8px 12px; margin-bottom: 8px; font-size: 13px; display: flex; justify-content: space-between;",
                        span { "Algorithm: Clean" }
                        span { style: "color: #888;", "▼" }
                    }

                    // Simulated toggle
                    div {
                        style: "display: flex; align-items: center; gap: 8px; margin-bottom: 8px;",
                        div {
                            style: "width: 36px; height: 20px; border-radius: 10px; background: #e94560; position: relative;",
                            div {
                                style: "width: 16px; height: 16px; border-radius: 50%; background: white; position: absolute; top: 2px; right: 2px;",
                            }
                        }
                        span { style: "font-size: 13px;", "Enabled" }
                    }

                    // Simulated slider
                    div {
                        style: "margin-bottom: 4px;",
                        div {
                            style: "font-size: 11px; color: #888; margin-bottom: 4px;",
                            "Dry/Wet"
                        }
                        div {
                            style: "height: 6px; background: #0f3460; border-radius: 3px; position: relative;",
                            div {
                                style: "width: 65%; height: 100%; background: #e94560; border-radius: 3px;",
                            }
                        }
                    }
                }
            }

            // Bottom panel — simulated meter / waveform area
            div {
                style: "background: #16213e; border-radius: 8px; padding: 12px;",

                div {
                    style: "font-size: 12px; color: #888; margin-bottom: 8px; text-transform: uppercase; letter-spacing: 1px;",
                    "Output"
                }

                // Simulated level meters
                div {
                    style: "display: flex; gap: 4px; height: 24px;",
                    for level in [0.8, 0.6, 0.9, 0.4, 0.7, 0.5, 0.85, 0.3, 0.65, 0.55, 0.75, 0.45, 0.6, 0.8, 0.5, 0.7] {
                        {
                            let pct = (level * 100.0) as u32;
                            let color = if level > 0.85 { "#ff4444" } else if level > 0.7 { "#ffaa00" } else { "#44ff44" };
                            rsx! {
                                div {
                                    style: "flex: 1; background: #0f3460; border-radius: 2px; display: flex; align-items: flex-end;",
                                    div {
                                        style: "width: 100%; height: {pct}%; background: {color}; border-radius: 2px;",
                                    }
                                }
                            }
                        }
                    }
                }

                // Labels
                div {
                    style: "display: flex; justify-content: space-between; margin-top: 4px;",
                    span { style: "font-size: 10px; color: #888;", "L" }
                    span { style: "font-size: 10px; color: #888;", "R" }
                }
            }
        }
    }
}
