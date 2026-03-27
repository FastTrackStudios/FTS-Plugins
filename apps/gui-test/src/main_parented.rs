//! Parented GUI test — simulates a DAW hosting a plugin editor.
//!
//! Creates an X11 window (the "DAW"), then spawns a Dioxus plugin editor
//! as a child window inside it using `open_parented` — the exact code path
//! that real DAWs use. Tests native wgpu surface rendering in the parented
//! scenario, including under XWayland.
//!
//! Run with:
//!   cargo run -p gui-test --bin gui-test-parented

#[cfg(target_os = "linux")]
fn main() {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;
    use x11rb::protocol::Event;
    use x11rb::wrapper::ConnectionExt as _;
    use x11rb::COPY_DEPTH_FROM_PARENT;

    let plugin_width: u32 = 700;
    let plugin_height: u32 = 500;
    // Parent window is slightly larger to act as a "DAW frame"
    let parent_width: u16 = plugin_width as u16 + 40;
    let parent_height: u16 = plugin_height as u16 + 60;

    eprintln!("=== FTS Parented GUI Test ===");
    eprintln!("Simulating DAW → plugin editor embedding via X11 reparenting");
    eprintln!("This tests the exact code path used by real DAW hosts.");
    eprintln!();

    // Connect to X11 (will go through XWayland if running on Wayland)
    let (conn, screen_num) = x11rb::connect(None).expect("Failed to connect to X11 server");
    let screen = &conn.setup().roots[screen_num];

    eprintln!(
        "Connected to X11 display (screen {}, root window 0x{:x})",
        screen_num, screen.root
    );

    // Create the parent window (simulating the DAW's plugin host window)
    let parent_win = conn.generate_id().unwrap();
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        parent_win,
        screen.root,
        100,
        100,
        parent_width,
        parent_height,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .background_pixel(0x1a1a2e) // Dark background
            .event_mask(
                EventMask::EXPOSURE
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::KEY_PRESS
                    | EventMask::BUTTON_PRESS,
            ),
    )
    .unwrap();

    // Set window title
    conn.change_property8(
        PropMode::REPLACE,
        parent_win,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"FTS DAW Simulator (Parent Window)",
    )
    .unwrap();

    // Map (show) the parent window
    conn.map_window(parent_win).unwrap();
    conn.flush().unwrap();

    eprintln!("Created parent window 0x{:x} ({}x{})", parent_win, parent_width, parent_height);
    eprintln!("Spawning plugin editor as child window...");
    eprintln!();

    // Spawn the plugin editor as a child — this is what the DAW does.
    // The X11 window ID is passed to baseview::Window::open_parented().
    let _handle = nih_plug_dioxus::open_parented_x11(App, parent_win, plugin_width, plugin_height);

    eprintln!("Plugin editor spawned! Close the window or press Escape to exit.");
    eprintln!();

    // Run the X11 event loop for the parent window
    loop {
        let event = conn.wait_for_event().unwrap();
        match event {
            Event::KeyPress(e) => {
                // Escape key = keycode 9 on most X11 setups
                if e.detail == 9 {
                    eprintln!("Escape pressed, exiting.");
                    break;
                }
            }
            Event::DestroyNotify(_) => {
                eprintln!("Window destroyed, exiting.");
                break;
            }
            Event::Expose(_) => {
                // Draw a simple label in the parent window area outside the plugin
                let gc = conn.generate_id().unwrap();
                conn.create_gc(
                    gc,
                    parent_win,
                    &CreateGCAux::new()
                        .foreground(0x888888)
                        .background(0x1a1a2e),
                )
                .unwrap();
                let text = b"DAW Parent Window (plugin editor is embedded below)";
                conn.image_text8(parent_win, gc, 10, 15, text).unwrap();
                conn.free_gc(gc).unwrap();
                conn.flush().unwrap();
            }
            _ => {}
        }
    }
}

use nih_plug_dioxus::dioxus_native::prelude::*;
use nih_plug_dioxus::TAILWIND_CSS;

#[component]
fn App() -> Element {
    let mut count = use_signal(|| 0);

    rsx! {
        document::Style { {TAILWIND_CSS} }
        div {
            class: "dark",
            style: "width: 100%; height: 100%; background: #1a1a2e; display: flex; flex-direction: column; align-items: center; justify-content: center; font-family: sans-serif; color: white;",

            div {
                style: "font-size: 24px; font-weight: bold; margin-bottom: 8px; color: #e94560;",
                "Parented wgpu Surface Test"
            }

            div {
                style: "font-size: 14px; color: #888; margin-bottom: 24px;",
                "Embedded in X11 parent window (simulating DAW host)"
            }

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

            button {
                style: "padding: 12px 24px; border-radius: 8px; border: none; background: #e94560; color: white; font-size: 16px; cursor: pointer;",
                onclick: move |_| count += 1,
                "Click me: {count}"
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This test is Linux-only (requires X11).");
    std::process::exit(1);
}
