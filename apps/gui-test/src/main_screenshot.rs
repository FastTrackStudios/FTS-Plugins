//! Headless screenshot tool for plugin GUI testing.
//!
//! Renders actual plugin editor GUIs offscreen via wgpu and saves as PNG.
//! This lets you visually verify rendering without a DAW.
//!
//! Run with:
//!   cargo run -p gui-test --bin gui-test-screenshot [plugin] [output.png]
//!
//! Examples:
//!   cargo run -p gui-test --bin gui-test-screenshot comp /tmp/comp.png
//!
//! To screenshot other plugins, uncomment the corresponding dependency in
//! Cargo.toml (only one plugin at a time to avoid duplicate CLAP/VST3 symbols).

use nih_plug_dioxus::SharedState;
use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let plugin_name = args.get(1).map(|s| s.as_str()).unwrap_or("comp");
    let output_path = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/fts-{}.png", plugin_name)));

    match plugin_name {
        "comp" => screenshot_comp(&output_path),
        // Uncomment in Cargo.toml to enable:
        // "delay" => screenshot_delay(&output_path),
        // "eq" => screenshot_eq(&output_path),
        _ => {
            eprintln!("Unknown plugin: {}", plugin_name);
            eprintln!("Available: comp");
            eprintln!("(Uncomment delay-plugin or eq-plugin in Cargo.toml to enable others)");
            std::process::exit(1);
        }
    }
}

fn save_png(pixels: &[u8], width: u32, height: u32, path: &std::path::Path) {
    let file = std::fs::File::create(path).expect("Failed to create output file");
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("Failed to write PNG header");
    writer
        .write_image_data(pixels)
        .expect("Failed to write PNG data");
    eprintln!("Saved to {}", path.display());
}

fn screenshot_comp(output: &std::path::Path) {
    use comp_plugin::{CompUiState, FtsCompParams, WAVEFORM_LEN};
    use std::sync::atomic::Ordering;

    let width = 900;
    let height = 620;

    let params = Arc::new(FtsCompParams::default());
    let ui_state = Arc::new(CompUiState::new(params));

    // Pre-fill waveform with realistic test data
    for i in 0..WAVEFORM_LEN {
        let t = i as f32 / WAVEFORM_LEN as f32;
        let level = 0.25
            + 0.45 * (t * std::f32::consts::TAU * 3.5).sin().abs()
            + 0.15 * (t * std::f32::consts::TAU * 7.0).sin().abs();
        ui_state.waveform_input[i].store(level.min(1.0), Ordering::Relaxed);
        let gr = if level > 0.5 {
            (level - 0.5) * 1.2
        } else {
            0.0
        };
        ui_state.waveform_gr[i].store(gr.min(1.0), Ordering::Relaxed);
    }
    ui_state.gain_reduction_db.store(4.2, Ordering::Relaxed);
    ui_state.input_peak_db.store(-12.0, Ordering::Relaxed);

    let shared = SharedState::new(ui_state);

    eprintln!("Rendering compressor ({}x{})...", width, height);
    let pixels =
        nih_plug_dioxus::render_screenshot(comp_plugin::editor::App, width, height, Some(shared));
    save_png(&pixels, width, height, output);
}
