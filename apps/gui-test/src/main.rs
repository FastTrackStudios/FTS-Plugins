//! Standalone GUI test — native wgpu surface rendering path.
//!
//! Opens a baseview window and renders a Dioxus component using direct
//! wgpu surface presentation (no softbuffer CPU readback).
//!
//! Run with:
//!   nix develop --command cargo run -p gui-test

use nih_plug_dioxus::dioxus_native::prelude::*;
use nih_plug_dioxus::open_standalone;
use nih_plug_dioxus::TAILWIND_CSS;

/// Test component that renders a simple UI to verify native rendering works.
#[component]
fn App() -> Element {
    let mut count = use_signal(|| 0);
    let mut hovered = use_signal(|| false);

    rsx! {
        document::Style { {TAILWIND_CSS} }
        div {
            class: "dark",
            style: "width: 100%; height: 100%; background: #1a1a2e; display: flex; flex-direction: column; align-items: center; justify-content: center; font-family: sans-serif; color: white;",

            // Title
            div {
                style: "font-size: 24px; font-weight: bold; margin-bottom: 8px; color: #e94560;",
                "FTS GUI Test — Native wgpu Surface"
            }

            // Subtitle showing rendering mode
            div {
                style: "font-size: 14px; color: #888; margin-bottom: 32px;",
                "Vello → wgpu surface → present (zero CPU readback)"
            }

            // Color gradient test — verifies GPU rendering is working
            div {
                style: "display: flex; gap: 4px; margin-bottom: 24px;",
                for i in 0..8 {
                    {
                        let hue = i * 45;
                        rsx! {
                            div {
                                style: "width: 48px; height: 48px; border-radius: 8px; background: hsl({hue}, 70%, 50%);",
                            }
                        }
                    }
                }
            }

            // Interactive button — verifies event handling works
            div {
                style: "display: flex; gap: 16px; align-items: center;",

                button {
                    style: "padding: 12px 24px; border-radius: 8px; border: none; background: #e94560; color: white; font-size: 16px; cursor: pointer;",
                    onclick: move |_| count += 1,
                    "Click me: {count}"
                }

                button {
                    style: "padding: 12px 24px; border-radius: 8px; border: none; background: #533483; color: white; font-size: 16px; cursor: pointer;",
                    onclick: move |_| count.set(0),
                    "Reset"
                }
            }

            // Hover test
            {
                let border = if *hovered.read() { "#e94560" } else { "#333" };
                rsx! {
                    div {
                        style: "margin-top: 24px; padding: 16px 32px; border-radius: 8px; border: 2px solid {border}; transition: all 0.2s;",
                        onmouseenter: move |_| hovered.set(true),
                        onmouseleave: move |_| hovered.set(false),
                        if *hovered.read() {
                            "Mouse is over this element!"
                        } else {
                            "Hover over me to test mouse events"
                        }
                    }
                }
            }

            // Nested layout test
            div {
                style: "margin-top: 24px; display: flex; gap: 8px;",
                for i in 0..3 {
                    div {
                        style: "padding: 16px; border-radius: 8px; background: #16213e; border: 1px solid #0f3460;",
                        div {
                            style: "font-size: 12px; color: #888; margin-bottom: 4px;",
                            "Panel {i}"
                        }
                        div {
                            style: "font-size: 20px; font-weight: bold;",
                            "{(i + 1) * 42}"
                        }
                    }
                }
            }
        }
    }
}

fn main() {
    eprintln!("Starting FTS GUI Test — Native wgpu Surface Path");
    eprintln!("If this window renders correctly, native rendering works on your system.");
    open_standalone(App, 700, 500);
}
