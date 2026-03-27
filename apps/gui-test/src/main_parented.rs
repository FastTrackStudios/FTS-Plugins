//! Parented GUI test — simulates a DAW hosting plugin editors.
//!
//! Creates X11 windows (the "DAW"), then spawns Dioxus plugin editors
//! as child windows inside them using `open_parented` — the exact code path
//! that real DAWs use. Tests native wgpu surface rendering in the parented
//! scenario, including under XWayland.
//!
//! Run with:
//!   cargo run -p gui-test --bin gui-test-parented [COUNT]
//!
//! Examples:
//!   cargo run -p gui-test --bin gui-test-parented       # 1 instance
//!   cargo run -p gui-test --bin gui-test-parented 10    # 10 instances
//!   cargo run -p gui-test --bin gui-test-parented 20    # 20 instances (stress test)

#[cfg(target_os = "linux")]
fn main() {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;
    use x11rb::protocol::Event;
    use x11rb::wrapper::ConnectionExt as _;
    use x11rb::COPY_DEPTH_FROM_PARENT;

    let instance_count: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let plugin_width: u32 = 400;
    let plugin_height: u32 = 300;

    eprintln!("=== FTS Parented GUI Stress Test ===");
    eprintln!(
        "Spawning {} plugin editor instance(s) with native wgpu surfaces",
        instance_count
    );
    eprintln!("This simulates a DAW with multiple plugin windows open.");
    eprintln!();

    // Connect to X11 (will go through XWayland if running on Wayland)
    let (conn, screen_num) = x11rb::connect(None).expect("Failed to connect to X11 server");
    let screen = &conn.setup().roots[screen_num];
    let screen_width = screen.width_in_pixels;
    let screen_height = screen.height_in_pixels;

    eprintln!(
        "Connected to X11 display (screen {}, {}x{})",
        screen_num, screen_width, screen_height
    );

    // Tile windows in a grid
    let cols = (instance_count as f32).sqrt().ceil() as usize;
    let rows = (instance_count + cols - 1) / cols;
    let win_width = (plugin_width + 20) as u16;
    let win_height = (plugin_height + 40) as u16;

    let mut parent_windows = Vec::new();
    let mut _handles = Vec::new(); // Keep handles alive

    for i in 0..instance_count {
        let col = i % cols;
        let row = i / cols;
        let x = (col as i16 * win_width as i16 + 20).min(screen_width as i16 - win_width as i16);
        let y = (row as i16 * win_height as i16 + 20).min(screen_height as i16 - win_height as i16);

        // Create parent window (simulating DAW plugin host window)
        let parent_win = conn.generate_id().unwrap();
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            parent_win,
            screen.root,
            x,
            y,
            win_width,
            win_height,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .background_pixel(0x1a1a2e)
                .event_mask(
                    EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY | EventMask::KEY_PRESS,
                ),
        )
        .unwrap();

        let title = format!("FTS Plugin #{} (native wgpu)", i + 1);
        conn.change_property8(
            PropMode::REPLACE,
            parent_win,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            title.as_bytes(),
        )
        .unwrap();

        conn.map_window(parent_win).unwrap();
        parent_windows.push(parent_win);
    }
    conn.flush().unwrap();

    // Give X11 a moment to map all windows before spawning plugin editors
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Spawn plugin editors inside each parent window
    for (i, &parent_win) in parent_windows.iter().enumerate() {
        let handle =
            nih_plug_dioxus::open_parented_x11(App, parent_win, plugin_width, plugin_height);
        _handles.push(handle);
        eprintln!("  Spawned editor #{} in parent 0x{:x}", i + 1, parent_win);
    }

    eprintln!();
    eprintln!(
        "All {} editors spawned! Press Escape in any window to exit.",
        instance_count
    );
    eprintln!("Watch system monitor for GPU/CPU usage.");
    eprintln!();

    // Run X11 event loop
    loop {
        let event = conn.wait_for_event().unwrap();
        match event {
            Event::KeyPress(e) => {
                if e.detail == 9 {
                    eprintln!("Escape pressed, exiting.");
                    break;
                }
            }
            Event::DestroyNotify(_) => {
                eprintln!("Window destroyed, exiting.");
                break;
            }
            Event::Expose(e) => {
                let win = e.window;
                if let Some(idx) = parent_windows.iter().position(|&w| w == win) {
                    let gc = conn.generate_id().unwrap();
                    conn.create_gc(
                        gc,
                        win,
                        &CreateGCAux::new().foreground(0x888888).background(0x1a1a2e),
                    )
                    .unwrap();
                    let label = format!("Plugin #{} (native wgpu surface)", idx + 1);
                    conn.image_text8(win, gc, 10, 15, label.as_bytes()).unwrap();
                    conn.free_gc(gc).unwrap();
                    conn.flush().unwrap();
                }
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
                style: "font-size: 20px; font-weight: bold; margin-bottom: 8px; color: #e94560;",
                "Native wgpu Surface"
            }

            div {
                style: "font-size: 12px; color: #888; margin-bottom: 16px;",
                "Parented X11 — zero CPU readback"
            }

            div {
                style: "display: flex; gap: 3px; margin-bottom: 16px;",
                for i in 0..8 {
                    {
                        let hue = i * 45;
                        rsx! {
                            div {
                                style: "width: 32px; height: 32px; border-radius: 6px; background: hsl({hue}, 70%, 50%);",
                            }
                        }
                    }
                }
            }

            button {
                style: "padding: 8px 16px; border-radius: 6px; border: none; background: #e94560; color: white; font-size: 14px; cursor: pointer;",
                onclick: move |_| count += 1,
                "Clicks: {count}"
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This test is Linux-only (requires X11).");
    std::process::exit(1);
}
